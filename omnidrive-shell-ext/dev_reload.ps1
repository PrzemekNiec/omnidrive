#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Build, unregister, register, and reload the OmniDrive shell extension DLL.
.DESCRIPTION
    Developer workflow script for the omnidrive-shell-ext COM DLL.
    Must be run as Administrator (regsvr32 writes to HKCR/HKLM).

    Steps:
    1. Kill explorer.exe (releases DLL lock!)
    2. Unregister old DLL
    3. cargo build
    4. regsvr32 /s (register new)
    5. Restart explorer.exe
.PARAMETER Unregister
    Only unregister the DLL and restart explorer (cleanup mode).
#>
param(
    [switch]$Unregister
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RepoRoot = Split-Path -Parent $PSScriptRoot
$DllPath  = Join-Path $RepoRoot "target\debug\omnidrive_shell_ext.dll"

Write-Host "=== OmniDrive Shell Extension Dev Reload ===" -ForegroundColor Cyan
Write-Host "DLL: $DllPath"
Write-Host ""

# ── Step 1: Kill Explorer (releases DLL lock) ──────────────────────────

Write-Host "[1/5] Stopping explorer.exe..." -ForegroundColor Yellow
$explorerProcs = Get-Process -Name explorer -ErrorAction SilentlyContinue
if ($explorerProcs) {
    Stop-Process -Name explorer -Force
    Start-Sleep -Seconds 2
    Write-Host "      Explorer stopped." -ForegroundColor Green
} else {
    Write-Host "      Explorer not running." -ForegroundColor DarkGray
}

# ── Step 2: Unregister old DLL ──────────────────────────────────────────

Write-Host "[2/5] Unregistering old DLL..." -ForegroundColor Yellow
if (Test-Path $DllPath) {
    $regsvr = Start-Process -FilePath "regsvr32.exe" -ArgumentList "/u", "/s", "`"$DllPath`"" `
        -Wait -PassThru -NoNewWindow
    if ($regsvr.ExitCode -eq 0) {
        Write-Host "      Unregistered OK." -ForegroundColor Green
    } else {
        Write-Host "      Unregister returned $($regsvr.ExitCode) (may be first run)." -ForegroundColor DarkYellow
    }
} else {
    Write-Host "      No existing DLL found, skipping." -ForegroundColor DarkGray
}

# ── Step 3: Build (skip if unregister-only) ─────────────────────────────

if (-not $Unregister) {
    Write-Host "[3/5] Building omnidrive-shell-ext..." -ForegroundColor Yellow
    Push-Location $RepoRoot
    try {
        $ErrorActionPreference = "Continue"
        cargo build -p omnidrive-shell-ext 2>&1 | ForEach-Object { Write-Host $_ }
        $ErrorActionPreference = "Stop"
        if ($LASTEXITCODE -ne 0) {
            Write-Host "BUILD FAILED. Starting explorer back and aborting." -ForegroundColor Red
            Start-Process explorer.exe
            exit 1
        }
        Write-Host "      Build OK." -ForegroundColor Green
    } finally {
        Pop-Location
    }
} else {
    Write-Host "[3/5] Skipping build (unregister mode)." -ForegroundColor DarkGray
}

# ── Step 4: Register new DLL (skip if unregister-only) ──────────────────

if (-not $Unregister) {
    if (-not (Test-Path $DllPath)) {
        Write-Host "ERROR: DLL not found at $DllPath" -ForegroundColor Red
        Start-Process explorer.exe
        exit 1
    }
    Write-Host "[4/5] Registering new DLL..." -ForegroundColor Yellow
    $regsvr = Start-Process -FilePath "regsvr32.exe" -ArgumentList "/s", "`"$DllPath`"" `
        -Wait -PassThru -NoNewWindow
    if ($regsvr.ExitCode -eq 0) {
        Write-Host "      Registered OK." -ForegroundColor Green
    } else {
        Write-Host "      REGISTER FAILED (exit $($regsvr.ExitCode))." -ForegroundColor Red
        Write-Host "      Check %TEMP%\omnidrive_shell_ext.log for details." -ForegroundColor Red
    }
} else {
    Write-Host "[4/5] Skipping registration (unregister mode)." -ForegroundColor DarkGray
}

# ── Step 5: Restart Explorer ───────────────────────────────────────────

Write-Host "[5/5] Restarting explorer.exe..." -ForegroundColor Yellow
Start-Process explorer.exe
Start-Sleep -Seconds 3

$running = Get-Process -Name explorer -ErrorAction SilentlyContinue
if ($running) {
    Write-Host "      Explorer is back." -ForegroundColor Green
} else {
    Write-Host "      WARNING: Explorer may not have restarted. Try manually." -ForegroundColor Red
}

Write-Host ""
Write-Host "=== Done ===" -ForegroundColor Cyan
if (-not $Unregister) {
    Write-Host "Right-click any file to test the context menu (O:\ filter disabled for testing)."
    Write-Host "Log file: $env:TEMP\omnidrive_shell_ext.log"
}
