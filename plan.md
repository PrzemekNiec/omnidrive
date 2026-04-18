# OmniDrive — Plan Implementacyjny

> Ostatnia aktualizacja: 2026-04-18 | Aktualna wersja: **v0.2.0** (commit `55a2a6a`)

## Status całego projektu

| Faza | Opis | Status |
|------|------|--------|
| Phase 0 | Checkpoint kryptograficzny + crypto-spec.md | ✅ DONE |
| Epic 32.5 | Envelope Encryption (KEK→VK→DEK, migracja, rotacja) | ✅ DONE |
| Epic 35 | Ghost Shell (cfapi, ingest, hydration, shell ext, tray) | ✅ DONE |
| Epic 33 | Zero-Knowledge Link Sharing | ✅ DONE |
| Epic 34 Sesja A | Audit Trail (34.5a+b) | ✅ DONE |
| Epic 34 Sesja B | Recovery Keys BIP-39 (34.6a + B.7) | ✅ DONE |
| Epic 34 Sesja Pre-C | Fix user_id owner-{device_id} → UUID | ✅ DONE |
| Epic 36 Sesja F | UI Shell + Przegląd (Stitch layout) | ✅ DONE |
| Epic 36 Sesja G.1-G.11 | Stats endpoints + wszystkie widoki + v0.2.0 release | ✅ DONE |
| **Faza H** | Dokończenie UI quick-wins (QR, logout, audit, recovery CTA) | ✅ DONE `e4ea91f` |
| **Faza I** | Brakujące API: `/api/lock`, `/api/filesystem/policies`, sysinfo, rotation | ⬜ NEXT |
| **Faza J** | Pre-C: Pełny refactor tożsamości UUID v4 (P0 blocker OAuth) | ⬜ TODO |
| **Faza K** | Sesja C: Google OAuth2 Backend | ⬜ TODO |
| **Faza L** | Sesja D: OAuth Frontend | ⬜ TODO |
| **Faza M** | Sesja E: Safety Numbers + E2E multi-user + THREAT_MODEL | ⬜ TODO |
| **Faza N** | Cleanup dead code + Release v0.3.0 | ⬜ TODO |

**Critical path:** I → J → K → L → M → N (~8-10 dni roboczych do v0.3.0)

---

## Następna faza: I — Brakujące endpointy API

### I.1 — `POST /api/lock`
- **Pliki:** nowy `angeld/src/api/lock.rs`, `api/mod.rs`, `static/index.html` (line ~501)
- **Akcja:** session guard + drop `VaultKeyStore.unlocked_keys` + audit `vault_locked`. Odblokować przycisk "Zablokuj Skarbiec".
- **Test:** unlock → `POST /api/lock` → `GET /api/vault/status` → `locked: true`

### I.2 — `GET /api/filesystem/policies`
- **Pliki:** nowy `angeld/src/api/policies.rs` (lub w `files.rs`), `static/index.html` (line ~2215)
- **Akcja:** zwraca polityki (`readonly_paths`, `exclude_extensions`, `max_file_size_mb`) z runtime config.
- **Test:** widok Pliki → realny status FORTECA zamiast dummy "OK"

### I.3 — Realne metryki `/api/stats/system`
- **Pliki:** `angeld/src/api/stats.rs` (lines 95/100/105), `angeld/Cargo.toml`
- **Akcja:** `sysinfo = "0.30"` → realne `cpu_percent`; instrumentacja latency w `uploader.rs`/`downloader.rs`; tracking `nodes_delta` między pollami.
- **Test:** `curl localhost:8787/api/stats/system` → niezerowe CPU

### I.4 — `POST /api/keys/rotate`
- **Pliki:** nowy `angeld/src/api/rotation.rs`, `static/index.html` (line ~520)
- **Akcja:** trigger istniejącej ścieżki rotacji + audit event. Odblokować przycisk "Wymuś rotację".
- **Test:** rotacja → stare DEK-i nadal odszyfrowują

**Commit:** `feat(api): Faza I — /api/lock, /api/filesystem/policies, /api/keys/rotate, sysinfo`

---

## Faza J — Pre-C: Refactor tożsamości UUID v4 (P0, ~2 dni)

**Blocker OAuth + multi-user.** Problem: `owner-{device_id}` = kruchy identyfikator; po OAuth potrzebujemy `users.id UUID`.

### J.1 — Migracja schematu DB
- **Pliki:** nowa migracja w `angeld/src/db.rs`, `omnidrive-core/src/`
- **Akcja:** tabela `users(id TEXT PRIMARY KEY, created_at INTEGER NOT NULL)` + `devices.user_id FK`. Backfill istniejących instalacji: 1 lokalny UUID, wszystkie devices przypisane.
- **Test:** migracja idempotentna, `PRAGMA foreign_keys = ON`, baza nie pada

### J.2 — Aktualizacja modułów
- **Pliki:** `angeld/src/onboarding.rs`, `api/sharing.rs`, `vault.rs`, `api/auth.rs`
- **Akcja:** `device_id` jako owner → `user_id`. Session zawiera `user_id + device_id`.
- **Test:** `cargo test --workspace`

### J.3 — Dokumentacja
- **Pliki:** `docs/crypto-spec.md` (sekcja Identity Model), `CLAUDE.md`
- **Akcja:** opisać `user_id` vs `device_id` + flow join-existing-vault

**Commit:** `refactor(identity): Faza J — user_id UUID v4 jako owner (Pre-C)`

---

## Faza K — Sesja C: Google OAuth2 Backend (~2 dni)

### K.1 — Zależności + konfiguracja
- `angeld/Cargo.toml`: `oauth2 = "4.4"`, `openidconnect = "3.5"`
- `.env.example`: `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `OAUTH_REDIRECT_URL`

### K.2 — `GET /api/auth/google/start`
- **Plik:** nowy `angeld/src/api/oauth.rs`
- PKCE + state + CSRF → redirect do Google. State w DB z TTL 10 min.

### K.3 — `GET /api/auth/google/callback`
- Weryfikacja state, exchange code→token, `email + sub`. Upsert `users` (`google_sub`). Session z `user_id`. Redirect `/`.

### K.4 — Join flow z istniejącym kontem
- Jeśli `google_sub` w `users` → zaloguj. Nowy vault → onboarding z OAuth identity.

### K.5 — Testy integracyjne
- **Plik:** `angeld/tests/e2e_oauth.rs`
- mock `mockito` → happy path + CSRF mismatch + expired state

**Commit:** `feat(auth): Sesja C — Google OAuth2 backend`

---

## Faza L — Sesja D: OAuth Frontend (~1-1.5 dnia)

### L.1 — Przycisk "Zaloguj przez Google" w onboarding
- `angeld/static/index.html`, `wizard.js` → redirect do `/api/auth/google/start`

### L.2 — Profil w topbarze
- `static/index.html` (line ~187 — TODO OAuth user wiring)
- `GET /api/auth/session` → `{ user_id, email, picture? }` → render zamiast "Local"

### L.3 — Logout po OAuth
- Opcjonalne revoke Google refresh token przy `POST /api/auth/logout`

### L.4 — Multi-device join z OAuth identity
- Flow "Dołącz do istniejącego vault" → najpierw OAuth → potem wybór urządzenia

**Commit:** `feat(ui): Sesja D — OAuth frontend + profil użytkownika`

---

## Faza M — Sesja E: Safety Numbers + THREAT_MODEL (~1.5-2 dni)

### M.1 — Generowanie Safety Numbers
- **Pliki:** nowy `omnidrive-core/src/safety_number.rs`, `angeld/src/api/vault.rs`
- `SHA-256(user_id_A ‖ user_id_B ‖ vault_key_fingerprint)` → 60-digit decimal, 5 bloków × 12 cyfr (Signal-style)

### M.2 — Widok weryfikacji w Multi-Device
- Safety Number + QR. Przycisk "Oznaczono jako zweryfikowane" → audit event.

### M.3 — E2E multi-user test
- **Plik:** `angeld/tests/e2e_multi_user.rs`
- Alice tworzy vault → share do Bob → Bob joins → Safety Number match po obu stronach

### M.4 — `docs/THREAT_MODEL.md`
- Assets / Adversaries / Trust Boundaries / Attack Trees / Mitigations

**Commit:** `feat(crypto): Sesja E — Safety Numbers + THREAT_MODEL`

---

## Faza N — Cleanup + Release v0.3.0 (~1 dzień)

### N.1 — Dead code audyt `vault.rs`
- Usunąć realnie nieużywane. Pozostawić z `// reserved for Epic X`.

### N.2 — Module-level `#![allow(dead_code)]` audyt
- `downloader.rs`, `gc.rs`, `identity.rs`, `migrator.rs`, `onboarding.rs`, `packer.rs`, `repair.rs`, `scrubber.rs`, `uploader.rs`, `watcher.rs` → function-level `#[allow]`

### N.3 — Bump wersji do 0.3.0
- Wszystkie 6× `Cargo.toml` + `installer/omnidrive.iss` → `0.2.0 → 0.3.0` + `cargo build --release --workspace`

### N.4 — Payload + instalator
- `cp target/release/*.exe dist/installer/payload/` + `cp angeld/static/* dist/installer/payload/static/` → Inno Setup

### N.5 — Smoke test + release
- Pełny flow: unlock → share → join → verify → lock. Commit `release: v0.3.0`, push, tag.

**Commit:** `release: v0.3.0`

---

## Krytyczne pliki (fazy I-N)

| Plik | Fazy |
|------|------|
| `angeld/static/index.html` | I.1-I.4, L.1-L.4, M.2 |
| `angeld/src/api/mod.rs` | I.1, I.2, I.4, K.2 |
| `angeld/src/api/stats.rs` | I.3 |
| `angeld/src/api/auth.rs` | J.2, K.2-K.4 |
| `angeld/src/api/sharing.rs` | J.2, M.1 |
| `angeld/src/vault.rs` | I.1, N.1 |
| `angeld/src/db.rs` | J.1, J.2 |
| `angeld/src/onboarding.rs` | J.2, K.4 |
| `omnidrive-core/src/` | J.1, M.1 |
| `installer/omnidrive.iss` | N.3 |
| `docs/crypto-spec.md` | J.3 |
| `docs/THREAT_MODEL.md` | M.4 |

---

*Poniżej: pełna historia implementacji (Phase 0 → Epic 36 Sesja G).*

---

## Phase 0 — Faza G.11: Historia implementacji (DONE)

---

Sekwencja: **Phase 0 → Epic 32.5 → Epic 35 → Epic 33 → Epic 34 → Epic 36**

Każdy blok to 1-3 dni pracy. Bloki w ramach fazy są sekwencyjne (każdy buduje na poprzednim).

---

## Pre-req: Domknięcie B8 — DONE (2026-04-06, v0.1.20)

B8 zamknięty. Trzy root causes naprawione w `smart_sync.rs`:
1. `convert_directory_to_placeholder` — `CreateFileW` z `FILE_FLAG_BACKUP_SEMANTICS` (std::fs nie otwiera katalogów)
2. `create_projection_placeholder` — `ensure_placeholder_directory_chain` wywoływane bezwarunkowo (nie tylko gdy plik nie istnieje)
3. `fetch_placeholders_callback` — `CfExecute(CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS)` z zero entries (minifilter nie blokuje enumeracji)

