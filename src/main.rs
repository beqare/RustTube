#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use directories::UserDirs;
use eframe::{
    App, Frame, NativeOptions,
    egui::{self, Color32, RichText, TextureHandle},
};
use ico::IconDir;
use image::GenericImageView;
use rfd::FileDialog;
use serde_json::Value;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([1040.0, 700.0])
            .with_icon(Arc::new(load_app_icon())),
        ..Default::default()
    };

    eframe::run_native(
        "RustTube",
        options,
        Box::new(|_cc| Ok(Box::<RustTubeApp>::default())),
    )
}

#[derive(Clone, PartialEq, Eq)]
enum DownloadMode {
    Video,
    AudioMp3,
    Manual,
}

impl DownloadMode {
    fn label(&self) -> &'static str {
        match self {
            Self::Video => "Video",
            Self::AudioMp3 => "Audio (MP3)",
            Self::Manual => "Manual yt-dlp format",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum QualityPreset {
    Best,
    P1080,
    P720,
    P480,
    Worst,
}

impl QualityPreset {
    fn label(&self) -> &'static str {
        match self {
            Self::Best => "Best available quality",
            Self::P1080 => "Up to 1080p",
            Self::P720 => "Up to 720p",
            Self::P480 => "Up to 480p",
            Self::Worst => "Lowest quality",
        }
    }
}

#[derive(Clone)]
struct FormatEntry {
    id: String,
    description: String,
}

#[derive(Clone)]
struct ToolPaths {
    lib_dir: PathBuf,
    yt_dlp_path: PathBuf,
}

#[derive(Clone)]
struct MediaPreview {
    title: String,
    uploader: String,
    duration: Option<String>,
    webpage_url: String,
    thumbnail_url: Option<String>,
    thumbnail_rgba: Option<(Vec<u8>, [usize; 2])>,
}

enum WorkerEvent {
    LogChunk(String),
    PreviewLoaded {
        url: String,
        preview: Option<MediaPreview>,
        error: Option<String>,
    },
    FormatsLoaded {
        entries: Vec<FormatEntry>,
    },
    DownloadFinished {
        success: bool,
    },
}

struct RustTubeApp {
    url: String,
    mode: DownloadMode,
    quality: QualityPreset,
    formats: Vec<FormatEntry>,
    selected_format: usize,
    status: String,
    logs: String,
    default_downloads_dir: Option<PathBuf>,
    download_path: String,
    tool_paths: Option<ToolPaths>,
    worker_tx: Sender<WorkerEvent>,
    worker_rx: Receiver<WorkerEvent>,
    loading_formats: bool,
    downloading: bool,
    log_auto_scroll: bool,
    preview_loading: bool,
    preview_requested_url: String,
    preview: Option<MediaPreview>,
    preview_texture: Option<TextureHandle>,
}

impl Default for RustTubeApp {
    fn default() -> Self {
        let (worker_tx, worker_rx) = mpsc::channel();
        let downloads_dir = UserDirs::new().map(|dirs| dirs.download_dir().unwrap_or(dirs.home_dir()).to_path_buf());
        let tool_paths = find_tool_paths();
        let download_path = downloads_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();

        let status = match (&tool_paths, &downloads_dir) {
            (Some(paths), Some(downloads)) => format!(
                "Ready. Tools folder: {} | Downloads folder: {}",
                paths.lib_dir.display(),
                downloads.display()
            ),
            (None, _) => "Error: lib/yt-dlp.exe was not found.".to_owned(),
            (_, None) => "Error: Could not determine the Windows Downloads folder.".to_owned(),
        };

        Self {
            url: String::new(),
            mode: DownloadMode::Video,
            quality: QualityPreset::Best,
            formats: Vec::new(),
            selected_format: 0,
            status,
            logs: String::new(),
            default_downloads_dir: downloads_dir,
            download_path,
            tool_paths,
            worker_tx,
            worker_rx,
            loading_formats: false,
            downloading: false,
            log_auto_scroll: true,
            preview_loading: false,
            preview_requested_url: String::new(),
            preview: None,
            preview_texture: None,
        }
    }
}

impl App for RustTubeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        self.maybe_start_preview_fetch();
        self.handle_worker_events(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(2, |columns| {
                let ui = &mut columns[0];
            ui.heading("RustTube");
                ui.label("Paste any URL supported by yt-dlp, choose a format and quality, then start the download.");
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("URL:");
                    ui.add_sized(
                        [650.0, 24.0],
                        egui::TextEdit::singleline(&mut self.url).hint_text("https://..."),
                    );
                });

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Mode:");
                    egui::ComboBox::from_id_salt("download_mode")
                        .selected_text(self.mode.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.mode, DownloadMode::Video, DownloadMode::Video.label());
                            ui.selectable_value(&mut self.mode, DownloadMode::AudioMp3, DownloadMode::AudioMp3.label());
                            ui.selectable_value(&mut self.mode, DownloadMode::Manual, DownloadMode::Manual.label());
                        });

                    ui.label("Quality:");
                    egui::ComboBox::from_id_salt("quality")
                        .selected_text(self.quality.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.quality, QualityPreset::Best, QualityPreset::Best.label());
                            ui.selectable_value(&mut self.quality, QualityPreset::P1080, QualityPreset::P1080.label());
                            ui.selectable_value(&mut self.quality, QualityPreset::P720, QualityPreset::P720.label());
                            ui.selectable_value(&mut self.quality, QualityPreset::P480, QualityPreset::P480.label());
                            ui.selectable_value(&mut self.quality, QualityPreset::Worst, QualityPreset::Worst.label());
                        });
                });

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Target folder:");
                    ui.add_sized(
                        [420.0, 24.0],
                        egui::TextEdit::singleline(&mut self.download_path)
                            .hint_text("C:\\Users\\...\\Downloads")
                            .interactive(false),
                    );

                    if ui.button("Browse...").clicked() {
                        let mut dialog = FileDialog::new();

                        if let Some(current_path) = self.target_download_dir() {
                            dialog = dialog.set_directory(current_path);
                        } else if let Some(default_path) = &self.default_downloads_dir {
                            dialog = dialog.set_directory(default_path);
                        }

                        if let Some(selected_folder) = dialog.pick_folder() {
                            self.download_path = selected_folder.display().to_string();
                        }
                    }

                    let can_reset = self.default_downloads_dir.is_some();
                    if ui.add_enabled(can_reset, egui::Button::new("Use default")).clicked() {
                        if let Some(path) = &self.default_downloads_dir {
                            self.download_path = path.display().to_string();
                        }
                    }
                });

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    let can_fetch = !self.loading_formats && self.can_run_commands();
                    if ui.add_enabled(can_fetch, egui::Button::new("Load formats")).clicked() {
                        self.load_formats();
                    }

                    let can_download = !self.downloading && self.can_start_download();
                    if ui.add_enabled(can_download, egui::Button::new("Start download")).clicked() {
                        self.start_download();
                    }
                });

                if self.mode == DownloadMode::Manual {
                    ui.add_space(10.0);
                    ui.label("Manual format:");
                    if self.formats.is_empty() {
                        ui.colored_label(Color32::YELLOW, "Click 'Load formats' first.");
                    } else {
                        let selected_text = self
                            .formats
                            .get(self.selected_format)
                            .map(|entry| entry.description.clone())
                            .unwrap_or_else(|| "No format".to_owned());

                        egui::ComboBox::from_id_salt("manual_format")
                            .width(780.0)
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                for (idx, entry) in self.formats.iter().enumerate() {
                                    ui.selectable_value(&mut self.selected_format, idx, &entry.description);
                                }
                            });
                    }
                }

                ui.add_space(12.0);
                ui.label(RichText::new(&self.status).strong());

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label("Output / Log:");

                    if ui
                        .add_enabled(!self.log_auto_scroll, egui::Button::new("Follow latest"))
                        .clicked()
                    {
                        self.log_auto_scroll = true;
                    }
                });

                let scroll_output = egui::ScrollArea::vertical()
                    .id_salt("log_scroll_area")
                    .stick_to_bottom(self.log_auto_scroll)
                    .max_height(360.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.logs)
                                .desired_width(f32::INFINITY)
                                .interactive(false)
                                .font(egui::TextStyle::Monospace),
                        )
                    });

                let user_scrolled = scroll_output.inner.hovered()
                    && ui.input(|input| input.raw_scroll_delta.y.abs() > 0.0);
                if user_scrolled {
                    self.log_auto_scroll = false;
                }

                columns[1].set_min_width(300.0);
                let preview_ui = &mut columns[1];
                preview_ui.heading("Preview");
                preview_ui.add_space(10.0);
                self.render_preview(preview_ui);
            });
        });

        if self.loading_formats || self.downloading || self.preview_loading {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
    }
}

