# OmniDrive — Known Issues Tracker

> **Single source of truth dla bugów.** Ten plik (nie GitHub Issues, nie STATUS.md) trzyma listę otwartych problemów z priorytetyzacją.
>
> **Ostatnia aktualizacja:** 2026-05-17
> **Aktualna wersja:** v0.3.23

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

### P1-001 + P1-005 (POŁĄCZONE) — Graft pomija krytyczne pola krypto vault_state + data_encryption_keys

- **Wykryto:** v0.3.22 (P1-001 hydration fail) + v0.3.23 wieczór (P1-005 safety numbers mismatch)
- **CONFIRMED 2026-05-10 wieczór:** porównanie `key_generation` przez `/api/vault/safety-numbers` na obu maszynach:
  - Lenovo: `key_generation=4`, mnemonic `town write alley critic unusual topple…`, safety `58975 53274 06638 …`
  - Dell:   `key_generation=1`, mnemonic `armed fiber cave strategy alert market…`, safety `03018 46227 27488 …`
  - user_id identyczny (`bb3cb95e-…`) — graft `users`/`devices`/`vault_members` z v0.3.23 OK
- **Root cause (jednoznaczny):** `db.rs::graft_restored_metadata_snapshot` (~linia 1830) kopiuje z `vault_state` TYLKO 3 pola: `master_key_salt`, `argon2_params`, `vault_id`. **Pomija `encrypted_vault_key` i `vault_key_generation`** (oraz wszelkie inne pola krypto w `vault_state`). Dell po grafcie używa swojego lokalnego gen=1 VK z bootstrap, nie Lenovo's gen=4 VK. KEK jest poprawny (salt+params grafted), ale unwrap zwraca losowy gen=1 VK Della, nie Lenovo's gen=4. Stąd:
  - Różny EVK → różne `safety_numbers` (P1-005)
  - DEK chunków (wrapped Lenovo's gen=4 VK) nie unwrap pod Dell's gen=1 → fallback tworzy gen=1 DEK → próba decrypt obcych chunków → `aes-gcm operation failed` (P1-001)
- **Impact:** **P0-style severity** dla single-user-multi-device (główny use case v0.4) — multi-device nie działa funkcjonalnie. UI pokazuje device w MultiDevice tab (cosmetic ok), ale ŻADEN plik nie da się pobrać na Dellu, a UI security verification (safety numbers) bezużyteczne. Nadal P1 bo pojedyncze urządzenie działa, tylko cross-device broken.
- **Fix scope (Faza α.4 z roadmapy v0.4):** Rozszerzyć graft o pełen „identity bundle" krypto:
  1. `vault_state.encrypted_vault_key` + `vault_key_generation` + `previous_envelope_key` (cała tabela poza KDF params i vault_id)
  2. Cała tabela `data_encryption_keys` (DEK per-plik, wrapped pod source's VK)
  3. Cała tabela `recovery_keys` (BIP-39, jeśli istnieje, Sesja 34.6a)
  4. **Audit pozostałych tabel** w `docs/crypto-spec.md` żeby nie zostawić jeszcze jakiegoś pola
  5. Test e2e: Lenovo wgra plik → Dell join → Dell otwiera plik z O:\ → checksum match → safety numbers identyczne na obu
- **Status:** OPEN. **Faza α.4** roadmapy v0.4 (po Argon2id bump α.1, ML-KEM α.2, X25519 α.3). Faza α formalnie wystartuje po Fazie 0 (QA Foundation) — nie robimy hot-fix v0.3.24, bo trzeba to zrobić systematycznie z testem e2e (kluczowy flow F5).

### P1-002 — Lenovo nie widzi Della w MultiDevice po join

- **Wykryto:** v0.3.23 Dell smoke test, MultiDevice tab Lenovo pokazuje tylko siebie
- **CONFIRMED 2026-05-10 wieczór:** Dell po v0.3.23 join-existing pokazuje OBA urządzenia (PN-THINKPAD + PN-OFFICE) ✅ — graft `devices` działa. Lenovo daemon zweryfikowany jako v0.3.23 (curl `/api/diagnostics` zwraca pełny JSON, endpoint dodany w v0.3.23). `members_count:1` w `/api/vault/status` na Lenovo — potwierdza że Lenovo nigdy nie pobrał zaktualizowanego snapshot z Della.
- **Symptom:** Dell po join-existing wgra zaktualizowany snapshot do chmury, ale Lenovo nigdy go nie pobiera, więc nie wie o nowym device
- **Hipoteza root cause:** Daemon ma snapshot **upload** worker (`MetadataBackupWorker`) ale nie ma symetrycznego snapshot **fetch** workera dla istniejących urządzeń. Tylko join-existing flow pobiera snapshot.
- **Impact:** Multi-device awareness jednokierunkowy. Gdy ktoś z rodziny dołącza nowy laptop (v5.0), admin nie zobaczy go bez restart daemona albo manual refresh.
- **Fix scope:** Periodic snapshot fetch worker (np. co 1h) w angeld. Decyzja: tylko gdy snapshot jest nowszy + lock wokół DB (nie nadpisuj jeśli były lokalne zmiany). Może wymagać per-device sequence number / lamport clock.
- **Status:** OPEN. Planowany w **Faza β.2** roadmapy v0.4.

### P1-005 → MERGED z P1-001 (2026-05-10 wieczór)

Diagnoza zakończona. Root cause potwierdzony empirycznie: `vault_state.encrypted_vault_key` + `vault_key_generation` nie kopiowane w grafcie, plus brak kopiowania `data_encryption_keys`. To ten sam underlying bug co P1-001 — patrz wpis P1-001+P1-005 (połączone) wyżej.

### P1-003 — Snapshot upload do Scaleway zwraca AccessDenied

- **Wykryto:** v0.3.23 Dell metadata-backup status — Scaleway 403 AccessDenied dla `_omnidrive/system/metadata/snapshots/*.db.enc`
- **Symptom:** B2 OK, R2 connection reset (osobny issue), Scaleway 403. Czyli z 3 providerów tylko jeden żywy.
- **Hipoteza:** Bucket policy / access key uprawnienia do prefix `_omnidrive/system/metadata/snapshots/` — może bucket nie pozwala PUT pod system/. Inny prefix (`packs/...`) działa wg logów.
- **Impact:** Brak redundancji metadanych: jedyna kopia snapshot na B2. Awaria B2 = utrata metadata, mimo że chunki są na 3 providerach.
- **Fix scope:** Sprawdzić Scaleway IAM policy + bucket policy + key permissions. Jeśli OK, zbadać dlaczego prefix `_omnidrive/system/` jest blokowany. Naprawić konfigurację albo udokumentować workaround.
- **Status:** OPEN. **Quality Gate 2.e** ("snapshot zawsze w ≥1 sprawnym miejscu") nie spełniony, ale technically B2 jest sprawny → tolerowalne tymczasowo. P1 bo bezpieczeństwo redundancji.

### P1-004 — Snapshot upload do R2 zwraca ConnectionReset

- **Wykryto:** v0.3.23 Dell metadata-backup status — R2 `ConnectionReset (os error 10054)` przy PUT
- **Symptom:** Brak 403, brak timeout — surowy reset połączenia od R2. Może być rate-limit / WAF / connection pool issue.
- **Hipoteza:** R2 hyper-1.x compatibility issue (memory: rustls/hyper consolidation odłożona). Może `keep-alive` pool trzyma wygasłe połączenie.
- **Impact:** Tak samo jak P1-003 — brak redundancji.
- **Fix scope:** Najpierw retry z fresh connection (`force-close` po 1 ConnReset). Drugorzędnie: Batch 7 C.3 (rustls/hyper consolidation z Backlog).
- **Status:** OPEN. Powiązany z C.3 (Backlog).

---

## P2 — Performance / SLA dług

### P2-001 — Watcher mieli CPU

- **Wykryto:** Subiektywna obserwacja Przemka, brak benchmarku
- **Symptom:** `angeld.exe` w taskmgr pokazuje wysokie CPU nawet w idle (do potwierdzenia liczbowego)
- **SLA cel:** < 1% CPU idle, < 5% active (per roadmap v0.4)
- **Fix scope:** (1) Mierzenie: profiling 60s idle + 60s active. (2) Audit `angeld/src/watcher.rs` (643 linie). Sprawdzić: polling vs event-driven? debounce? batch? file system event API (Windows ReadDirectoryChangesW)?
- **Status:** OPEN. **Faza β** (po pomiarach).

### P2-002 — VFS laguje przy dużych plikach

- **Wykryto:** Subiektywna obserwacja Przemka, brak benchmarku
- **Symptom:** Otwarcie dużego pliku (>50MB?) z O:\ trwa zauważalnie długo
- **SLA cel:** Cold fetch < 2s/10MB, < 10s/100MB; warm < 100ms (per roadmap v0.4)
- **Fix scope:** (1) Benchmark: cold fetch 1MB/10MB/100MB/1GB; warm fetch tych samych. (2) Audit `angeld/src/smart_sync.rs` (2197 linii — monolit do dekomponozycji). Sprawdzić: streaming hydration czy fetch-all-then-decrypt? EC reconstruction blokująca? Cache hit path?
- **Status:** OPEN. **Faza ε** (po pomiarach).

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
- **Status:** OPEN. P2 — blokuje v0.4 (clean architecture przed mobile). Decyzja Opcja A vs B vs C → Task 2 Fazy 0 lub Faza α/β.

---

## P3 — Drobne UX / kosmetyka

*Brak otwartych. Po Fazie 0 (audit projektu) będę dopisywał.*

---

## Closed

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
