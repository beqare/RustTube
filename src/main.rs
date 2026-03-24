#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use directories::UserDirs;
use eframe::{
    App, Frame, NativeOptions,
    egui::{self, Color32, RichText},
};

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([860.0, 620.0])
            .with_min_inner_size([720.0, 520.0]),
        ..Default::default()
    };

    eframe::run_native(
        "RustTube Downloader",
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
            Self::Manual => "Manuelles yt-dlp-Format",
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
            Self::Best => "Beste verfuegbare Qualitaet",
            Self::P1080 => "Bis 1080p",
            Self::P720 => "Bis 720p",
            Self::P480 => "Bis 480p",
            Self::Worst => "Kleinste Qualitaet",
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

enum WorkerEvent {
    FormatsLoaded {
        entries: Vec<FormatEntry>,
        raw_output: String,
    },
    DownloadFinished {
        success: bool,
        output: String,
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
    downloads_dir: Option<PathBuf>,
    tool_paths: Option<ToolPaths>,
    worker_tx: Sender<WorkerEvent>,
    worker_rx: Receiver<WorkerEvent>,
    loading_formats: bool,
    downloading: bool,
}

impl Default for RustTubeApp {
    fn default() -> Self {
        let (worker_tx, worker_rx) = mpsc::channel();
        let downloads_dir = UserDirs::new().map(|dirs| dirs.download_dir().unwrap_or(dirs.home_dir()).to_path_buf());
        let tool_paths = find_tool_paths();

        let status = match (&tool_paths, &downloads_dir) {
            (Some(paths), Some(downloads)) => format!(
                "Bereit. Tool-Ordner: {} | Download-Ordner: {}",
                paths.lib_dir.display(),
                downloads.display()
            ),
            (None, _) => "Fehler: lib/yt-dlp.exe wurde nicht gefunden.".to_owned(),
            (_, None) => "Fehler: Windows Download-Ordner konnte nicht ermittelt werden.".to_owned(),
        };

        Self {
            url: String::new(),
            mode: DownloadMode::Video,
            quality: QualityPreset::Best,
            formats: Vec::new(),
            selected_format: 0,
            status,
            logs: String::new(),
            downloads_dir,
            tool_paths,
            worker_tx,
            worker_rx,
            loading_formats: false,
            downloading: false,
        }
    }
}

impl App for RustTubeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        self.handle_worker_events();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("RustTube Downloader");
            ui.label("Fuege einen von yt-dlp unterstuetzten Link ein, waehle Format/Qualitaet und starte den Download.");
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.label("Link:");
                ui.add_sized(
                    [650.0, 24.0],
                    egui::TextEdit::singleline(&mut self.url).hint_text("https://..."),
                );
            });

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.label("Modus:");
                egui::ComboBox::from_id_salt("download_mode")
                    .selected_text(self.mode.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.mode, DownloadMode::Video, DownloadMode::Video.label());
                        ui.selectable_value(&mut self.mode, DownloadMode::AudioMp3, DownloadMode::AudioMp3.label());
                        ui.selectable_value(&mut self.mode, DownloadMode::Manual, DownloadMode::Manual.label());
                    });

                ui.label("Qualitaet:");
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
                let can_fetch = !self.loading_formats && self.can_run_commands();
                if ui.add_enabled(can_fetch, egui::Button::new("Formate laden")).clicked() {
                    self.load_formats();
                }

                let can_download = !self.downloading && self.can_start_download();
                if ui.add_enabled(can_download, egui::Button::new("Download starten")).clicked() {
                    self.start_download();
                }
            });

            if self.mode == DownloadMode::Manual {
                ui.add_space(10.0);
                ui.label("Manuelles Format:");
                if self.formats.is_empty() {
                    ui.colored_label(Color32::YELLOW, "Bitte zuerst 'Formate laden' klicken.");
                } else {
                    let selected_text = self
                        .formats
                        .get(self.selected_format)
                        .map(|entry| entry.description.clone())
                        .unwrap_or_else(|| "Kein Format".to_owned());

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

            if let Some(downloads) = &self.downloads_dir {
                ui.label(format!("Zielordner: {}", downloads.display()));
            }

            ui.add_space(10.0);
            ui.label("Ausgabe / Log:");
            ui.add(
                egui::TextEdit::multiline(&mut self.logs)
                    .desired_rows(22)
                    .desired_width(f32::INFINITY),
            );
        });

        if self.loading_formats || self.downloading {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
    }
}

impl RustTubeApp {
    fn can_run_commands(&self) -> bool {
        self.tool_paths.is_some() && self.downloads_dir.is_some() && !self.url.trim().is_empty()
    }

    fn can_start_download(&self) -> bool {
        if !self.can_run_commands() {
            return false;
        }

        self.mode != DownloadMode::Manual || !self.formats.is_empty()
    }

