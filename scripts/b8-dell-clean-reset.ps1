# B8 Dell Clean Reset
# Ensures a completely fresh OmniDrive state on the secondary (Dell) machine.
# Run as the normal user (not elevated) before reinstalling.

$ErrorActionPreference = 'SilentlyContinue'

Write-Host "=== B8 Dell Clean Reset ===" -ForegroundColor Cyan

# 1. Kill daemon and autostart wrapper
Write-Host "[1/6] Stopping angeld..." -ForegroundColor Yellow
Get-Process angeld -ErrorAction SilentlyContinue | Stop-Process -Force
Get-CimInstance Win32_Process |
    Where-Object { $_.Name -in @('wscript.exe','cscript.exe') -and $_.CommandLine -like '*angeld-autostart*' } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Start-Sleep -Seconds 1

# 2. Uninstall if present
Write-Host "[2/6] Uninstalling OmniDrive..." -ForegroundColor Yellow
$unins = Join-Path $env:LOCALAPPDATA 'Programs\OmniDrive\unins000.exe'
if (Test-Path $unins) {
    Start-Process $unins -ArgumentList '/VERYSILENT' -Wait
    Start-Sleep -Seconds 2
}

# 3. Remove autostart registry key
Write-Host "[3/6] Cleaning registry..." -ForegroundColor Yellow
Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'OmniDriveAngeld' -ErrorAction SilentlyContinue

# 4. Remove runtime data with retry for locked files
Write-Host "[4/6] Removing runtime data..." -ForegroundColor Yellow
$paths = @(
    "$env:LOCALAPPDATA\OmniDrive",
    "$env:LOCALAPPDATA\Programs\OmniDrive",
    "$env:USERPROFILE\OmniDrive Vault"
)
foreach ($p in $paths) {
    if (Test-Path $p) {
        for ($i = 0; $i -lt 3; $i++) {
            Remove-Item $p -Recurse -Force -ErrorAction SilentlyContinue
            if (-not (Test-Path $p)) { break }
            Write-Host "  Retry $($i+1)/3 for $p ..." -ForegroundColor DarkYellow
            Start-Sleep -Seconds 2
        }
        if (Test-Path $p) {
            Write-Host "  WARNING: Could not fully remove $p" -ForegroundColor Red
            # Try individual files
            Get-ChildItem $p -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object {
                Remove-Item $_.FullName -Force -ErrorAction SilentlyContinue
            }
        }
    }
}

# 5. Unmount virtual drive
Write-Host "[5/6] Unmounting O: drive..." -ForegroundColor Yellow
cmd /c "subst O: /D" 2>$null
cmd /c "net use O: /delete /y" 2>$null

# 6. Verify clean state
Write-Host "[6/6] Verifying clean state..." -ForegroundColor Yellow
$port = Get-NetTCPConnection -LocalPort 8787 -State Listen -ErrorAction SilentlyContinue
$dbExists = Test-Path "$env:LOCALAPPDATA\OmniDrive\omnidrive.db"
$dirExists = Test-Path "$env:LOCALAPPDATA\OmniDrive"
$progExists = Test-Path "$env:LOCALAPPDATA\Programs\OmniDrive"

Write-Host ""
Write-Host "=== Results ===" -ForegroundColor Cyan
Write-Host "  Port 8787 free:     $(-not $port)" -ForegroundColor $(if (-not $port) { 'Green' } else { 'Red' })
Write-Host "  DB removed:         $(-not $dbExists)" -ForegroundColor $(if (-not $dbExists) { 'Green' } else { 'Red' })
Write-Host "  Runtime dir clean:  $(-not $dirExists)" -ForegroundColor $(if (-not $dirExists) { 'Green' } else { 'Red' })
Write-Host "  Program dir clean:  $(-not $progExists)" -ForegroundColor $(if (-not $progExists) { 'Green' } else { 'Red' })

if ($dirExists) {
    Write-Host ""
    Write-Host "  Remaining files:" -ForegroundColor DarkYellow
    Get-ChildItem "$env:LOCALAPPDATA\OmniDrive" -Recurse -ErrorAction SilentlyContinue |
        Select-Object FullName, Length | Format-Table -AutoSize
}

$allClean = (-not $port) -and (-not $dbExists) -and (-not $dirExists) -and (-not $progExists)
if ($allClean) {
    Write-Host ""
    Write-Host "CLEAN. Ready for fresh install of OmniDrive-Setup-0.1.11.exe" -ForegroundColor Green
} else {
    Write-Host ""
    Write-Host "NOT FULLY CLEAN. Restart the machine and run this script again." -ForegroundColor Red
}
