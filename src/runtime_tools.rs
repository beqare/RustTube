use std::{
    fs::{self, File},
    io::{self, copy},
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use reqwest::blocking::Client;
use zip::ZipArchive;

use crate::app_model::{ToolPaths, WorkerEvent};

const YT_DLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
const DENO_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";
const FFMPEG_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";

pub fn resolve_lib_dir() -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        let candidates = [
            exe_dir.join("lib"),
            exe_dir.join("..").join("lib"),
            exe_dir.join("..").join("..").join("lib"),
        ];

        if let Some(existing) = candidates.into_iter().find(|path| tool_paths_if_ready(path).is_some()) {
            return existing;
        }
    }

    if let Some(project_dirs) = ProjectDirs::from("de", "RustTube", "RustTube") {
        return project_dirs.data_local_dir().join("lib");
    }

    if let Ok(current_dir) = std::env::current_dir() {
        return current_dir.join("lib");
    }

    PathBuf::from("lib")
}

pub fn tool_paths_if_ready(lib_dir: &Path) -> Option<ToolPaths> {
    let yt_dlp_path = lib_dir.join("yt-dlp.exe");
    let ffmpeg_path = lib_dir.join("ffmpeg.exe");
    let ffprobe_path = lib_dir.join("ffprobe.exe");
    let deno_path = lib_dir.join("deno.exe");

    (yt_dlp_path.is_file() && ffmpeg_path.is_file() && ffprobe_path.is_file() && deno_path.is_file()).then_some(
        ToolPaths {
            lib_dir: lib_dir.to_path_buf(),
            yt_dlp_path,
        },
    )
}

pub fn missing_tools(lib_dir: &Path) -> Vec<&'static str> {
    let mut missing = Vec::new();

    if !lib_dir.join("yt-dlp.exe").is_file() {
        missing.push("yt-dlp.exe");
    }
    if !lib_dir.join("ffmpeg.exe").is_file() {
        missing.push("ffmpeg.exe");
    }
    if !lib_dir.join("ffprobe.exe").is_file() {
        missing.push("ffprobe.exe");
    }
    if !lib_dir.join("deno.exe").is_file() {
        missing.push("deno.exe");
    }

    missing
}

pub fn ensure_runtime_tools(sender: &std::sync::mpsc::Sender<WorkerEvent>, lib_dir: &Path) -> Result<ToolPaths, String> {
    fs::create_dir_all(lib_dir).map_err(|error| format!("failed to create lib directory: {error}"))?;

    let missing = missing_tools(lib_dir);
    if missing.is_empty() {
        return tool_paths_if_ready(lib_dir).ok_or_else(|| "required tools are still unavailable".to_owned());
    }

    let client = Client::builder()
        .user_agent("RustTube/0.1")
        .build()
        .map_err(|error| format!("failed to create download client: {error}"))?;

    let _ = sender.send(WorkerEvent::LogChunk(format!(
        "Downloading required tools to {}...\n",
        lib_dir.display()
    )));

    if missing.contains(&"yt-dlp.exe") {
        let _ = sender.send(WorkerEvent::LogChunk("Downloading yt-dlp.exe...\n".to_owned()));
        download_to_file(&client, YT_DLP_URL, &lib_dir.join("yt-dlp.exe"))?;
    }

    if missing.contains(&"deno.exe") {
        let _ = sender.send(WorkerEvent::LogChunk("Downloading deno.exe...\n".to_owned()));
        let zip_path = lib_dir.join("deno.download.zip");
        download_to_file(&client, DENO_URL, &zip_path)?;
        extract_zip_entry(&zip_path, &lib_dir.join("deno.exe"), |name| {
            name.eq_ignore_ascii_case("deno.exe")
        })?;
        remove_if_exists(&zip_path);
    }

    if missing.contains(&"ffmpeg.exe") || missing.contains(&"ffprobe.exe") {
        let _ = sender.send(WorkerEvent::LogChunk("Downloading ffmpeg and ffprobe...\n".to_owned()));
        let zip_path = lib_dir.join("ffmpeg.download.zip");
        download_to_file(&client, FFMPEG_URL, &zip_path)?;
        extract_zip_entry(&zip_path, &lib_dir.join("ffmpeg.exe"), |name| {
            name.eq_ignore_ascii_case("ffmpeg.exe")
        })?;
        extract_zip_entry(&zip_path, &lib_dir.join("ffprobe.exe"), |name| {
            name.eq_ignore_ascii_case("ffprobe.exe")
        })?;
        remove_if_exists(&zip_path);
    }

    let _ = sender.send(WorkerEvent::LogChunk("Required tools are ready.\n".to_owned()));

    tool_paths_if_ready(lib_dir).ok_or_else(|| "required tools could not be prepared".to_owned())
}

fn download_to_file(client: &Client, url: &str, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("failed to create download directory: {error}"))?;
    }

    let mut response = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("failed to download {url}: {error}"))?;

    let mut file =
        File::create(destination).map_err(|error| format!("failed to create {}: {error}", destination.display()))?;

    copy(&mut response, &mut file)
        .map_err(|error| format!("failed to write {}: {error}", destination.display()))?;

    Ok(())
}

fn extract_zip_entry<F>(zip_path: &Path, destination: &Path, predicate: F) -> Result<(), String>
where
    F: Fn(&str) -> bool,
{
    let file = File::open(zip_path).map_err(|error| format!("failed to open {}: {error}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("failed to read zip archive {}: {error}", zip_path.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("failed to inspect zip entry: {error}"))?;
        let Some(name) = Path::new(entry.name()).file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if !predicate(name) {
            continue;
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create destination directory: {error}"))?;
        }

        let mut output =
            File::create(destination).map_err(|error| format!("failed to create {}: {error}", destination.display()))?;
        io::copy(&mut entry, &mut output)
            .map_err(|error| format!("failed to extract {}: {error}", destination.display()))?;
        return Ok(());
    }

    Err(format!(
        "expected file was not found inside {}",
        zip_path.display()
    ))
}

fn remove_if_exists(path: &Path) {
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}
