$ErrorActionPreference = "Stop"

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$releaseDir = Join-Path $root "target\release"
$distDir = Join-Path $root "dist\RustTube"
$libDir = Join-Path $releaseDir "lib"
$exeName = "RustTube.exe"

cargo build --release
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

if (Test-Path $distDir) {
    Remove-Item -LiteralPath $distDir -Recurse -Force
}

New-Item -ItemType Directory -Path $distDir | Out-Null
New-Item -ItemType Directory -Path (Join-Path $distDir "lib") | Out-Null

Copy-Item -LiteralPath (Join-Path $releaseDir $exeName) -Destination $distDir

$runtimeFiles = @("yt-dlp.exe", "ffmpeg.exe", "ffprobe.exe", "deno.exe")
foreach ($file in $runtimeFiles) {
    $source = Join-Path $libDir $file
    if (Test-Path $source) {
        Copy-Item -LiteralPath $source -Destination (Join-Path $distDir "lib")
    }
}

Write-Host ""
Write-Host "Packaged runtime folder:" $distDir
