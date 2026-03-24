$ErrorActionPreference = "Stop"

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$releaseDir = Join-Path $root "target\release"
$distDir = Join-Path $root "dist\RustTube"
$exeName = "RustTube.exe"

cargo build --release
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

if (Test-Path $distDir) {
    Remove-Item -LiteralPath $distDir -Recurse -Force
}

New-Item -ItemType Directory -Path $distDir | Out-Null

Copy-Item -LiteralPath (Join-Path $releaseDir $exeName) -Destination $distDir

Write-Host ""
Write-Host "Packaged runtime folder:" $distDir