Wynik: `dir O:\`, `dir O:\nested`, `dir O:\nested\alpha` — natychmiastowa odpowiedź na obu maszynach (Lenovo + Dell)

---

## Phase 0: Checkpoint Kryptograficzny

### P0.1: Audyt obecnego modelu szyfrowania
- Przeczytać `omnidrive-core/src/` — zrozumieć jak dziś działa KDF, szyfrowanie chunków, vault unlock
- Zmapować: co jest w `vault_state`, jak `master_key_salt` i `argon2_params` są używane
- Udokumentować obecny flow: passphrase → Argon2id → klucz → szyfrowanie chunków

### P0.2: Decyzja — algorytmy i parametry
- AES-256-GCM dla DEK (potwierdzić — już używamy AES-GCM)
- Argon2id parametry: wybrać m_cost, t_cost, p_cost (benchmark na Lenovo i Dell)
- DEK wrapping: AES-256-KW vs AES-256-GCM-SIV — decyzja z uzasadnieniem
- Dokumentacja: `docs/crypto-spec.md`

### P0.3: Decyzja — kompatybilność z WebCrypto
- Zbadać `window.crypto.subtle` — które algorytmy są dostępne cross-browser
- X25519 vs ECDH P-256 dla asymetrii — decyzja pod kątem Epic 33 (browser decrypt) i Epic 34 (key wrapping)
- Dodać wynik do `docs/crypto-spec.md`

### P0.4: Decyzja — vault_format_version schemat
- Zdefiniować wersje: v1 (obecny flat), v2 (envelope)
- Zdefiniować ścieżkę forward-compatibility: co robi daemon v2 gdy widzi v1 bazę?
- Zdefiniować rollback: co robi daemon v1 gdy widzi v2 bazę? (fail-safe refuse)
- Dodać do `docs/crypto-spec.md`

**Deliverable Phase 0:** `docs/crypto-spec.md` — DONE (2026-04-06)

Kluczowe decyzje podjęte w RFC:
- 3-warstwowa hierarchia: passphrase → KEK (HKDF) → Vault Key (losowy, AES-KW wrapped) → DEK (losowy per-plik, AES-KW wrapped) → AES-256-GCM
- AES-256-KW (RFC 3394) do wrappowania kluczy (nie AES-GCM) — brak nonce, WebCrypto-kompatybilny
- ChunkRecordPrefix V2 — ten sam rozmiar 80 bytes, `record_version=2`, random nonce, `dek_id_hint`
- DEK per-plik (nie per-chunk) — jeden secret w share URL dla Epic 33
- Lazy migration V1→V2 — nowe pliki V2, stare czytane V1, opcjonalny batch re-encryption
- Nowy crate: `aes-kw` (pure Rust, RFC 3394)

---

## Phase 1: Epic 32.5 — Envelope Encryption — DONE (2026-04-07)

### 32.5.1a-b: KEK + Vault Key — DONE
- `omnidrive-core/crypto.rs`: dodany `aes-kw` crate, `derive_kek()`, `wrap_key()`, `unwrap_key()`, `generate_random_key()`
- `db.rs`: nowe kolumny (`vault_format_version`, `encrypted_vault_key`, `vault_key_generation`), tabela `data_encryption_keys`
- `vault.rs`: unlock flow generuje/unwrapuje V2 Vault Key, `UnlockedVaultKeys.envelope_vault_key`
- 9 unit testów crypto (w tym RFC 3394 test vectors)

### 32.5.1c-d: DEK per-file + chunk encrypt V2 — DONE
- `vault.rs`: `get_or_create_dek()` — generuj/unwrapuj DEK per inode
- `packer.rs`: `encrypt_chunk_v2(dek, ...)`, `build_manifest_bytes_v2()` z `record_version=2`, `key_wrapping_algo=AES-KW`, `dek_id_hint`
- `downloader.rs`: dual-read V1/V2 (`record[4]` auto-detect), `decrypt_chunk_record(vault_key, dek)`
- 7 vault testów + roundtrip packer↔downloader test

### 32.5.2a-c: Batch Migrator V1→V2 — DONE
- `migrator.rs`: `MigrationManager` — `run_batch()` / `run_to_completion()`
- Per-pack: decrypt V1 (vault_key) → get/create DEK → re-encrypt V2 (dek) → nowy pack + shardy → stary → UNREADABLE
- `db.rs`: `get_v1_packs_for_migration()`, `count_v1_packs()`, `finalize_vault_format_v2()`
- Finalizacja: `vault_format_version = 2` gdy V1 count = 0
- Integration test: inject V1 pack → migrate → verify V2 readback

### 32.5.2d: Vault Key Rotation — DONE
- `vault.rs`: `rotate_vault_key(pool, new_passphrase)` — fresh salt → new root keys → new Vault Key → re-wrap all DEKs → bump generation
- `db.rs`: `get_all_wrapped_deks()`, `update_wrapped_dek()`, `rotate_vault_state()`
- Stare hasło natychmiast nieważne, DEKi identyczne po rotacji
- Test: create vault → encrypt → rotate → old pass fails → new pass decrypts

**Commit chain:** `9ded01a` (32.5.1a-d) → `f6286dc` (32.5.2a-c migrator) → `ad65cc2` (32.5.2d rotation)
**Test count:** 24 (15 angeld + 9 omnidrive-core)

---

## Phase 2a: Epic 35 — Ghost Shell PoC ✅ SKIPPED (ready from B8)

**Status:** SKIPPED — `smart_sync.rs` (~1900 linii) z B8 pokrywa cały PoC.
**Go/No-Go gate:** **GO** (2026-04-07). cfapi stabilne, brak potrzeby fallback na ProjFS.

### 35.0a-d: cfapi PoC — SyncRoot, hydracja, streaming, dehydracja ✅
- Wszystko zaimplementowane w `angeld/src/smart_sync.rs` podczas B8
- SyncRoot registration + connect z callbackami (FETCH_DATA, FETCH_PLACEHOLDERS, CANCEL)
- Hydracja: `fetch_data_callback` → `downloader.read_range()` → `CfExecute(TRANSFER_DATA)`
- Dehydracja: `CfUpdatePlaceholder(CF_UPDATE_FLAG_DEHYDRATE)`
- Pin state, eviction, audit/repair, shell notifications

---

## Phase 2b: Epic 35 — Full Ghost Shell

### 35.1a: Ingest State Machine — model stanów ✅
- `angeld/src/ingest.rs` — stany: `PENDING → CHUNKING → UPLOADING → GHOSTED` (+FAILED)
- Tabela `ingest_jobs` w SQLite z indeksem na `state`
- Crash recovery: CHUNKING/UPLOADING → PENDING przy restarcie
- Background worker w `tokio::select!`, diagnostics `WorkerKind::Ingest`

### 35.1b: Ingest — chunking + DEK + upload ✅
- `do_chunking()`: inode upsert → `Packer::pack_file()` (SHA-256, DEK, V2 AES-GCM, EC RS 2+1, spool, DB records)
- `do_uploading()`: polluje `summarize_pack_shards()` co 2s, timeout 600s
- UploadWorker automatycznie przetwarza queued `upload_jobs`
- Progress tracking w `ingest_jobs.bytes_processed`

### 35.1c: Ingest — atomowa zamiana na widmo ✅
- CfConvertToPlaceholder in-place + dehydrate (nie rename+create)
- Non-fatal failure — plik zostaje nietknięty
- Job cleanup: DELETE z ingest_jobs po GHOSTED
- E2E test: `ingest_pipeline_full_cycle`

### 35.1d: Hydration z chmury ✅
- Chunk-streamed transfer: peak RAM ≤ 1 chunk (~4 MB)
- `read_range_streamed<F>` z callback per-chunk → `complete_transfer_chunk` → CfExecute
- Offset slicing obsługuje niezalignowane żądania Windows
- Prefetch zachowany, stary `read_range` + `complete_transfer_success` utrzymane

### 35.1e: Ingest — failure recovery i rollback ✅
- `fail_ingest_job(job_id, error_message)` — zapisuje powód do DB
- `cleanup_failed_ingest(pool, spool_dir, job_id)` — usuwa lokalne spool files, GC zbierze cloud shards
- `POST /api/ingest/{id}/retry` — reset FAILED→PENDING (czyści error, attempt_count)
- `POST /api/ingest/{id}/cleanup` — usunięcie śmieci i joba
- `GET /api/ingest` — lista jobów ze stanem, postępem, błędami (dashboard)

### 35.2a: Shell Extension DLL — thin client
- Nowy projekt: `omnidrive-shell-ext` (C++ lub Rust DLL)
- Rejestracja jako IContextMenu handler
- DLL robi MINIMUM: wysyła HTTP request do `angeld` localhost
- Crash safety: żadna logika biznesowa w DLL

### 35.2b: Context menu — 4 poziomy ochrony
- Menu kontekstowe na plikach/folderach w O:\
- Opcje: LOKALNIE, COMBO, CHMURA, FORTECA
- Kliknięcie → POST do angeld z polityką i ścieżką pliku
- Angeld stosuje politykę (zmiana sync_policy, ingest jeśli potrzebny)

### 35.2c: Natywne stany cfapi (SKASOWANE custom overlays)
- **DECYZJA ARCHITEKTONICZNA (2026-04-13):** Zero custom `IShellIconOverlayIdentifier`. Używamy WYŁĄCZNIE natywnych stanów i ikon Windows Cloud Files API (cfapi).
- Powód: slot overlay'i w Windowsie jest ograniczony (15 globalnie, zazwyczaj 4 wolne), OneDrive/Dropbox/Google Drive już je zajmują — własne nakładki prowadzą do konfliktów i bugów. cfapi dostarcza natywne wizualizacje (chmurka, ptaszek, pobieranie) bez rejestracji w rejestrze.
- Zakres: upewnić się, że `CfSetPlaceholderState` / `CfUpdatePlaceholder` poprawnie raportują stany (`CF_PLACEHOLDER_STATE_IN_SYNC`, `PARTIAL`, `PARTIALLY_ON_DISK`) i pin state (`CfSetPinState`), żeby Eksplorator rysował natywne ikony
- Test: zmień stan placeholdera → natywna ikona cfapi w Eksploratorze się zmienia (bez własnych DLL overlay)

### 35.3: System Tray Companion
- **Cel:** Lekka aplikacja w Rust (biblioteka `tray-item` lub `windows-rs` Shell_NotifyIcon), działająca niezależnie od angeld
- **Architektura:** Osobny crate (`omnidrive-tray`), osobny proces — thin client do API angeld, zero logiki biznesowej
- **Monitoring:** Polling `GET /api/health` na 127.0.0.1:8787 co 5s
- **Ikona tray:**
  - Zielona — Połączono (daemon healthy, vault unlocked)
  - Żółta — Ostrzeżenie (daemon healthy, vault locked lub degraded providers)
  - Czerwona — Offline (daemon nie odpowiada)
- **Menu kontekstowe:**
  - Otwórz Skarbiec (O:) — `explorer.exe O:\`
  - Otwórz Dashboard — domyślna przeglądarka na `http://127.0.0.1:8787`
  - Restart Daemona — zabicie procesu angeld (`taskkill`) + ponowne uruchomienie (re-spawn)
  - Wymuś Odświeżenie Eksploratora — `SHChangeNotify(SHCNE_UPDATEDIR)` na O:\ (preferowane) lub `taskkill /IM explorer.exe && explorer.exe` jako fallback przy blokadzie dysku O:
