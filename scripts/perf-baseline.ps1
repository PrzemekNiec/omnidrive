# OmniDrive — perf-baseline.ps1 (Faza 0, krok 0.3 — Task 4)
#
# Mierzy 4 metryki bazowe (M1-M4) na izolowanym test daemonie:
#   M1  Cold start (start angeld.exe -> /api/diagnostics 200 OK)  [SLA: <3000ms]
#   M2  RAM idle (po 60s od startu, WorkingSet64)                  [SLA: <150 MB]
#   M3  Watcher CPU idle (60s na PUSTYM .tmp_perf/watch/)          [SLA: <1% avg]
#   M4  Watcher CPU under load (100 plikow * 1KB w .tmp_perf/watch)[SLA: <5% avg]
#
# Faza C (M5/M6 VFS fetch) jest CELOWO POMINIETA per decyzja Przemka 2026-05-17.
#
# ===== ZASADY BEZPIECZENSTWA (CLAUDE.md Swieta Zasada Integralnosci Danych) =====
#
# 1. IZOLACJA SCIEZEK: wszystkie operacje TYLKO w .tmp_perf/ (root repo).
# 2. OSOBNA BAZA: .tmp_perf/perf-test.db -- NIGDY %LOCALAPPDATA%\OmniDrive\omnidrive.db
# 3. OSOBNY PORT API: 8788 (prod = 8787 -- zero kolizji)
# 4. OSOBNY SyncRoot: .tmp_perf/localapp/OmniDrive/OmniSync (przez LOCALAPPDATA)
# 5. --no-sync flag: zero realnego uploadu do B2/R2/Scaleway
# 6. THROWAWAY passphrase: vault NIE jest unlocked -- tylko start/stop daemona
# 7. AUDIT: kazda operacja zapisana w .tmp_perf/perf-run.log
# 8. GATE: skrypt aborts jesli prod daemon dziala lub porty zajete
# 9. CLEANUP: try/finally gwarantuje Stop-TestDaemon nawet po crash
# 10. ROLLBACK: .tmp_perf/ jest w .gitignore (.tmp_*), nie wycieknie do gita
#
# Uruchomienie:
#   pwsh scripts/perf-baseline.ps1                # default Phase=AB
#   pwsh scripts/perf-baseline.ps1 -Phase A       # tylko M1-M3 (zero ryzyka)
#   pwsh scripts/perf-baseline.ps1 -Phase AB      # + M4 (niskie ryzyko)
#
# Output:
#   docs/perf-baseline-2026-05-17.md     -- tabela wynikow vs SLA
#   .tmp_perf/perf-run.log               -- audit kazdej operacji
#   .tmp_perf/daemon-stdout.log          -- daemon logs do diagnostyki
#   .tmp_perf/daemon-stderr.log

