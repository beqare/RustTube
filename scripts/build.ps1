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

    Set-CargoVersion -Version $newVersion
    Write-Host "New version: $newVersion"
    return @{
        OldVersion = $match.Groups[1].Value + "." + $match.Groups[2].Value + "." + $match.Groups[3].Value
        NewVersion = $newVersion
    }
}

function Set-CargoVersion {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Version
    )

    $cargoTomlPath = Join-Path $root "Cargo.toml"
    $content = Get-Content -LiteralPath $cargoTomlPath -Raw
    $updated = [regex]::Replace(
        $content,
        '(?m)^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"',
        ('version = "' + $Version + '"'),
        1
    )

    Set-Content -LiteralPath $cargoTomlPath -Value $updated -Encoding UTF8
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
        throw "cargo build --release failed with exit code $LASTEXITCODE"
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

function Build-ReleaseZip {
    $appExePath = Join-Path $root "dist\RustTube\RustTube.exe"
    $installerPath = Join-Path $root "dist\installer\RustTube-Setup.exe"
    $zipPath = Join-Path $root "dist\RustTube.zip"
    $stagingDir = Join-Path $root "dist\zip-staging"

    if (-not (Test-Path $appExePath)) {
        throw "App file was not found at $appExePath"
    }

    if (-not (Test-Path $installerPath)) {
        throw "Installer file was not found at $installerPath"
    }

    if (Test-Path $stagingDir) {
        Remove-Item -LiteralPath $stagingDir -Recurse -Force
    }

    New-Item -ItemType Directory -Path $stagingDir | Out-Null
    Copy-Item -LiteralPath $appExePath -Destination (Join-Path $stagingDir "RustTube.exe")
    Copy-Item -LiteralPath $installerPath -Destination (Join-Path $stagingDir "RustTube-Setup.exe")

    if (Test-Path $zipPath) {
        Remove-Item -LiteralPath $zipPath -Force
    }

    Compress-Archive -Path (Join-Path $stagingDir "*") -DestinationPath $zipPath -Force
    Remove-Item -LiteralPath $stagingDir -Recurse -Force

    Write-Host ""
    Write-Host "Release zip created:" $zipPath
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

function Publish-GitHubReleaseIfRequested {
    param(
        [Parameter(Mandatory = $false)]
        [bool]$IncludeAppExe = $false,
        [Parameter(Mandatory = $false)]
        [bool]$IncludeZip = $false
    )

    $appVersion = Read-CargoVersion
    $tagName = "v$appVersion"
    $appExePath = Join-Path $root "dist\RustTube\RustTube.exe"
    $installerPath = Join-Path $root "dist\installer\RustTube-Setup.exe"
    $zipPath = Join-Path $root "dist\RustTube.zip"
    $releaseAssets = @()

    if (-not (Test-Path $installerPath)) {
        throw "Installer file was not found at $installerPath"
    }

    $releaseAssets += $installerPath

    if ($IncludeAppExe) {
        if (-not (Test-Path $appExePath)) {
            throw "App file was not found at $appExePath"
        }

        $releaseAssets += $appExePath
    }

    if ($IncludeZip) {
        if (-not (Test-Path $zipPath)) {
            throw "Zip file was not found at $zipPath"
        }

        $releaseAssets += $zipPath
    }

    $assetNames = ($releaseAssets | ForEach-Object { Split-Path $_ -Leaf }) -join ", "
    $answer = Read-Host "Create GitHub release $tagName and upload $assetNames? (y/n)"
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
        & $gh release upload $tagName @releaseAssets --clobber
    }
    else {
        & $gh release create $tagName @releaseAssets --title $tagName --generate-notes
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
    Write-Host "What do you want to build?"
    Write-Host "  0. Check GitHub release login"
    Write-Host "  1. Build the app"
    Write-Host "  2. Build the app and setup"
    Write-Host "  3. Build only the setup"
    Write-Host "  4. Build app, setup and upload both files"
    Write-Host "  5. Build app, setup, zip and upload all files"
    Write-Host ""
    $Mode = Read-Host "Choose 0, 1, 2, 3, 4 or 5"
}

if ($Mode -notin @("0", "1", "2", "3", "4", "5")) {
    throw "Invalid selection. Please run again and choose 0, 1, 2, 3, 4 or 5."
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
    Publish-GitHubReleaseIfRequested
}
else {
    $versionUpdate = Update-CargoVersion
    try {
        Build-AppPackage
    }
    catch {
        Write-Host ""
        Write-Host "Build failed. Restoring Cargo.toml version to $($versionUpdate.OldVersion)..."
        Set-CargoVersion -Version $versionUpdate.OldVersion
        throw
    }
}

if ($Mode -eq "2" -or $Mode -eq "4" -or $Mode -eq "5") {
    Build-Installer
    Write-Host ""
    Write-Host "Installer build finished successfully."
    Write-Host "Installer output should be in dist\installer"

    if ($Mode -eq "5") {
        Build-ReleaseZip
    }

    Publish-GitHubReleaseIfRequested -IncludeAppExe ($Mode -in @("4", "5")) -IncludeZip ($Mode -eq "5")
    if ($Mode -eq "4" -or $Mode -eq "5") {
        Write-Host ""
        Write-Host "App and setup build finished successfully."
    }
}
elseif ($Mode -eq "1") {
    Write-Host ""
    Write-Host "Build finished successfully."
}
