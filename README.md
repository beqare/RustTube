# RustTube

RustTube is a simple Windows desktop GUI for `yt-dlp`, written in Rust.

It is designed to make downloading videos and audio easier for users who do not want to work with command-line tools directly. You can paste a supported URL, preview the media, choose a format and quality, and download it with a few clicks.

## Features

- Simple Windows GUI built with Rust
- Supports URLs handled by `yt-dlp`
- Video download mode
- MP3 audio download mode
- Manual format selection mode
- Media preview with thumbnail, title, creator, and duration
- Live output log
- Download progress display
- Cancel running downloads
- Select custom target folder
- Tool management inside the app
- Optional installer build with Inno Setup

## How It Works

RustTube uses `yt-dlp` for extraction and downloading.

Depending on what you want to download, it can also use:

- `ffmpeg`
- `ffprobe`
- `deno`

The app can download these tools into its own tools folder when needed.

## Project Structure

```text
RustTube/
  assets/
    icon.ico
  scripts/
    build.bat
    build.ps1
    setup.iss
  src/
    app_model.rs
    icon.rs
    main.rs
    preview.rs
    process_utils.rs
    progress.rs
    runtime_tools.rs
    settings.rs
  build.rs
  Cargo.toml
```

## Requirements

- Windows
- Rust toolchain
- Cargo

Optional:

- Inno Setup 6, if you want to build the installer
- GitHub CLI (`gh`), if you want to create GitHub releases from the build script

## Running in Development

Start the app locally with:

```powershell
cargo run
```

## Build Script

The project includes a PowerShell-based build script:

```powershell
.\scripts\build.ps1
```

or:

```bat
scripts\build.bat
```

### Build Options

- `0` = Check GitHub CLI
- `1` = Build app package
- `2` = Build app package + installer
- `3` = Build installer only
- `4` = Build portable package

When using build mode `1`, `2`, or `4`, the script automatically increases the patch version in `Cargo.toml`.

## Installer

The installer is built with Inno Setup and installs RustTube into:

```text
AppData\Roaming\jonasgrimm.de\RustTube
```

The uninstaller is configured to remove the `RustTube` folder again on uninstall.

## Tool Storage

RustTube stores its downloaded helper tools in:

```text
AppData\Roaming\jonasgrimm.de\RustTube\tools
```

The app can also open the tool folder, settings folder, and program folder directly from the UI.

## GitHub Releases

If GitHub CLI is installed and authenticated, the build script can optionally:

- create a GitHub release
- upload `RustTube-Setup.exe`

This is available after an installer build.

## Notes

- RustTube is a GUI frontend for `yt-dlp`; it does not replace `yt-dlp`
- Download support depends on what `yt-dlp` currently supports
- Some websites may require additional external runtimes or may change over time

## License

This project is licensed under the MIT License.
