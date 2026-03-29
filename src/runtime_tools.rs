use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::Sender,
};

use reqwest::blocking::Client;
use zip::ZipArchive;

use crate::app_model::{RuntimeTool, ToolPackage, ToolPaths, WorkerEvent};

const YT_DLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
const DENO_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";
const FFMPEG_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";

pub fn resolve_lib_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(&appdata)
            .join("jonasgrimm.de")
            .join("RustTube")
            .join("tools");
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

pub fn tool_installed(lib_dir: &Path, tool: RuntimeTool) -> bool {
    lib_dir.join(tool.file_name()).is_file()
}

pub fn package_for_tool(tool: RuntimeTool) -> ToolPackage {
    match tool {
        RuntimeTool::YtDlp => ToolPackage::YtDlp,
        RuntimeTool::Ffmpeg | RuntimeTool::Ffprobe => ToolPackage::FfmpegBundle,
        RuntimeTool::Deno => ToolPackage::Deno,
    }
}

pub fn missing_packages(lib_dir: &Path) -> Vec<ToolPackage> {
    let mut packages = Vec::new();

    if !tool_installed(lib_dir, RuntimeTool::YtDlp) {
        packages.push(ToolPackage::YtDlp);
    }
    if !tool_installed(lib_dir, RuntimeTool::Ffmpeg) || !tool_installed(lib_dir, RuntimeTool::Ffprobe) {
        packages.push(ToolPackage::FfmpegBundle);
    }
    if !tool_installed(lib_dir, RuntimeTool::Deno) {
        packages.push(ToolPackage::Deno);
    }

    packages
}

pub fn download_package(sender: &Sender<WorkerEvent>, lib_dir: &Path, package: ToolPackage) -> Result<(), String> {
    fs::create_dir_all(lib_dir).map_err(|error| format!("failed to create lib directory: {error}"))?;

    let client = Client::builder()
        .user_agent("RustTube/0.1")
        .build()
        .map_err(|error| format!("failed to create download client: {error}"))?;

    match package {
        ToolPackage::YtDlp => {
            let target = lib_dir.join("yt-dlp.exe");
            download_to_file(&client, sender, package, YT_DLP_URL, &target)?;
        }
        ToolPackage::Deno => {
            let zip_path = lib_dir.join("deno.download.zip");
            download_to_file(&client, sender, package, DENO_URL, &zip_path)?;
            extract_zip_entry(&zip_path, &lib_dir.join("deno.exe"), |name| {
                name.eq_ignore_ascii_case("deno.exe")
            })?;
            remove_if_exists(&zip_path);
        }
        ToolPackage::FfmpegBundle => {
            let zip_path = lib_dir.join("ffmpeg.download.zip");
            download_to_file(&client, sender, package, FFMPEG_URL, &zip_path)?;
            extract_zip_entry(&zip_path, &lib_dir.join("ffmpeg.exe"), |name| {
                name.eq_ignore_ascii_case("ffmpeg.exe")
            })?;
            extract_zip_entry(&zip_path, &lib_dir.join("ffprobe.exe"), |name| {
                name.eq_ignore_ascii_case("ffprobe.exe")
            })?;
            remove_if_exists(&zip_path);
        }
    }

    Ok(())
}

fn download_to_file(
    client: &Client,
    sender: &Sender<WorkerEvent>,
    package: ToolPackage,
    url: &str,
    destination: &Path,
) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("failed to create download directory: {error}"))?;
    }

    let mut response = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("failed to download {url}: {error}"))?;

    let total_bytes = response.content_length();
    let mut downloaded_bytes = 0_u64;
    let mut file =
        File::create(destination).map_err(|error| format!("failed to create {}: {error}", destination.display()))?;
    let mut buffer = [0_u8; 16 * 1024];

    loop {
        let bytes_read = response
            .read(&mut buffer)
            .map_err(|error| format!("failed to read {url}: {error}"))?;

        if bytes_read == 0 {
            break;
        }

        file.write_all(&buffer[..bytes_read])
            .map_err(|error| format!("failed to write {}: {error}", destination.display()))?;
        downloaded_bytes += bytes_read as u64;

        let _ = sender.send(WorkerEvent::ToolDownloadProgress {
            package,
            downloaded_bytes,
            total_bytes,
        });
    }

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
