# OmniDrive Roadmap v2 — Plan Implementacyjny

Sekwencja: **Phase 0 → Epic 32.5 → Epic 35 → Epic 33 → Epic 34**

Każdy blok to 1-3 dni pracy. Bloki w ramach fazy są sekwencyjne (każdy buduje na poprzednim).

---

## Pre-req: Domknięcie B8

### B8.1: Diagnoza Dell O: hydration timeout
- Sprawdzić `GET /api/onboarding/status` na Dellu — czy providerzy są załadowani do runtime
- Sprawdzić logi angeld.log — szukać błędów downloadu/hydracji
- Zidentyfikować czy problem to R2 connectivity, brak provider reload, czy za krótki timeout

### B8.2: Naprawa hydration na Dellu
- Naprawić zidentyfikowaną przyczynę
- Potwierdzić że Dell może przeglądać pliki na O:
- Potwierdzić LAN peer discovery między Lenovo i Dell (opcjonalnie)

### B8.3: Oficjalne zamknięcie B8
- Wszystkie acceptance criteria zielone
- Zapis wyników do `b8-acceptance-*.json`

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

**Deliverable Phase 0:** `docs/crypto-spec.md` — 2-3 strony, jedno źródło prawdy

---

## Phase 1: Epic 32.5 — Envelope Encryption

### 32.5.1a: Schemat bazy — nowe tabele i kolumny
- Dodać `vault_format_version` do `vault_state`
- Dodać tabelę `data_encryption_keys`: `dek_id`, `inode_id`, `wrapped_dek`, `wrapping_key_version`, `created_at`
- Dodać `encrypted_vault_key` do `vault_state` (Vault Key zaszyfrowany przez KDF-derived key)
- Migracja SQLite: ALTER TABLE + nowe tabele
- Testy: schemat się tworzy, stara baza się otwiera z `vault_format_version = 1`

### 32.5.1b: Vault Key generation i storage
- Generacja losowego 256-bit Vault Key przy tworzeniu nowego vaultu
- Szyfrowanie Vault Key przez klucz z Argon2id(passphrase) — zapis do `vault_state.encrypted_vault_key`
- Unlock flow: passphrase → Argon2id → unwrap Vault Key → trzymaj w pamięci (secrecy crate)
- Testy: generate → store → unlock → porównaj klucz

### 32.5.1c: DEK generation i wrapping
- Przy tworzeniu nowej rewizji pliku: generuj losowy DEK
- Wrap DEK kluczem Vault Key (AES-256-KW lub GCM-SIV — wg decyzji P0.2)
- Zapis wrapped DEK do `data_encryption_keys`
- Testy: generate DEK → wrap → unwrap → porównaj

### 32.5.1d: Szyfrowanie chunków przez DEK
- Zmienić ścieżkę szyfrowania: chunk encryption używa DEK zamiast bezpośrednio KDF-derived key
- Zmienić ścieżkę deszyfrowania: chunk decryption pobiera wrapped DEK → unwrap → decrypt
- Zachować backward compatibility: jeśli `vault_format_version = 1`, używaj starego flow
- Testy: encrypt plik nowym flow → decrypt → porównaj bajt po bajcie

### 32.5.1e: Integracja z uploader/downloader
- Uploader: przy tworzeniu paczki, generuj DEK, wrap, zapisz, szyfruj chunki DEK-iem
- Downloader: przy odczycie paczki, pobierz wrapped DEK, unwrap Vault Key-em, deszyfruj chunki
- Peer read path: DEK jest lokalny, nie przesyłaj go przez LAN (peer wysyła zaszyfrowane chunki)
- Testy E2E: upload plik → download plik → porównaj

### 32.5.2a: Migrator — wykrywanie i planowanie
- Przy starcie daemon: sprawdź `vault_format_version`
- Jeśli v1: zaplanuj migrację — policz ile plików do prze-szyfrowania
- Wyświetl informację w dashboard: "Migracja formatu w toku: X/Y plików"
- Checkpoint tracking: `migration_progress` w `system_config`

