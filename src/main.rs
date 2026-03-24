#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_model;
mod icon;
mod progress;
mod preview;
mod process_utils;
mod runtime_tools;
mod settings;

use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use app_model::{
    DownloadMode, FormatEntry, MediaPreview, QualityPreset, RuntimeTool, ToolPackage, ToolPaths,
    WorkerEvent, audio_quality, parse_formats, tool_command_prefix, video_selector,
};
use directories::UserDirs;
use eframe::{
    App, Frame, NativeOptions,
    egui::{self, Color32, RichText, TextureHandle},
};
use image::{ImageBuffer, Rgba};
use icon::load_app_icon;
use progress::{DownloadProgress, ProgressPhase};
use preview::fetch_media_preview;
use process_utils::{
    ActiveProcess, cancel_child_process, clear_active_process, configure_background_command,
    run_command_streaming, run_command_streaming_with_handle,
};
use rfd::FileDialog;
use runtime_tools::{
    download_package, missing_packages, package_for_tool, resolve_lib_dir, tool_installed, tool_paths_if_ready,
};
use settings::{AppSettings, load_settings, save_settings, settings_dir_path};

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
    lib_dir: PathBuf,
    tool_paths: Option<ToolPaths>,
    tool_states: HashMap<RuntimeTool, ToolUiState>,
    worker_tx: Sender<WorkerEvent>,
    worker_rx: Receiver<WorkerEvent>,
    active_tool_package: Option<ToolPackage>,
    loading_formats: bool,
    downloading: bool,
    cancel_requested: bool,
    log_auto_scroll: bool,
    preview_loading: bool,
    preview_requested_url: String,
    preview: Option<MediaPreview>,
    preview_texture: Option<TextureHandle>,
    progress: DownloadProgress,
    active_child: Arc<Mutex<Option<ActiveProcess>>>,
    last_saved_settings: Option<AppSettings>,
}

#[derive(Clone, Default)]
struct ToolUiState {
    downloading: bool,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    error: Option<String>,
}

impl Default for RustTubeApp {
    fn default() -> Self {
        let (worker_tx, worker_rx) = mpsc::channel();
        let downloads_dir = UserDirs::new().map(|dirs| dirs.download_dir().unwrap_or(dirs.home_dir()).to_path_buf());
        let lib_dir = resolve_lib_dir();
        let tool_paths = tool_paths_if_ready(&lib_dir);
        let tool_states = RuntimeTool::ALL
            .into_iter()
            .map(|tool| (tool, ToolUiState::default()))
            .collect();
        let mut download_path = downloads_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let mut mode = DownloadMode::Video;
        let mut quality = QualityPreset::Best;
        let mut last_url = String::new();

        if let Some(saved) = load_settings() {
            let (saved_download_path, saved_mode, saved_quality, saved_last_url) = saved.apply_to_runtime();
            if !saved_download_path.trim().is_empty() {
                download_path = saved_download_path;
            }
            mode = saved_mode;
            quality = saved_quality;
            last_url = saved_last_url;
        }

        let missing = RuntimeTool::ALL
            .into_iter()
            .filter(|tool| !tool_installed(&lib_dir, *tool))
            .map(|tool| tool.label())
            .collect::<Vec<_>>();

        let status = match (&tool_paths, &downloads_dir, missing.is_empty()) {
            (Some(_paths), Some(_downloads), true) => String::new(),
            (_, None, _) => "Error: Could not determine the Windows Downloads folder.".to_owned(),
            _ => format!("Install required tools from the sidebar: {}", missing.join(", ")),
        };

        let initial_settings =
            AppSettings::from_runtime(download_path.clone(), &mode, &quality, last_url.clone());

        Self {
            url: last_url,
            mode,
            quality,
            formats: Vec::new(),
            selected_format: 0,
            status,
            logs: String::new(),
            default_downloads_dir: downloads_dir,
            download_path,
            lib_dir,
            tool_paths,
            tool_states,
            worker_tx,
            worker_rx,
            active_tool_package: None,
            loading_formats: false,
            downloading: false,
            cancel_requested: false,
            log_auto_scroll: true,
            preview_loading: false,
            preview_requested_url: String::new(),
            preview: None,
            preview_texture: None,
            progress: DownloadProgress::default(),
            active_child: Arc::new(Mutex::new(None)),
            last_saved_settings: Some(initial_settings),
        }
    }
}