- **Autostart:** Rejestracja w `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` obok angeld
- **Instalacja:** Dodać do Inno Setup payload, uruchamiać po instalacji razem z angeld

---

## Phase 3: Epic 33 — Zero-Knowledge Link Sharing

### 33.1a: Fragment URI — format i generacja
- Zdefiniować format linku: `https://skarbiec.app/{file_id}#{base64url(DEK)}`
- Endpoint: `POST /api/share/create` → przyjmuje inode_id, zwraca share URL
- DEK z `data_encryption_keys` → base64url encode → fragment URI
- Share metadata w DB: `shares` tabela (share_id, inode_id, created_at, ttl, max_downloads)

### 33.1b: Share backend — serving encrypted chunks
- Endpoint: `GET /api/share/{share_id}/chunks/{chunk_index}`
- Serwuje zaszyfrowane chunki (serwer NIE ma DEK)
- Rate limiting i abuse protection
- TTL enforcement: expired shares zwracają 410 Gone

### 33.1c: Share options — TTL i burn-after-read
- TTL: opcjonalny czas życia linku (1h, 24h, 7d, 30d, unlimited)
- Burn-after-read: `max_downloads = 1`, po pierwszym pobraniu share jest disabled
- Dashboard: lista aktywnych shares z opcją revoke

### 33.2a: Web Receiver — static page
- Statyczna strona HTML/JS hostowana (lub embedded w angeld)
- Parsuje fragment URI → wyciąga DEK
- UI: "Deszyfrowanie pliku..." z progress bar

### 33.2b: Web Receiver — WebCrypto decrypt
- `window.crypto.subtle.importKey` + `decrypt` z DEK z URL
- Streaming: `ReadableStream` → `TransformStream` (decrypt) → `WritableStream`
- Limit: pliki >500 MB → chunked download z progresywnym deszyfrowaniem
- Test: share plik → otwórz link w przeglądarce → pobierz → porównaj

### 33.2c: Web Receiver — UX polish
- Progress bar deszyfrowania
- "Zapisz jako..." przycisk (FileSaver.js lub native download)
- Obsługa błędów: wrong key, expired link, network error
- Mobile-friendly layout

---

## Phase 4: Epic 34 — Family Cloud

Przejście z single-user vault na multi-user z zachowaniem Zero-Knowledge.
Gemini suggestions włączone: device revocation (P0), lazy re-wrapping (P0), audit trail (P1), safety numbers (P2).

### Faza 34.0: Schemat DB i model danych (fundament)

#### 34.0a: Tabele tożsamości i członkostwa
Nowe tabele w `db.rs::initialize_database()`:

```sql
CREATE TABLE IF NOT EXISTS users (
    user_id TEXT PRIMARY KEY,               -- UUID v4
    display_name TEXT NOT NULL,
    email TEXT,                              -- opcjonalny, z OAuth
    auth_provider TEXT NOT NULL DEFAULT 'local', -- 'local' | 'google'
    auth_subject TEXT,                       -- Google sub claim (unikalny per provider)
    created_at INTEGER NOT NULL,
    UNIQUE(auth_provider, auth_subject)
);

CREATE TABLE IF NOT EXISTS devices (
    device_id TEXT PRIMARY KEY,             -- reuse istniejącego local_device_identity.device_id
    user_id TEXT NOT NULL REFERENCES users(user_id),
    device_name TEXT NOT NULL,
    public_key BLOB NOT NULL,               -- X25519 public key (32 bytes)
    wrapped_vault_key BLOB,                 -- AES-KW(ECDH_shared, Vault Key) — NULL dopóki owner nie zaakceptuje
    vault_key_generation INTEGER,           -- która generacja VK
    revoked_at INTEGER,                     -- NULL = aktywne, timestamp = odwołane
    last_seen_at INTEGER,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS vault_members (
    user_id TEXT NOT NULL REFERENCES users(user_id),
    vault_id TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'member',     -- 'owner' | 'admin' | 'member' | 'viewer'
    invited_by TEXT REFERENCES users(user_id),
    joined_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, vault_id)
);

CREATE TABLE IF NOT EXISTS audit_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp INTEGER NOT NULL,
    actor_user_id TEXT,
    actor_device_id TEXT,
    action TEXT NOT NULL,                    -- 'invite', 'join', 'remove', 'revoke_device', 'rotate_vk', 'change_role'
    target_user_id TEXT,
    target_device_id TEXT,
    details TEXT,                            -- JSON z dodatkowymi danymi
    vault_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS invite_codes (
    code TEXT PRIMARY KEY,                   -- 128-bit random, base64url
    vault_id TEXT NOT NULL,
    created_by TEXT NOT NULL REFERENCES users(user_id),
    role TEXT NOT NULL DEFAULT 'member',
    max_uses INTEGER NOT NULL DEFAULT 1,
    used_count INTEGER NOT NULL DEFAULT 0,
    expires_at INTEGER,
    created_at INTEGER NOT NULL
);
```

- **Deliverable:** Migracja schematu, structy Rust, CRUD functions w `db.rs`
- **Testy:** unit testy CRUD lifecycle per tabela

#### 34.0b: Migracja istniejącego vault na multi-user
- Obecny vault nie ma pojęcia `user_id` — jest single-owner
- **Migracja:** przy starcie daemona:
  1. Jeśli tabela `users` jest pusta i vault jest unlocked → auto-create owner user + device
  2. `local_device_identity.device_id` staje się `devices.device_id`
  3. Owner dostaje `wrapped_vault_key = NULL` (ma VK z passphrase, nie potrzebuje ECDH)
  4. `vault_members` entry z `role = 'owner'`
- **Backward compat:** single-user vault działa bez zmian, multi-user jest opt-in

---

### Faza 34.1: Kryptografia asymetryczna i invite flow

#### 34.1a: X25519 key pair per device
- Nowy moduł: `angeld/src/identity.rs`
- Każde urządzenie generuje parę X25519 przy pierwszym starcie:
  - `x25519_dalek` crate: `StaticSecret` + `PublicKey`
  - Private key → encrypted at rest: `AES-256-GCM(KEK, private_key)` (KEK z passphrase)
  - Public key → plaintext w `devices.public_key` (32 bytes)
- Nowe kolumny w `local_device_identity`:
  ```sql
  ALTER TABLE local_device_identity ADD COLUMN encrypted_private_key BLOB;
  ALTER TABLE local_device_identity ADD COLUMN public_key BLOB;
  ```
- **Testy:** generate → persist → reload → sign/verify roundtrip

#### 34.1b: Invite flow (owner → member)
- Owner tworzy invite code: `POST /api/vault/invite`
  - Generuje 128-bit random code
  - Zapisuje do `invite_codes` z TTL + role
  - Zwraca code (wyświetlany w UI, kopiowany ręcznie/QR)
- Member akceptuje invite: `POST /api/vault/join`
  1. Wysyła: invite code + swoją public key X25519
  2. Server sprawdza code validity (TTL, max_uses)
  3. Server dodaje usera do `users` + `devices` (z public key, BEZ wrapped VK)
  4. Server notyfikuje ownera: "nowe urządzenie czeka na akceptację"
- Owner akceptuje: `POST /api/vault/accept-device/{device_id}`
  1. Owner's daemon: `ECDH(owner_private, member_public)` → `shared_secret`
  2. `HKDF(shared_secret, "vault-key-wrap-v1")` → `wrapping_key`
  3. `AES-256-KW(wrapping_key, Vault_Key)` → `wrapped_vault_key`
  4. Zapisuje `wrapped_vault_key` w `devices` dla nowego device
  5. Audit log: `action = 'invite'`
- Member odbiera wrapped VK: `GET /api/vault/my-wrapped-key`
  1. `ECDH(member_private, owner_public)` → `shared_secret`
  2. `HKDF(shared_secret, "vault-key-wrap-v1")` → `wrapping_key`
  3. `AES-KW-Unwrap(wrapping_key, wrapped_vault_key)` → Vault Key
  4. Vault Key cached in memory → member ma pełny dostęp
- **Testy:** full invite → accept → unwrap → decrypt file roundtrip

#### 34.1c: Multi-device key distribution
- Ten sam user, nowe urządzenie → analogiczny flow jak invite:
  1. Nowe urządzenie generuje X25519 pair
  2. User loguje się (passphrase lub OAuth)
  3. Istniejące urządzenie wrappuje VK dla nowej public key
  4. Nowe urządzenie unwrappuje i jest gotowe
- Automatyzacja: jeśli user jest already member i ma ≥1 active device → auto-accept (bez ręcznej akceptacji ownera)
- **Testy:** existing user + new device → automatic VK distribution

---

### Faza 34.2: Device revocation i lazy re-wrapping

#### 34.2a: Device revocation
- Endpoint: `POST /api/devices/{device_id}/revoke`
- Flow:
  1. Sprawdź ACL: tylko owner/admin może revoke'ować
  2. `UPDATE devices SET revoked_at = ? WHERE device_id = ?`
  3. Usuń `wrapped_vault_key` z revoked device (natychmiast traci dostęp do nowych operacji)
  4. Audit log: `action = 'revoke_device'`
  5. Trigger: Vault Key rotation (→ 34.2b)
- Revoked device na następnym API call → 403 + komunikat "device revoked"
- **Ważne:** revoke device ≠ revoke user. User z innymi aktywnymi urządzeniami zachowuje dostęp
- **Testy:** revoke → stary wrapped VK invalid → nowy VK distributed do remaining devices

#### 34.2b: Vault Key rotation z lazy re-wrapping
- Trigger: device revocation, user removal, explicit rotation (passphrase change)
- **Immediate phase (synchronous, <1s):**
  1. Generuj nowy random Vault Key
  2. Wrap nowy VK z KEK ownera → update `vault_state.encrypted_vault_key`
  3. Bump `vault_key_generation`
  4. Re-wrap VK dla każdego active device (`devices WHERE revoked_at IS NULL`)
  5. Nowe pliki od teraz szyfrowane nowym VK
- **Background phase (async, batch):**
  1. Nowa tabela:
     ```sql
     CREATE TABLE IF NOT EXISTS dek_rewrap_queue (
         dek_id INTEGER PRIMARY KEY REFERENCES data_encryption_keys(dek_id),
         source_vk_generation INTEGER NOT NULL,
         target_vk_generation INTEGER NOT NULL,
         status TEXT NOT NULL DEFAULT 'PENDING',  -- PENDING | DONE | FAILED
         attempted_at INTEGER,
         error TEXT
     );
     ```
  2. Po rotacji: INSERT INTO dek_rewrap_queue wszystkie DEK-i z `vault_key_gen < new_generation`
  3. Background task (analogiczny do token cleanup): co 2s batch 500 DEK-ów:
     - Unwrap DEK starym VK (z `vault_key_gen` → lookup VK tej generacji)
     - Wrap DEK nowym VK
     - Update `data_encryption_keys.wrapped_dek` + `vault_key_gen`
     - DELETE z queue
  4. Progress tracking: `GET /api/vault/rewrap-status` → `{total, done, pending}`