### 32.5.2b: Migrator — re-encryption loop
- Dla każdego istniejącego pliku (v1):
  1. Decrypt chunk starym kluczem
  2. Generate DEK
  3. Re-encrypt chunk DEK-iem
  4. Wrap DEK Vault Key-em
  5. Zapisz wrapped DEK + zaktualizuj pack metadata
  6. Checkpoint po każdym pliku
- Resumable: po restarcie, kontynuuj od ostatniego checkpointa
- Testy: migruj 10 plików → przerwij w połowie → restart → kontynuuj → wszystkie pliki v2

### 32.5.2c: Migrator — finalizacja i rollback
- Po migracji wszystkich plików: ustaw `vault_format_version = 2`
- Rollback: jeśli migracja failuje, oznacz plik jako `migration_failed`, kontynuuj z resztą
- Dashboard: pokaż status migracji, błędy, opcję retry
- Testy: symuluj failure w środku → rollback → retry → sukces

### 32.5.2d: Vault Key rotation
- Nowy endpoint: `POST /api/vault/rotate-key`
- Flow: generate new Vault Key → re-wrap wszystkie DEK nowym kluczem → update `encrypted_vault_key`
- NIE re-szyfruj chunków (to jest cały sens envelope encryption)
- Testy: rotate → unlock → decrypt plików → OK

---

## Phase 2a: Epic 35 — Ghost Shell PoC

### 35.0a: Izolowany cfapi.dll PoC — setup
- Nowy crate: `omnidrive-shell` (lub subdirectory `angeld/src/cfapi/`)
- Dodać `windows` crate z feature `Win32_Storage_CloudFilters`
- Minimalny program: zarejestruj SyncRoot, utwórz placeholder, zakończ
- Test: placeholder pojawia się w Eksploratorze jako "cloud file"

### 35.0b: cfapi.dll PoC — hydracja lokalna
- Zaimplementować callback `CF_CALLBACK_TYPE_FETCH_DATA`
- Hydracja z lokalnego ukrytego folderu (nie z chmury)
- Flow: kliknij placeholder → callback → czytaj z cache → dane pojawiają się w pliku
- Test: utwórz plik → zamień na placeholder → kliknij → treść wraca

### 35.0c: cfapi.dll PoC — streaming i progress
- `CfExecute` z `CF_OPERATION_TYPE_TRANSFER_DATA` — progresywny transfer
- Testuj z dużym plikiem (100 MB+) — czy streaming działa bez OOM
- Test interakcji z Windows Defender / Mark of the Web
- **Go/No-Go gate:** jeśli niestabilne → plan B (ProjFS lub inna strategia)

### 35.0d: cfapi.dll PoC — dehydracja
- Flow odwrotny: plik istnieje → zamień na placeholder (dehydrate)
- `CfDehydratePlaceholder` lub manual: backup content → convert to placeholder
- Test: plik 10 MB → dehydrate → 0 bytes on disk → hydrate → 10 MB wraca

---

## Phase 2b: Epic 35 — Full Ghost Shell

### 35.1a: Ingest State Machine — model stanów
- Nowy moduł: `angeld/src/ingest.rs`
- Stany: `PENDING → CHUNKING → UPLOADING → GHOSTED`
- Plus: `HYDRATING`, `FAILED`
- Stan persisted w SQLite: nowa tabela `ingest_state`
- Testy: state transitions, invalid transitions rejected

### 35.1b: Ingest — chunking + DEK + upload
- User request "Ingests plik X" → PENDING
- CHUNKING: read file → generate DEK → encrypt chunks → create pack
- UPLOADING: upload shards to providers (EC_2_1 lub SINGLE_REPLICA wg polityki)
- Confirmation: ALL shards confirmed → GHOSTED
- Error: any shard fails → FAILED z diagnostyką, file untouched

### 35.1c: Ingest — atomowa zamiana na widmo
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
