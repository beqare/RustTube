$ErrorActionPreference = "Stop"

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$libDir = Join-Path $root "lib"
$tempDir = Join-Path $root ".tmp-runtime-tools"

New-Item -ItemType Directory -Force -Path $libDir | Out-Null
New-Item -ItemType Directory -Force -Path $tempDir | Out-Null

function Download-File {
    param(
        [Parameter(Mandatory = $true)][string]$Url,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    Write-Host "Downloading $Url"
    Invoke-WebRequest -Uri $Url -OutFile $Destination
}

function Copy-IfExists {
    param(
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    if (-not (Test-Path $Source)) {
        throw "Expected file not found: $Source"
    }

    Copy-Item -LiteralPath $Source -Destination $Destination -Force
}

$ytDlpPath = Join-Path $libDir "yt-dlp.exe"
if (-not (Test-Path $ytDlpPath)) {
    Download-File `
        -Url "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" `
        -Destination $ytDlpPath
}

$denoPath = Join-Path $libDir "deno.exe"
if (-not (Test-Path $denoPath)) {
    $denoZip = Join-Path $tempDir "deno.zip"
    $denoExtractDir = Join-Path $tempDir "deno"
    if (Test-Path $denoExtractDir) {
        Remove-Item -LiteralPath $denoExtractDir -Recurse -Force
    }

    Download-File `
        -Url "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip" `
        -Destination $denoZip

    Expand-Archive -LiteralPath $denoZip -DestinationPath $denoExtractDir -Force
    Copy-IfExists -Source (Join-Path $denoExtractDir "deno.exe") -Destination $denoPath
}

$ffmpegPath = Join-Path $libDir "ffmpeg.exe"
$ffprobePath = Join-Path $libDir "ffprobe.exe"
if (-not (Test-Path $ffmpegPath) -or -not (Test-Path $ffprobePath)) {
    $ffmpegZip = Join-Path $tempDir "ffmpeg.zip"
    $ffmpegExtractDir = Join-Path $tempDir "ffmpeg"
    if (Test-Path $ffmpegExtractDir) {
        Remove-Item -LiteralPath $ffmpegExtractDir -Recurse -Force
    }

    Download-File `
        -Url "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" `
        -Destination $ffmpegZip

    Expand-Archive -LiteralPath $ffmpegZip -DestinationPath $ffmpegExtractDir -Force

    $ffmpegExe = Get-ChildItem -Path $ffmpegExtractDir -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1
    $ffprobeExe = Get-ChildItem -Path $ffmpegExtractDir -Recurse -Filter "ffprobe.exe" | Select-Object -First 1

    if (-not $ffmpegExe -or -not $ffprobeExe) {
        throw "Could not find ffmpeg.exe and ffprobe.exe in extracted FFmpeg archive."
    }

    Copy-Item -LiteralPath $ffmpegExe.FullName -Destination $ffmpegPath -Force
    Copy-Item -LiteralPath $ffprobeExe.FullName -Destination $ffprobePath -Force
}

Write-Host ""
Write-Host "Runtime tools are ready in $libDir"
