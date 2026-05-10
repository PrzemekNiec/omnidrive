---
description: Refresh wiedzy o codebase OmniDrive — jcodemunch high-level + outline kluczowych modułów
---

Odśwież swoją mental mapę projektu OmniDrive. Wykonaj **równolegle** w jednym tool-call message:

1. `mcp__jcodemunch__summarize_repo` na `local/omnidrive-f9f4205b` — AI overview całego repo
2. `mcp__jcodemunch__get_tectonic_map` na `local/omnidrive-f9f4205b` — graf zależności modułów (warstwy daemon/api/storage)
3. `mcp__jcodemunch__get_file_outline` na każdy z kluczowych plików:
   - `angeld/src/lib.rs`
   - `angeld/src/db.rs`
   - `angeld/src/uploader.rs`
   - `angeld/src/scrubber.rs`
   - `angeld/src/repair.rs`
   - `angeld/src/disaster_recovery.rs`
   - `angeld/src/onboarding.rs`
   - `angeld/src/vault.rs`
   - `angeld/src/cloud_guard.rs`
   - `angeld/src/api/mod.rs`
   - `angeld/src/smart_sync/mod.rs` (jeśli istnieje, inaczej `smart_sync.rs`)
   - `angeld/src/main.rs`

Po wszystkim wypisz **bardzo krótkie** (≤8 bullets) podsumowanie:
- Aktualne warstwy architektury
- Workery aktywne w daemonie
- Kluczowe punkty entry dla API
- Co zmieniło się w sesji (jeśli jcodemunch_session_snapshot pokazuje delty)

Nie czytaj wszystkich plików — wystarczy outline. Cel: ja mam mental mapę, nie raport dla użytkownika.