    fn handle_worker_events(&mut self) {
        while let Ok(event) = self.worker_rx.try_recv() {
            match event {
                WorkerEvent::FormatsLoaded { entries, raw_output } => {
                    self.loading_formats = false;
                    self.logs = raw_output;
                    if entries.is_empty() {
                        self.status = "Keine Formate erkannt. Manche Seiten liefern nur wenige oder spezielle Streams.".to_owned();
                    } else {
                        self.selected_format = 0;
                        self.status = format!("{} Formate geladen.", entries.len());
                    }
                    self.formats = entries;
                }
                WorkerEvent::DownloadFinished { success, output } => {
                    self.downloading = false;
                    self.logs = output;
                    self.status = if success {
                        "Download abgeschlossen.".to_owned()
                    } else {
                        "Download fehlgeschlagen. Details stehen im Log.".to_owned()
                    };
                }
            }
        }
    }

    fn load_formats(&mut self) {
        let Some(tool_paths) = self.tool_paths.clone() else {
            self.status = "yt-dlp.exe fehlt in lib/.".to_owned();
            return;
        };

        let url = self.url.trim().to_owned();
        if url.is_empty() {
            self.status = "Bitte zuerst einen Link eingeben.".to_owned();
            return;
        }

        self.loading_formats = true;
        self.status = "Lade verfuegbare Formate...".to_owned();
        let sender = self.worker_tx.clone();

        thread::spawn(move || {
            let result = Command::new(&tool_paths.yt_dlp_path)
                .args(tool_command_prefix(&tool_paths.lib_dir))
                .args(["-F".to_owned(), url])
                .output();
            let event = match result {
                Ok(output) => {
                    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
                    if !output.stderr.is_empty() {
                        text.push_str("\n\n");
                        text.push_str(&String::from_utf8_lossy(&output.stderr));
                    }
                    WorkerEvent::FormatsLoaded {
                        entries: parse_formats(&text),
                        raw_output: text,
                    }
                }
                Err(error) => WorkerEvent::FormatsLoaded {
                    entries: Vec::new(),
                    raw_output: format!("Fehler beim Starten von yt-dlp: {error}"),
                },
            };

            let _ = sender.send(event);
        });
    }

    fn start_download(&mut self) {
        let Some(tool_paths) = self.tool_paths.clone() else {
            self.status = "yt-dlp.exe fehlt in lib/.".to_owned();
            return;
        };
        let Some(downloads_dir) = self.downloads_dir.clone() else {
            self.status = "Download-Ordner konnte nicht gefunden werden.".to_owned();
            return;
        };

        let url = self.url.trim().to_owned();
        if url.is_empty() {
            self.status = "Bitte zuerst einen Link eingeben.".to_owned();
            return;
        }

        let mut args: Vec<String> = vec![
            "--ffmpeg-location".to_owned(),
            tool_paths.lib_dir.display().to_string(),
            "--no-playlist".to_owned(),
            "-P".to_owned(),
            downloads_dir.display().to_string(),
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
            }
            DownloadMode::Manual => {
                let Some(entry) = self.formats.get(self.selected_format) else {
                    self.status = "Bitte zuerst ein Format auswaehlen.".to_owned();
                    return;
                };
                args.push("-f".to_owned());
                args.push(entry.id.clone());
            }
        }

        args.push(url);

        self.downloading = true;
        self.status = "Download laeuft...".to_owned();
        let sender = self.worker_tx.clone();

        thread::spawn(move || {
            let result = Command::new(&tool_paths.yt_dlp_path)
                .args(tool_command_prefix(&tool_paths.lib_dir))
                .args(&args)
                .output();
            let event = match result {
                Ok(output) => {
                    let success = output.status.success();
                    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
                    if !output.stderr.is_empty() {
                        text.push_str("\n\n");
                        text.push_str(&String::from_utf8_lossy(&output.stderr));
                    }
                    WorkerEvent::DownloadFinished {
                        success,
                        output: text,
                    }
                }
                Err(error) => WorkerEvent::DownloadFinished {
                    success: false,
                    output: format!("Fehler beim Starten von yt-dlp: {error}"),
                },
            };

            let _ = sender.send(event);
        });
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
        QualityPreset::Best => "bv*+ba/b",
        QualityPreset::P1080 => "bv*[height<=1080]+ba/b[height<=1080]",
        QualityPreset::P720 => "bv*[height<=720]+ba/b[height<=720]",
        QualityPreset::P480 => "bv*[height<=480]+ba/b[height<=480]",
        QualityPreset::Worst => "wv*+wa/w",
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

fn tool_command_prefix(lib_dir: &Path) -> Vec<String> {
    let mut args = vec!["--ffmpeg-location".to_owned(), lib_dir.display().to_string()];

    let deno_path = lib_dir.join("deno.exe");
    if deno_path.is_file() {
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