impl RustTubeApp {
    fn maybe_start_preview_fetch(&mut self) {
        let url = self.url.trim().to_owned();

        if url.is_empty() {
            self.preview_requested_url.clear();
            self.preview_loading = false;
            self.preview = None;
            self.preview_texture = None;
            return;
        }

        if self.preview_loading || self.preview_requested_url == url {
            return;
        }

        let Some(tool_paths) = self.tool_paths.clone() else {
            return;
        };

        self.preview_loading = true;
        self.preview_requested_url = url.clone();
        self.preview = None;
        self.preview_texture = None;
        let sender = self.worker_tx.clone();

        thread::spawn(move || {
            let result = fetch_media_preview(&tool_paths, &url);
            let event = match result {
                Ok(preview) => WorkerEvent::PreviewLoaded {
                    url,
                    preview: Some(preview),
                    error: None,
                },
                Err(error) => WorkerEvent::PreviewLoaded {
                    url,
                    preview: None,
                    error: Some(error),
                },
            };

            let _ = sender.send(event);
        });
    }

    fn can_run_commands(&self) -> bool {
        self.tool_paths.is_some() && !self.url.trim().is_empty() && self.target_download_dir().is_some()
    }

    fn can_start_download(&self) -> bool {
        if !self.can_run_commands() {
            return false;
        }

        self.mode != DownloadMode::Manual || !self.formats.is_empty()
    }

