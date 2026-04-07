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

### 35.1c: Ingest — atomowa zamiana na widmo ← NEXT
- Dopiero po UPLOADING success: zamień oryginalny plik na placeholder
- Użyj cfapi z Phase 2a
- Atomowość: jeśli dehydrate failuje, plik zostaje nienaruszony
- Test: ingest plik → sprawdź placeholder → sprawdź że dane są w chmurze

### 35.1d: Hydration z chmury
- Kliknięcie placeholder → CF_CALLBACK_TYPE_FETCH_DATA
- Download shards → EC reconstruct → decrypt with DEK → stream to callback
- Graceful degradation: jeśli 2/3 providerów niedostępne, timeout + ikona error
- Retry logic z backoff
- Test: ghost → hydrate → porównaj z oryginałem bajt po bajcie

### 35.1e: Ingest — failure recovery i rollback
- FAILED state: diagnostyka w DB (który shard failował, na którym providerze)
- Retry endpoint: `POST /api/ingest/{id}/retry`
- Cleanup: jeśli UPLOADING failuje, usuń częściowo uploadowane shardy
- Dashboard: lista ingestów z ich stanami

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
- Zdefiniować format linku: `https://share.omnidrive.app/{file_id}#{base64url(DEK)}`
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

### 34.1a: OAuth2 — Google Login integration
- Dodać `oauth2` crate do angeld
- Google OAuth flow: redirect → callback → JWT
- Session management w SQLite: `sessions` tabela
- Dashboard: "Zaloguj przez Google" przycisk

### 34.1b: OAuth2 — separation od kryptografii
- Google token = tożsamość, NIE klucz
- X25519 key pair generowany lokalnie z hasła użytkownika (niezależnie od Google)
- Jasny UX: "Twoje hasło Skarbca jest niezależne od konta Google"

### 34.1c: User registry
- Tabela `users`: user_id, google_sub, display_name, public_key, created_at
- Tabela `vault_members`: vault_id, user_id, role (owner/member), wrapped_vault_key
- Public key distribution: serwer przechowuje public keys, nie private

### 34.2a: Asymmetric key generation
- Każde urządzenie generuje parę X25519 (lub ECDH P-256)
- Private key encrypted at rest (DPAPI na Windows)
- Public key rejestrowany na serwerze

### 34.2b: Key wrapping — invite flow
- Owner zaprasza member: `HKDF(ECDH(owner_priv, member_pub)) → AES-256-KW(Vault_Key)`
- Wrapped Vault Key zapisany w `vault_members`
- Member odbiera: unwrap z ECDH(member_priv, owner_pub)
- Serwer widzi tylko zaszyfrowany blob

### 34.2c: Key wrapping — multi-device sync
- Ten sam user, nowe urządzenie: wrap Vault Key dla nowego device key pair
- Flow: potwierdź tożsamość (Google + vault password) → wrap → distribute

### 34.3a: ACL — permissions model
- Role: owner (full), member (read/write), viewer (read-only)
- Per-folder permissions (opcjonalnie, v1 = vault-level only)
- Enforcement w angeld: sprawdź role przed operacją

### 34.3b: Revocation — user removal
- Usunięcie usera → remove wrapped key → rotate Vault Key
- Re-wrap ALL DEK z nowym Vault Key (ale NIE re-encrypt chunków)
- Propagacja do pozostałych members: nowy wrapped Vault Key
- Test: remove user → stary klucz nie działa → nowy klucz działa

### 34.3c: Recovery keys
- Shamir's Secret Sharing: Vault Key split na N parts, K required to reconstruct
- Alternatywnie: BIP-39 mnemonic (24 słowa) jako backup
- UX: "Zapisz te słowa na papierze. Nikt nie pomoże Ci odzyskać danych bez nich."
- Test: utrata hasła → recovery z mnemonic → pełny dostęp

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
