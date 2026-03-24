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

function Find-GitHubCli {
    $ghCommand = Get-Command gh.exe -ErrorAction SilentlyContinue
    if (-not $ghCommand) {
        $ghCommand = Get-Command gh -ErrorAction SilentlyContinue
    }

    if ($ghCommand) {
        return $ghCommand.Source
    }

    $candidates = @(
        "$env:ProgramFiles\GitHub CLI\gh.exe",
        "${env:ProgramFiles(x86)}\GitHub CLI\gh.exe",
        "$env:LocalAppData\Programs\GitHub CLI\gh.exe"
    )

    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path $candidate)) {
            return $candidate
        }
    }

    throw "GitHub CLI was not found. Install gh and log in before creating releases."
}

function Maybe-PublishGitHubRelease {
    $appVersion = Read-CargoVersion
    $tagName = "v$appVersion"
    $installerPath = Join-Path $root "dist\installer\RustTube-Setup.exe"

    if (-not (Test-Path $installerPath)) {
        throw "Installer file was not found at $installerPath"
    }

    $answer = Read-Host "Create GitHub release $tagName and upload RustTube-Setup.exe? (y/n)"
    if ($answer -notmatch '^(y|yes)$') {
        Write-Host ""
        Write-Host "Skipped GitHub release creation."
        return
    }

    $gh = Find-GitHubCli

    Write-Host ""
    Write-Host "Publishing GitHub release $tagName..."
    $releaseExists = $false
    try {
        & $gh release view $tagName 2>$null 1>$null
        $releaseExists = ($LASTEXITCODE -eq 0)
    }
    catch {
        $releaseExists = $false
    }

    if ($releaseExists) {
        & $gh release upload $tagName $installerPath --clobber
    }
    else {
        & $gh release create $tagName $installerPath --title $tagName --generate-notes
    }

    if ($LASTEXITCODE -ne 0) {
        throw "GitHub release upload failed."
    }

    Write-Host "GitHub release published successfully."
}

function Test-GitHubCliSetup {
    $gh = Find-GitHubCli

    Write-Host ""
    Write-Host "GitHub CLI found at:"
    Write-Host "  $gh"
    Write-Host ""
    Write-Host "Checking GitHub authentication status..."

    & $gh auth status
    if ($LASTEXITCODE -ne 0) {
        throw "GitHub CLI is installed, but authentication failed. Run 'gh auth login'."
    }

    Write-Host ""
    Write-Host "GitHub CLI is ready."
}

if ([string]::IsNullOrWhiteSpace($Mode)) {
    Write-Host ""
    Write-Host "Select an action:"
    Write-Host "  0. Check GitHub CLI"
    Write-Host "  1. Build app package"
    Write-Host "  2. Build app package + installer"
    Write-Host "  3. Build installer only"
    Write-Host ""
    $Mode = Read-Host "Enter 0, 1, 2 or 3"
}

if ($Mode -notin @("0", "1", "2", "3")) {
    throw "Invalid selection. Please run again and choose 0, 1, 2 or 3."
}

if ($Mode -eq "0") {
    Test-GitHubCliSetup
    exit 0
}

if ($Mode -eq "3") {
    Build-Installer
    Write-Host ""
    Write-Host "Installer finished successfully."
    Write-Host "Installer output should be in dist\installer"
    Maybe-PublishGitHubRelease
}
else {
    Update-CargoVersion
    Build-AppPackage
}

if ($Mode -eq "2") {
    Build-Installer
    Write-Host ""
    Write-Host "Installer build finished successfully."
    Write-Host "Installer output should be in dist\installer"
    Maybe-PublishGitHubRelease
}
elseif ($Mode -eq "1") {
    Write-Host ""
    Write-Host "Build finished successfully."
}
