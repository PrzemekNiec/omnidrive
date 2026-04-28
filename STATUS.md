# OmniDrive — Kronika projektu & Roadmapa (Single Source of Truth)

> **Ostatnia aktualizacja:** 2026-04-28
> **Aktualna wersja:** `v0.3.6` — commit `0931683` — instalator `OmniDrive-Setup-0.3.6.exe` gotowy (23 MB)
> **Status:** Faza N.5 Pre-Dell Hardening **ZAKOŃCZONA**. Czekamy na Dell smoke test → tag `v0.3.6`.
> **Zasada:** ten plik to jedyne źródło prawdy. Stare pliki planowania w `docs/archive/`.

---

## Spis treści

1. [Wizja produktu](#1-wizja-produktu)
2. [Genesis — Fundament techniczny](#2-genesis--fundament-techniczny)
3. [Epiki 19–30 — Storage Engine & Infrastruktura](#3-epiki-1930--storage-engine--infrastruktura)
4. [Epic 31+32 — Multi-Device Core & Bridge B0–B8](#4-epic-3132--multi-device-core--bridge-b0b8)
5. [Phase 0 + Epic 32.5 — Krypto: Envelope Encryption](#5-phase-0--epic-325--krypto-envelope-encryption)
6. [Epic 35 — Ghost Shell (cfapi + ingest + tray)](#6-epic-35--ghost-shell-cfapi--ingest--tray)
7. [Epic 33 Tryb A — Zero-Knowledge Link Sharing (LAN)](#7-epic-33-tryb-a--zero-knowledge-link-sharing-lan)
8. [Epic 34 — Family Cloud (audit + recovery + OAuth + Safety Numbers)](#8-epic-34--family-cloud-audit--recovery--oauth--safety-numbers)
9. [Epic 36 — UI Redesign (Stitch Layout)](#9-epic-36--ui-redesign-stitch-layout)
10. [Fazy H–M.6 — Quick Wins + Local-First Lock-in](#10-fazy-hm6--quick-wins--local-first-lock-in)
11. [Faza N — Stabilizacja, Hardening, Release v0.3.6](#11-faza-n--stabilizacja-hardening-release-v036)
12. [Co przed nami](#12-co-przed-nami)
13. [Decyzje architektoniczne](#13-decyzje-architektoniczne)
14. [Risk register](#14-risk-register)
15. [Workflow przypomnienie](#15-workflow-przypomnienie)

---

## 1. Wizja produktu

**OmniDrive** — local-first, zero-knowledge Windows storage:
- Jeden logiczny skarbiec (Vault) widoczny jako `O:\`
- On-demand access przez Eksplorator Windows (cfapi / Cloud Files API)
- Multi-cloud backend (S3-compatible: Backblaze B2, Cloudflare R2, Scaleway)
- 3-warstwowa envelope encryption: passphrase → KEK (Argon2id) → Vault Key (AES-KW) → DEK per-plik (AES-KW) → chunki AES-256-GCM
- Recovery, scrub, repair, reconciliation wbudowane w produkt
- Multi-device (LAN peer discovery, conflict-aware revisions)
- Docelowo: mobile (Android first, UniFFI)

**Stack:** Rust Edition 2024, Tokio, SQLite (`sqlx`), `windows-rs`, `cfapi.dll`, Vanilla JS + Tailwind.

---

## 2. Genesis — Fundament techniczny

> **Pierwsze commity projektu.** Zanim był produkt, była architektura: wielomodułowy workspace Rust, silnik kryptograficzny, lokalny magazyn SQLite i pierwsze endpointy API.

| Commit | Co zbudowano |
|--------|-------------|
| `63f3460` | Init workspace: crates `angeld`, `angelctl`, `omnidrive-core`. Pierwsze `Cargo.toml`. |
| `f4a8101` | Zasady agenta (`.codexrules`). |
| `5689c93` | **Warstwa kryptograficzna:** Argon2id (KDF), AES-256-GCM (szyfrowanie chunków). Pierwsza implementacja `omnidrive-core`. |
| `30ff31e` | **SQLite:** init store z `sqlx` + Tokio runtime. Schema: `vaults`, `inodes`, `packs`, `object_locations`. |
| `7b7b399` | Mapowanie chunk→pack + kolejka uploadu. |
| `a371836` | Lokalne pakowanie (packer) + resilient upload worker z `exponential backoff`. |
| `cb831bd` | API bridge dla statusu transferu (lokalny HTTP). |
| `e173276` | **Downloader:** pełny read-path (cloud → decrypt → plik). |
| `3b2df18` | **Vault master key:** unlock flow — passphrase → Argon2id → klucz AES. Pierwsza wersja krypto-pipeline. |
| `2493cbe` | Stabilizacja startu daemona + TLS init. |
| `9f4a5be` | **Local web dashboard UI** — pierwsze HTML/JS serwowane z daemona. |
| `8f5e056` | Rozszerzony REST API + `angelctl` CLI. |
| `cb55990` | Throttling pasma uploadu (globalny rate-limiter). |
| `972fe84` | Quota controls + silnik polityk synchronizacji. |
| `39a8a8b` | **Deduplikacja na poziomie chunków** (hash-based dedup). |
| `2fe8f99` | **Erasure Coding:** lifecycle EC_2_1 (2 data shards + 1 parity). |
| `2cf34c4` | **Smart Sync bootstrap:** placeholders w Eksploratorze (pierwsze cfapi). |
| `e50d66d` | Smart Sync hydration + control surface. |
| `3cf35f0` | **Disaster Recovery:** kompletny silnik odtwarzania z chmury. |
| `bde4a30` | Snapshotting metadanych na chmurę (backup DB). |
| `0c302b5` | **Wirtualny dysk `O:\`** — mount jako Windows Virtual Drive. |
| `e006e72` | Własna ikona + etykieta dla `O:\`. |

---

## 3. Epiki 19–30 — Storage Engine & Infrastruktura

> **Zbudowanie całego silnika przechowywania danych:** scrubbing, cache, storage modes, diagnostics, installer, shell self-heal, dashboardy kosztów i konserwacji.

### ✅ Epic 19.5 — Virtual Drive Mapping (`O:\`)

| Co zrobiono | Wynik |
|-------------|-------|
| OmniDrive wystawiony przez `O:\` | Eksplorator widzi skarbiec jako dysk |
| Custom drive label + icon | Profesjonalny wygląd; czytelne entry-point |
| Stały punkt wejścia dla użytkownika | `O:\` jako główna powierzchnia produktu |

### ✅ Epic 20 — Disaster Recovery

| Co zrobiono | Wynik |
|-------------|-------|
| Zaszyfrowane backupy metadanych na chmurę | Restore możliwy bez działającego daemona |
| Flow odtwarzania z S3-compatible storage | Odbudowa struktury inodów z chmury |
| Ekspozycja statusu recovery w API, CLI, UI | Operator widzi postęp i błędy |

### ✅ Epic 21 — Deep Data Scrubbing (`d6ed453`)

| Co zrobiono | Wynik |
|-------------|-------|
| Background scrubber weryfikujący shards | Ciągła weryfikacja integralności danych |
| Tryby light i deep verification | Elastyczne kosztownie weryfikacji |
| Detekcja stanów `degraded` i `unreadable` | Wczesne ostrzeganie o uszkodzeniach |

### ✅ Epic 22 — Intelligent Local Cache & Predictive Prefetching (`fc10171`)

| Co zrobiono | Wynik |
|-------------|-------|
| Zaszyfrowany cache lokalny (`%LOCALAPPDATA%\OmniDrive\Cache`) | Szybszy odczyt bez kolejnych pobrań |
| Downloader cache-aware | Unika duplikatów downloadu |
| LRU eviction + predictive prefetching | Inteligentne zarządzanie miejscem |

### ✅ Epic 23.5 — Flexible Storage & Policy Reconciliation (`16b0ac9`)

| Co zrobiono | Wynik |
|-------------|-------|
| Tryby `EC_2_1`, `SINGLE_REPLICA`, `LOCAL_ONLY` | Wybór trade-off: redundancja vs koszt |
| Read path rozumie każdy tryb | Transparentna praca niezależnie od trybu |
| Reconciliacja migrująca dane między trybami | Zmiana polityki bez utraty danych |

### ✅ Epic 24 — Secure Local Runtime (`cea248d`)

| Co zrobiono | Wynik |
|-------------|-------|
| Lepsza ochrona klucza w pamięci | Klucz nie wisi w plain stacku |
| Cache encryption oddzielona od vault key | Izolacja przestrzeni kluczy |
| ACL hardening katalogów runtime | Inne procesy nie czytają danych OmniDrive |

### ✅ Epic 26 — End-to-End Test Matrix (`e8471c1`)

| Co zrobiono | Wynik |
|-------------|-------|
| Recovery, reconciliation, self-heal pokryte E2E | Krytyczne ścieżki nie tylko unit-testowane |
| Full-stack disaster recovery test (`f8b07d7`) | Dowód działania end-to-end |
| Policy reconciliation E2E (`5bb6ac4`) | Bezpieczna zmiana trybu storage |

### ✅ Epic 27 — Installer and First-Run Bootstrap (`8bea9ed`)

| Co zrobiono | Wynik |
|-------------|-------|
| Per-user installer (Inno Setup) | Instalacja bez uprawnień admina |
| Runtime bootstrap pod `%LOCALAPPDATA%\OmniDrive` | Stabilny cold start |
| Local-only onboarding bez providerów | Pierwszy start bez konfiguracji chmury |
| Autostart daemona | OmniDrive aktywny po restarcie |
| Walidacja po restarcie | Reboot validation passed |

### ✅ Epic 28 — Self-Healing Shell Integration (`300c96f`, `a1b221a`)

| Co zrobiono | Wynik |
|-------------|-------|
| Audyt stanu shella | Wykrywanie uszkodzeń w SyncRoot / rejestrze |
| Shell repair + sync-root repair | Automatyczna naprawa |
| Startup recovery | Daemon naprawia się przy starcie |
| Second-machine validation passed | Potwierdzenie na drugiej maszynie |

### ✅ Epic 29 — Storage Cost and Policy Dashboard (`eb4e2b2`)

| Co zrobiono | Wynik |
|-------------|-------|
| `GET /api/storage/cost` | Koszty cloud widoczne w UI |
| Policy mix, provider distribution, GC debt | Pełny obraz ekonomii skarbca |
| Reconcile backlog metrics | Widoczny dług synchronizacji |
| Acceptance passed | Zatwierdzone na maszynie testowej |

### ✅ Epic 30 — Maintenance Console (`1a232c2`)

| Co zrobiono | Wynik |
|-------------|-------|
| Maintenance actions w dashboardzie | Repair, scrub, backup, reconciliation z UI |
| Diagnostyki zagregowane w jednym widoku | Operator ma pełny obraz stanu |
| Triggerable z UI | Zero komendy CLI dla rutynowych operacji |
| Acceptance passed | Zatwierdzone na maszynie testowej |

---

## 4. Epic 31+32 — Multi-Device Core & Bridge B0–B8

> **Przekształcenie OmniDrive z single-device vault w prawdziwy multi-device system.** LAN peer discovery, konflikt-świadome rewizje, i pełny onboarding flow dla drugiej maszyny.

### ✅ Epic 31+32 — Multi-Device Core (`fd768e7`–`c9b4150`)

| Task | Co zrobiono | Wynik |
|------|-------------|-------|
| **31.1 Device Identity** | Trwały `device_id` w SQLite, rejestr zaufanych peers | Każda instalacja ma unikalną tożsamość |
| **31.2 Peer Discovery** | LAN discovery + handshake service w daemonie | Automatyczne wykrywanie OmniDrive w sieci |
| **31.3 Peer Read Path** | Downloader: peer-first → cloud fallback | Niższy egress, szybsze LAN reads |
| **31.4 Peer Cache Policy** | Retry rules, timeout, health scoring, source preference | Przewidywalne zachowanie peer-assisted reads |
| **32.1 Revision Lineage** | `device_id` + `parent_revision_id` + `origin` na rewizjach | OmniDrive odróżnia update od konfliktu |
| **32.2 Conflict Detection** | Wykrywanie równoległych revision heads | Konkurencyjne edycje nie są nadpisywane |
| **32.3 Conflict Materialization** | Conflict-copy naming + materialization | Obie wersje zachowane, użytkownik widzi konflikt |
| **32.4 Policy Rules** | Linear lineage vs competing-head rules | Zachowanie rewizji zrozumiałe i bezpieczne |
| **32.5 Multi-Device Diagnostics** | `GET /api/multidevice/status`, dashboard panel | Operator widzi zdrowie multi-device |

### ✅ Bridge B0–B8 — Onboarding + Join Existing Vault (v0.1.15–v0.1.20)

> Most między teoria multi-device a jego testowalnym wdrożeniem produkcyjnym.

| Task | Commit / Wersja | Co zrobiono |
|------|-----------------|-------------|
| **B0** Cloud safety + DRY_RUN | v0.1.x | Daily quota circuit breaker, single-file guard, `--dry-run` mode z API/UI visibility |
| **B1** Onboarding State Persistence | v0.1.x | `system_config` (onboarding_state, mode, step, cloud_enabled) + `provider_configs` + DPAPI secrets |
| **B2** `.env` draft import | v0.1.x | Detekcja `.env` tylko przy niekompletnym onboardingu, import jako draft |
| **B3** Onboarding API | v0.1.x | 6 endpointów: `status`, `bootstrap-local`, `setup-identity`, `setup-provider`, `join-existing`, `complete` |
| **B4** Provider validation | v0.1.x | auth + bucket access + read/list + write/delete probe |
| **B5** First-run wizard UI | v0.1.x | Glassmorphism overlay, krok-po-kroku onboarding |
| **B6** Join Existing + graft | v0.1.x | Restore metadanych z chmury, vault_id graft, sync-root activation |
| **B7** DB-backed providers + hot-reload | v0.1.x | Daemon ładuje providerów z SQLite; `POST /api/onboarding/setup-provider` reloads |
| **B8** Lenovo+Dell `dir O:\` instant | v0.1.20 | 3 cfapi fixes w `smart_sync.rs` (`DISABLE_ON_DEMAND_POPULATION`, `FETCH_PLACEHOLDERS`, `PARTIAL policy`) |

---

## 5. Phase 0 + Epic 32.5 — Krypto: Envelope Encryption

> **Przejście od prostego hasło→klucz do 3-warstwowej hierarchii kluczy.** Fundamentalna zmiana architektury bezpieczeństwa. Przed Phase 0 jeden klucz na plik; po — DEK per-plik wrappowany przez losowy Vault Key.

### ✅ Phase 0 — Crypto Checkpoint (`docs/crypto-spec.md`)

| Co zrobiono | Wynik |
|-------------|-------|
| Formalny dokument `docs/crypto-spec.md` | Single Source of Truth dla krypto |
| Decyzja: 3-warstwowa hierarchia (passphrase → KEK → VK → DEK) | Architektura zgodna z WebCrypto, revokable VK |
| AES-256-KW (RFC 3394) dla wrappingu kluczy | WebCrypto-kompatybilny, brak nonce, deterministyczny |
| DEK per-plik (nie per-chunk) | Jeden secret w share URL dla Epic 33 |
| Format ChunkRecordPrefix V2 (80B, `record_version=2`) | Versioned format, backward compatible |

### ✅ Epic 32.5 — Envelope Encryption (`9ded01a`, `f6286dc`, `ad65cc2`)

| Krok | Commit | Co zrobiono |
|------|--------|-------------|
| **32.5.1a-b** KEK + Vault Key | `9ded01a` | `derive_root_keys()` → KEK + vault_key. `wrap_key()` / `unwrap_key()` (AES-256-KW). Losowy Vault Key generowany przy init. |
| **32.5.1c-d** DEK per-file + V2 encrypt | `9ded01a` | DEK per-plik w SQLite. `pack_file_v2()` + `unpack_file_v2()`. Chunki szyfrowane DEK (AES-256-GCM, nonce random 12B). |
| **32.5.2a-c** Batch Migrator V1→V2 | `f6286dc` | `migrator.rs`: lazy migration (nowe pliki V2, stare czytane V1). Opcjonalny batch re-encryption. Resumable z checkpointami. |
| **32.5.2d** Vault Key Rotation | `ad65cc2` | `rotate_vault_key()`: nowy losowy VK, re-wrap wszystkich DEKów w transakcji, bump `vault_key_generation`. |

---

## 6. Epic 35 — Ghost Shell (cfapi + ingest + tray)

> **OmniDrive jako natywna integracja z Windows Explorer:** on-demand placeholders (plik wygląda jakby był lokalnie, pobiera się tylko przy otwarciu), shell extension z menu kontekstowym, tray companion.

### ✅ Epic 35 — Ghost Shell

| Krok | Co zrobiono | Wynik |
|------|-------------|-------|
| **35.0a-d** cfapi PoC | SyncRoot register + connect + hydracja + streaming + dehydracja w `smart_sync.rs` (zamknięte w B8) | `O:\` z placeholderami działa w Eksploratorze |
| **35.1a** Ingest State Machine | State transitions: `IDLE→LEASE_ACQUIRE→STAGED→CHUNKING→PACKING→UPLOADING→COMMITTING→COMMITTED` | Bezpieczny potok zapisu bez partial writes |
| **35.1b** Chunking + DEK | Plik → chunks → DEK z VK → DB record | Szyfrowanie inline przy ingest |
| **35.1c** Atomic swap | Tmp-file write → atomic rename w DB | Zero corruption przy przerwaniu |
| **35.1d** Hydration | `CfHydratePlaceholder` + streaming decrypt | Plik pojawia się lokalnie przy otwarciu |
| **35.1e** Failure recovery | Retry ingest po crash, cleanupStaleUploads przy starcie | Daemon przeżywa restarty bez leftoverów |
| **35.2a** Shell Extension DLL | `omnidrive-shell-ext` crate, thin client `IContextMenu` | Menu kontekstowe w Eksploratorze bez crash-ryzyka |
| **35.2b** Context menu 4 poziomy | LOKALNIE / COMBO / CHMURA / FORTECA | Użytkownik widzi stan i ma akcje per-plik |
| **35.2c** Natywne stany cfapi | Wyłącznie `CfSetPlaceholderState` + `CfSetPinState` (zero custom overlays) | Ikonki Windows natywne; zero konfliktu z innymi programami |
| **35.3** System Tray Companion | `omnidrive-tray` crate, polling `/api/health` co 5s, `Shell_NotifyIcon` | Status daemona widoczny w zasobniku systemowym |

---

## 7. Epic 33 Tryb A — Zero-Knowledge Link Sharing (LAN)

> **Udostępnianie plików bez serwera pośredniczącego.** Link zawiera DEK w URL fragment — nigdy nie trafia na serwer. Odbiornik deszyfruje lokalnie w przeglądarce. Tryb A = LAN (ten sam router).

### ✅ Epic 33 Tryb A — LAN Share

| Co zrobiono | Wynik |
|-------------|-------|
| Fragment-based link: `http://IP:8787/share/{id}#{dek}` | DEK nigdy nie opuszcza przeglądarki odbiorcy |
| Dynamic host: `Host:` header lub `OMNIDRIVE_SHARE_HOST` (M.6.6) | Alice generuje link ze swoim LAN IP; Bob klika i pobiera |
| Wbudowany decryptor w `dist/share-site/index.html` | Zero zewnętrznych zależności przy dekryptowaniu |
| Chunked download + WebCrypto decrypt (AES-GCM) | Streaming — duże pliki bez ładowania do RAM |
| Revocation przez usunięcie z DB | Alice może wycofać link natychmiast |
| Manifest JSON per-share w SQLite | Śledzenie aktywnych shares |

---

## 8. Epic 34 — Family Cloud (audit + recovery + OAuth + Safety Numbers)

> **OmniDrive jako system wielu użytkowników:** invite, ACL, audit trail, recovery keys BIP-39, Google OAuth2, Safety Numbers (Identicon + mnemonik). Sesje A–E to kompletna pełna implementacja.

### ✅ Epic 34 — Sesje A–E

| Sesja | Commit | Co zrobiono |
|-------|--------|-------------|
| **34.0–34.4a** Schema + Crypto + Invite + ACL | — | Tabele `vault_members`, `invite_codes`, `devices` z ACL. X25519 asymmetric crypto dla device keys. Invite flow z linkiem. Revocation. Role: Owner/Member. |
| **34.5a+b** Audit Trail | — | Tabela `audit_logs` (vault_id, action, user_id, device_id, target_*, details). `GET /api/audit/logs` + UI widok Audyt. Każda krytyczna operacja zostawia ślad. |
| **34.6a** Recovery Keys BIP-39 | — | 24-słowny mnemonik (AES-KW recovery_key → wrapped VK). `POST /api/recovery/generate`. `POST /api/recovery/restore`. Revocation. Rate-limit + state-guard (N.5/B.2). |
| **Faza J** Pre-C: UUID v4 | `13177b6` | `user_id` → UUID v4 (zamiast `owner-{device_id}`). `backfill_uuid_user_ids` naprawia legacy bazy przy starcie. FK-safe migracja. |
| **Faza K** Google OAuth2 Backend | `667b0d5`, `45ca50a` | PKCE (S256, base64url, SHA-256). `oauth_states` TTL 10 min. `GET /api/auth/google/start` → Google. `GET /api/auth/google/callback` → upsert user, sesja, redirect. `google_refresh_token` w DB. 11 testów. |
| **Faza L** OAuth Frontend | `6530194` | Przycisk „Zaloguj przez Google" w onboardingu. Profil w topbarze (`GET /api/auth/session`). `#oauth_token` z URL → sesja JS. |
| **Faza M** Safety Numbers | `5570091`, `803a865`, `a267cf8` | `SHA-256(envelope_vault_key ‖ user_id)` → 60-cyfrowy fingerprint (Signal-style). `GET /api/vault/safety-numbers`. `POST /api/devices/{id}/verify`. Sekcja „Bezpieczeństwo" w UI z QR kodem. |
| **Faza M.5** Human-Friendly Verification | `45a9b89`, `29dded3` | BIP-39 mnemonik (12 słów, 128-bit entropy z `hash[..16]`). Identicon (jdenticon 3.2.0). Trzy reprezentacje fingerprinta: cyfry 4×3 + słowa + SVG. 2 nowe testy (`safety_mnemonic_is_12_english_words_and_stable`, `safety_mnemonic_differs_per_user`). |

---

## 9. Epic 36 — UI Redesign (Stitch Layout)

> **Wymiana starego „local web dashboard" na profesjonalną konsolę (Skarbiec Console):** sidebar 240px + header 64px + 7 dedykowanych widoków. Hash-router. Glassmorphism. v0.2.0 release.

### ✅ Epic 36 — Sesja F + G

| Krok | Commit | Co zrobiono |
|------|--------|-------------|
| **F.1–F.4** Layout shell | `cd679d9`–`ac4f91e` | Stitch layout (sidebar + header + content area). Nawigacja po 7 zakładkach. `/legacy` fallback dla starego UI. |
| **F.5–F.6** Shard status card + polling | `cd679d9` | Status pill shard health z polling co 5s. `GET /api/vault/status` wired. |
| **F.7–F.8** Hash router + weryfikacja | `ac4f91e` | `#pliki`, `#skarbiec`, `#chmura`, `#audyt`, `#multi-device`, `#ustawienia`. Deep-link friendly. |
| **G.1–G.3** Stats Backend | `f1a2dc3` | `stats.rs`: `GET /api/stats/overview`, `/traffic`, `/system` (CPU, RAM, dysk, sysinfo). |
| **G.4** Stats → UI | `aa12e2e` | Widok domyślny z kartami metryk, polling 5s. |
| **G.5** Widok Pliki | `5ea1f91` | `GET /api/files` → tabela plików z rozmiarem, statusem, akcjami. |
| **G.6** Widok Skarbiec | `954f02e` | Unlock, recovery keys (druk A4), status klucza, generowanie kluczy recovery. |
| **G.7** Widok Multi-Device | `93a7d66` | `GET /api/vault/devices` → lista urządzeń, health, peer status. |
| **G.8–G.9** Widoki Chmura + Audyt | `55a2a6a` | Cloud stats, egress, koszty. Audit log z filtrowaniem. |
| **G.10** Widok Ustawienia | `9e67bf8` | `GET/POST /api/settings/*`. Wszystkie opcje konfiguracji w UI. |
| **G.11 + release** v0.2.0 | `7af859a` | Finał Sesji G. Release v0.2.0. 87/87 testów zielonych. |

---

## 10. Fazy H–M.6 — Quick Wins + Local-First Lock-in

> **Seria szybkich ulepszeń po v0.2.0:** domknięcie UI (QR, logout, OAuth UI, Safety Numbers), API (lock, rotate-key, policies), tożsamość (UUID v4), Google OAuth backend i frontend, Safety Numbers, Local-First hardening architektury sieci.

| Faza | Commit | Co zrobiono |
|------|--------|-------------|
| **H** Quick-wins UI | `e4ea91f` | H.1 QR code (qrcode.min.js lokalnie). H.2 logout (`POST /api/auth/logout`). H.3 fetchAuditLog z Bearer. H.4 Recovery CTA → modal. H.5 link „Pełny log" → `#audyt`. |
| **I** API vault | `de0ce1b` | `POST /api/vault/lock` (zeruje VaultKeyStore + audit). `POST /api/vault/rotate-key` (rotacja z hasłem). `GET /api/filesystem/policies` (AppConfig). sysinfo CPU w `/api/stats/system`. |
| **J** UUID v4 user_id | `13177b6` | `uuid = "1"` (v4). `db::new_user_id()`. `migrate_single_to_multi_user` generuje UUID. `backfill_uuid_user_ids` naprawia legacy bazy. FK-safe (PRAGMA FK OFF/ON). Test backfill. |
| **K** Google OAuth2 Backend | `45ca50a`, `667b0d5` | PKCE (S256). `oauth_states` TTL 10 min. Callback: exchange code→token, GET /userinfo, upsert users, sesja, redirect `/#oauth_token=...`. `google_refresh_token TEXT`. 8 testów DB + 3 PKCE. Uwagi Gemini zaadresowane. |
| **L** OAuth Frontend | `6530194` | Przycisk „Zaloguj przez Google". Profil (email + avatar) w topbarze. Logout opcjonalnie revoke refresh token. |
| **M** Safety Numbers | `5570091`–`a267cf8` | 60-digit Signal-style fingerprint. API `safety-numbers` + `verify device`. UI sekcja Bezpieczeństwo + QR. |
| **M.5** BIP-39 + Identicon | `45a9b89`, `29dded3` | Mnemonik 12 słów (BIP-39, `hash[..16]`). Identicon (jdenticon SVG). Grid 4×3 cyfry. Hotfix overflow. |
| **M.6.1** CORS exact-match | `4cfca26` | `host_from_http_origin()` z `IpAddr::parse()`. `http://localhost.evil.com` → reject. Tylko true loopback + RFC1918. |
| **M.6.2** OAuth loopback assert | `4cfca26` | Runtime assertion: `oauth_redirect_url` musi zaczynać się od `http://127.0.0.1:` lub `http://localhost:`. |
| **M.6.3–M.6.5** Docs purge + README | `6ec4af5` | Usunięte stale referencje do skarbiec.app z kodu/docs. README sekcja „Architektura sieci: 100% Local-First". |
| **M.6.6** Dynamic share host | `6ec4af5` | Link generowany z `Host:` headera lub `OMNIDRIVE_SHARE_HOST`. LAN share działa end-to-end. |
| **post-M.6** CF Pages / D1a | `59ed4ae`, `0433bbc` | Decyzja: `skarbiec.app` → wyłącznie CF Pages static content (decryptor Trybu B, landing). Daemon nigdy publicznie. Zatwierdzone jako D1a. |

---

## 11. Faza N — Stabilizacja, Hardening, Release v0.3.6

> **Droga od v0.2.0 do v0.3.6:** stabilizacja E2E testów, audyt bezpieczeństwa wykrył 20 znalezisk, implementacja Pre-Dell Hardening w 6 batchach, build instalatora gotowy.

### ✅ Faza N.1+N.2 — Dead Code + Hybrid E2E (`7819811`, `0f1af36`)

| Co zrobiono | Wynik |
|-------------|-------|
| `#![allow(dead_code)]` → komentarze `// reserved for Epic X` w 10 plikach | Dead-code audit ma kontekst, nie zgubi się w przyszłości |
| `roundtrip_pack_upload_download_restore_file` w `downloader::tests` | Full cycle: pack → mock S3 (Axum, 3 providery) → restore → assert bytes |
| Naprawiony `set_and_get_safety_verified_roundtrip` (brakujący INSERT users, FK fail) | Test suite zielony |
| **98 testów** (87/87 angeld + 11/11 omnidrive-core) | Wszystkie zielone po N.2 |

### ✅ v0.3.0 — Lazy Mount + Lock Screen + Smart Sync fixes (`598c914`–`f11a8e7`)

| Commit | Co zrobiono |
|--------|-------------|
| `9494ddb`, `a4c518d` | Smart Sync: `DISABLE_ON_DEMAND_POPULATION=0x2`, `PARTIAL` policy, `FETCH_PLACEHOLDERS` fix |
| `daf24f7` | Dehydrate wszystkich CF placeholderów przy lock vaulta |
| `f0389c5` | **Lazy Mount + Brutal Lock:** `O:\` widoczny tylko po unlock; pri lock → unmount + dehydrate |
| `cc8054b` | B2 transfer amplification fix: in-flight pack dedup + `NOT_CONTENT_INDEXED` |
| `79e2ba9` | **UniFFI scaffold:** `ffi_unwrap_key` + `ffi_decrypt_chunk_v2` w `omnidrive-core` (seed dla Fazy P) |
| `598c914` | Bump 0.2.0 → 0.3.0. Release artifacts. |

### ✅ v0.3.1–v0.3.3 — Windows Hello + Lock Screen + Console fixes

| Commit | Co zrobiono |
|--------|-------------|
| `9bbe957`, `d186ef3` | **Lock Screen UI:** Zero-Knowledge startup gate, Stitch-inspired redesign |
| `83f61a5` | **Windows Hello DPAPI unlock** (passphrase sealed z DPAPI → automatyczny unlock po TPM). `POST /api/change-password`. |
| `d15fe23` | Bump → v0.3.1 |
| `9004ed8` | No console window w release (`CREATE_NO_WINDOW`) + vault init gate + wizard link |
| `31e80f4` | Windows Registry API zamiast `reg.exe` spawn |
| `28f5f91` | `CREATE_NO_WINDOW` na wszystkich `Command::new` + bump v0.3.3 |

### ✅ v0.3.4–v0.3.5 — Wizard Onboarding Page + Multi-user register fix

| Commit | Co zrobiono |
|--------|-------------|
| `4e8ef07`, `d456dc6` | **Nowa strona `/wizard`:** kompletny onboarding bez zależności od `/legacy`. Guard bez flasha przed redirectem. |
| `4221845` | Code-review fixes: fail-closed guard, CSP headers, `data-current-step` |
| `5f1e757` | Fix: register local device w multi-user tables po graftcie |
| `c8a1c59` | Bump → v0.3.5 |

### ✅ Faza N.5 — Pre-Dell Hardening (Batch 1–6)

> **Geneza:** audyt security-reviewer + tech-lead-reviewer (2026-04-27). **20 znalezisk** (7 HIGH + 7 MEDIUM + 6 LOW). Cel: Skarbiec hermetyczny przed wgraniem na Della.

#### Batch 1 — Foundation + Cross-Device Critical

| Item | Commit | Co zrobiono |
|------|--------|-------------|
| `A.0` | `bb6e596` | `retry_io` helper w `secure_fs.rs` (5 prób × 500ms backoff). `secure_delete` używa retry. Jeden punkt dla file-lock handling w całym daemonie. |
| `A.2` | `796180e` | Staging file `secure_delete`: zero-overwrite przed delete + retry 5×500ms. Plaintext metadata nie zostaje po graftcie. |
| `A.4` | `f55d810` | `drop(restored_pool) + yield_now()` w `db.rs`. Daje async runtime czas na finalizację handle przed próbą kasowania pliku. |

#### Batch 2 — Watcher + Pubkey Defense

| Item | Commit | Co zrobiono |
|------|--------|-------------|
| `A.1` | `5c31ec4` | Watcher DRY_RUN gate: sprawdza `dry_run_active` + `onboarding_state != Completed`. Zero modyfikacji plików na świeżym Dellu. |
| `A.3` | `4f949bb` | X25519 low-order point rejection: `validate_x25519_pubkey()` odrzuca `[0;32]` i 8 punktów małego rzędu (RFC 7748). `devices.enrolled_at` schema. Migracja. |

#### Batch 3 — Crypto Quick Wins

| Item | Commit | Co zrobiono |
|------|--------|-------------|
| `B.4` | `ebd3220` | `thread_rng` → `OsRng` w `db.rs` + `oauth.rs`. Kryptograficznie bezpieczny RNG zgodny z policy. |
| `B.1` | `ebd3220` | CORS exact-match: `host_from_http_origin()` + `IpAddr::parse()`. `http://localhost.evil.com` → reject. Unit testy. |

#### Batch 4 — Auth Surface Hardening

| Item | Commit | Co zrobiono |
|------|--------|-------------|
| `B.2` | `35a95bb` | `recovery/restore`: rate-limit (DashMap, 3 próby/5min, lockout 30s) + state-guard (blokada przy aktywnym vault + próba < 24h) + audit IP+UA. |
| `B.5` | `0803908` | `join-existing`: state-guard + progressive delay (1s→5s→30s). Brute-force join nieopłacalny. |
| `B.3 K1` | `a6446db` | `Referrer-Policy: no-referrer` + `X-Frame-Options: DENY` na index. Krok 1 OAuth URL cleanup. |

#### Batch 5 — Polish / Diagnostyka

| Item | Commit | Co zrobiono |
|------|--------|-------------|
| `A.5` | `348ed0d` | Restore state markery: `restore_state ∈ {idle, downloading, applying, last_failed, last_succeeded}`. `GET /api/diagnostics/restore`. |
| `A.6` | `348ed0d` | `provider_configs` graft: `created_at = epoch_secs()` lokalny (nie timestamp właściciela). |
| `A.7` | `2a0a763` | `migrate_single_to_multi`: `target_user_id` + `target_device_id` wypełniane w audit logu. |
| `A.8` | `517b5a0` | `CONNECTION_KEY.lock().unwrap_or_else(|e| e.into_inner())` (5 miejsc w `smart_sync.rs`). Daemon przeżywa paniki w cfapi callbacks. |
| `A.9` | `9e42575` | `verify_vault_device_binding()` przy starcie. `panic!` przy niezgodności `vault_id ↔ device_id`. |
| `B.6` | `fda2cec` | `validate_user_session` bez constant-time — udokumentowane w `crypto-spec.md` §11 (256-bit random token + LAN only = atak niewykonalny). |
| `B.7` | `fda2cec` | `OMNIDRIVE_AUTO_RESTORE_PASSPHRASE` ignorowany w release, WARN na dev. |

#### Batch 6 — Defense in Depth

| Item | Commit | Co zrobiono |
|------|--------|-------------|
| `C.1` | `3a8fd88` | Google refresh token → AES-GCM sealed blob (`HKDF(EVK, "oauth-refresh-tokens-v1", user_id)` jako AAD). Kolumna `google_refresh_token_ciphertext BLOB`. `vault.unlock()` auto-migruje plaintext. Callback seals jeśli vault open. 3 testy (roundtrip, locked-vault guard, random-nonce). 102/102 testów. |
| `C.2` | `0534281` | `passphrase: String` → `secrecy::SecretString` w 4 request DTO (`api/auth.rs`, `api/recovery.rs`, `api/onboarding.rs`, `api/vault.rs`). `secrecy = { version = "0.10", features = ["serde"] }` w workspace. `expose_secret()` przy use-site. Zeroize on drop. |

### ✅ v0.3.6 — Version Bump + Release Build (`0931683`)

| Co zrobiono | Wynik |
|-------------|-------|
| Bump 0.3.5 → 0.3.6 we wszystkich 6 `Cargo.toml` + `installer/omnidrive.iss` | Spójna wersja w całym workspace |
| `cargo build --release --workspace` — czyste | Wszystkie crate skompilowane bez błędów |
| `cp target/release/*.exe dist/installer/payload/` | Payload aktualny |
| Inno Setup → `OmniDrive-Setup-0.3.6.exe` (23 MB) | Instalator gotowy do Dell smoke testu |

---

## 12. Co przed nami

### 🔜 Dell Smoke Test Gate (NEXT — P0)

> Gate przed tagiem `v0.3.6`. Świeża instalacja na Dellu.

| Assert | Co weryfikuje |
|--------|---------------|
| `Join Existing Vault` z passphrase + B2/R2 | Graft metadanych + unlock end-to-end |
| `%LOCALAPPDATA%\OmniDrive\restore-staging-*` pusty po graftcie | A.0+A.2+A.4 działają na Dellu (Defender) |
| Watcher w logach pokazuje `[DRY_RUN]` | A.1 gate aktywny przed Completed onboarding |
| `accept_device` z `[0;32]` → `400 low_order_pubkey` | A.3 działa |
| `curl -H "Origin: http://localhost.evil.com"` → CORS reject | B.1 działa |
| Brute-force `/api/recovery/restore` 10× → 429 po 3. próbie | B.2 działa |
| OAuth callback → `window.location.hash === ""` | B.3 Krok 1 działa |
| Cross-device Identicon + mnemonik (Lenovo ↔ Dell) | M.5 deterministyczny SVG + te same słowa BIP-39 |
| Crash daemona (`kill -9`) podczas graftu → restart → retry | Resilience po crash |

### 🔜 Tag v0.3.6 Release (po Dell smoke — P0)

| Krok | Co zrobiono |
|------|-------------|
| N.4 | Commit `release: v0.3.6`, push, tag `v0.3.6` |
| N.5 | SHA-256 instalatora w GitHub Releases + `README.md` |
| CHANGELOG | Wpis v0.3.6: N.5 Paczki A+B+C — 20 znalezisk hardening |

### ⬜ Batch 7 — POST-DELL (P2 — Backlog)

| Item | Zakres |
|------|--------|
| **C.3** rustls/hyper consolidation | `aws-config`/`aws-sdk-s3` → hyper-1. `sqlx` → rustls. `cargo audit` clean. Odłożone: major AWS SDK bump = wysokie ryzyko regresji. |
| **B.3 Krok 2** | OAuth code-exchange refactor: `/?oauth_code=` + `POST /api/auth/oauth/exchange` → `Set-Cookie: HttpOnly; Secure; SameSite=Strict`. |
| **B.2 Tray confirmation** | Task 35.3 IPC: tray popup przy próbie recovery restore (fizyczna obecność). |

### ⬜ Faza O.1 — Quota Fix (P1 — po v0.3.6)

Raportowanie pojemności `O:\` z faktycznego cloud quota (B2/R2) zamiast z `C:`. ~1 dzień.

### ⬜ Epic 33 Tryb B — Public Shares przez CF Pages (P2 — po v0.3.6)

| Komponent | Zakres |
|-----------|--------|
| Upload | Duplicate encrypted chunków pod `shares/{share_id}/chunk-*.bin` |
| Link format | `https://skarbiec.app/s/{id}#{dek}@{b2_base}` |
| Password protection | `wrapped_dek = AES-KW(dek, PBKDF2(password, salt))` w manifeście |
| Revocation | DELETE prefix w B2 → 404 dla Boba; SQLite mapping `share_id → [object keys]` |
| `share-site` repo | Osobny repo: `index.html` + `decryptor.js`, CF Pages auto-deploy |
| Fallback | `omnidrive.github.io/share` jeśli CF odpadnie |
| Szacunek | 2–3 tygodnie |

### ⬜ Faza O.2+ — Cross-Platform VFS Foundation (P2 — backlog)

Trait `FileSystemAdapter`, refactor `cfapi` → implementacja traitu, prototyp FUSE adaptera Linux/macOS. Enabler dla Fazy P–R. ~2–4 tygodnie.

### ⬜ Faza P — Core Extraction dla Mobile (P3 — po v0.4.0)

| Krok | Zakres |
|------|--------|
| P.1 | Audit `angeld/src/*.rs`: „pure logic" → `omnidrive-core` (packer, downloader, vault crypto) vs „platform glue" (zostaje w angeld) |
| P.2 | `index.json.enc` — SQLite snapshot generowany przez daemon (daemon writes, mobile reads). D4 = Opcja C zatwierdzona. |
| P.3 | UniFFI proc-macro: eksport `ffi_unwrap_key`, `ffi_decrypt_chunk_v2` (scaffold już w `79e2ba9`). Kotlin bindingi. `libomni_core.so`. |
| Szacunek | 2–3 dni |

### ⬜ Faza Q — Mobile Bridge & Handshake (P3)

| Krok | Zakres |
|------|--------|
| Q.1 | Android skeleton (Kotlin + Compose, `minSdkVersion 26`) |
| Q.2 | ML Kit QR scanning + parsowanie `omnidrive://pair?...` |
| Q.3 | ECDH X25519 (Tink Android) + SAS 4-cyfrowy kod (7-step protocol z spec `docs/superpowers/specs/2026-04-20-mobile-qr-pairing-design.md`) |
| Q.4 | Android Keystore AES-256 wrapping VK |
| Nowe endpointy | `/api/mobile/pair-{start,init,confirm,cancel}` |
| Schema | `devices`: `platform`, `paired_at`, `pairing_status`, `vault_key_generation` |
| Szacunek | 5–7 dni |

### ⬜ Faza R — Mobile V1 Read-Only (P3)

| Krok | Zakres |
|------|--------|
| R.1 | BiometricPrompt unlock → VK w pamięci (zeroize po 5 min inactivity) |
| R.2 | `GET /api/snapshot/latest` → `vault_snapshot.db` lokalnie |
| R.3 | Compose file browser (katalogi, metadane, search) |
| R.4 | Streaming decrypt: chunki LAN → `decrypt_chunk` UniFFI → Android Intent |
| R.5 | Offline pinning: long-press → cache w internal storage (zaszyfrowany) |
| Szacunek | 7–10 dni |

### ⬜ Faza S — Mobile V2 Read-Write (P4)

| Krok | Zakres |
|------|--------|
| S.1 | Inbox upload: ShareSheet → `POST /api/mobile/inbox/upload` → daemon szyfruje V2 → B2/R2 |
| S.2 | iOS File Provider Extension + Android SAF |
| S.3 | Conflict resolution: Opcja c = version branching `file (Conflict from iPhone).pdf` |
| S.4 | Camera upload WorkManager + `READ_MEDIA_IMAGES` (opcjonalne) |
| Szacunek | 5–7 dni |

---

## 13. Decyzje architektoniczne

| ID | Decyzja | Uzasadnienie |
|----|---------|--------------|
| **D1** | `skarbiec.app` = static content ONLY (CF Pages) | Cloudflare Tunnel → daemon = attack surface (session hijack, RCE). Static HTML = brak tajemnic do hackowania. |
| **D1a** | Cloudflare Pages dla decryptora (nie GH Pages) | Zero kosztu. Domena już w CF → jeden klik. Edge CDN. Fallback: `omnidrive.github.io/share`. |
| **D2** | Hybrid E2E: mockito CI + manual smoke na real B2 | Real B2 smoke = prawdziwy test; mockito = szybkie CI bez kosztów egress. |
| **D3** | Desktop polish first → mobile dopiero po v0.4.0 | Cryptomator latami desktop-only. Mobile zbyt wcześnie = rozproszenie. |
| **D4** | Mobile V1 = read-only SQLite snapshot (Opcja C) | Daemon writes / mobile reads = zero konfliktu. Write (CRDT) = osobna decyzja po V1. |
| **D5** | UniFFI (rekomendacja) — decyzja przed Fazą P | Native quality dla security app. Flutter = ciężki runtime Dart. |
| **D6** | Landing page `skarbiec.app` (post v0.3.0) | What is it / Screenshots / Download / GitHub. Proste. |
| **M.6.1** | CORS = loopback + RFC1918 only, zero public domains | XSS na GH Pages → fetch do daemona z ukradzioną sesją. Attack surface eliminowany. |
| **B.6** | `validate_user_session` bez constant-time | 256-bit random token + LAN-only + SQLite overhead = timing attack niewykonalny. Udokumentowane §11 `crypto-spec.md`. |
| **C.1 Wariant B** | VK Sealing dla refresh tokena | Refresh wymaga unlocked vault = zero dostępu bez hasła. Zgodne z Zero-Knowledge. |
| **C.3 POST-DELL** | rustls/hyper consolidation odłożone | Major AWS SDK bump = wysokie ryzyko regresji przed smoke testem. |

---

## 14. Risk register

| Risk | Level | Status / Mitigation |
|------|-------|---------------------|
| Low-order X25519 pubkey attack (A.3) | HIGH | ✅ DONE — `validate_x25519_pubkey()` odrzuca `[0;32]` + 8 low-order points |
| Watcher szyfruje pliki na świeżym Dellu (A.1) | HIGH | ✅ DONE — DRY_RUN gate + pre-onboarding passive |
| Staging file zostaje po crashu graftu (A.2) | HIGH | ✅ DONE — zero-overwrite + retry 5×500ms |
| CORS prefix-match pozwala `localhost.evil.com` (B.1) | HIGH | ✅ DONE — exact-match `IpAddr::parse()` |
| Brute-force recovery mnemonika (B.2) | HIGH | ✅ DONE — rate-limit + state-guard + audit |
| OAuth token w historii URL (B.3) | HIGH | ✅ DONE (Krok 1) — `replaceState` + `Referrer-Policy` |
| `thread_rng` w krypto (B.4) | HIGH | ✅ DONE — `OsRng` wszędzie |
| Refresh token Google plaintext w DB (C.1) | MEDIUM | ✅ DONE — VK Sealing (AES-GCM HKDF) |
| Passphrase w pamięci (heap String) (C.2) | MEDIUM | ✅ DONE — `secrecy::SecretString` + zeroize on drop |
| Staging pool handle nie zwolniony przed delete (A.4) | HIGH | ✅ DONE — `drop(pool) + yield_now()` |
| Poisoned mutex w cfapi crash (A.8) | LOW | ✅ DONE — `unwrap_or_else(|e| e.into_inner())` |
| `vault_id ↔ device_id` mismatch po graftcie (A.9) | LOW | ✅ DONE — startup assertion |
| rustls/hyper duplicate versions (C.3) | MEDIUM | ⬜ POST-DELL — zbyt duże ryzyko przed smoke |
| Tray IPC complexity (B.2 Krok 2) | HIGH | ⬜ Task 35.3 — odłożone |
| OAuth code-exchange (B.3 Krok 2) | MEDIUM | ⬜ POST-DELL |
| Dell graft fail (Defender + cfapi races) | MEDIUM | Mitygacja: A.0+A.2+A.4 DONE; Dell smoke test = weryfikacja |
| Windows Defender blokuje hydrated files | MEDIUM | Early MotW testing + placeholder signature (post v0.3.0) |
| Mobile conflict resolution | HIGH | Świadoma decyzja: version branching (Faza S) |
| Multi-device formal lease/fencing brak | LOW | Produkt-first nad spec v1. Szczegóły: `docs/archive/spec_review.md` |

---

## 15. Workflow przypomnienie (z CLAUDE.md)

1. **Kompilacja:** `cargo check` → przed instalatorem `cargo build --release --workspace`.
2. **Payload:** **MUSISZ** `cp target/release/*.exe dist/installer/payload/` przed Inno Setup.
3. **Wersja:** podbij we wszystkich 6 `Cargo.toml` (angeld, omnidrive-core, angelctl, omnidrive-tray, omnidrive-shell-ext, omnidrive-cli) + `installer/omnidrive.iss`.
4. **Zero-Knowledge:** zero plaintext haseł / kluczy / tokenów w logach (`[REDACTED]`).
5. **Święta Zasada:** żadnych operacji zapisu/szyfrowania poza `SYNC_PATH`. Watcher = DRY_RUN podczas pracy nad UI/API.
6. **Token budget:** po każdym mikro-kroku pytaj „kontynuujemy czy commit+push?".

---

*Stare pliki planowania zarchiwizowane w `docs/archive/`. Ten plik = jedno źródło prawdy o całym projekcie.*
