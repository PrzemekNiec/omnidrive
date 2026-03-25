[CmdletBinding()]
param(
    [string]$Configuration = "release",
    [string]$IsccPath = $env:ISCC_PATH
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$installerDir = Join-Path $repoRoot "installer"
$issPath = Join-Path $installerDir "omnidrive.iss"
$distRoot = Join-Path $repoRoot "dist\installer"
$payloadDir = Join-Path $distRoot "payload"
$outputDir = Join-Path $distRoot "output"
$targetDir = Join-Path $repoRoot "target\$Configuration"
$angeldExe = Join-Path $targetDir "angeld.exe"
$cliExe = Join-Path $targetDir "omnidrive.exe"
$staticDir = Join-Path $repoRoot "angeld\static"
$iconsDir = Join-Path $repoRoot "icons"
$angeldManifest = Join-Path $repoRoot "angeld\Cargo.toml"

function Resolve-InnoSetupCompiler {
    param([string]$PreferredPath)

    $candidates = @()
    if ($PreferredPath) {
        $candidates += $PreferredPath
    }

    $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($command) {
        $candidates += $command.Source
    }

    $candidates += @(
        "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
        "C:\Program Files\Inno Setup 6\ISCC.exe"
    )

    foreach ($candidate in $candidates | Select-Object -Unique) {
        if ($candidate -and (Test-Path $candidate)) {
            return (Resolve-Path $candidate).Path
        }
    }

    throw "ISCC.exe not found. Install Inno Setup 6 or set ISCC_PATH."
}

function Get-AppVersion {
    param([string]$ManifestPath)

    $content = Get-Content -Path $ManifestPath -Raw
    if ($content -match '(?m)^version\s*=\s*"([^"]+)"') {
        return $Matches[1]
    }

    throw "Could not determine OmniDrive version from $ManifestPath"
}

function Reset-Directory {
    param([string]$Path)

    if (Test-Path $Path) {
        Remove-Item -Recurse -Force $Path
    }
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
}

function Copy-DirectoryContents {
    param(
        [string]$Source,
        [string]$Destination
    )

    if (-not (Test-Path $Source)) {
        throw "Required source directory is missing: $Source"
    }

    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    Copy-Item -Path (Join-Path $Source '*') -Destination $Destination -Recurse -Force
}

$version = Get-AppVersion -ManifestPath $angeldManifest
$iscc = Resolve-InnoSetupCompiler -PreferredPath $IsccPath

Write-Host "Building OmniDrive binaries ($Configuration)..."
Push-Location $repoRoot
try {
    & cargo build --$Configuration -p angeld -p omnidrive-cli
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }
}
finally {
    Pop-Location
}

if (-not (Test-Path $angeldExe)) {
    throw "Missing build output: $angeldExe"
}

if (-not (Test-Path $cliExe)) {
    throw "Missing build output: $cliExe"
}

Reset-Directory -Path $payloadDir
New-Item -ItemType Directory -Force -Path $outputDir | Out-Null

Copy-Item -Path $angeldExe -Destination (Join-Path $payloadDir "angeld.exe") -Force
Copy-Item -Path $cliExe -Destination (Join-Path $payloadDir "omnidrive.exe") -Force
Copy-DirectoryContents -Source $staticDir -Destination (Join-Path $payloadDir "static")
Copy-DirectoryContents -Source $iconsDir -Destination (Join-Path $payloadDir "icons")

Write-Host "Compiling installer with Inno Setup..."
& $iscc "/DAppVersion=$version" "/DPayloadDir=$payloadDir" "/DOutputDir=$outputDir" $issPath
if ($LASTEXITCODE -ne 0) {
    throw "ISCC.exe failed with exit code $LASTEXITCODE"
}

Write-Host "Installer ready in $outputDir"