    fn handle_worker_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.worker_rx.try_recv() {
            match event {
                WorkerEvent::LogChunk(chunk) => {
                    self.logs.push_str(&chunk);
                }
                WorkerEvent::PreviewLoaded { url, preview, error } => {
                    if url != self.url.trim() {
                        continue;
                    }

                    self.preview_loading = false;

                    match (preview, error) {
                        (Some(preview), None) => {
                            self.preview_texture = preview.thumbnail_rgba.as_ref().map(|(rgba, [w, h])| {
                                ctx.load_texture(
                                    "media_preview_thumbnail",
                                    egui::ColorImage::from_rgba_unmultiplied([*w, *h], rgba),
                                    egui::TextureOptions::LINEAR,
                                )
                            });
                            self.preview = Some(preview);
                        }
                        (_, Some(error)) => {
                            self.preview = Some(MediaPreview {
                                title: "Preview unavailable".to_owned(),
                                uploader: error,
                                duration: None,
                                webpage_url: url,
                                thumbnail_url: None,
                                thumbnail_rgba: None,
                            });
                            self.preview_texture = None;
                        }
                        _ => {
                            self.preview = None;
                            self.preview_texture = None;
                        }
                    }
                }
                WorkerEvent::FormatsLoaded { entries } => {
                    self.loading_formats = false;
                    if entries.is_empty() {
                        self.status = "No formats detected. Some sites expose only a few or unusual streams.".to_owned();
                    } else {
                        self.selected_format = 0;
                        self.status = format!("Loaded {} formats.", entries.len());
                    }
                    self.formats = entries;
                }
                WorkerEvent::DownloadFinished { success } => {
                    self.downloading = false;
                    self.status = if success {
                        "Download finished.".to_owned()
                    } else {
                        "Download failed. See the log for details.".to_owned()
                    };
                }
            }
        }
    }

    fn render_preview(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(280.0);

            if self.preview_loading {
                ui.label("Loading preview...");
                return;
            }

            let Some(preview) = &self.preview else {
                ui.colored_label(Color32::GRAY, "Paste a supported URL to load a thumbnail and media info.");
                return;
            };

            if let Some(texture) = &self.preview_texture {
                let available_width = ui.available_width().clamp(180.0, 320.0);
                let texture_size = texture.size_vec2();
                let max_height = 180.0;
                let width_scale = available_width / texture_size.x;
                let height_scale = max_height / texture_size.y;
                let scale = width_scale.min(height_scale).min(1.0);
                let display_size = texture_size * scale;
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::new(texture).fit_to_exact_size(display_size));
                });
                ui.add_space(10.0);
            }

            ui.label(RichText::new(&preview.title).strong().size(18.0));
            ui.add_space(4.0);
            ui.label(format!("Creator: {}", preview.uploader));

            if let Some(duration) = &preview.duration {
                ui.label(format!("Duration: {duration}"));
            }

            ui.label(format!("Source: {}", preview.webpage_url));

            if self.preview_texture.is_none() {
                let cover_status = if preview.thumbnail_url.is_some() {
                    "Thumbnail was found, but could not be rendered."
                } else {
                    "No thumbnail was provided for this media."
                };
                ui.colored_label(Color32::GRAY, cover_status);
            }

            if self.mode == DownloadMode::AudioMp3 {
                ui.add_space(6.0);
                ui.colored_label(
                    Color32::LIGHT_GREEN,
                    "MP3 downloads include metadata and embedded cover art when yt-dlp provides it.",
                );
            }
        });
    }

    fn load_formats(&mut self) {
        let Some(tool_paths) = self.tool_paths.clone() else {
            self.status = "yt-dlp.exe is missing in lib/.".to_owned();
            return;
        };

        let url = self.url.trim().to_owned();
        if url.is_empty() {
            self.status = "Please enter a URL first.".to_owned();
            return;
        }

        self.loading_formats = true;
        self.status = "Loading available formats...".to_owned();
        self.logs.clear();
        self.log_auto_scroll = true;
        let sender = self.worker_tx.clone();

        thread::spawn(move || {
            let mut command = Command::new(&tool_paths.yt_dlp_path);
            command
                .args(tool_command_prefix(&tool_paths.lib_dir))
                .args(["-F".to_owned(), url]);
            configure_background_command(&mut command);

            let result = run_command_streaming(command, &sender);
            let error_log = result
                .as_ref()
                .err()
                .map(|error| format!("Failed to start yt-dlp: {error}\n"));

            let event = match result {
                Ok((_success, output)) => WorkerEvent::FormatsLoaded {
                    entries: parse_formats(&output),
                },
                Err(_error) => WorkerEvent::FormatsLoaded {
                    entries: Vec::new(),
                },
            };

            if let Some(error_log) = error_log {
                let _ = sender.send(WorkerEvent::LogChunk(error_log));
            }
            let _ = sender.send(event);
        });
    }

    fn start_download(&mut self) {
        let Some(tool_paths) = self.tool_paths.clone() else {
            self.status = "yt-dlp.exe is missing in lib/.".to_owned();
            return;
        };
        let Some(downloads_dir) = self.target_download_dir() else {
            self.status = "Please enter a valid target folder.".to_owned();
            return;
        };

        let url = self.url.trim().to_owned();
        if url.is_empty() {
            self.status = "Please enter a URL first.".to_owned();
            return;
        }

        let mut args: Vec<String> = vec![
            "--ffmpeg-location".to_owned(),
            tool_paths.lib_dir.display().to_string(),
            "--no-playlist".to_owned(),
            "-P".to_owned(),
            downloads_dir.display().to_string(),
            "-o".to_owned(),
            "%(title)s.%(ext)s".to_owned(),
        ];

        match self.mode {
            DownloadMode::Video => {
                args.push("-f".to_owned());
                args.push(video_selector(&self.quality).to_owned());
                args.push("--merge-output-format".to_owned());
                args.push("mp4".to_owned());
            }
            DownloadMode::AudioMp3 => {
                args.push("-x".to_owned());
                args.push("--audio-format".to_owned());
                args.push("mp3".to_owned());
                args.push("--audio-quality".to_owned());
                args.push(audio_quality(&self.quality).to_owned());
                args.push("--add-metadata".to_owned());
                args.push("--embed-thumbnail".to_owned());
            }
            DownloadMode::Manual => {
                let Some(entry) = self.formats.get(self.selected_format) else {
                    self.status = "Please select a format first.".to_owned();
                    return;
                };
                args.push("-f".to_owned());
                args.push(entry.id.clone());
            }
        }

        args.push(url);

        self.downloading = true;
        self.status = "Download in progress...".to_owned();
        self.logs.clear();
        self.log_auto_scroll = true;
        let sender = self.worker_tx.clone();

        thread::spawn(move || {
            let mut command = Command::new(&tool_paths.yt_dlp_path);
            command
                .args(tool_command_prefix(&tool_paths.lib_dir))
                .args(&args);
            configure_background_command(&mut command);

            let result = run_command_streaming(command, &sender);
            let error_log = result
                .as_ref()
                .err()
                .map(|error| format!("Failed to start yt-dlp: {error}\n"));

            let event = match result {
                Ok((success, _output)) => WorkerEvent::DownloadFinished { success },
                Err(_error) => WorkerEvent::DownloadFinished {
                    success: false,
                },
            };

            if let Some(error_log) = error_log {
                let _ = sender.send(WorkerEvent::LogChunk(error_log));
            }
            let _ = sender.send(event);
        });
    }

    fn target_download_dir(&self) -> Option<PathBuf> {
        let trimmed = self.download_path.trim();
        if trimmed.is_empty() {
            return None;
        }

        Some(PathBuf::from(trimmed))
    }
}