[CmdletBinding()]
param(
    [ValidateSet("A", "B", "AB")]
    [string]$Phase = "AB",

    [ValidateRange(10, 600)]
    [int]$IdleSampleSeconds = 60,

    [ValidateRange(10, 1000)]
    [int]$LoadFileCount = 100,

    [ValidateRange(1, 1024)]
    [int]$LoadFileSizeKb = 1
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# ===== Sciezki + stale (read-only) =====
$script:RepoRoot       = (Resolve-Path "$PSScriptRoot/..").Path
$script:TestDir        = Join-Path $script:RepoRoot ".tmp_perf"
$script:DaemonExe      = Join-Path $script:RepoRoot "target\release\angeld.exe"
$script:TestPort       = 8788
$script:ProdPort       = 8787   # tylko do sprawdzenia ze wolny
$script:Passphrase     = "perf-baseline-2026-05-17-throwaway"
$script:ReportPath     = Join-Path $script:RepoRoot "docs\perf-baseline-2026-05-17.md"
$script:AuditLog       = Join-Path $script:TestDir "perf-run.log"

# ===== Mutable state =====
$script:DaemonProcess  = $null
$script:EnvBackup      = @{}
$script:Results        = @{}

# ===== Helpers =====
function Write-Step($msg) { Write-Host "`n[STEP] $msg" -ForegroundColor Cyan }
function Write-Ok($msg)   { Write-Host "[OK]   $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "[WARN] $msg" -ForegroundColor Yellow }
function Write-Err($msg)  { Write-Host "[ERR]  $msg" -ForegroundColor Red }

function Confirm-Action {
    param([string]$Prompt)
    $response = Read-Host "`n>>> $Prompt [Y/N]"
    return ($response -eq "Y" -or $response -eq "y")
}

function Write-Audit {
    param([string]$Action, [string]$Detail)
    $line = "[$(Get-Date -Format 'yyyy-MM-ddTHH:mm:ss.fff')] $Action :: $Detail"
    if (Test-Path $script:AuditLog) {
        Add-Content -Path $script:AuditLog -Value $line -Encoding UTF8
    }
    Write-Verbose $line
}

# ===== Safety gate (pre-checks) =====
function Assert-Safety {
    Write-Step "Pre-checks bezpieczenstwa..."

    # 1. PROD DAEMON nie moze dzialac
    $prod = Get-Process angeld -ErrorAction SilentlyContinue
    if ($prod) {
        Write-Err "PROD DAEMON DZIALA (PID=$($prod.Id), RAM=$([math]::Round($prod.WorkingSet64/1MB,1))MB)."
        Write-Err "Wylacz go RECZNIE przez UI (Lock vault + tray Exit), nastepnie ponow."
        Write-Err "Skrypt celowo NIE ubije Twojego daemona sam."
        throw "Prod daemon running"
    }
    Write-Ok "Prod daemon: nie dziala"

    # 2. Porty wolne
    $port8787 = Test-NetConnection -ComputerName 127.0.0.1 -Port $script:ProdPort `
                                    -InformationLevel Quiet -WarningAction SilentlyContinue
    if ($port8787) {
        Write-Err "Port $($script:ProdPort) zajety mimo ze prod daemon nie dziala. Sprawdz: netstat -ano | findstr :$($script:ProdPort)"
        throw "Port $($script:ProdPort) in use"
    }
    $port8788 = Test-NetConnection -ComputerName 127.0.0.1 -Port $script:TestPort `
                                    -InformationLevel Quiet -WarningAction SilentlyContinue
    if ($port8788) {
        Write-Err "Port $($script:TestPort) (test) zajety. Inny test daemon? Sprawdz: netstat -ano | findstr :$($script:TestPort)"
        throw "Port $($script:TestPort) in use"
    }
    Write-Ok "Porty $($script:ProdPort) + $($script:TestPort): wolne"

    # 3. Binary musi istniec
    if (-not (Test-Path $script:DaemonExe)) {
        Write-Err "$($script:DaemonExe) nie istnieje. Uruchom: cargo build --release -p angeld"
        throw "Daemon binary missing"
    }
    $binSize = (Get-Item $script:DaemonExe).Length / 1MB
    Write-Ok "Binary: $($script:DaemonExe) ($([math]::Round($binSize,1)) MB)"

    # 4. .tmp_perf/ — wyczysc lub utworz
    if (Test-Path $script:TestDir) {
        Write-Warn ".tmp_perf/ istnieje (poprzedni run?). Wymagam usuniecia przed kolejnym pomiarem."
        if (-not (Confirm-Action "Usunac .tmp_perf/ i kontynuowac?")) {
            throw "User declined cleanup"
        }
        Remove-Item -Recurse -Force $script:TestDir
    }
    New-Item -ItemType Directory -Path $script:TestDir | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $script:TestDir "watch") | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $script:TestDir "localapp") | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $script:TestDir "spool") | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $script:TestDir "cache") | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $script:TestDir "download-spool") | Out-Null

    "" | Out-File -FilePath $script:AuditLog -Encoding UTF8
    Write-Audit "INIT" ".tmp_perf/ created at $($script:TestDir)"
    Write-Audit "INIT_SAFETY_PASSPHRASE" "throwaway (vault NIE bedzie unlocked dla M1-M4)"
    Write-Audit "INIT_PHASE" $Phase
    Write-Ok ".tmp_perf/ ready"

    # 5. .gitignore zawiera .tmp_*
    $gitignore = Get-Content (Join-Path $script:RepoRoot ".gitignore")
    if (-not ($gitignore -match "^\.tmp_\*")) {
        Write-Warn ".gitignore moze nie zawierac .tmp_* glob. Sprawdz przed commitem."
    } else {
        Write-Ok ".gitignore: .tmp_* glob obecny"
    }

    Write-Audit "SAFETY_GATE_PASSED" "all 5 checks green"
}