- **Compat:** Daemon zna aktualny VK + poprzedni VK (trzyma oba w pamięci aż queue = 0)
  - Read: chunk → manifest → `dek.vault_key_gen` → wybierz właściwy VK do unwrap
  - Write: zawsze nowy VK
- **Testy:** rotate → verify reads work during rewrap → verify all DEKs migrated → old VK purged

#### 34.2c: User removal (full revocation)
- Endpoint: `POST /api/vault/members/{user_id}/remove`
- Flow:
  1. Revoke ALL devices tego usera
  2. DELETE z `vault_members`
  3. Trigger VK rotation + lazy re-wrap (jak 34.2b)
  4. Audit log: `action = 'remove'`
- **Test:** remove user → all their devices rejected → VK rotated → remaining members work

---

### Faza 34.3: Uwierzytelnianie i sesje

#### 34.3a: Local auth (passphrase-based, domyślne)
- **Bez zmian w core flow:** passphrase → Argon2id → master_key → KEK → unwrap VK
- Nowe: session token po unlock (analogicznie do share tokens):
  ```sql
  CREATE TABLE IF NOT EXISTS user_sessions (
      token TEXT PRIMARY KEY,
      user_id TEXT NOT NULL REFERENCES users(user_id),
      device_id TEXT NOT NULL,
      created_at INTEGER NOT NULL,
      expires_at INTEGER NOT NULL
  );
  ```
- Po unlock → generuj session token (256-bit random, base64url) → cookie/header
- API endpoints sprawdzają session token zamiast ponownego unlockowania
- TTL: 24h, odnawialny
- **Testy:** unlock → get session → API call with session → expire → 401

#### 34.3b: Google OAuth2 (opcjonalny, convenience layer)
- Nowe dependencies: `oauth2 = "4"`, `jsonwebtoken = "9"`
- Flow:
  1. `GET /api/auth/google` → redirect do Google OAuth consent screen
  2. Google callback → `GET /api/auth/google/callback?code=...`
  3. Exchange code → Google access token → userinfo endpoint → email + sub
  4. Lookup/create user w `users` (by `auth_provider = 'google'` + `auth_subject = sub`)
  5. Generuj session token
- **WAŻNE:** Google OAuth = tożsamość. NIE = klucz kryptograficzny.
  - Po OAuth login daemon nadal wymaga vault passphrase do unlock VK
  - OAuth tylko identyfikuje usera (np. "ten request pochodzi od user_id=X")
  - UX: "Zaloguj przez Google" → session → osobne "Odblokuj Skarbiec" z passphrase