fn parse_formats(raw: &str) -> Vec<FormatEntry> {
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty()
                || trimmed.starts_with('[')
                || trimmed.starts_with("ID")
                || trimmed.starts_with('-')
            {
                return None;
            }

            let id = trimmed.split_whitespace().next()?;
            if !id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-') {
                return None;
            }

            Some(FormatEntry {
                id: id.to_owned(),
                description: trimmed.to_owned(),
            })
        })
        .collect()
}

fn video_selector(quality: &QualityPreset) -> &'static str {
    match quality {
        QualityPreset::Best => {
            "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/bestvideo+bestaudio/best"
        }
        QualityPreset::P1080 => {
            "bestvideo[height<=1080][ext=mp4]+bestaudio[ext=m4a]/best[height<=1080][ext=mp4]/bestvideo[height<=1080]+bestaudio/best[height<=1080]"
        }
        QualityPreset::P720 => {
            "bestvideo[height<=720][ext=mp4]+bestaudio[ext=m4a]/best[height<=720][ext=mp4]/bestvideo[height<=720]+bestaudio/best[height<=720]"
        }
        QualityPreset::P480 => {
            "bestvideo[height<=480][ext=mp4]+bestaudio[ext=m4a]/best[height<=480][ext=mp4]/bestvideo[height<=480]+bestaudio/best[height<=480]"
        }
        QualityPreset::Worst => "worst[ext=mp4]/worstvideo+worstaudio/worst",
    }
}

