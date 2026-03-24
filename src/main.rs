#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_model;
mod icon;
mod preview;
mod process_utils;

use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::Arc,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use app_model::{
    DownloadMode, FormatEntry, MediaPreview, QualityPreset, ToolPaths, WorkerEvent, audio_quality,
    find_tool_paths, parse_formats, tool_command_prefix, video_selector,
};
use directories::UserDirs;
use eframe::{
    App, Frame, NativeOptions,
    egui::{self, Color32, RichText, TextureHandle},
};
use image::{ImageBuffer, Rgba};
use icon::load_app_icon;
use preview::fetch_media_preview;
use process_utils::{configure_background_command, run_command_streaming};
use rfd::FileDialog;

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([1040.0, 700.0])
            .with_icon(Arc::new(load_app_icon())),
        ..Default::default()
    };

    eframe::run_native("RustTube", options, Box::new(|_cc| Ok(Box::<RustTubeApp>::default())))
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
            (Some(_paths), Some(_downloads)) => String::new(),
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

        egui::SidePanel::right("preview_panel")
            .min_width(320.0)
            .max_width(320.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Preview");
                ui.add_space(10.0);
                self.render_preview(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("RustTube");
            ui.label("Paste any URL supported by yt-dlp, choose a format and quality, then start the download.");
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.label("URL:");
                let input_width = (ui.available_width() - 50.0).max(220.0);
                ui.add_sized(
                    [input_width, 24.0],
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
                let path_width = (ui.available_width() - 190.0).max(160.0);
                ui.add_sized(
                    [path_width, 24.0],
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
                        .width(ui.available_width().max(220.0))
                        .selected_text(selected_text)
                        .show_ui(ui, |ui| {
                            for (idx, entry) in self.formats.iter().enumerate() {
                                ui.selectable_value(&mut self.selected_format, idx, &entry.description);
                            }
                        });
                }
            }

            if !self.status.trim().is_empty() {
                ui.add_space(12.0);
                ui.label(RichText::new(&self.status).strong());
            }

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

            let user_scrolled =
                scroll_output.inner.hovered() && ui.input(|input| input.raw_scroll_delta.y.abs() > 0.0);
            if user_scrolled {
                self.log_auto_scroll = false;
            }
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
                return;
            };

            if let Some(texture) = &self.preview_texture {
                let available_width = ui.available_width().clamp(160.0, 260.0);
                let texture_size = texture.size_vec2();
                let max_height = 150.0;
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

            ui.label("Source:");
            ui.add(egui::Label::new(preview.webpage_url.as_str()).wrap());

            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Open source").clicked() {
                    let _ = webbrowser::open(&preview.webpage_url);
                }

                let has_thumbnail_url = preview.thumbnail_url.is_some();
                if ui
                    .add_enabled(has_thumbnail_url, egui::Button::new("Open image"))
                    .clicked()
                {
                    if let Some(url) = &preview.thumbnail_url {
                        let _ = webbrowser::open(url);
                    }
                }

                let has_thumbnail_data = preview.thumbnail_rgba.is_some();
                if ui
                    .add_enabled(has_thumbnail_data, egui::Button::new("Save thumbnail"))
                    .clicked()
                {
                    if let Some(path) = suggest_thumbnail_save_path(&preview.title, &self.default_downloads_dir)
                        && let Some((rgba, size)) = &preview.thumbnail_rgba
                    {
                        match save_thumbnail_png(path.clone(), rgba, *size) {
                            Ok(()) => {
                                self.status = format!("Thumbnail saved to {}", path.display());
                            }
                            Err(error) => {
                                self.status = format!("Failed to save thumbnail: {error}");
                            }
                        }
                    }
                }
            });

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
                Err(_error) => WorkerEvent::FormatsLoaded { entries: Vec::new() },
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
            command.args(tool_command_prefix(&tool_paths.lib_dir)).args(&args);
            configure_background_command(&mut command);

            let result = run_command_streaming(command, &sender);
            let error_log = result
                .as_ref()
                .err()
                .map(|error| format!("Failed to start yt-dlp: {error}\n"));

            let event = match result {
                Ok((success, _output)) => WorkerEvent::DownloadFinished { success },
                Err(_error) => WorkerEvent::DownloadFinished { success: false },
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

fn suggest_thumbnail_save_path(title: &str, default_dir: &Option<PathBuf>) -> Option<PathBuf> {
    let file_name = format!("{}-thumbnail.png", sanitize_file_name(title));
    let mut dialog = FileDialog::new().set_file_name(&file_name);

    if let Some(dir) = default_dir {
        dialog = dialog.set_directory(dir);
    }

    dialog.save_file()
}

fn save_thumbnail_png(path: PathBuf, rgba: &[u8], [width, height]: [usize; 2]) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let image = ImageBuffer::<Rgba<u8>, _>::from_raw(width as u32, height as u32, rgba.to_vec())
        .ok_or_else(|| "invalid thumbnail image buffer".to_owned())?;

    image.save(&path).map_err(|error| error.to_string())
}

fn sanitize_file_name(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect();

    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        "thumbnail".to_owned()
    } else {
        trimmed.to_owned()
    }
}
