param(
    [string]$Mode
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

function Update-CargoVersion {
    Write-Host ""
    Write-Host "Bumping Cargo.toml version..."

    $cargoTomlPath = Join-Path $root "Cargo.toml"
    $content = Get-Content -LiteralPath $cargoTomlPath -Raw
    $match = [regex]::Match($content, '(?m)^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"')

    if (-not $match.Success) {
        throw "Could not find semver version in Cargo.toml"
    }

    $major = [int]$match.Groups[1].Value
    $minor = [int]$match.Groups[2].Value
    $patch = [int]$match.Groups[3].Value + 1
    $newVersion = "$major.$minor.$patch"

    $updated = [regex]::Replace(
        $content,
        '(?m)^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"',
        ('version = "' + $newVersion + '"'),
        1
    )

    Set-Content -LiteralPath $cargoTomlPath -Value $updated -Encoding UTF8
    Write-Host "New version: $newVersion"
}

function Read-CargoVersion {
    $cargoTomlPath = Join-Path $root "Cargo.toml"
    $content = Get-Content -LiteralPath $cargoTomlPath -Raw
    $match = [regex]::Match($content, '(?m)^version\s*=\s*"([^"]+)"')

    if (-not $match.Success) {
        throw "Could not read version from Cargo.toml"
    }

    return $match.Groups[1].Value
}

function Build-AppPackage {
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
}

function Find-InnoSetupCompiler {
    $candidates = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
    )

    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path $candidate)) {
            return $candidate
        }
    }

    $pathCommand = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($pathCommand) {
        return $pathCommand.Source
    }

    throw "Inno Setup Compiler not found. Install Inno Setup 6 or add ISCC.exe to PATH."
}

function Build-Installer {
    $iscc = Find-InnoSetupCompiler
    $appVersion = Read-CargoVersion

    Write-Host ""
    Write-Host "Running Inno Setup compiler..."
    & $iscc "/DMyAppVersion=$appVersion" ".\scripts\setup.iss"
    if ($LASTEXITCODE -ne 0) {
        throw "Installer build failed."
    }
}

if ([string]::IsNullOrWhiteSpace($Mode)) {
    Write-Host ""
    Write-Host "Select an action:"
    Write-Host "  1. Build app package"
    Write-Host "  2. Build app package + installer"
    Write-Host "  3. Build installer only"
    Write-Host ""
    $Mode = Read-Host "Enter 1, 2 or 3"
}

if ($Mode -notin @("1", "2", "3")) {
    throw "Invalid selection. Please run again and choose 1, 2 or 3."
}

if ($Mode -eq "3") {
    Build-Installer
    Write-Host ""
    Write-Host "Installer finished successfully."
    Write-Host "Installer output should be in dist\installer"
}
else {
    Update-CargoVersion
    Build-AppPackage
}

if ($Mode -eq "2") {
    Build-Installer
    Write-Host ""
    Write-Host "Release finished successfully."
    Write-Host "Installer output should be in dist\installer"
}
elseif ($Mode -eq "1") {
    Write-Host ""
    Write-Host "Build finished successfully."
}
