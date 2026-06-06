# OmniDrive — Known Issues Tracker

> **Single source of truth dla bugów.** Ten plik (nie GitHub Issues, nie STATUS.md) trzyma listę otwartych problemów z priorytetyzacją.
>
> **Ostatnia aktualizacja:** 2026-06-06
> **Aktualna wersja:** v0.3.27

---

## Priorytetyzacja

| Tier | Definicja | Gate |
|------|-----------|------|
| **P0** | Crash, data loss, niemożliwy unlock — system unusable | Blokuje każdy release, fix natychmiast |
| **P1** | Krytyczna funkcja działa nieprawidłowo, ale nie traci danych — np. ACL fail, niedziałający flow | Blokuje v0.4 release; nie blokuje v0.3.x patcha |
| **P2** | Performance / UX dług który łamie SLA z roadmapy ale system funkcjonalnie OK | Blokuje v0.4 release; tolerowane w v0.3.x |
| **P3** | Drobne UX / kosmetyka / nice-to-have | Nie blokuje v0.4; może iść do v0.4.x patch lub v5.0 |

**Workflow:**
- Claude dodaje wpis po wykryciu (review code lub testy)
- Przemek zatwierdza priorytet ("OK P1") lub koryguje
- Po fixie: status `→ FIXED in vX.Y.Z`, wpis przenoszony do `## Closed` na dole

---

## P0 — Crash / Data Loss

*Brak otwartych. Sukces.*

---

## P1 — Krytyczne błędy logiczne

### P1-002 — Lenovo nie widzi Della w MultiDevice po join

- **Wykryto:** v0.3.23 Dell smoke test, MultiDevice tab Lenovo pokazuje tylko siebie
- **CONFIRMED 2026-05-10 wieczór:** Dell po v0.3.23 join-existing pokazuje OBA urządzenia (PN-THINKPAD + PN-OFFICE) ✅ — graft `devices` działa. Lenovo daemon zweryfikowany jako v0.3.23 (curl `/api/diagnostics` zwraca pełny JSON, endpoint dodany w v0.3.23). `members_count:1` w `/api/vault/status` na Lenovo — potwierdza że Lenovo nigdy nie pobrał zaktualizowanego snapshot z Della.
- **Symptom:** Dell po join-existing wgra zaktualizowany snapshot do chmury, ale Lenovo nigdy go nie pobiera, więc nie wie o nowym device
- **Hipoteza root cause:** Daemon ma snapshot **upload** worker (`MetadataBackupWorker`) ale nie ma symetrycznego snapshot **fetch** workera dla istniejących urządzeń. Tylko join-existing flow pobiera snapshot.
- **Impact:** Multi-device awareness jednokierunkowy. Gdy ktoś z rodziny dołącza nowy laptop (v5.0), admin nie zobaczy go bez restart daemona albo manual refresh.
- **Fix scope:** Periodic snapshot fetch worker (np. co 1h) w angeld. Decyzja: tylko gdy snapshot jest nowszy + lock wokół DB (nie nadpisuj jeśli były lokalne zmiany). Może wymagać per-device sequence number / lamport clock.
- **Status:** OPEN. Planowany w **Faza β** roadmapy v0.4.

### P1-003 — Snapshot upload do Scaleway zwraca AccessDenied

- **Wykryto:** v0.3.23 Dell metadata-backup status — Scaleway 403 AccessDenied dla `_omnidrive/system/metadata/snapshots/*.db.enc`
- **Symptom:** B2 OK, R2 connection reset (osobny issue), Scaleway 403. Czyli z 3 providerów tylko jeden żywy.
- **Hipoteza:** Bucket policy / access key uprawnienia do prefix `_omnidrive/system/metadata/snapshots/` — może bucket nie pozwala PUT pod system/. Inny prefix (`packs/...`) działa wg logów.
- **Impact:** Brak redundancji metadanych: jedyna kopia snapshot na B2. Awaria B2 = utrata metadata, mimo że chunki są na 3 providerach.
- **Fix scope:** Sprawdzić Scaleway IAM policy + bucket policy + key permissions. Jeśli OK, zbadać dlaczego prefix `_omnidrive/system/` jest blokowany. Naprawić konfigurację albo udokumentować workaround.
- **Status:** OPEN. **Faza β** roadmapy v0.4 (snapshot redundancy fix). **Quality Gate 2.e** ("snapshot zawsze w ≥1 sprawnym miejscu") nie spełniony, ale technically B2 jest sprawny → tolerowalne tymczasowo. P1 bo bezpieczeństwo redundancji.