- Env vars: `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `GOOGLE_REDIRECT_URI`
- **Testy:** mock OAuth flow → user created → session works → vault still locked

---

### Faza 34.4: ACL i permissions

#### 34.4a: Role-based access control
- Role (od najwyższej):
  | Rola | Uprawnienia |
  |------|-------------|
  | `owner` | Wszystko + delete vault + manage members + rotate VK |
  | `admin` | Invite/remove members + revoke devices + read/write |
  | `member` | Read + write + share links |
  | `viewer` | Read only |
- Enforcement: middleware w `api.rs` — extract `user_id` z session, lookup role w `vault_members`
- Moduł: `angeld/src/acl.rs` — `check_permission(pool, user_id, vault_id, required_role) -> Result<()>`
- **V1:** Vault-level roles only (nie per-folder). Per-folder permissions = future Epic 34.5
- **Testy:** viewer can read, viewer cannot write, member can share, only owner can invite

#### 34.4b: API route protection
- Nowa warstwa middleware:
  ```rust
  async fn require_role(role: &str, session: &Session, pool: &SqlitePool) -> Result<(), ApiError>
  ```
- Mapowanie:
  | Endpoint | Minimum role |
  |----------|--------------|
  | `GET /api/files/*` | viewer |
  | `POST /api/files/*` | member |
  | `POST /api/files/*/share` | member |
  | `POST /api/vault/invite` | admin |
  | `POST /api/vault/members/*/remove` | admin |
  | `POST /api/devices/*/revoke` | admin |
  | `POST /api/vault/rotate-key` | owner |
  | `GET /api/audit-logs` | admin |
- **Testy:** role-based access matrix test (parametric)

---

### Faza 34.5: Audit trail i dashboard

#### 34.5a: Audit logging
- Każda operacja zarządzania → INSERT do `audit_logs`:
  - invite, join, remove, revoke_device, rotate_vk, change_role
- `details` field: JSON z kontekstem (np. `{"reason": "lost device", "device_name": "Lenovo"}`)
- **Zero performance impact:** INSERT jest fire-and-forget (nie blokuje response)
- Retention: domyślnie 90 dni, configurable w `system_config`

#### 34.5b: Audit log API + UI
- `GET /api/audit-logs?limit=50&offset=0` → lista eventów
- `GET /api/audit-logs?action=revoke_device` → filtrowanie po akcji
- Dashboard UI: nowa sekcja "Historia zmian" w panelu administracyjnym
- Kolumny: Data, Kto, Akcja, Cel, Szczegóły

---

### Faza 34.6: Recovery keys (paper backup)

#### 34.6a: BIP-39 mnemonic backup
- Dependency: `bip39 = "2"` (lub ręczna implementacja — wordlist + checksum)
- Flow:
  1. Owner generuje recovery key: `Vault Key → BIP-39 encode → 24 słowa`
  2. UI: "Zapisz te 24 słowa na papierze. Bez nich nie odzyskasz danych."
  3. Confirmation: user wpisuje 3 losowe słowa jako dowód zapisania
  4. Recovery: `24 słowa → BIP-39 decode → Vault Key → unlock vault`
- Przechowywanie: hash mnemonic w DB (do weryfikacji, nie do odtworzenia)
- **NIGDY nie logować mnemonic** — zero-knowledge rule
- **Testy:** generate → encode → decode → roundtrip, recovery flow e2e

#### 34.6b: Safety Numbers (P2 — later)
- Po zaakceptowaniu invite: obie strony widzą "Safety Number" (hash public keys)
- Format: 6 grup po 5 cyfr (np. `12345 67890 12345 67890 12345 67890`)
- Porównanie out-of-band (telefon, osobiście) potwierdza brak MITM
- **Nie blokuje niczego** — pure UI feature, implementacja po core Epic 34

---

### Podsumowanie Epic 34 — kolejność implementacji

| Sub-epic | Zakres | Zależności | Priorytet | Status |
|----------|--------|------------|-----------|--------|
| **34.0a-b** | Schemat DB + migracja single→multi | Brak | **PIERWSZY** | ✅ DONE (2026-04-08) |
| **34.1a** | X25519 key pair per device | 34.0 | 2 | ✅ DONE (2026-04-08) |
| **34.1b** | Invite flow (ECDH + AES-KW) | 34.1a | 3 | ✅ DONE (2026-04-08) |
| **34.1c** | Multi-device key distribution | 34.1b | 4 | ✅ DONE (2026-04-09) |
| **34.2a** | Device revocation | 34.0 | 5 | ✅ DONE (2026-04-09) |
| **34.2b** | Lazy VK rotation + re-wrap queue | 34.2a, 34.1b | 6 | ✅ DONE (2026-04-09) |
| **34.2c** | User removal | 34.2a, 34.2b | 7 | ✅ DONE (2026-04-09) |
| **34.3a** | Session tokens (local auth) | 34.0 | 5 (parallel z 34.2) | ✅ DONE (2026-04-09) |
| **34.3b** | Google OAuth2 | Fix user_id | 8 (opcjonalny) | ⬜ Sesja C+D |
| **34.4a** | ACL + route protection | 34.3a | 9 | ✅ DONE (2026-04-09) |
| **Refactor** | ApiError migration + cleanup | 34.4a | — | ✅ DONE (2026-04-11) |
| **E2E fix** | 3 e2e testy (reconciliation, recovery, scrubber) | Refactor | — | ✅ DONE (2026-04-11) |
| **34.5a-b** | Audit trail + UI | 34.0 | P1 | ⬜ Sesja A |
| **34.6a** | Recovery keys (BIP-39) | 34.1b | P1 | ⬜ Sesja B |
| **Fix ID** | Naprawa owner-{device_id} → UUID | 34.0b | **P0 blocker** | ⬜ Sesja Pre-C |
| **34.6b** | Safety Numbers | 34.1b | P3 | ⬜ Sesja E |
| **E2E multi** | Multi-user lifecycle test | 34.2c | P1 | ⬜ Sesja E |
| **THREAT** | THREAT_MODEL.md (model zagrożeń + strategia platform) | — | P1 | ⬜ Sesja E |

### Refactoring: Unified ApiError + API module split — DONE (2026-04-09 → 2026-04-11)

Dwie fazy porządkowania po Epic 34:

**Faza 1 (2026-04-09):**
- CI: GitHub Actions (`windows-latest`, cargo check + clippy + test)
- Clippy cleanup: 85 warnings → 0 (dead code, unused imports, redundant patterns)
- Split monolitycznego `api.rs` (5026 linii) → `api/` directory (8 modułów + `mod.rs`)
- Początkowy `ApiError` enum w `api/error.rs` (7 wariantów)
- 6 e2e testów zaktualizowanych o session token auth

**Faza 2 (2026-04-11):**
- `ApiError` przeniesiony do `api_error.rs` (crate root) — rozwiązuje problem widoczności `lib.rs` vs `main.rs`
- `api/error.rs` → re-export z `crate::api_error::ApiError`
- Rozszerzenie do 10 wariantów: BadRequest, Unauthorized, Forbidden, NotFound, Conflict, Gone, Locked, Internal, BadGateway, ServiceUnavailable
- `From` impls: `sqlx::Error`, `std::io::Error`, `Box<dyn Error>`, `Box<dyn Error + Send + Sync>`
- `acl.rs` → zwraca `Result<_, ApiError>` zamiast `Result<_, Response>`
- Wszystkie 7 plików handlerów zmigrowane: `auth.rs`, `diagnostics.rs`, `files.rs`, `vault.rs`, `sharing.rs`, `onboarding.rs`, `maintenance.rs`
- Usunięto `internal_server_error()` i `io_error()` helpery z `mod.rs`
- Server-level `ApiError` przemianowany na `ApiServerError`
- Wynik: 0 warnings (check + clippy), wszystkie testy pass

---

### Nowe dependencies

| Crate | Wersja | Cel |
|-------|--------|-----|
| `x25519-dalek` | `2` | X25519 ECDH key exchange |
| `oauth2` | `4` | Google OAuth2 flow (opcjonalny) |
| `jsonwebtoken` | `9` | JWT parsing (opcjonalny) |
| `bip39` | `2` | Recovery key mnemonic |
| `uuid` | `1` | User ID generation |

### Estymowane nowe pliki

| Plik | Rola |
|------|------|
| `angeld/src/identity.rs` | X25519 key pair management, ECDH, key wrapping |
| `angeld/src/acl.rs` | Permission checks, role enforcement |
| `angeld/src/auth.rs` | Session management, OAuth2 flow |
| `angeld/src/rewrap.rs` | Background DEK re-wrapping worker |

### Test count target: ~30 nowych testów
- 8 testy DB CRUD (users, devices, vault_members, audit_logs, invites, sessions, rewrap_queue)
- 5 testów crypto (X25519 roundtrip, ECDH+AES-KW invite flow, recovery BIP-39)
- 6 testów invite flow (create invite, join, accept, reject, expire, multi-device)
- 4 testy revocation (device revoke, user remove, VK rotation trigger, lazy rewrap)
- 4 testy ACL (owner/admin/member/viewer matrix)
- 3 testy auth (session lifecycle, expire, OAuth mock)

---

## Plan Sesji — Pozostałe Zadania Epic 34

Stan na 2026-04-11. Kompletna migracja ApiError zakończona, 0 warnings, 7/7 e2e testów przechodzi (+ 1 ignored: shell_repair wymaga sesji desktopowej). Trzy testy e2e (reconciliation, recovery, scrubber_repair) które wcześniej failowały — teraz przechodzą (naprawione w commitach f518a08 + refaktor ApiError).

### Pozostałe zadania (8 pozycji)

| # | Zadanie | Priorytet | Zależności | Estymowany rozmiar |
|---|---------|-----------|------------|-------------------|
| 1 | 34.5a: Audit logging — brakujące callsites | P1 | Brak | Mały |
| 2 | 34.5b: Audit log API + dashboard UI | P1 | 34.5a | Średni |
| 3 | 34.6a: Recovery keys (BIP-39 mnemonic) | P1 | Brak | Średni |
| 4 | **Fix user_id** — naprawa `owner-{device_id}` na UUID | **P0** | Brak | Średni |
| 5 | 34.3b: Google OAuth2 | P2 (opcjonalny) | **Fix user_id** | Duży |
| 6 | 34.6b: Safety Numbers | P3 | 34.1b | Mały (UI-only) |
| 7 | **E2E test multi-user lifecycle** | P1 | 34.2c | Średni |
| 8 | **THREAT_MODEL.md** — model zagrożeń i strategia platform | P1 | Brak | Mały (dokument) |

### Analiza współdzielonych plików

Które pliki dotykają które zadania — klucz do minimalizacji ładowania kontekstu:

| Plik | 34.5a | 34.5b | 34.6a | Fix ID | 34.3b | 34.6b | E2E test | THREAT |
|------|-------|-------|-------|--------|-------|-------|----------|--------|
| `db.rs` | — | filtr queries | nowe fn | migracja | nowe tabele/fn | — | setup | — |
| `api/vault.rs` | callsites | GET endpoint | generate/recover | user_id refs | — | display | — | — |
| `api/auth.rs` | — | — | — | — | OAuth flow | — | — | — |
| `api/mod.rs` | — | — | — | — | routing | — | — | — |
| `identity.rs` | — | — | — | — | — | hash fn | — | — |
| `local_device_identity` | — | — | — | +user_id col | — | — | — | — |
| `static/index.html` | — | panel UI | panel UI | — | login UI | panel UI | — | — |
| `Cargo.toml` | — | — | `bip39` | — | `oauth2` | — | — | — |
| `acl.rs` | — | require_role | — | user lookup | session | — | — | — |
| `tests/` | — | — | — | migracja testów | — | — | **nowy e2e** | — |

**Wnioski:**
- 34.5a+b naturalnie łączą się w jedną sesję (audit pisze → audit czyta)
- 34.6a jest niezależny — osobna sesja
- Fix user_id **musi** być przed OAuth — fundament pod multi-device i multi-user
- 34.3b jest największy — 2 sesje (backend + frontend)
- E2E test + THREAT_MODEL + Safety Numbers = naturalna sesja finalizacyjna

---

### Sesja A: Audit Trail (34.5a + 34.5b)

**Cel:** Kompletny audit trail — logowanie zdarzeń + API + panel w dashboardzie.

**Pliki do załadowania:** `db.rs` (queries), `api/vault.rs` (istniejące callsites), `api/maintenance.rs` (nowy endpoint lub osobny moduł), `static/index.html` (panel UI)

#### Krok A.1: Przegląd istniejących callsites i brakujących zdarzeń
- `vault.rs` ma 7x `insert_audit_log` — zmapować które akcje już logujemy
- Zidentyfikować brakujące: share create/revoke, role change, session login/logout, onboarding events
- Zidentyfikować brak `vault_id` w kontekstach gdzie nie jest oczywisty (np. share)

#### Krok A.2: Dodać brakujące callsites
- `api/sharing.rs`: audit na create_share, revoke_share, delete_share
- `api/auth.rs`: audit na login (unlock), logout
- `api/onboarding.rs`: audit na join-existing, complete
- Każdy INSERT jest fire-and-forget (`let _ = ...`) — zero wpływu na latencję

#### Krok A.3: Audit log API endpoint
- `GET /api/audit-logs?limit=50&offset=0` — paginacja
- `GET /api/audit-logs?action=revoke_device` — filtrowanie po akcji
- `GET /api/audit-logs?actor=user_id` — filtrowanie po aktorze
- ACL: `require_role(Admin)` — tylko admin+ widzi logi
- Nowa fn w `db.rs`: `list_audit_logs_filtered(pool, vault_id, filters, limit, offset)`
- Dodać endpoint w `api/vault.rs` lub nowy `api/audit.rs`

#### Krok A.4: Dashboard panel "Historia zmian"
- Nowa sekcja w `index.html` — tabela z kolumnami: Data, Kto, Akcja, Cel, Szczegóły
- Polling `GET /api/audit-logs?limit=50` co 30s
- Filtrowanie po typie akcji (dropdown)
- Formatowanie timestampów do czytelnej daty

#### Krok A.5: Testy i weryfikacja
- Rozszerzyć istniejący test `audit_log_lifecycle` o nowe filtry
- `cargo check` + `cargo clippy` + `cargo test`
- Ręczna weryfikacja w przeglądarce: audit panel wyświetla logi

**Exit criteria:** `cargo test` green, nowy panel widoczny w dashboardzie, audit loguje wszystkie operacje zarządzania.

---

### Sesja B: Recovery Keys — BIP-39 (34.6a)

**Cel:** Owner może wygenerować 24-słowny klucz odzyskiwania i użyć go do odblokowania skarbca.

**Pliki do załadowania:** `Cargo.toml` (dependency), `vault.rs` (VK encode/decode), `db.rs` (hash storage), `api/vault.rs` (endpoints), `static/index.html` (UI)

#### Krok B.1: Dependency i core logic
- Dodać `bip39 = "2"` do `angeld/Cargo.toml`
- Nowy moduł `angeld/src/recovery.rs`:
  - `generate_mnemonic(vault_key: &[u8; 32]) -> String` — VK → 24 słowa (256 bits = 24 words)
  - `recover_vault_key(mnemonic: &str) -> Result<[u8; 32], RecoveryError>` — 24 słowa → VK
  - `hash_mnemonic(mnemonic: &str) -> String` — do weryfikacji w DB (nie do odtworzenia)
- Unit testy: generate → recover roundtrip, invalid mnemonic rejection

#### Krok B.2: DB i persistence
- Nowa kolumna w `vault_state`: `recovery_key_hash TEXT` (hash mnemonic, do sprawdzenia czy user zapisał)
- `db.rs`: `set_recovery_key_hash()`, `get_recovery_key_hash()`, `has_recovery_key()`
- Recovery key jest generowany z aktualnego Vault Key — po rotacji VK trzeba wygenerować nowy

#### Krok B.3: API endpoints
- `POST /api/vault/generate-recovery-key` — generuje mnemonic, zwraca 24 słowa, zapisuje hash w DB
  - ACL: Owner only
  - Zwraca mnemonic TYLKO RAZ — potem nie da się go odczytać z API
  - NIGDY nie logować mnemonic (zero-knowledge rule)
- `POST /api/vault/recover` — przyjmuje mnemonic, odzyskuje VK, unlockuje vault
  - Bez ACL (vault jest locked, nie ma sesji)
  - Walidacja: hash mnemonic musi zgadzać się z `recovery_key_hash`
- `GET /api/vault/recovery-status` — czy recovery key został wygenerowany (bool)

#### Krok B.4: Dashboard UI — generacja
- Nowa sekcja "Klucz Odzyskiwania" w panelu Skarbca
- Przycisk "Generuj Klucz Odzyskiwania" → modal z 24 słowami
- Confirmation step: user wpisuje 3 losowe słowa jako dowód zapisania
- Po potwierdzeniu: modal się zamyka, UI pokazuje "Klucz odzyskiwania: skonfigurowany"
- Warning: "Zapisz te 24 słowa na papierze. Bez nich nie odzyskasz danych."

#### Krok B.5: Dashboard UI — odzyskiwanie
- Na ekranie unlock (vault locked): link "Zapomniałeś hasła? Użyj klucza odzyskiwania"
- Formularz: 24 pola input (po 1 słowo) lub jedno pole textarea
- Submit → `POST /api/vault/recover` → jeśli OK, vault unlocked → redirect do dashboardu
- Error handling: "Nieprawidłowy klucz odzyskiwania"

#### Krok B.6: Testy i weryfikacja
- Unit: roundtrip, invalid words, wrong checksum
- Integration: generate → verify hash in DB → recover → vault unlocked
- `cargo check` + `cargo clippy` + `cargo test`
- Ręczna weryfikacja w przeglądarce

**Exit criteria:** `cargo test` green, recovery key flow działa end-to-end, mnemonic nigdy nie jest logowany.

#### Krok B.7: Unlock-screen recovery link + Karta wydruku A4 (follow-up po review)

**Kontekst:** Implementacja B.4+B.5 (commit `57d0a76`) wpięła restore modal w dashboard panelu Skarbca — czyli **dostępny dopiero po zalogowaniu**. Klasyczny flow „zapomniałem hasła" zakłada że user **nie jest zalogowany**, więc obecny restore jest funkcjonalnie niedostępny w jedynym momencie kiedy jest potrzebny. Plan oryginalny B.5 mówił o linku w unlock screen — implementacja od tego odeszła. Dodatkowo brakuje karty wydruku A4 (standard branżowy: Bitwarden Emergency Sheet, 1Password Emergency Kit).

**B.7.1 — Recovery link na unlock screen (`wizard.js`)**
- Pod polem master password w wizardzie unlock dodać link „Zapomniałem hasła / Użyj klucza odzyskiwania"
- Klik otwiera ten sam restore modal (lub osobny widok wizard'a) z polami: 24 słowa + nowe hasło + potwierdzenie
- Po udanym `POST /api/recovery/restore`: backend musi zwrócić token sesji (sprawdzić obecny response — jeśli nie, dorobić); FE od razu odblokowuje vault i ładuje dashboard z nowym hasłem (auto-login, bez powrotu do unlocka)
- Restore w dashboardzie zostawić jako wtórny entry point (np. dla użytkownika który chce zmienić hasło bez zapominania go)

**B.7.2 — Print karty wydruku A4**
- W generate modal nowy duży CTA `[Wydrukuj kartę odzyskiwania]` obok `Skopiuj`
- Dwa warianty implementacji: (a) `@media print` z dedykowanymi stylami chowającymi sidebar/header/modal-chrome i pokazującymi tylko czystą kartę, lub (b) `window.open()` nowego okna z wbudowanym minimalnym HTML
- Szablon karty (A4 portrait, czarno-biały, monospace na słowa):
  - Nagłówek: „OmniDrive — Karta Odzyskiwania Skarbca"
  - Nazwa skarbca + data wygenerowania + vault_key generation
  - Numerowana lista 24 słów (np. siatka 4×6 z indeksami `1.` do `24.`)
  - Sekcja „Bezpieczeństwo" z punktami: „Nie rób zdjęć tej karty", „Nie przechowuj cyfrowo (skanu, fotki, chmury)", „Trzymaj w sejfie / safety deposit box", „Każdy kto zna te 24 słowa może odszyfrować Twój skarbiec"
  - Stopka: skrócony fingerprint klucza (do weryfikacji że to ta sama karta) + „Wygenerowano przez OmniDrive vX.Y.Z"
- `tabindex` i fokus na `Wydrukowano i zabezpieczono` przed zamknięciem modala (żeby user nie zamknął okna bez akcji)

**B.7.3 — Ostrzeżenie o nadpisaniu poprzedniego klucza**
- Sprawdzić w `recovery.rs` czy `/generate` automatycznie unieważnia poprzedni klucz (czy wymaga osobnego `/revoke` najpierw)
- Jeśli automatycznie nadpisuje: w generate modal **przed** wywołaniem API pokazać confirm step z ostrzeżeniem „Wygenerowanie nowego klucza unieważni poprzedni — papierowa karta którą posiadasz nie będzie już działać. Kontynuować?"
- Jeśli nie nadpisuje (klucze kumulatywne): pokazać aktualną liczbę aktywnych w confirm step

**B.7.4 — Testy i weryfikacja**
- Manualnie: zapomnij hasło → otwórz unlock → klik recovery link → wpisz 24 słowa + nowe hasło → vault otwarty bez powrotu do unlocka
- Manualnie: print preview karty A4 (Chrome `Ctrl+P`) — czysta karta, brak sidebar/header, czytelne 24 słowa
- Manualnie: drugi `/generate` na żywym vaulcie — confirm dialog z ostrzeżeniem o nadpisaniu pojawia się
- `cargo test --workspace` zielony

**Exit criteria:** recovery dostępne na unlock screen z auto-login, karta wydruku A4 generowalna z generate modala, generate ostrzega o nadpisaniu starego klucza.

**Rozmiar:** Średni (zmiany w `wizard.js` + nowy print template w `static/index.html` + drobna zmiana w `recovery.rs` jeśli `/restore` nie zwraca tokenu)

**Mikro-kroki:** 4 (B.7.1–B.7.4)

---

### Sesja Pre-C: Naprawa User ID (P0 — fundament pod OAuth i multi-device)

**Cel:** Zastąpić kruchy schemat `owner-{device_id}` prawdziwymi UUID. Bez tego OAuth i multi-device z wieloma urządzeniami per user nie będą działać poprawnie.

**Problem:** Migrator `34.0b` generuje `user_id = format!("owner-{}", device_id)`. Jeśli user ma 2 urządzenia i oba migrują niezależnie, powstają dwa osobne "ownerzy". `user_id` jest pochodną urządzenia zamiast być stałym identyfikatorem użytkownika.

**Pliki do załadowania:** `db.rs` (migracja, CRUD), `api/vault.rs` (referencje do user_id), `acl.rs` (session → user lookup), unit testy

#### Krok Pre-C.1: Nowa kolumna w local_device_identity
- `ALTER TABLE local_device_identity ADD COLUMN user_id TEXT`
- Przy starcie: jeśli `user_id IS NULL` → wygeneruj UUID v4, zapisz
- Jedno urządzenie = jeden user_id, ale ten sam user na wielu urządzeniach = ten sam user_id (przekazywany przez invite/join flow)
- Unit test: migracja zachowuje istniejące dane, nowe urządzenie dostaje UUID

#### Krok Pre-C.2: Migracja istniejących danych
- `migrate_single_to_multi_user()` — zmienić z `format!("owner-{}", device_id)` na:
  1. Sprawdź czy `local_device_identity.user_id` istnieje → użyj go
  2. Jeśli nie → wygeneruj UUID, zapisz w `local_device_identity`, użyj w `users`
- Join flow (`accept-device`, `join`): nowe urządzenie dziedziczy `user_id` od zapraszającego lub z invite code context
- Backward compat: istniejące vaults z `owner-{device_id}` → jednorazowa migracja na UUID

#### Krok Pre-C.3: Aktualizacja referencji w API i ACL
- `acl.rs`: session → `user_id` lookup bez polegania na konwencji nazewnictwa
- `api/vault.rs`: invite/join/accept-device → operują na UUID zamiast `owner-{device_id}`
- Sprawdzić czy `vault_members`, `devices`, `audit_logs` poprawnie referencjonują nowe UUID

#### Krok Pre-C.4: Testy i weryfikacja
- Unit: migracja starych `owner-{device_id}` → UUID
- Unit: nowe urządzenie dostaje UUID, join flow dziedziczy user_id
- Integration: pełny cykl owner+device z nowymi identyfikatorami
- `cargo check` + `cargo clippy` + `cargo test`
- Weryfikacja: istniejące e2e testy nadal przechodzą (brak regresji)

**Exit criteria:** `cargo test` green, zero referencji do `owner-{device_id}` poza kodem migracyjnym, `user_id` jest UUID v4.

---

### Sesja C: Google OAuth2 (34.3b) — Część 1: Backend

**Cel:** Backend OAuth2 flow — Google login → user identity → session token. Bez UI.

**Pliki do załadowania:** `Cargo.toml` (dependencies), `api/auth.rs` (OAuth routes), `db.rs` (user lookup/create), `acl.rs` (session integration)

**Uwaga:** To jest OPCJONALNY epic. Google OAuth = convenience identity, NIE = klucz kryptograficzny. Vault nadal wymaga passphrase do unlock po OAuth login.

#### Krok C.1: Dependencies i konfiguracja
- Dodać do `angeld/Cargo.toml`: `oauth2 = "4"`, `reqwest` (jeśli nie ma)
- Env vars: `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `GOOGLE_REDIRECT_URI`
- `config.rs`: `OAuthConfig` struct, opcjonalny (None = OAuth disabled)