fn audio_quality(quality: &QualityPreset) -> &'static str {
    match quality {
        QualityPreset::Best => "0",
        QualityPreset::P1080 => "2",
        QualityPreset::P720 => "4",
        QualityPreset::P480 => "6",
        QualityPreset::Worst => "9",
    }
}

fn fetch_media_preview(tool_paths: &ToolPaths, url: &str) -> Result<MediaPreview, String> {
    let mut command = Command::new(&tool_paths.yt_dlp_path);
    command.args(tool_command_prefix(&tool_paths.lib_dir)).args([
        "--dump-single-json".to_owned(),
        "--skip-download".to_owned(),
        "--no-playlist".to_owned(),
        url.to_owned(),
    ]);
    configure_background_command(&mut command);

    let output = command
        .output()
        .map_err(|error| format!("Could not load preview: {error}"))?;

    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    if text.trim().is_empty() && !output.stderr.is_empty() {
        text = String::from_utf8_lossy(&output.stderr).to_string();
    }

    let json: Value = serde_json::from_str(&text)
        .map_err(|error| format!("Could not parse preview data: {error}"))?;

    let title = json
        .get("track")
        .and_then(Value::as_str)
        .or_else(|| json.get("title").and_then(Value::as_str))
        .unwrap_or("Unknown title")
        .to_owned();

    let uploader = json
        .get("artist")
        .and_then(Value::as_str)
        .or_else(|| json.get("uploader").and_then(Value::as_str))
        .or_else(|| json.get("channel").and_then(Value::as_str))
        .unwrap_or("Unknown creator")
        .to_owned();

    let duration = json
        .get("duration")
        .and_then(Value::as_f64)
        .map(|seconds| format_duration(seconds as u64));

    let webpage_url = json
        .get("webpage_url")
        .and_then(Value::as_str)
        .unwrap_or(url)
        .to_owned();

    let thumbnail_url = json
        .get("thumbnail")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let thumbnail_rgba = thumbnail_url
        .as_deref()
        .and_then(|thumb_url| download_thumbnail(thumb_url).ok());

    Ok(MediaPreview {
        title,
        uploader,
        duration,
        webpage_url,
        thumbnail_url,
        thumbnail_rgba,
    })
}