### P1-004 — Snapshot upload do R2 zwraca ConnectionReset

- **Wykryto:** v0.3.23 Dell metadata-backup status — R2 `ConnectionReset (os error 10054)` przy PUT
- **Symptom:** Brak 403, brak timeout — surowy reset połączenia od R2. Może być rate-limit / WAF / connection pool issue.
- **Hipoteza:** R2 hyper-1.x compatibility issue (memory: rustls/hyper consolidation odłożona). Może `keep-alive` pool trzyma wygasłe połączenie.
- **Impact:** Tak samo jak P1-003 — brak redundancji.
- **Fix scope:** Najpierw retry z fresh connection (`force-close` po 1 ConnReset). Drugorzędnie: Batch 7 C.3 (rustls/hyper consolidation z Backlog).
- **Status:** OPEN. **Faza β** roadmapy v0.4 (snapshot redundancy fix). Powiązany z C.3 (Backlog).

---

## P2 — Performance / SLA dług

### P2-001 — Watcher mieli CPU

- **Wykryto:** Subiektywna obserwacja Przemka, brak benchmarku
- **Symptom:** `angeld.exe` w taskmgr pokazuje wysokie CPU nawet w idle (do potwierdzenia liczbowego)
- **SLA cel:** < 1% CPU idle, < 5% active (per roadmap v0.4)
- **Pomiar (Faza 0, 2026-05-17):** perf baseline M3 watcher CPU idle **0%** + M4 load **avg 0.01% / max 0.14%** — **w pełni w SLA** (`docs/perf-baseline-2026-05-17.md`). Pierwotna subiektywna obserwacja NIE potwierdzona benchmarkiem.
- **Fix scope:** brak — wynik PASS. Pozostawione OPEN do formalnego domknięcia decyzją (czy zamknąć jako „resolved-by-measurement", czy re-mierzyć po Fazie β z aktywnym watcherem na realnym obciążeniu).
- **Status:** OPEN (kandydat do zamknięcia — pomiar PASS). **Faza β.d** = bez akcji.

### P2-002 — VFS laguje przy dużych plikach

- **Wykryto:** Subiektywna obserwacja Przemka, brak benchmarku
- **Symptom:** Otwarcie dużego pliku (>50MB?) z O:\ trwa zauważalnie długo
- **SLA cel:** Cold fetch < 2s/10MB, < 10s/100MB; warm < 100ms (per roadmap v0.4)
- **Fix scope:** (1) Benchmark: cold fetch 1MB/10MB/100MB/1GB; warm fetch tych samych. (2) Audit `angeld/src/smart_sync.rs` (2197 linii — monolit do dekomponozycji). Sprawdzić: streaming hydration czy fetch-all-then-decrypt? EC reconstruction blokująca? Cache hit path?
- **Status:** OPEN. **Faza ε.a/β.e** (po pomiarach — dekompozycja smart_sync.rs).

### P2-003 — Bin `angeld` duplikuje 27 modułów z lib (dual-compile)

- **Wykryto:** 2026-05-17, Task 1 Fazy 0 / fix CI-red (clippy 1.94). Audyt znalazł 7 lintów w lib, ale `cargo clippy --workspace --all-targets` ujawnił 6 dodatkowych w bin których lib-only check nie złapał.
- **Symptom:** `angeld/src/main.rs` deklaruje `mod xxx;` dla **27 modułów** które są jednocześnie `pub mod xxx;` w `angeld/src/lib.rs` (acl, api_error, autostart, ingest, aws_http, cache, cloud_guard, config, db, device_identity, diagnostics, disaster_recovery, downloader, identity, logging, migrator, onboarding, packer, peer, pipe_server, recovery, runtime_paths, secure_fs, smart_sync, uploader, vault, win_acl). Każdy z nich jest kompilowany dwa razy (raz jako część `lib angeld`, raz jako część `bin angeld`).
- **Bin-only moduły (10, prawidłowo poza lib):** api, gc, repair, scrubber, sharing, shell_integration, shell_state, virtual_drive, watcher, windows_hello.
- **Konsekwencje:**
  - **2× compile time** dla 27 modułów (w tym `db.rs` 8.6k linii, `smart_sync.rs` 2.2k, `downloader.rs` 1.7k).
  - **2× clippy reports** z różnymi setami warningów per target — bug pattern wykryty w audycie: lib-only `cargo clippy --workspace -- -D warnings` przepuścił 6 błędów które ujawniły się dopiero przy `--all-targets`.
  - **Risk inkonsystencji**: jeśli kiedyś `lib` i `bin` rozjadą się (np. różne ścieżki w `mod xxx { ... }` body), będą efektywnie dwie wersje tego samego symbolu — debugowanie trudne.
  - **Drift między lib API a bin internals**: niektóre symbole są `pub` w lib ale używane prywatnie w bin → utrudnia świadome projektowanie API biblioteki (np. dla przyszłej integracji testów e2e jako library consumer).
- **Fix scope (opcje, do decyzji w Fazie α/β):**
  - **Opcja A (preferowana):** Usunąć `mod xxx;` z `main.rs` dla 27 zduplikowanych modułów, zamienić na `use angeld::xxx;`. Bin staje się cienkim wrapperem nad library. Wymaga: przeniesienia bin-only modułów albo do lib (jeśli mają sens jako reusable), albo zostawienia w `main.rs` (private to bin).
  - **Opcja B:** Skasować `angeld/src/lib.rs` całkowicie (bin-only crate). Tracimy library API dla testów e2e i przyszłej integracji.
  - **Opcja C (status quo + safeguard):** Zostawić duplikację, ale dodać do CI sztywne `cargo clippy --workspace --all-targets -- -D warnings` żeby zawsze sprawdzać obie konfiguracje.
- **Impact:** Dług techniczny. Nie blokuje funkcjonalności, ale zwiększa risk regresji (jeden review nie wystarczy — trzeba uruchomić oba targety) + 2× czas CI + utrudnia świadome projektowanie API biblioteki.
- **Status:** OPEN. P2 — blokuje v0.4 (clean architecture przed mobile). Decyzja Opcja A vs B vs C → Faza α lub β (wstawić jako β.f lub γ.a-pre, do decyzji).

---

## P3 — Drobne UX / kosmetyka

### P3-002 — 23 prod unwrap/expect — triage

- **Wykryto:** 2026-05-17, Task 2 Fazy 0 (audit unwrap/expect).
- **Status raw vs prod:** 368 raw, ale po odfiltrowaniu `#[cfg(test)]` tail to **23** (audit report wcześniej zliczył 24, ale `downloader.rs:1582` jest w bloku testów). **Test count uzasadniony i konsekwentny — `unwrap` w testach OK.**
- **Triage:**
  - **11× UI tray binary** (`omnidrive-tray/src/main.rs`) — fail-fast na ładowaniu ikony, panic OK dla GUI app.
  - **3× mutex poison** (`cloud_guard.rs:185, 239, 273` — `.expect("session usage mutex poisoned")`) — idiomatic, mutex poison = bug w innym tasku, panic OK.
  - **3× post-guard / post-invariant**: `secure_fs.rs:72` (`.expect("retry loop must capture last error")`), `main.rs:1042` (analogicznie), `api/mod.rs:64` (`.first().copied().unwrap()` po `len() >= 3` guardzie), `device_identity.rs:51` (`.expect("local device identity must exist after upsert")`) — wszystkie po programowym invariancie.
  - **3× hardcoded Argon2** (`sharing.rs:46, 52, 75` — `.expect("valid argon2 params")` / `.expect("argon2 hash")`) — Argon2id z hardcoded params (8192, 2, 1, Some(32)). Niemożliwe do faili przy stałych parametrach + 32-byte output.
  - **2× ❗P2 (eskalacja)**:
    - `peer.rs:159` `reqwest::Client::builder().timeout(...).build().expect("peer client")` — startup crash przy nieprawidłowej konfiguracji reqwest (np. brak rustls feature, mismatched TLS backend). **Daemon nie wstanie.** Lepiej: `Result<Peer, PeerError>` propagation.
    - `ingest.rs:184` `.expect("ingest: packer initialization failed")` — analogicznie, packer init może faili przy złych params. Hot path = ingest pipeline, crash blokuje cały watcher.
- **Fix scope (per kategoria):**
  - UI tray + idiomy: nic do roboty.
  - Argon2 hardcoded: zostawić (sanity expect).
  - **Eskalowane (2):** zrefaktorować `peer.rs::Peer::new` i `ingest.rs::IngestWorker::new` do zwrotu `Result<Self, E>` zamiast panic. Wymaga zmiany sygnatury wywoływaczy (callerze już mają `?` Pattern).
- **Status:** OPEN dla 2 eskalowanych do P2 — pozostałe 21 udokumentowane jako świadome decyzje. **Eskalowane 2 → Faza β** (kandydat do β.f jako P2-003 quick wins batch, do decyzji).

---

## Closed

### Faza β — Task 0: Crypto Debt Elimination (2026-06-06, dyrektywa ZERO DŁUGU TECHNICZNEGO)

Plan: `docs/superpowers/plans/2026-06-06-beta-task0-crypto-debt-elimination.md`. Wszystkie 3 findings QG5 naprawione TDD subagent-driven przed jakąkolwiek logiką sieciową β. Bramka `--all-targets` (oba tryby) + core 28 + angeld 142 lib zielone. Bez bumpu wersji (v0.3.27).

- ~~**P2-006 (F-1)** — `revoke_device` nie NULLuje `wrapped_vault_key_kyber` (niekompletna rewokacja hybrydowa)~~ → **FIXED** (`d0c03ce` + test-strengthen `900a92e`). SQL czyści teraz OBIE kolumny wrapu w jednym atomowym UPDATE; `kyber_public_key` świadomie zostaje (klucz publiczny). Test `revoke_device_nulls_both_wraps` (oba wrapy NULL + generation NULL + public key survives).
- ~~**P3-003 (F-2)** — V2 chunk nie rekomputuje chunk_id po dekrypcji~~ → **FIXED** (`3053216`). Nowy `decrypt_chunk_v2_verified` (rekomputuje `HMAC(DEK, plaintext)`, parytet z V1) wpięty w daemon read-path (downloader, z DB-autorytatywnym chunk_id). **FFI/share-link (`ffi_decrypt_chunk_v2`) i `migrator.rs` świadomie nietknięte** (browser nie ma manifestu). Testy: roundtrip OK + wrong-id → `ChunkIdMismatch`.
- ~~**P3-004 (F-3)** — świeży vault na słabszym parameter_set 1~~ → **FIXED** (`5cd36bd` + cfg-gate `03f276c`). `ensure_vault_config` tworzy świeże vaulty od razu na parameter_set 2 (m=256 MiB) → brak okna słabszego KDF + brak podwójnego Argon2id przy 1. unlocku. Logika re-key migracji v1→v2 nietknięta i nadal testowana (test_pool_v1 jawnie seeduje v1). `DEFAULT_*` consts → `#[cfg(test)]` (legacy v1, test-only). Testy: `fresh_vault_starts_at_target_param_set` + `fresh_vault_needs_no_kdf_migration`.

### Faza α — Crypto Hardening (v0.3.24–v0.3.27, zamknięte 2026-06-06)

- ~~**P1-001 + P1-005** — Graft pomija krytyczne pola krypto (`vault_state.encrypted_vault_key`/`vault_key_generation` + `data_encryption_keys`) → różne EVK/safety-numbers + `aes-gcm operation failed` cross-device~~ → **FIXED w α.C.b** (HEAD `226ee72`, v0.3.27). `graft_restored_metadata_snapshot` rozszerzony o pełen identity bundle (vault_state EVK+gen+legacy_read_key, `data_encryption_keys`, `vault_recovery_keys`) w tx `BEGIN IMMEDIATE`. DoD Rust gate zamknięty in-process: joined EVK == source + safety_numbers identyczne (P1-005) + grafted DEK unwrapuje ten sam plaintext (P1-001). **Live SMOKE Dell↔Lenovo (C3/D7) = osobna akceptacja operacyjna, NIE blokuje zamknięcia kodu.**
- ~~**P1-006** — `/api/auth/logout` nie blokuje vaulta (klucze zostają w RAM)~~ → **FIXED w α.A.a** (commit `ed35ecb`, v0.3.24). `post_auth_logout` woła `vault_keys.lock()` PRZED `delete_user_session` + teardown CF/dysku. SMOKE H1 4/4 PASS na Lenovo.
- ~~**P2-004** — Brak auto-lock po idle~~ → **FIXED w α.A.b** (v0.3.25). Konfigurowalny idle timeout (`vault.auto_lock_idle_minutes`, default 15) + Win+L hook (`WM_WTSSESSION_CHANGE`) + UI chip/settings + `lock_flow::force_lock_and_dismount`. Bug ACL idle-timer reset znaleziony i naprawiony (`8e0d116`). SMOKE H2/H3 PASS live.
- ~~**P2-005** — Brak Zeroize na temp kopiach kluczy~~ → **FIXED w α.A.c** (HEAD `285b913`, v0.3.26). `KeyBytes` newtype z `#[derive(Zeroize, ZeroizeOnDrop)]` + redacted Debug + non-Copy + buildery in-place. SMOKE H4 memdump: after-lock = 0 trafień known-key.
- ~~**P3-001** — AAD pusty (`&[]`) na chunk encrypt/decrypt — niespecyfikowane w crypto-spec~~ → **FIXED w α.D.a** (HEAD `c502bb1`). Świadoma decyzja udokumentowana w `docs/crypto-spec.md §12` (AAD semantics): `&[]` chunki = WebCrypto Tryb B compat; `user_id` OAuth = cross-user tampering protection; trade-off cross-file swap vs share-link. Doc-only, brak zmian w kodzie. (Defense-in-depth follow-up rekomputacji chunk_id w V2 → naprawiony osobno jako **P3-003** w β Task 0.)

### v0.3.23

- ~~Dell po join-existing dostaje 403 na każdym chronionym endpoincie~~ → FIXED v0.3.23 (graft kopiuje users/devices/vault_members + ensure_local_device_in_vault)
- ~~Safety numbers Dell ≠ Lenovo (różne user_id)~~ → FIXED v0.3.23 (Dell adopts owner user_id ze snapshot)
- ~~MultiDevice tab Della pokazuje tylko Della~~ → FIXED v0.3.23 (graft kopiuje devices)
- ~~Diagnostyka „Limity dzienne ERROR"~~ → FIXED v0.3.23 (dodany endpoint `/api/diagnostics`)
- ~~Sidebar link „Diagnostyka" otwiera Przegląd~~ → FIXED v0.3.21 (dodano `'diagnostyka'` do `VALID_VIEWS`)
- ~~Wyloguj nie działa po join~~ → FIXED v0.3.21 (token handoff przez sessionStorage)

### v0.3.22

- ~~Token wystawiany dla user_id którego nie ma w vault_members~~ → FIXED (był wstępem do prawdziwego fix v0.3.23)

### v0.3.21

- ~~Brak session_token po join-existing → wszystkie chronione endpointy 401/403~~ → FIXED (post_join_existing zwraca session_token; frontend handoff przez sessionStorage)
