# OmniDrive -- Perf Baseline 2026-05-17 (Lenovo / dev box)

**Wersja:** v0.3.23 (commit `d4497d4`)
**Maszyna:** Lenovo ThinkPad (PN-THINKPAD, dev box)
**CPU:** Intel(R) Core(TM) Ultra 7 155H (22 logical cores)
**RAM total:** 95.5 GB
**Scope:** Faza A + B (M1-M4)
**Izolacja:** `.tmp_perf/` + port 8788 + `--no-sync` + throwaway passphrase + vault locked
**Sampling:** 60 s per metryka

## Wyniki vs SLA (STATUS.md 12.2)

| ID | Metryka | Wynik | SLA cel | Status |
|----|---------|-------|---------|--------|
| M1 | Cold start (start -> /api/diagnostics 200) | 1863 ms | <3000 ms | PASS |
| M2 | RAM idle (po 60s od startu) | 34.2 MB | <150 MB | PASS |
| M3 | Watcher CPU idle (pusty watch dir, 60 s) | avg 0 pct / max 0 pct | <1 pct avg | PASS |
| M4 | Watcher CPU pod load (100 x 1KB) | avg 0.01 pct / max 0.14 pct | <5 pct avg | PASS |

## Konfiguracja testowa

- `--no-sync`: daemon nie laczy sie z B2/R2/Scaleway.
- Vault NIE jest unlocked (testujemy daemon w stanie locked).
- Watcher widzi events na `.tmp_perf/watch/` ale bez vault unlock nie pakuje chunkow.
- `LOCALAPPDATA = .tmp_perf/localapp` -- caly state daemona w izolowanym folderze.
- Audit: `.tmp_perf/perf-run.log` (zachowany do review).

## Faza C (wstrzymana 2026-05-17)

M5 (VFS cold fetch) i M6 (VFS warm fetch) wymagaja mount T: + vault unlock + plikow z chunkami chmurowymi.
Decyzja: poczekaj az M1-M4 PASS, potem osobno zatwierdz Faze C.

## Cleanup

```powershell
Remove-Item -Recurse -Force .tmp_perf
```