#### Krok C.2: OAuth2 flow — backend
- `GET /api/auth/google` — generuj authorization URL, redirect do Google
- `GET /api/auth/google/callback?code=...&state=...` — exchange code → access token → userinfo
- Google userinfo → `email`, `sub` (subject)
- Lookup/create user: `users WHERE auth_provider='google' AND auth_subject=sub`
- Jeśli nowy user → auto-create z `role='viewer'` (owner musi upgrade'ować)
- Generuj session token → zwróć jako cookie/JSON

#### Krok C.3: Session integration
- OAuth session = identyczny token jak passphrase session (z 34.3a)
- Różnica: OAuth session NIE unlockuje vault — `vault_keys` pozostają locked
- Po OAuth login: dashboard wyświetla "Zalogowano jako X" ale vault jest locked
- Unlock nadal wymaga `POST /api/vault/unlock` z passphrase

#### Krok C.4: Testy
- Unit: mock OAuth exchange, user create/lookup, session generation
- Integration: full flow z mock Google server (nie prawdziwy Google)
- Verify: OAuth login does NOT unlock vault
- `cargo check` + `cargo clippy` + `cargo test`

**Exit criteria:** `cargo test` green, OAuth backend kompletny, vault bezpiecznie oddzielony od OAuth identity.

---

### Sesja D: Google OAuth2 (34.3b) — Część 2: Frontend

**Cel:** Dashboard UI dla Google login + user management.

**Pliki do załadowania:** `static/index.html` (UI), `api/auth.rs` (weryfikacja flow), `api/vault.rs` (user management UI hooks)

#### Krok D.1: Login UI
- Przycisk "Zaloguj przez Google" na stronie dashboardu (jeśli OAuth skonfigurowany)
- Redirect do `GET /api/auth/google` → Google consent → callback → dashboard
- Po powrocie: wyświetl "Zalogowano jako {email}" w headerze

#### Krok D.2: User management panel
- Lista userów z rolami (owner/admin/member/viewer)
- Invite flow: generuj kod zaproszenia, wyświetl do skopiowania
- Role management: owner może zmienić role (dropdown)
- Remove user: przycisk z potwierdzeniem

#### Krok D.3: UX — vault unlock po OAuth
- Jeśli user zalogowany OAuth ale vault locked:
  - Wyświetl: "Zalogowano jako X. Odblokuj Skarbiec aby uzyskać dostęp do plików."
  - Formularz passphrase (jak dotychczas)
- Jeśli user zalogowany OAuth i vault unlocked:
  - Pełny dostęp do dashboardu

#### Krok D.4: Weryfikacja
- Ręczna weryfikacja w przeglądarce (prawdziwy Google login)
- Edge cases: OAuth disabled, expired session, role mismatch
- `cargo check` + `cargo clippy` + `cargo test`

**Exit criteria:** Google login działa end-to-end, vault bezpiecznie oddzielony od OAuth, user management w UI.

---

### Sesja E: Safety Numbers + E2E Multi-User Test + THREAT_MODEL (Finalizacja Epic 34)

**Cel:** Domknąć wszystkie otwarte luki architektoniczne. Safety Numbers, integracyjny test pełnego cyklu życia użytkownika, model zagrożeń i strategia platformowa.

**Pliki do załadowania:** `identity.rs`, `api/vault.rs`, `static/index.html`, `tests/` (nowy e2e), `docs/` (nowy dokument)

#### Krok E.1: Safety Number generation
- `identity.rs`: `compute_safety_number(pub_key_a: &[u8; 32], pub_key_b: &[u8; 32]) -> String`
- Format: 6 grup po 5 cyfr (np. `12345 67890 12345 67890 12345 67890`)
- SHA-256(sorted_keys) → truncate → decimal format
- Symetryczny: `safety_number(A, B) == safety_number(B, A)`

#### Krok E.2: Safety Numbers — API + UI
- `GET /api/vault/safety-number/{device_id}` — zwraca safety number między moim device a wskazanym
- Dashboard: w panelu "Urządzenia" → kliknięcie na device → modal z Safety Number
- Instrukcja: "Porównaj ten numer z osobą posiadającą to urządzenie (telefon, osobiście)"

#### Krok E.3: E2E test — pełny cykl życia multi-user
- Nowy plik: `angeld/tests/e2e_multi_user_lifecycle.rs`
- Scenariusz testowy:
  1. Alice tworzy vault, staje się ownerem
  2. Alice generuje invite code
  3. Bob dołącza z invite code, otrzymuje public key registration
  4. Alice akceptuje urządzenie Boba → Bob dostaje wrapped Vault Key
  5. Bob unwrapuje VK → ma dostęp do plików (verify: read succeeds)
  6. Alice usuwa Boba → VK rotation → lazy re-wrap
  7. Bob próbuje API call → 403 Forbidden (verify: access denied)
  8. Verify: audit logs zawierają pełny ślad (invite, join, accept, remove, rotate_vk)
- Test operuje na in-memory SQLite, bez prawdziwego daemona (jak istniejące testy DB/vault)
- Pokrywa styk warstw: DB ↔ krypto ↔ ACL ↔ vault — właśnie tam kryją się błędy

#### Krok E.4: THREAT_MODEL.md — model zagrożeń i strategia platformowa
- Nowy plik: `docs/THREAT_MODEL.md`
- Sekcje:
  1. **Granice zaufania:**
     - Cloudflare Tunnel = brzeg sieci (TLS termination, rate limiting, DDoS protection)
     - Daemon API = czysty backend za tunelem, nie wystawiony bezpośrednio
     - localhost:8787 = trusted zone (brak TLS, brak CSRF — akceptowalne bo local-only na desktopie)
  2. **Adversary model:**
     - Cloud provider (B2/R2/Scaleway): widzi tylko zaszyfrowane blobs, zero-knowledge
     - Sieć (MITM): Cloudflare Tunnel = TLS, fragment URI (#DEK) nigdy nie opuszcza przeglądarki
     - Atakujący z dostępem do maszyny: vault locked = dane chronione (Argon2id + AES-256-GCM)
     - Atakujący z dostępem do API (przez tunel): sesja + ACL + rate limiting
  3. **Strategia platformowa:**
     - Desktop (Windows): pełny klient z wirtualnym dyskiem O:\ (cfapi.dll) — Windows-only ze względu na koszt i specyfikę API
     - Mobile (iOS/Android): docelowo thin client → REST API przez Cloudflare Tunnel
     - API jest platform-agnostic (czyste REST + JSON) — backend obsługuje dowolnego klienta
     - Brak planów na natywny klient macOS/Linux z wirtualnym dyskiem (koszt vs. wartość)
  4. **Kiedy API przestaje być localhost-only:**
     - Moment: aktywacja Cloudflare Tunnel (już zaimplementowana w architekturze)
     - Wymagania: session tokens (done), ACL (done), audit trail (done po Sesji A)
     - Brakujące do produkcji: CSRF protection dla browser clients, per-user rate limiting

#### Krok E.5: Epic 34 finalizacja
- Przegląd wszystkich endpointów — spójność error handling, response format
- Przegląd audit trail — czy wszystkie operacje są logowane
- `cargo clippy`, `cargo test`, sprawdzenie CI
- Zamknięcie Epic 34 w `PROJECT_STATUS.md` i `plan.md`

**Exit criteria:** Safety Numbers w UI, e2e multi-user test przechodzi, THREAT_MODEL.md zamknięty, Epic 34 oficjalnie zakończony.

---

## Phase 5: Epic 36 — UI Redesign (Skarbiec Console)

**Cel:** Wdrożyć nowy layout dashboardu bazujący na mockupie ze Stitcha (glass-panel, paleta cyan/green/orange, Material Symbols Outlined, sidebar + header + content). Obecny `index.html` (panel diagnostyki) staje się legacy; nowy UI organizuje funkcje w 6 zakładek: **Przegląd, Pliki, Skarbiec, Multi-Device, Chmura, Audyt** + Ustawienia.

**Snapshot designu:** `docs/ui-mockups/stitch-dashboard.html` (NIE edytować — referencja).

Sesje F i G są sekwencyjne. F dostarcza działający shell + zakładkę „Przegląd" z realnymi danymi audit/recovery (reszta KPI/chart/tiles to placeholdery). G wypełnia brakujące endpointy backendu i pozostałe widoki.

---

### Sesja F: UI Shell + Przegląd (z placeholderami)

**Cel:** Podmienić obecny `index.html` na layout Stitcha — sidebar 240px + header 64px + scrollowalny content. Podłączyć TYLKO te sekcje które mają gotowe API (audit log + recovery alert). Hero KPI, chart 24h i stat tiles zostają jako wizualne placeholdery z widocznym badge'em „MOCK" + TODO.

**Pliki do załadowania:** `dist/installer/payload/static/index.html` (całkowita podmiana), `api/audit.rs` (kontrakt odpowiedzi), `api/recovery.rs` (kontrakt odpowiedzi), `docs/ui-mockups/stitch-dashboard.html` (referencja), `angeld/src/api/mod.rs` (routing, zachowanie wizard.js)

#### Krok F.1: Zachowanie obecnego UI jako legacy ✅ DONE (commit `5d1527d`)
- Nowy route: `GET /legacy` — serwuje obecny `index.html` (zostawia dostęp do panelu diagnostyki na wypadek regresji)
- `GET /` — będzie serwował nowy layout po F.2
- **Realizacja:** snapshot `static/legacy.html` (kopia poprzedniego `index.html`, 117KB / 2258 linii) + handler `get_legacy()` w `api/mod.rs`

#### Krok F.2: Shell layout (sidebar + header + main) ✅ DONE (commit `5d1527d`)
- Podmienić `static/index.html` na strukturę ze Stitcha (Tailwind config + tokeny + `.glass-panel` + keyframe `pulse-secondary`)
- Sidebar: 6 pozycji nawigacji + Ustawienia + Wyloguj (na razie wszystkie to `href="#"`, aktywna `Przegląd`)
- Header: pill „Skarbiec: OK" (status dynamiczny z `/api/health`), avatar/user (z session po OAuth, inaczej „Local")
- Main: `<div id="view-przeglad">` z sekcjami — pozostałe widoki ukryte w Sesji F (pojawią się w Sesji G)
- **Realizacja:** pełna podmiana `static/index.html`; wszystkie sekcje Przegląd (Hero, Recovery alert, Status shardów, Chart, Audit log, 4 stat tiles) z DOM placeholderami i MOCK badge'ami; status pill jako statyczny „Skarbiec: OK" (live fetch w F.6); nav z `data-view` pod hash router F.7; user widget z `account_circle` (zamiast `<img>`) i fallbackiem „Local"

#### Krok F.3: Wiring sekcji „Logi Audytowe" ✅ DONE
- Fetch `GET /api/audit?limit=5` (istnieje od commit 5a2ee26)
- Render 5 ostatnich wpisów: ikona Material Symbols per `action` type (login/sync_saved_locally/add_moderator/gpp_maybe/upload_file), tytuł PL, timestamp relative (dzisiaj → `HH:MM:SS`, wczoraj → „Wczoraj", starsze → data)
- „Zobacz Pełny Log" → na razie disabled (otwarty w Sesji G jako widok Audyt)
- **Realizacja:** `AUDIT_ACTION_MAP` z 25 akcjami obecnymi w kodzie (vault_unlock, recovery_*, share_*, accept_device, revoke_device, scrub/repair/backup/reconcile, onboarding_*, migrate_single_to_multi …) z mapowaniem ikona / polski tytuł / `tone` (slate/primary/secondary/tertiary); `formatAuditTimestamp` używa unix seconds (`epoch_secs` z `db.rs`); `auditSubtitle` z fallbackiem `details → actor→target → "Zdarzenie systemowe"`; empty state „Brak zdarzeń audytowych" + error state w kolorze rose; polling co 30s przyjdzie z G.4

#### Krok F.4: Wiring alertu „Brak klucza odzyskiwania" ✅ DONE
- Fetch `GET /api/recovery/status` (istnieje od commit 57d0a76)
- Jeśli status = „missing" lub „not_verified_30d" → pokaż kartę tertiary z CTA `Weryfikuj teraz` (przenosi do widoku Skarbiec → sekcja recovery w Sesji G, na razie otwiera modal z istniejącym flow)
- Jeśli status = „ok" → ukryj kartę, grid staje się 1-kolumnowy
- **Realizacja:** `/api/recovery/status` zwraca `{ active_count, last_created_at, vk_generation, word_count }` (bez stringa statusu) — klasyfikator `classifyRecoveryStatus` po FE: `active_count <= 0` → `missing`, klucz starszy niż 30 dni → `stale`, inaczej `ok`; `applyRecoveryStatus` przełącza grid `#overviewAlertsGrid` przez toggle klasy `md:grid-cols-2`; CTA otwiera `/legacy#recoveryKeyGenerateButton` w nowej karcie (reuse modala B.4); G.6 podmieni to na natywny modal w widoku Skarbiec; przy błędzie sieci pokazuję `missing` (bezpieczniej zaalarmować niż zignorować)

#### Krok F.5: Placeholdery dla Hero / Chart / Tiles ✅ DONE
- Hero KPI: wartości `—` + badge `MOCK` + TODO G.1 — zrobione w F.2
- Chart 24h: statyczne słupki + badge `MOCK` + TODO G.2 — zrobione w F.2
- 4 stat tiles: wartości `—` + badge `MOCK` + TODO G.3 — zrobione w F.2
- **Realizacja:** karta „Status Shardów" podłączona do `GET /api/health/vault` (total/healthy/degraded/unreadable); dynamiczny kolor ikony/tytułu (secondary/tertiary/error) + countery; usunięto MOCK badge; reuse danych z poll F.6

#### Krok F.6: Status pill w headerze + pulse ✅ DONE
- Fetch `GET /api/health/vault` co 10s (`setInterval`); odpowiedź: `{ total_packs, healthy_packs, degraded_packs, unreadable_packs }`
- Mapowanie: `ok` → green (`bg-secondary` + pulse), `degraded` → orange (`bg-tertiary`), `error` → red (`bg-error`)
- **Realizacja:** `fetchSystemStatus()` → `deriveVaultState()` → `applyPillState()` + `applyShardStatus()`; pill startuje w loading (szary); błąd sieci → stan error; jeden fetch obsługuje pill + shard card

#### Krok F.7: Routing klientowy (stub) ✅ DONE
- Prosty hash-router: `#przeglad` (domyślny), `#pliki`, `#skarbiec`, `#multi-device`, `#chmura`, `#audyt`, `#ustawienia`
- W Sesji F wszystkie widoki poza `#przeglad` pokazują placeholder z ikoną Material Symbols + „{Nazwa} — wkrótce"
- Sidebar update: active state pod aktualny hash, klik na link → `location.hash`, `hashchange` → `navigateTo()`
- **Realizacja:** `VALID_VIEWS[]` + `PLACEHOLDER_META{}` (ikona/tytuł per widok); `currentView()` parsuje hash; `navigateTo()` przełącza `#view-przeglad` / `#view-placeholder`; `updateSidebarActive()` iteruje `.nav-item[data-view]` i toggluje klasę `active`; link `wyloguj` ma TODO guard

#### Krok F.8: Weryfikacja ✅ DONE
- `cargo check --workspace` — OK
- `cargo test --workspace` — 11 passed, 0 failed
- `cargo build --release --workspace` — OK
- Kopia `angeld.exe` (32 MB) do `dist/installer/payload/`
- Ręczny test: daemon na :8787 → `/api/health/vault` zwraca `{total:2, healthy:2, degraded:0, unreadable:0}` → pill zielony; `/api/recovery/status` → `active_count:0` → alert widoczny; `/legacy` → stary panel działa
- Routing weryfikacja: placeholder div + router JS present w serwowanym HTML

**Exit criteria:** ✅ spełnione — nowy layout, audit log + recovery alert z live data, shard card z /api/health/vault, status pill polling, hash router + sidebar, `/legacy` jako fallback, testy zielone.

**Rozmiar:** Duży (pojedynczy plik HTML ~700 linii + ~200 linii JS fetch/router/poll + drobna zmiana w `api/mod.rs`)

**Mikro-kroki:** 8 (F.1–F.8)

---

### Sesja G: Dashboard backend (stats endpoints) + pozostałe widoki

**Cel:** Dopełnić UI — dorobić 3 brakujące endpointy statystyk (hero/chart/tiles) i 5 widoków (Pliki, Skarbiec, Multi-Device, Chmura, Audyt, Ustawienia).

**Pliki do załadowania:** `api/mod.rs`, nowy moduł `api/stats.rs`, `api/files.rs` (istnieje), `api/vault.rs`, `api/audit.rs`, `api/recovery.rs`, `static/index.html`, `diagnostics.rs`, `config.rs` (dla widoku Ustawienia)

Ze względu na rozmiar Sesja G dzieli się na podsesje G-BE (backend) i G-FE (frontend views). Każdą można osobno commitować.

#### Krok G.1: `GET /api/stats/overview` — Hero KPI
- Nowy plik: `angeld/src/api/stats.rs`, route w `api/mod.rs`
- Response: `{ files_count, logical_size_bytes, monthly_cost_usd, devices_count }`
- Źródła:
  - `files_count` = `SELECT COUNT(*) FROM inodes WHERE deleted_at IS NULL`
  - `logical_size_bytes` = `SELECT SUM(size) FROM inodes WHERE deleted_at IS NULL`
  - `monthly_cost_usd` = estymata z `cloud_guard` (istnieje moduł) — bytes × rate per provider
  - `devices_count` = `SELECT COUNT(*) FROM devices WHERE revoked_at IS NULL`
- Cache: 30s (wartości i tak wolno się zmieniają)
- Test: unit z in-memory SQLite + seeded data

#### Krok G.2: `GET /api/stats/traffic?hours=24` — Chart
- Response: `{ buckets: [{ timestamp, upload_bytes, download_bytes }, ...] }` — 12 buckets po 2h
- Źródło: nowa tabela `traffic_stats` (albo istniejąca jeśli już coś jest w `uploader.rs`/`downloader.rs` — sprawdzić w Sesji G kick-off)
- Jeśli brak agregacji → dodać zapisy w `uploader::complete_upload()` i `downloader::read_range_streamed()` (bump counterów per bucket)
- Test: seed kilka wpisów → fetch → weryfikacja bucket-owania

#### Krok G.3: `GET /api/stats/system` — Tiles
- Response: `{ nodes_count, nodes_delta, cpu_percent, latency_ms, latency_delta_ms, integrity_percent }`
- Źródła:
  - `nodes_count` = liczba aktywnych devices + providers
  - `cpu_percent` = proc self stats (crate `sysinfo` — prawdopodobnie już w drzewie)
  - `latency_ms` = ewma latency z `diagnostics.rs` (istnieje worker tracking)
  - `integrity_percent` = wynik ostatniego przebiegu `scrubber.rs`
- Cache: 5s

#### Krok G.4: Podłączenie endpointów w Hero/Chart/Tiles
- Usunąć badge `MOCK` z F.5
- Dodać poll: KPI co 30s, chart co 60s, tiles co 5s, audit log co 30s
- Wskaźnik „ostatnio odświeżono" w headerze (reuse logiki z legacy)

#### Krok G.5: Widok „Pliki"
- Użyć `api/files.rs` (istnieje `GET /api/files/list`)
- Drzewo folderów + lista: ikona, nazwa, rozmiar, status (LOCAL/COMBO/CLOUD/FORTECA — 4 poziomy z Epic 35.2b), data modyfikacji
- Kontekst menu (prawy klik): zmień politykę, pobierz, share (Epic 33), usuń
- Breadcrumbs, search

#### Krok G.6: Widok „Skarbiec"
- Sekcje:
  1. Status vault (locked/unlocked, format v1/v2, ostatnia rotacja)
  2. Unlock/lock (passphrase)
  3. Rotate Vault Key (istniejące z Epic 32.5.2d)
  4. Migracja V1→V2 (progress bar, live status z `migrator.rs`)
  5. Recovery Keys — full view (generate, restore, revoke) — reuse z 34.6a + uzupełnienia z B.7:
     - przycisk `[Wydrukuj kartę odzyskiwania]` w generate modalu (szablon A4, `@media print`, czarno-biały, 24 słowa numerowane + sekcja bezpieczeństwa, patrz B.7.2)
     - confirm step z ostrzeżeniem o nadpisaniu poprzedniego klucza przed `/generate` (patrz B.7.3)
     - restore także dostępny tutaj jako wtórny entry point (główny w wizard'zie unlock z B.7.1)
     - po udanym restore: auto-login bez powrotu do unlocka (jeśli backend zwraca token sesji)

#### Krok G.7: Widok „Multi-Device"
- Lista `devices` z: nazwa, public key fingerprint, last_seen, safety number (Epic 34 E.1–E.2)
- Invite flow: przycisk „Zaproś urządzenie" → modal z kodem + QR
- Pending devices: lista czekających na akceptację + przycisk „Akceptuj" (ECDH wrap VK)
- Revoke device

#### Krok G.8: Widok „Chmura"
- Lista providerów (B2/R2/Scaleway) z: status, bytes stored, monthly cost, latency, error rate
- Cloud Guard: budget threshold, ostrzeżenia
- Provider config (read-only; edycja wymaga re-auth)

#### Krok G.9: Widok „Audyt"
- Pełny widok audit log (paginacja, filtry: action type, user, date range)
- Export CSV/JSON
- Link z „Zobacz Pełny Log" na Przeglądzie

#### Krok G.10: Widok „Ustawienia"
- Autostart (HKCU\...\Run toggle)
- Auto-refresh interval
- OAuth (Sesja C+D) login status
- Cache size, spool dir
- Shell Extension: 4 poziomy ochrony — pokaż stan rejestracji DLL
- Diagnostics: link do logów, restart daemona, factory reset (z potwierdzeniem)

#### Krok G.11: Weryfikacja pełna
- `cargo check --workspace`, `cargo clippy -- -D warnings`, `cargo test --workspace`
- `cargo build --release --workspace` + kopia binarek do `dist/installer/payload/`
- Bump wersji instalatora + wszystkich `Cargo.toml` (zgodnie z CLAUDE.md workflow)
- Generacja `.exe` instalatora
- Ręczny test: każda zakładka ładuje się, dane są realne, `/legacy` nadal dostępny
- Usunięcie `/legacy` (decyzja: zostawić czy usunąć? — dyskusja przy G.11)

**Exit criteria:** Wszystkie 7 widoków mają realne dane, 3 nowe endpointy stats są stabilne, `/legacy` decyzja podjęta, nowa wersja zbudowana i zainstalowana, token budget nie przekroczony.

**Rozmiar:** Bardzo duży — prawdopodobnie zostanie rozbity na G-BE (kroki G.1–G.4) i G-FE (G.5–G.10) jako dwa osobne commity w obrębie sesji.

**Mikro-kroki:** 11 (G.1–G.11)

---

### Podsumowanie sesji

| Sesja | Zadania | Pliki główne | Rozmiar | Mikro-kroki |
|-------|---------|-------------|---------|-------------|
| **A** | 34.5a + 34.5b (Audit Trail) | db.rs, api/vault.rs, sharing.rs, auth.rs, index.html | Średni | 5 (A.1–A.5) |
| **B** | 34.6a (Recovery Keys) | recovery.rs (nowy), vault.rs, db.rs, api/vault.rs, index.html | Średni | 6 (B.1–B.6) |
| **Pre-C** | Fix user_id (owner-{id} → UUID) | db.rs, api/vault.rs, acl.rs, local_device_identity | Średni | 4 (Pre-C.1–Pre-C.4) |
| **C** | 34.3b backend (OAuth2) | Cargo.toml, config.rs, api/auth.rs, db.rs | Średni | 4 (C.1–C.4) |
| **D** | 34.3b frontend (OAuth2 UI) | index.html, api/auth.rs | Mały-Średni | 4 (D.1–D.4) |
| **E** | Safety Numbers + E2E test + THREAT_MODEL | identity.rs, tests/e2e_multi_user.rs, docs/THREAT_MODEL.md | Średni | 5 (E.1–E.5) |
| **F** | Epic 36 — UI Shell + Przegląd (Stitch layout, placeholdery) | static/index.html, api/mod.rs | Duży | 8 (F.1–F.8) |
| **G** | Epic 36 — Stats endpoints + pozostałe widoki | api/stats.rs (nowy), api/files/vault/audit/recovery, static/index.html, diagnostics.rs | Bardzo duży | 11 (G.1–G.11) |

**Rekomendowana kolejność:** A → B → Pre-C → C → D → E → F → G

**Uzasadnienie:**
- **A (Audit)** — P1, nie wymaga nowych dependencies, szybka wygrana
- **B (Recovery Keys)** — P1, niezależny od OAuth, bezpieczna implementacja
- **Pre-C (Fix user_id)** — **P0 blocker** dla OAuth. Musi być przed C, bo OAuth tworzy nowych userów i operuje na user_id. Kruchy schemat `owner-{device_id}` złamie multi-device OAuth flow
- **C+D (OAuth)** — P2, największy, wymaga Google Cloud Console setup
- **E (Finalizacja)** — domyka wszystkie luki architektoniczne z krytycznej oceny projektu
- **F (UI Shell)** — ten sam chrome obsłuży Sesję G i kolejne; placeholdery zamiast blokowania się na endpointach stats
- **G (Backend + widoki)** — wykonalne dopiero po F (layout), powinno być po A/B/E, bo część widoków pokazuje dane z audit/recovery/multi-device

**Każda sesja kończy się:** `cargo check` (0 warnings) + `cargo clippy --workspace -- -D warnings` (clean) + `cargo test --workspace` (all pass) + ręczna weryfikacja UI.

**Każdy mikro-krok kończy się:** pytaniem "kontynuujemy czy commit+push?" (ochrona budżetu tokenów).

---

## Risk Register

| Risk | Level | Mitigation |
|------|-------|------------|
| cfapi.dll bindings unstable | HIGH | Task 35.0 as isolated PoC with go/no-go gate; fallback: ProjFS |
| Ingest race conditions | HIGH | Transactional state machine with rollback; file lock during operation |
| Shell Extension DLL crash = Explorer crash | HIGH | Thin client architecture; all logic in angeld |
| Migration interrupted (power loss) | HIGH | Resumable with checkpoints; rollback path |
| WebCrypto OOM on large files | MEDIUM | ReadableStream + TransformStream; explicit size limit |
| Windows Defender blocks hydrated files | MEDIUM | Early MotW testing; placeholder file signature |
| Cloud costs surprise user | MEDIUM | Cloud Guard + predictive budget; threshold alerts |
| Private key loss (sole owner) | HIGH | Shamir's Secret Sharing / paper recovery keys |
| Hydration timeout (slow provider) | MEDIUM | EC_2_1 graceful degradation + adaptive per-provider timeouts |
