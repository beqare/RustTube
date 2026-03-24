use std::{
    fs,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::app_model::{DownloadMode, QualityPreset};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    pub download_path: String,
    pub mode: SavedDownloadMode,
    pub quality: SavedQualityPreset,
    pub last_url: String,
}

impl AppSettings {
    pub fn from_runtime(download_path: String, mode: &DownloadMode, quality: &QualityPreset, last_url: String) -> Self {
        Self {
            download_path,
            mode: SavedDownloadMode::from(mode),
            quality: SavedQualityPreset::from(quality),
            last_url,
        }
    }

    pub fn apply_to_runtime(&self) -> (String, DownloadMode, QualityPreset, String) {
        (
            self.download_path.clone(),
            self.mode.to_runtime(),
            self.quality.to_runtime(),
            self.last_url.clone(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedDownloadMode {
    Video,
    AudioMp3,
    Manual,
}

impl SavedDownloadMode {
    fn to_runtime(&self) -> DownloadMode {
        match self {
            Self::Video => DownloadMode::Video,
            Self::AudioMp3 => DownloadMode::AudioMp3,
            Self::Manual => DownloadMode::Manual,
        }
    }
}

impl From<&DownloadMode> for SavedDownloadMode {
    fn from(value: &DownloadMode) -> Self {
        match value {
            DownloadMode::Video => Self::Video,
            DownloadMode::AudioMp3 => Self::AudioMp3,
            DownloadMode::Manual => Self::Manual,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedQualityPreset {
    Best,
    P1080,
    P720,
    P480,
    Worst,
}

impl SavedQualityPreset {
    fn to_runtime(&self) -> QualityPreset {
        match self {
            Self::Best => QualityPreset::Best,
            Self::P1080 => QualityPreset::P1080,
            Self::P720 => QualityPreset::P720,
            Self::P480 => QualityPreset::P480,
            Self::Worst => QualityPreset::Worst,
        }
    }
}

impl From<&QualityPreset> for SavedQualityPreset {
    fn from(value: &QualityPreset) -> Self {
        match value {
            QualityPreset::Best => Self::Best,
            QualityPreset::P1080 => Self::P1080,
            QualityPreset::P720 => Self::P720,
            QualityPreset::P480 => Self::P480,
            QualityPreset::Worst => Self::Worst,
        }
    }
}

pub fn load_settings() -> Option<AppSettings> {
    let path = settings_file_path()?;
    let contents = fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let path = settings_file_path().ok_or_else(|| "Could not determine settings path".to_owned())?;
    ensure_parent_dir(&path)?;
    let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())
}

fn settings_file_path() -> Option<PathBuf> {
    let project_dirs = ProjectDirs::from("de", "JonasGrimm", "RustTube")?;
    Some(project_dirs.config_dir().join("settings.json"))
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    Ok(())
}
