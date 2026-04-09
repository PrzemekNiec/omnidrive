# OmniDrive Roadmap v2 — Plan Implementacyjny

Sekwencja: **Phase 0 → Epic 32.5 → Epic 35 → Epic 33 → Epic 34**

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

### 35.2c: Overlay icons
- Ikony overlay w Eksploratorze: synced, uploading, ghost, error
- IShellIconOverlayIdentifier implementation
- Stan z angeld via named pipe lub shared memory (szybki polling)
- Test: zmień stan pliku → ikona się zmienia w Eksploratorze

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
| **34.3a** | Session tokens (local auth) | 34.0 | 5 (parallel z 34.2) | ⬜ |
| **34.3b** | Google OAuth2 | 34.3a | 8 (opcjonalny) | ⬜ |
| **34.4a-b** | ACL + route protection | 34.3a | 9 | ⬜ |
| **34.5a-b** | Audit trail + UI | 34.0 | Parallel z wszystkim | ⬜ |
| **34.6a** | Recovery keys (BIP-39) | 34.1b | 10 | ⬜ |
| **34.6b** | Safety Numbers | 34.1b | P2 (later) | ⬜ |

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