# ===== Test daemon — start =====
function Start-TestDaemon {
    Write-Step "Uruchamianie test daemona..."

    $watchDir      = Join-Path $script:TestDir "watch"
    $localApp      = Join-Path $script:TestDir "localapp"
    $spool         = Join-Path $script:TestDir "spool"
    $cache         = Join-Path $script:TestDir "cache"
    $downloadSpool = Join-Path $script:TestDir "download-spool"
    $stdoutLog     = Join-Path $script:TestDir "daemon-stdout.log"
    $stderrLog     = Join-Path $script:TestDir "daemon-stderr.log"

    # DB url — SQLite path z forward slashami
    $dbPath = (Join-Path $script:TestDir "perf-test.db").Replace('\','/')
    $dbUrl  = "sqlite:///$dbPath"

    # Env vars — backup + set
    $envMap = @{
        "OMNIDRIVE_DB_URL"             = $dbUrl
        "OMNIDRIVE_WATCH_DIR"          = $watchDir
        "OMNIDRIVE_SPOOL_DIR"          = $spool
        "OMNIDRIVE_DOWNLOAD_SPOOL_DIR" = $downloadSpool
        "OMNIDRIVE_CACHE_DIR"          = $cache
        "OMNIDRIVE_API_BIND"           = "127.0.0.1:$($script:TestPort)"
        "LOCALAPPDATA"                 = $localApp
        "RUST_LOG"                     = "warn"
    }
    foreach ($key in $envMap.Keys) {
        $script:EnvBackup[$key] = [Environment]::GetEnvironmentVariable($key, "Process")
        [Environment]::SetEnvironmentVariable($key, $envMap[$key], "Process")
        Write-Audit "ENV_SET" "$key=$($envMap[$key])"
    }

    # Cold start measurement begin
    Write-Audit "DAEMON_START_BEGIN" $script:DaemonExe
    $sw = [System.Diagnostics.Stopwatch]::StartNew()

    $proc = Start-Process -FilePath $script:DaemonExe `
                          -ArgumentList "--no-sync" `
                          -WorkingDirectory $script:RepoRoot `
                          -RedirectStandardOutput $stdoutLog `
                          -RedirectStandardError $stderrLog `
                          -PassThru -NoNewWindow

    $script:DaemonProcess = $proc

    # Po starcie: tylko 1 proces angeld w systemie
    Start-Sleep -Milliseconds 200
    $angelds = @(Get-Process angeld -ErrorAction SilentlyContinue)
    if ($angelds.Count -ne 1 -or $angelds[0].Id -ne $proc.Id) {
        Write-Warn "Wykryto $($angelds.Count) procesow angeld. Spodziewam sie tylko 1 (PID=$($proc.Id)). CPU measurement moze byc zbedny."
    }

    # Wait for /api/diagnostics/health
    $url = "http://127.0.0.1:$($script:TestPort)/api/diagnostics/health"
    $ready = $false
    while ($sw.ElapsedMilliseconds -lt 30000) {
        if ($proc.HasExited) {
            $stderr = Get-Content $stderrLog -Raw -ErrorAction SilentlyContinue
            Write-Err "Daemon zakonczyl sie z kodem $($proc.ExitCode). stderr:"
            Write-Host $stderr -ForegroundColor DarkRed
            throw "Daemon exited before API ready"
        }
        try {
            $r = Invoke-WebRequest -Uri $url -TimeoutSec 1 -UseBasicParsing -ErrorAction Stop
            if ($r.StatusCode -eq 200) { $ready = $true; break }
        } catch {
            Start-Sleep -Milliseconds 100
        }
    }
    $sw.Stop()

    if (-not $ready) {
        throw "Daemon API nie odpowiada w 30s (PID=$($proc.Id))"
    }

    $script:Results["M1_cold_start_ms"] = [int]$sw.ElapsedMilliseconds
    Write-Audit "DAEMON_READY" "PID=$($proc.Id) ms=$($sw.ElapsedMilliseconds)"
    Write-Ok "M1 cold start = $($sw.ElapsedMilliseconds) ms (PID=$($proc.Id), port=$($script:TestPort))"
}

# ===== Test daemon — stop =====
function Stop-TestDaemon {
    if ($null -eq $script:DaemonProcess) { return }
    if ($script:DaemonProcess.HasExited) {
        Write-Ok "Test daemon juz zakonczony (PID=$($script:DaemonProcess.Id))"
        Write-Audit "DAEMON_ALREADY_EXITED" "PID=$($script:DaemonProcess.Id) exit=$($script:DaemonProcess.ExitCode)"
        return
    }
    Write-Step "Zatrzymywanie test daemona (PID=$($script:DaemonProcess.Id))..."
    Write-Audit "DAEMON_STOP_BEGIN" "PID=$($script:DaemonProcess.Id)"
    try {
        $script:DaemonProcess.Kill()
        $script:DaemonProcess.WaitForExit(5000) | Out-Null
        Write-Ok "Test daemon zatrzymany"
        Write-Audit "DAEMON_STOPPED" "PID=$($script:DaemonProcess.Id)"
    } catch {
        Write-Warn "Kill failed: $_"
        Write-Audit "DAEMON_STOP_FAIL" "$_"
    }
}

# ===== Env restore (po stop daemona) =====
function Restore-Env {
    foreach ($key in $script:EnvBackup.Keys) {
        $prev = $script:EnvBackup[$key]
        [Environment]::SetEnvironmentVariable($key, $prev, "Process")
    }
    Write-Audit "ENV_RESTORED" "$($script:EnvBackup.Keys -join ',')"
}

# ===== M2 — RAM idle =====
function Measure-RamIdle {
    Write-Step "M2: czekam $IdleSampleSeconds s na ustabilizowanie RAM..."
    Start-Sleep -Seconds $IdleSampleSeconds
    $proc = Get-Process -Id $script:DaemonProcess.Id -ErrorAction SilentlyContinue
    if (-not $proc) {
        Write-Err "Daemon zniknal podczas pomiaru RAM!"
        throw "Daemon died"
    }
    $ramMB = [math]::Round($proc.WorkingSet64 / 1MB, 1)
    $script:Results["M2_ram_idle_mb"] = $ramMB
    Write-Audit "M2_RAM_IDLE" "${ramMB}MB"
    Write-Ok "M2 RAM idle = $ramMB MB"
}

# ===== CPU sampling helper =====
# Uzywamy Process.TotalProcessorTime delta zamiast Get-Counter:
# - culture-invariant (Get-Counter ma pl-PL/en-US issue z nazwami licznikow)
# - exact PID match (nie ryzyko zlapania innego angeld.exe)
# - prostsze deps (brak Performance Counter infrastructure)
function Get-CpuSamples {
    param(
        [int]$Seconds,
        [int]$ProcessId
    )

    $cores = (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
    $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if (-not $proc) { return $null }

    $samples = @()
    $prevCpuMs  = $proc.TotalProcessorTime.TotalMilliseconds
    $prevTickMs = [Environment]::TickCount

    for ($i = 0; $i -lt $Seconds; $i++) {
        Start-Sleep -Seconds 1
        $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
        if (-not $proc) { break }
        $currCpuMs  = $proc.TotalProcessorTime.TotalMilliseconds
        $currTickMs = [Environment]::TickCount
        $deltaCpu   = $currCpuMs - $prevCpuMs
        $deltaWall  = $currTickMs - $prevTickMs
        if ($deltaWall -gt 0) {
            # Normalize: % of total CPU capacity = (cpu_time / wall_time) / cores * 100
            $pct = ($deltaCpu / $deltaWall) * 100.0 / $cores
            if ($pct -lt 0) { $pct = 0 }
            $samples += $pct
        }
        $prevCpuMs  = $currCpuMs
        $prevTickMs = $currTickMs
    }

    if ($samples.Count -eq 0) { return $null }
    return @{
        Avg   = [math]::Round(($samples | Measure-Object -Average).Average, 2)
        Max   = [math]::Round(($samples | Measure-Object -Maximum).Maximum, 2)
        Min   = [math]::Round(($samples | Measure-Object -Minimum).Minimum, 2)
        Cores = $cores
        Count = $samples.Count
    }
}

# ===== M3 — Watcher CPU idle (pusty watch dir) =====
function Measure-WatcherIdle {
    Write-Step "M3: probkowanie CPU $IdleSampleSeconds s na PUSTYM .tmp_perf/watch/..."
    Write-Audit "M3_BEGIN" "$IdleSampleSeconds s sample"

    $stats = Get-CpuSamples -Seconds $IdleSampleSeconds -ProcessId $script:DaemonProcess.Id
    if (-not $stats) {
        Write-Warn "M3: pomiar CPU nieudany (daemon zniknal?)."
        return
    }

    $script:Results["M3_watcher_cpu_idle_avg"] = $stats.Avg
    $script:Results["M3_watcher_cpu_idle_max"] = $stats.Max
    $script:Results["M3_cores"] = $stats.Cores
    Write-Audit "M3_RESULT" "avg=$($stats.Avg)% max=$($stats.Max)% cores=$($stats.Cores) samples=$($stats.Count)"
    Write-Ok "M3 watcher CPU idle: avg=$($stats.Avg)% max=$($stats.Max)% (normalizowane na $($stats.Cores) cores)"
}

# ===== M4 — Watcher CPU pod obciazeniem =====
function Measure-WatcherLoad {
    Write-Step "M4: kopiowanie $LoadFileCount plikow x ${LoadFileSizeKb}KB do .tmp_perf/watch/ + sampling CPU..."
    $watchDir = Join-Path $script:TestDir "watch"
    Write-Audit "M4_BEGIN" "files=$LoadFileCount size=${LoadFileSizeKb}KB target=$watchDir"

    # Pre-generuj payload raz (te same bytes do kazdego pliku — content-hash dedupe nas nie interesuje, mierzymy watchera)
    $payloadBytes = [byte[]]::new($LoadFileSizeKb * 1024)
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    $rng.GetBytes($payloadBytes)

    # File-writing job in background. Spaced ~50ms = pelen burst trwa ~5s dla 100 plikow.
    $writerJob = Start-Job -ScriptBlock {
        param($dir, $count, $bytes)
        for ($i = 1; $i -le $count; $i++) {
            $path = Join-Path $dir "perf-load-$i.bin"
            [System.IO.File]::WriteAllBytes($path, $bytes)
            Start-Sleep -Milliseconds 50
        }
    } -ArgumentList $watchDir, $LoadFileCount, $payloadBytes

    # Sample CPU dluzej niz burst (zeby uchwycic watcher debounce + ewentualny processing)
    $sampleSeconds = [math]::Max(30, [int]($LoadFileCount * 0.05) + 10)
    $stats = Get-CpuSamples -Seconds $sampleSeconds -ProcessId $script:DaemonProcess.Id

    Wait-Job $writerJob -Timeout 60 | Out-Null
    $writerErrors = Receive-Job $writerJob -ErrorAction SilentlyContinue
    Remove-Job $writerJob -Force -ErrorAction SilentlyContinue
    if ($writerErrors) {
        Write-Warn "Writer job errors: $writerErrors"
    }

    if (-not $stats) {
        Write-Warn "M4: pomiar CPU nieudany (daemon zniknal?)."
        Write-Audit "M4_FAIL" "no samples"
        return
    }

    # Sprawdz ile plikow faktycznie wyladowalo
    $filesActual = @(Get-ChildItem $watchDir -Filter "perf-load-*.bin").Count
    $script:Results["M4_files_written"] = $filesActual
    $script:Results["M4_watcher_cpu_load_avg"] = $stats.Avg
    $script:Results["M4_watcher_cpu_load_max"] = $stats.Max
    Write-Audit "M4_RESULT" "avg=$($stats.Avg)% max=$($stats.Max)% files=$filesActual sample_s=$sampleSeconds"
    Write-Ok "M4 watcher CPU load: avg=$($stats.Avg)% max=$($stats.Max)% (po zapisaniu $filesActual/$LoadFileCount plikow)"
}

# ===== Generuj raport markdown =====
function Write-Results {
    if ($script:Results.Count -eq 0) {
        Write-Warn "Brak wynikow do zapisania."
        return
    }

    Write-Step "Generowanie raportu: $($script:ReportPath)"

    $cpuCores = (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
    $cpuName  = ((Get-CimInstance Win32_Processor) | Select-Object -First 1).Name.Trim()
    $totalRam = [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB, 1)
    $commit   = (git rev-parse --short HEAD).Trim()

    # Build table rows (warunki na obecnosc kazdej metryki)
    $rows = @()
    if ($script:Results.ContainsKey("M1_cold_start_ms")) {
        $v = $script:Results["M1_cold_start_ms"]
        $st = if ($v -lt 3000) { "PASS" } else { "FAIL" }
        $rows += "| M1 | Cold start (start -> /api/diagnostics 200) | $v ms | <3000 ms | $st |"
    }
    if ($script:Results.ContainsKey("M2_ram_idle_mb")) {
        $v = $script:Results["M2_ram_idle_mb"]
        $st = if ($v -lt 150) { "PASS" } else { "FAIL" }
        $rows += "| M2 | RAM idle (po $($IdleSampleSeconds)s od startu) | $v MB | <150 MB | $st |"
    }
    if ($script:Results.ContainsKey("M3_watcher_cpu_idle_avg")) {
        $avg = $script:Results["M3_watcher_cpu_idle_avg"]
        $max = $script:Results["M3_watcher_cpu_idle_max"]
        $st = if ($avg -lt 1) { "PASS" } else { "FAIL" }
        $rows += "| M3 | Watcher CPU idle (pusty watch dir, $IdleSampleSeconds s) | avg $avg pct / max $max pct | <1 pct avg | $st |"
    }
    if ($script:Results.ContainsKey("M4_watcher_cpu_load_avg")) {
        $avg = $script:Results["M4_watcher_cpu_load_avg"]
        $max = $script:Results["M4_watcher_cpu_load_max"]
        $files = $script:Results["M4_files_written"]
        $st = if ($avg -lt 5) { "PASS" } else { "FAIL" }
        $rows += "| M4 | Watcher CPU pod load ($files x $($LoadFileSizeKb)KB) | avg $avg pct / max $max pct | <5 pct avg | $st |"
    }
    $rowsBlock = $rows -join "`r`n"

    $report = @"
# OmniDrive -- Perf Baseline 2026-05-17 (Lenovo / dev box)

**Wersja:** v0.3.23 (commit ``$commit``)
**Maszyna:** Lenovo ThinkPad (PN-THINKPAD, dev box)
**CPU:** $cpuName ($cpuCores logical cores)
**RAM total:** $totalRam GB
**Scope:** Faza A + B (M1-M4)
**Izolacja:** ``.tmp_perf/`` + port 8788 + ``--no-sync`` + throwaway passphrase + vault locked
**Sampling:** $IdleSampleSeconds s per metryka

## Wyniki vs SLA (STATUS.md 12.2)

| ID | Metryka | Wynik | SLA cel | Status |
|----|---------|-------|---------|--------|
$rowsBlock

## Konfiguracja testowa

- ``--no-sync``: daemon nie laczy sie z B2/R2/Scaleway.
- Vault NIE jest unlocked (testujemy daemon w stanie locked).
- Watcher widzi events na ``.tmp_perf/watch/`` ale bez vault unlock nie pakuje chunkow.
- ``LOCALAPPDATA = .tmp_perf/localapp`` -- caly state daemona w izolowanym folderze.
- Audit: ``.tmp_perf/perf-run.log`` (zachowany do review).

## Faza C (wstrzymana 2026-05-17)

M5 (VFS cold fetch) i M6 (VFS warm fetch) wymagaja mount T: + vault unlock + plikow z chunkami chmurowymi.
Decyzja: poczekaj az M1-M4 PASS, potem osobno zatwierdz Faze C.

## Cleanup

``````powershell
Remove-Item -Recurse -Force .tmp_perf
``````
"@

    $report | Out-File -FilePath $script:ReportPath -Encoding utf8
    Write-Ok "Raport: $($script:ReportPath)"
    Write-Audit "REPORT_WRITTEN" $script:ReportPath
}

# ===== Main =====
try {
    Write-Host "`n=== OmniDrive perf baseline -- Faza $Phase ===" -ForegroundColor Cyan
    Write-Host "Scope: M1 cold start | M2 RAM idle | M3 watcher CPU idle$(if($Phase -ne 'A'){' | M4 watcher CPU load'})"
    Write-Host "Izolacja: .tmp_perf/ + port 8788 + --no-sync + vault locked"
    Write-Host ""
    if (-not (Confirm-Action "Continue with pre-checks?")) {
        Write-Ok "Aborted by user."
        return
    }

    Assert-Safety

    if (-not (Confirm-Action "Pre-checks OK. Start test daemon (M1 cold start) + Phase A measurements?")) {
        Write-Ok "Aborted by user."
        return
    }

    Start-TestDaemon
    Measure-RamIdle
    Measure-WatcherIdle

    if ($Phase -ne "A") {
        if (Confirm-Action "Phase A done. Continue to M4 watcher pod load ($LoadFileCount plikow w .tmp_perf/watch/)?") {
            Measure-WatcherLoad
        } else {
            Write-Ok "Stopping after Phase A per user choice."
        }
    }
} catch {
    Write-Err "BLAD: $_"
    Write-Audit "ABORT" "$_"
    throw
} finally {
    Stop-TestDaemon
    Restore-Env
    Write-Results

    Write-Host "`n=== Cleanup (manualny) ===" -ForegroundColor Cyan
    Write-Host "Po review wynikow + perf-run.log, usun .tmp_perf/:"
    Write-Host "  Remove-Item -Recurse -Force .tmp_perf" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Audit log: $($script:AuditLog)" -ForegroundColor Gray
}