impl App for RustTubeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        self.maybe_start_preview_fetch();
        self.handle_worker_events(ctx);
        self.persist_settings_if_needed();

        egui::SidePanel::right("preview_sidebar")
            .resizable(false)
            .min_width(340.0)
            .max_width(340.0)
            .show(ctx, |ui| {
                ui.heading("Preview");
                ui.add_space(10.0);
                self.render_preview(ui);
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                self.render_tools_sidebar(ui);
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(RichText::new("Links").strong());
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Website").clicked() {
                        let _ = webbrowser::open("https://jonasgrimm.de");
                    }

                    if ui.button("GitHub").clicked() {
                        let _ = webbrowser::open("https://github.com/beqare/RustTube");
                    }
                });
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

                let can_cancel = self.downloading;
                if ui.add_enabled(can_cancel, egui::Button::new("Cancel")).clicked() {
                    match cancel_child_process(&self.active_child) {
                        Ok(true) => {
                            self.cancel_requested = true;
                            self.status = "Cancel requested...".to_owned();
                            self.progress.phase = ProgressPhase::Canceled;
                        }
                        Ok(false) => {
                            self.status = "No active download to cancel.".to_owned();
                        }
                        Err(error) => {
                            self.status = error;
                        }
                    }
                }

                let can_open_folder = self.target_download_dir().is_some();
                if ui
                    .add_enabled(can_open_folder, egui::Button::new("Open folder"))
                    .clicked()
                {
                    if let Some(path) = self.target_download_dir() {
                        if let Err(error) = open_in_file_explorer(&path) {
                            self.status = error;
                        }
                    }
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

            let content_width = ui.available_width();

            if !self.status.trim().is_empty() {
                ui.add_space(12.0);
                ui.label(RichText::new(&self.status).strong());
            }

            if self.loading_formats || self.downloading || self.progress.percent.is_some() {
                ui.add_space(10.0);
                ui.scope(|ui| {
                    ui.set_max_width(content_width);
                    self.render_progress(ui);
                });
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

            let scroll_output = ui.scope(|ui| {
                ui.set_max_width(content_width);
                egui::ScrollArea::vertical()
                    .id_salt("log_scroll_area")
                    .stick_to_bottom(self.log_auto_scroll)
                    .max_height(360.0)
                    .show(ui, |ui| {
                        let width = ui.available_width();
                        ui.add_sized(
                            [width, 0.0],
                            egui::TextEdit::multiline(&mut self.logs)
                                .desired_width(width)
                                .interactive(false)
                                .font(egui::TextStyle::Monospace),
                        )
                    })
            });

            let user_scrolled =
                scroll_output.inner.inner.hovered() && ui.input(|input| input.raw_scroll_delta.y.abs() > 0.0);
            if user_scrolled {
                self.log_auto_scroll = false;
            }
        });

        if self.loading_formats || self.downloading || self.preview_loading || self.active_tool_package.is_some() {
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
        self.active_tool_package.is_none()
            && self.tool_paths.is_some()
            && !self.url.trim().is_empty()
            && self.target_download_dir().is_some()
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
                    self.progress.update_from_chunk(&chunk);
                }
                WorkerEvent::ToolDownloadProgress {
                    package,
                    downloaded_bytes,
                    total_bytes,
                } => {
                    self.active_tool_package = Some(package);
                    for tool in package.tools() {
                        if let Some(state) = self.tool_states.get_mut(tool) {
                            state.downloading = true;
                            state.downloaded_bytes = downloaded_bytes;
                            state.total_bytes = total_bytes;
                            state.error = None;
                        }
                    }
                }
                WorkerEvent::ToolDownloadFinished { package, result } => {
                    self.active_tool_package = None;
                    match result {
                        Ok(()) => {
                            for tool in package.tools() {
                                if let Some(state) = self.tool_states.get_mut(tool) {
                                    *state = ToolUiState::default();
                                }
                            }
                            self.tool_paths = tool_paths_if_ready(&self.lib_dir);
                            self.status = format!("{} ready.", package_label(package));
                        }
                        Err(error) => {
                            for tool in package.tools() {
                                if let Some(state) = self.tool_states.get_mut(tool) {
                                    state.downloading = false;
                                    state.error = Some(error.clone());
                                }
                            }
                            self.status = error;
                        }
                    }
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
                    self.progress.reset();
                    if entries.is_empty() {
                        self.status = "No formats detected. Some sites expose only a few or unusual streams.".to_owned();
                    } else {
                        self.selected_format = 0;
                        self.status = format!("Loaded {} formats.", entries.len());
                    }
                    self.formats = entries;
                }
                WorkerEvent::DownloadFinished { success, canceled } => {
                    self.downloading = false;
                    let was_canceled = canceled || self.cancel_requested;
                    self.cancel_requested = false;
                    if success {
                        self.progress.phase = ProgressPhase::Finished;
                        self.progress.percent = Some(1.0);
                    } else if was_canceled || self.progress.phase == ProgressPhase::Canceled {
                        self.progress.phase = ProgressPhase::Canceled;
                    } else {
                        self.progress.phase = ProgressPhase::Failed;
                    }
                    self.status = if success {
                        "Download finished.".to_owned()
                    } else if was_canceled || self.progress.phase == ProgressPhase::Canceled {
                        "Download canceled.".to_owned()
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

    fn render_tools_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Tools").strong());
        ui.add_space(4.0);
        ui.small(format!("Storage: {}", self.lib_dir.display()));
        ui.add_space(8.0);

        let missing_count = missing_packages(&self.lib_dir).len();
        if missing_count > 0 {
            let can_download_all = self.active_tool_package.is_none();
            if ui
                .add_enabled(can_download_all, egui::Button::new("Download All"))
                .clicked()
            {
                self.start_missing_tool_downloads();
            }

            ui.add_space(8.0);
        }

        for tool in RuntimeTool::ALL {
            let installed = tool_installed(&self.lib_dir, tool);
            let package = package_for_tool(tool);
            let active_package = self.active_tool_package;
            let is_downloading = active_package == Some(package);
            let button_enabled = !installed && active_package.is_none();
            let status_text = self.tool_status_text(tool, installed, is_downloading);

            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tool.label()).strong());
                    ui.label(RichText::new(status_text).small().color(Color32::GRAY));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !installed
                            && ui
                                .add_enabled(
                                    button_enabled,
                                    egui::Button::new(if is_downloading {
                                        "Downloading..."
                                    } else {
                                        "Download"
                                    }),
                                )
                                .clicked()
                        {
                            self.start_tool_download(package);
                        }
                    });
                });

                if is_downloading
                    && let Some(state) = self.tool_states.get(&tool)
                    && let Some(total) = state.total_bytes
                    && total > 0
                {
                    let progress = (state.downloaded_bytes as f32 / total as f32).clamp(0.0, 1.0);
                    ui.add(
                        egui::ProgressBar::new(progress)
                            .show_percentage()
                            .desired_width(ui.available_width()),
                    );
                }
            });

            ui.add_space(4.0);
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label(RichText::new("Folders").strong());
        ui.add_space(6.0);

        ui.horizontal_wrapped(|ui| {
            if ui.button("Tools installation folder").clicked() {
                if let Err(error) = open_in_file_explorer(&self.lib_dir) {
                    self.status = error;
                }
            }

            if ui.button("Settings folder").clicked() {
                if let Some(path) = settings_dir_path() {
                    if let Err(error) = open_in_file_explorer(&path) {
                        self.status = error;
                    }
                } else {
                    self.status = "Could not determine settings folder.".to_owned();
                }
            }

            if ui.button("Program folder").clicked() {
                match std::env::current_exe()
                    .ok()
                    .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
                {
                    Some(path) => {
                        if let Err(error) = open_in_file_explorer(&path) {
                            self.status = error;
                        }
                    }
                    None => {
                        self.status = "Could not determine program folder.".to_owned();
                    }
                }
            }
        });
    }

    fn tool_status_text(&self, tool: RuntimeTool, installed: bool, is_downloading: bool) -> String {
        let Some(state) = self.tool_states.get(&tool) else {
            return if installed { "Installed".to_owned() } else { "Missing".to_owned() };
        };

        if installed {
            "Installed".to_owned()
        } else if is_downloading {
            format_download_progress(state.downloaded_bytes, state.total_bytes)
        } else if let Some(error) = &state.error {
            format!("Failed: {error}")
        } else {
            "Missing".to_owned()
        }
    }

    fn start_missing_tool_downloads(&mut self) {
        if self.active_tool_package.is_some() {
            return;
        }

        let packages = missing_packages(&self.lib_dir);
        if packages.is_empty() {
            self.tool_paths = tool_paths_if_ready(&self.lib_dir);
            self.status = "All tools are already installed.".to_owned();
            return;
        }

        let sender = self.worker_tx.clone();
        let lib_dir = self.lib_dir.clone();
        self.logs.clear();
        self.log_auto_scroll = true;
        self.status = "Downloading required tools...".to_owned();
        self.active_tool_package = packages.first().copied();
        clear_tool_errors(&mut self.tool_states, &packages);

        thread::spawn(move || {
            for package in packages {
                let result = download_package(&sender, &lib_dir, package);
                let is_err = result.is_err();
                let _ = sender.send(WorkerEvent::ToolDownloadFinished { package, result });
                if is_err {
                    break;
                }
            }
        });
    }

    fn start_tool_download(&mut self, package: ToolPackage) {
        if self.active_tool_package.is_some() {
            return;
        }

        self.logs.clear();
        self.log_auto_scroll = true;
        self.active_tool_package = Some(package);
        clear_tool_errors(&mut self.tool_states, &[package]);
        self.status = format!("Downloading {}...", package_label(package));

        let sender = self.worker_tx.clone();
        let lib_dir = self.lib_dir.clone();

        thread::spawn(move || {
            let result = download_package(&sender, &lib_dir, package);
            let _ = sender.send(WorkerEvent::ToolDownloadFinished { package, result });
        });
    }

    fn load_formats(&mut self) {
        if self.active_tool_package.is_some() {
            self.status = "Please wait until the current tool download finishes.".to_owned();
            return;
        }

        let Some(tool_paths) = self.tool_paths.clone() else {
            self.status = "Install all required tools from the sidebar first.".to_owned();
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
        self.progress.reset();
        self.progress.phase = ProgressPhase::Preparing;
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
        if self.active_tool_package.is_some() {
            self.status = "Please wait until the current tool download finishes.".to_owned();
            return;
        }

        let Some(tool_paths) = self.tool_paths.clone() else {
            self.status = "Install all required tools from the sidebar first.".to_owned();
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
        self.cancel_requested = false;
        self.status = "Download in progress...".to_owned();
        self.logs.clear();
        self.log_auto_scroll = true;
        self.progress.reset();
        self.progress.phase = ProgressPhase::Preparing;
        let sender = self.worker_tx.clone();
        let child_slot = Arc::clone(&self.active_child);

        thread::spawn(move || {
            let mut command = Command::new(&tool_paths.yt_dlp_path);
            command.args(tool_command_prefix(&tool_paths.lib_dir)).args(&args);
            configure_background_command(&mut command);

            let result = run_command_streaming_with_handle(command, &sender, Some(child_slot));
            let error_log = result
                .as_ref()
                .err()
                .map(|error| format!("Failed to start yt-dlp: {error}\n"));

            let event = match result {
                Ok((success, output)) => WorkerEvent::DownloadFinished {
                    success,
                    canceled: output.to_ascii_lowercase().contains("terminated"),
                },
                Err(_error) => WorkerEvent::DownloadFinished {
                    success: false,
                    canceled: false,
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

    fn persist_settings_if_needed(&mut self) {
        let current = AppSettings::from_runtime(
            self.download_path.clone(),
            &self.mode,
            &self.quality,
            self.url.clone(),
        );

        if self.last_saved_settings.as_ref() == Some(&current) {
            return;
        }

        if save_settings(&current).is_ok() {
            self.last_saved_settings = Some(current);
        }
    }

    fn render_progress(&self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            let width = ui.available_width();
            ui.horizontal(|ui| {
                ui.label(RichText::new(self.progress.label()).strong());
                if let Some(speed) = &self.progress.speed {
                    ui.label(format!("Speed: {speed}"));
                }
                if let Some(eta) = &self.progress.eta {
                    ui.label(format!("ETA: {eta}"));
                }
            });

            let progress_value = self.progress.percent.unwrap_or(0.0);
            ui.add_sized(
                [width, 0.0],
                egui::ProgressBar::new(progress_value)
                    .show_percentage()
                    .desired_width(width),
            );

            if let Some(file) = &self.progress.current_file {
                ui.label(format!("Current file: {file}"));
            }
        });
    }
}

impl Drop for RustTubeApp {
    fn drop(&mut self) {
        clear_active_process(&self.active_child);
    }
}

fn package_label(package: ToolPackage) -> &'static str {
    match package {
        ToolPackage::YtDlp => "yt-dlp",
        ToolPackage::FfmpegBundle => "ffmpeg + ffprobe",
        ToolPackage::Deno => "deno",
    }
}

fn clear_tool_errors(states: &mut HashMap<RuntimeTool, ToolUiState>, packages: &[ToolPackage]) {
    for package in packages {
        for tool in package.tools() {
            if let Some(state) = states.get_mut(tool) {
                *state = ToolUiState::default();
            }
        }
    }
}

fn format_download_progress(downloaded_bytes: u64, total_bytes: Option<u64>) -> String {
    match total_bytes {
        Some(total) if total > 0 => format!(
            "Downloading {}/{}",
            format_megabytes(downloaded_bytes),
            format_megabytes(total)
        ),
        _ => format!("Downloading {}", format_megabytes(downloaded_bytes)),
    }
}

fn format_megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
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

fn open_in_file_explorer(path: &PathBuf) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|error| format!("Could not open folder: {error}"))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|error| format!("Could not open folder: {error}"))?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|error| format!("Could not open folder: {error}"))?;
        Ok(())
    }
}
