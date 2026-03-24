use std::process::Command;

use image::GenericImageView;
use serde_json::Value;

use crate::{
    app_model::{MediaPreview, ToolPaths, tool_command_prefix},
    process_utils::configure_background_command,
};

pub fn fetch_media_preview(tool_paths: &ToolPaths, url: &str) -> Result<MediaPreview, String> {
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