fn download_thumbnail(url: &str) -> Result<(Vec<u8>, [usize; 2]), String> {
    let response = reqwest::blocking::get(url)
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("Could not download thumbnail: {error}"))?;

    let bytes = response
        .bytes()
        .map_err(|error| format!("Could not read thumbnail bytes: {error}"))?;

    let image = image::load_from_memory(&bytes)
        .map_err(|error| format!("Could not decode thumbnail: {error}"))?;
    let rgba = image.to_rgba8();
    let (width, height) = image.dimensions();

    Ok((rgba.into_raw(), [width as usize, height as usize]))
}

fn format_duration(total_seconds: u64) -> String {
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn tool_command_prefix(lib_dir: &Path) -> Vec<String> {
    let mut args = vec!["--ffmpeg-location".to_owned(), lib_dir.display().to_string()];

    let deno_path = find_deno_in_lib(lib_dir);
    if let Some(deno_path) = deno_path {
        args.push("--js-runtimes".to_owned());
        args.push(format!("deno:{}", deno_path.display()));
    }

    args
}

fn find_tool_paths() -> Option<ToolPaths> {
    let mut candidates = Vec::new();

    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir.join("lib"));
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.join("lib"));
            candidates.push(exe_dir.join("..").join("lib"));
            candidates.push(exe_dir.join("..").join("..").join("lib"));
        }
    }

    candidates.into_iter().find_map(|lib_dir| {
        let yt_dlp_path = lib_dir.join("yt-dlp.exe");
        yt_dlp_path.is_file().then_some(ToolPaths { lib_dir, yt_dlp_path })
    })
}

fn find_deno_in_lib(lib_dir: &Path) -> Option<PathBuf> {
    let candidates = [
        lib_dir.join("deno.exe"),
        lib_dir.join("bin").join("deno.exe"),
        lib_dir.join("deno").join("bin").join("deno.exe"),
    ];

    candidates.into_iter().find(|path| path.is_file())
}

fn load_app_icon() -> egui::IconData {
    let icon_bytes = include_bytes!("../assets/icon.ico");
    let mut cursor = std::io::Cursor::new(icon_bytes.as_slice());
    let icon_dir = IconDir::read(&mut cursor).expect("failed to read assets/icon.ico");

    let best_entry = icon_dir
        .entries()
        .iter()
        .max_by_key(|entry| entry.width() * entry.height())
        .expect("assets/icon.ico does not contain any icon entries");

    let image = best_entry
        .decode()
        .expect("failed to decode icon image from assets/icon.ico");

    egui::IconData {
        rgba: image.rgba_data().to_vec(),
        width: image.width(),
        height: image.height(),
    }
}

fn configure_background_command(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

fn run_command_streaming(mut command: Command, sender: &Sender<WorkerEvent>) -> Result<(bool, String), String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|error| format!("could not launch process: {error}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "could not capture stdout".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "could not capture stderr".to_owned())?;

    let combined_output = Arc::new(Mutex::new(String::new()));

    let stdout_handle = spawn_stream_reader(stdout, sender.clone(), Arc::clone(&combined_output));
    let stderr_handle = spawn_stream_reader(stderr, sender.clone(), Arc::clone(&combined_output));

    let status = child
        .wait()
        .map_err(|error| format!("failed while waiting for process: {error}"))?;

    let _ = stdout_handle.join();
    let _ = stderr_handle.join();

    let output = match Arc::try_unwrap(combined_output) {
        Ok(buffer) => buffer.into_inner().unwrap_or_default(),
        Err(buffer) => buffer.lock().map(|text| text.clone()).unwrap_or_default(),
    };

    Ok((status.success(), output))
}

fn spawn_stream_reader<R: Read + Send + 'static>(
    mut reader: R,
    sender: Sender<WorkerEvent>,
    output: Arc<Mutex<String>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0_u8; 2048];

        loop {
            let bytes_read = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(_) => break,
            };

            let chunk = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();

            if let Ok(mut text) = output.lock() {
                text.push_str(&chunk);
            }

            let _ = sender.send(WorkerEvent::LogChunk(chunk));
        }
    })
}
