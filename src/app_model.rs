use std::path::PathBuf;

#[derive(Clone, PartialEq, Eq)]
pub enum DownloadMode {
    Video,
    AudioMp3,
    Manual,
}

impl DownloadMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Video => "Video",
            Self::AudioMp3 => "Audio (MP3)",
            Self::Manual => "Manual yt-dlp format",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum QualityPreset {
    Best,
    P1080,
    P720,
    P480,
    Worst,
}

impl QualityPreset {
    pub fn label(&self) -> &'static str {
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
pub struct FormatEntry {
    pub id: String,
    pub description: String,
}

#[derive(Clone)]
pub struct ToolPaths {
    pub lib_dir: PathBuf,
    pub yt_dlp_path: PathBuf,
}

#[derive(Clone)]
pub struct MediaPreview {
    pub title: String,
    pub uploader: String,
    pub duration: Option<String>,
    pub webpage_url: String,
    pub thumbnail_url: Option<String>,
    pub thumbnail_rgba: Option<(Vec<u8>, [usize; 2])>,
}

pub enum WorkerEvent {
    LogChunk(String),
    ToolsReady {
        result: Result<ToolPaths, String>,
    },
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
        canceled: bool,
    },
}

pub fn parse_formats(raw: &str) -> Vec<FormatEntry> {
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

pub fn video_selector(quality: &QualityPreset) -> &'static str {
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

pub fn audio_quality(quality: &QualityPreset) -> &'static str {
    match quality {
        QualityPreset::Best => "0",
        QualityPreset::P1080 => "2",
        QualityPreset::P720 => "4",
        QualityPreset::P480 => "6",
        QualityPreset::Worst => "9",
    }
}

pub fn tool_command_prefix(lib_dir: &std::path::Path) -> Vec<String> {
    let mut args = vec!["--ffmpeg-location".to_owned(), lib_dir.display().to_string()];

    let deno_path = find_deno_in_lib(lib_dir);
    if let Some(deno_path) = deno_path {
        args.push("--js-runtimes".to_owned());
        args.push(format!("deno:{}", deno_path.display()));
    }

    args
}

pub fn find_deno_in_lib(lib_dir: &std::path::Path) -> Option<PathBuf> {
    let candidates = [
        lib_dir.join("deno.exe"),
        lib_dir.join("bin").join("deno.exe"),
        lib_dir.join("deno").join("bin").join("deno.exe"),
    ];

    candidates.into_iter().find(|path| path.is_file())
}
