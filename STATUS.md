# OmniDrive — Kronika projektu & Roadmapa (Single Source of Truth)

> **Ostatnia aktualizacja:** 2026-05-25 (**Faza α.A.c ZAMKNIĘTA** — P2-005 Zeroize newtype dla `KeyBytes`: type alias → `pub struct KeyBytes([u8; KEY_LEN])` z `#[derive(Zeroize, ZeroizeOnDrop)]` + Deref/AsRef/From/redacted Debug. 4 commity (HEAD `4b6415a`) + spec `eb1be7b`/plan `084f875`, security-review CLEAN, 14/14 core. **SMOKE H4 PASS** live na Lenovo — memdump po `IdleTimeout` lock: `before.dmp`=1 trafienie known-key (kontrola) / `after.dmp`=0 (klucz wyzerowany z RAM). Workspace bump v0.3.26. **Następna sesja → α.B.a** Argon2id 2026 params bump.)
> **Aktualna wersja:** `v0.3.26` w kodzie (workspace bump, close α.A.c). Ostatni gotowy instalator: `OmniDrive-Setup-0.3.23.exe` — `v0.3.25.exe` pending (build w osobnym etapie). Lokalny daemon Lenovo działa z `target/release` workspace mode.
> **Status:** Faza 0 ZAMKNIĘTA + **Faza α.A.a (P1-006 logout-locks-vault) DONE** — fix `api/auth.rs::post_auth_logout` woła `state.vault_keys.lock().await` przed `delete_user_session`. SMOKE H1 gate: logout=200, session=invalid, safety-numbers=401, O: not mounted — 4/4 zielone. Pre-push hook aktywny, CI z fmt-check gate. **α.A.b.3 ZAMKNIĘTE 4/4** — 3.1 scaffold (`204b287`) + 3.2 message pump (`241030b`: kanoniczne WNDCLASSW + feature `Win32_Graphics_Gdi`, `catch_unwind` trampoline, `OBSERVER_CTX` OnceLock, `WTS_SESSION_LOCK`→`force_lock_and_dismount(WinSessionLock)`, UNLOCK ignored zero-trust) + 3.3 test-helpers bridge (`72e0ed8`: `OBSERVER_HANDLE` + `test_routes`/`post_test_simulate` + e2e mpsc) + 3.4 spawn observer w `ApiServer::run` z graceful degradation (`ead8039`: cfg-windows blok, `OBSERVER_HANDLE.set`, info/warn timer-only fallback) DONE+pushed. 4 ścieżki locka (logout/idle/Win+L/manual) zbiegają się na `force_lock_and_dismount`. **α.A.b.4 frontend DONE** (`2031e85`: chip countdown + settings card inline w index.html, review-first 2 subagenci + 4 a11y fixy). **SMOKE H2 (idle) + H3 (Win+L) PASS** live na Lenovo (log-confirmed `reason=IdleTimeout` / `reason=WinSessionLock`). **Bug wykryty przez SMOKE i naprawiony** (`8e0d116`): `require_session`/`require_role` touchowały timer na każdym auth-callu → dashboard `fetchAuditLog` (30s) resetował idle → auto-lock nigdy nie odpalał; fix = idle activity tylko z realnego inputu (POST /touch) + plików (CfApi). **Faza α.A.b ZAMKNIĘTA. α.A.c ZAMKNIĘTA** — Zeroize newtype KeyBytes (P2-005), SMOKE H4 PASS (memdump po lock: `after.dmp`=0 trafień), workspace v0.3.26. **Następna sesja → α.B.a** (Argon2id 2026 params bump). Build instalatora pending.
>
> **Schemat ID kroków (od 2026-05-17 wieczór):** etap = grecka litera (`α`, `β`, `γ`...) · grupa = duża łacińska (`A`, `B`, `C`...) tylko gdy etap ma podgrupy · zadanie = mała łacińska (`a`, `b`, `c`...) · sub-krok = cyfra (`1`, `2`, `3`...) tylko gdy zadanie ma kilka konkretnych implementacji. Przykłady: `α.A.b.2` (etap α, grupa A hot-fixy, zadanie b auto-lock, sub-krok 2 timer reset), `β.a` (faza β jednorodna, pierwsze zadanie), `0.c.1` (Faza 0 zadanie c perf, sub-krok 1 harness). Historyczne sekcje (Epic 19-36, Faza N, H-M.6) zostają w starym schemacie jako archeologia.
> **Zasada:** ten plik to jedyne źródło prawdy o roadmapie. Bugi w `docs/KNOWN_ISSUES.md`. Stare pliki planowania w `docs/archive/`.

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
12. [Roadmap v0.4 → v5.0 → v6.0](#12-roadmap--droga-do-v04--v50--v60)
    - 12.0 Wizja docelowa · 12.1 Threat Model · 12.2 SLA Performance · 12.3 Quality Gate
    - 12.4 Faza 0 (QA Foundation) · 12.5 Faza α (Crypto) · 12.6 Faza β (Bug Fixes)
    - 12.7 Faza γ (Zero Data Loss) · 12.8 Faza δ (Multi-User Closure) · 12.9 Faza ε (VFS Stability)
    - 12.10 Faza ζ (Test Automation) · 12.11 v0.4.0 Gate · 12.12 v5.0 · 12.13 v6.0
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

### ✅ v0.3.7 — Wizard single-column redesign + tray icons fix

| Co zrobiono | Wynik |
|-------------|-------|
| Wizard onboarding przemodelowany na single-column layout | Czytelniejszy flow, mniej nawigacji bocznej |
| Tray icons fix — poprawione warianty BASE/SYNCING/SYNCED/ERROR/LOCKED | Status w zasobniku zgodny ze stanem daemona |
| `OmniDrive-Setup-0.3.7.exe` zbudowany | Gotowy do Dell smoke testu (zaakceptowany jako poprzednia bazówka v0.3.x) |

### ✅ v0.3.8–v0.3.17 — Sesja stabilizacji onboarding+vault (2026-05-10)

> **Geneza:** seria 11 wersji wydana w jednej sesji 2026-05-10 — fixy sequencyjne wykryte podczas Lenovo+Dell testów, każdy bez większego rozgrzebywania architektury.

| Wersja | Commit | Co zrobiono |
|--------|--------|-------------|
| v0.3.14 | `ce9ff10` | **Post-join membership fix:** po `join-existing` graft tworzymy `user+device+vault_member(owner)` dla lokalnego urządzenia → `create_session_for_local_device` przestaje failować, vault unlock zwraca `session_token`, `lock` przestaje wracać 403. Wizard kończy się przez `location.replace('/')` zamiast `loadDashboard()`. |
| v0.3.15 | `ce9ff10` | **Split-brain change-password fix:** `post_rotate_key` i `post_change_password` natychmiast wywołują `spawn_post_rotation_backup()` → upload `latest.db.enc` na wszystkich providerów bez czekania na 1h tick metadata-backup workera. |
| v0.3.16 | `8c33d19` | **`IncorrectPassphrase` fallback fix:** błąd od jednego providera (np. tylko Scaleway ma stary klucz) nie przerywa fallbacka — daemon próbuje dalszych providerów, finalny `IncorrectPassphrase` zwraca tylko gdy WSZYSCY odrzucili. Klucz dla Dell join-existing kiedy Scaleway krzywo. |
| v0.3.17 | `c08e164` | **Provider state guard + read-only test endpoint:** `post_setup_provider` nie cofa już `COMPLETED → IN_PROGRESS` (regresja). Dodany `POST /api/providers/{name}/test` — sprawdza stored credentials bez aktualizacji onboarding state. |

### ✅ v0.3.19–v0.3.23 — Sesja Dell Smoke Test (2026-05-10 wieczór)

| Wersja | Co zrobiono |
|--------|-------------|
| **v0.3.19** | „Silent & Smart" — adaptive Google OAuth button (ukryty dla solo vault z `members_count==1`). |
| **v0.3.20** | Diagnostyka tab — wszystkie operacje serwisowe jako klikalne przyciski. |
| **v0.3.21** | Fix #1 (HTTP 403 po join-existing): brak session_token w `JoinExistingResponse` → token handoff przez sessionStorage; idempotentny multi-user setup; `'diagnostyka'` w `VALID_VIEWS`. |
| **v0.3.22** | Fix #2: `post_join_existing` używa `device.user_id` z istniejących `devices` (po `migrate_single_to_multi_user`) zamiast wymyślać `"user-{device_id}"` — token mintowany z prawidłowym user_id. **Częściowy** — odsłonił że Dell+Lenovo to dwóch różnych userów w jednym vault. |
| **v0.3.23** | **Identity Grafting (Single-User-Multi-Device).** `graft_restored_metadata_snapshot` kopiuje teraz `users`/`devices`/`vault_members` ze snapshot. `post_join_existing` wywołuje `db::ensure_local_device_in_vault` — Dell adoptuje user_id ze snapshot Lenovo. Safety numbers identyczne na obu urządzeniach. MultiDevice tab Della widzi Lenovo + Della. Plus brakujący endpoint `GET /api/diagnostics` (cloud_guard quotas) → fix „Limity dzienne ERROR". |

**Kluczowa lekcja sesji 2026-05-10:** seria reaktywnych fixów (v0.3.21 → v0.3.22 → v0.3.23) była objawem braku zaplanowanej akcji. Identity rozjazd Dell↔Lenovo był decyzją architektoniczną którą można było zauważyć od pierwszego symptomu, gdyby fix nie był reaktywny. Skutek: formalny roadmap v0.4 (§12) z jasnymi kryteriami sukcesu.

---

### ✅ v0.3.18 — Bleeding B2 + retry storm fixes (NEW — 2026-05-10)

> **Geneza:** Backblaze B2 zaalarmował 2026-05-10 wieczorem o 75% daily download cap mimo „tylko logowania". Diagnoza wykazała: orphaned pack `5962635a87...` z `attempts: 3158` na Scaleway od kwietnia + scrubber co 5 min robi GET deep verify na małym vaulcie + cloud_guard `daily_egress_bytes` raportuje 0 (BUG — accounting nie liczy egressu workerów). Daemon zatrzymany na noc; v0.3.18 = naprawienie wszystkich 4 wektorów.

| Krok | Commit | Co zrobiono |
|------|--------|-------------|
| **Fix #1** Cloud guard egress accounting | `6ee434c` | `cloud_guard::try_authorize_read()` + `reconcile_read_bytes()`. Hooki w `scrubber` (HEAD + GET deep verify), `repair::download_shard` (+`estimated_size` arg w 3 callsitach), `disaster_recovery::download_bytes`/`list_snapshot_keys` (+`Option<&SqlitePool>`), `downloader` (+post-GET reconcile). Wszystkie GET-y storage zliczają realny `content_length()` do `daily_egress_bytes`. |
| **Fix #2** Backoff plateau + PERMANENTLY_FAILED | `da5a113` | `UPLOAD_RETRY_PLATEAU_AT=100` → `retry_delay()` zwraca 1h plateau zamiast 60s. `UPLOAD_PERMANENT_FAILURE_AT=1000` → target dostaje status `PERMANENTLY_FAILED`, jest wykluczony z `get_incomplete_pack_shards`. Pack z PERMANENTLY_FAILED targetami eventualnie dostaje `mark_upload_job_failed` → retry storm zamyka się naturalnie. Helper `escalate_target_if_permanent` w 3 retry callsitach uploaderze. |
| **Fix #3** Dashboard retry-storm alert | `aa4aaa7` | `db::list_retry_storm_targets(threshold)` + `RetryStormTargetRecord` (join `upload_jobs`). `GET /api/maintenance/retry-storms` zwraca thresholds + max_attempts + targets. UI: nowy `retryStormAlertSection` (hidden by default) w sekcji Przegląd; `fetchRetryStorms` polluje co 60s, pokazuje worst pack z liczbą prób + lista 6 targetów. |
| **Fix #4** GC orphan packs endpoint | `b158514` | `db::gc_orphan_packs()` znajduje packs gdzie żaden `pack_locations.chunk_id` nie ma referencji w `chunk_refs`, w jednym TX kasuje: `upload_job_targets` → `upload_jobs` → `pack_locations` → `packs` (cascade `pack_shards`). `POST /api/maintenance/gc-orphans` (Role::Admin) zwraca `GcOrphanReport` (counts per tabela + lista pack_id). |
| **Fix #5** Adaptive scrubber poll/modulus | `91fa8f5` | `db::count_all_packs(pool)`. Dla `pack_count < 100`: `effective_poll_interval` ≥ 1h (zamiast 5 min default), `effective_deep_verify_modulus` ≥ 100 (zamiast 20). 5× mniej deep GET-ów na małym vault → eliminuje 215 MB B2 egress/dzień. |
| **Release** v0.3.18 bump + build | `d5f71e3` | Bump 0.3.17 → 0.3.18 we wszystkich 6 `Cargo.toml` + `installer/omnidrive.iss`. `cargo build --release --workspace` (1m 09s). Binarki skopiowane do `dist/installer/payload/`. `OmniDrive-Setup-0.3.18.exe` (24 MB) wygenerowany przez Inno Setup. |

**Testy:** 200+ unit testów PASS (90 + 102 + 11 + e2e_sync). 1 e2e_recovery FAIL (`disaster_recovery_rebuilds_local_db_inventory_after_total_db_loss`) — pre-existing, fail też na v0.3.17 baseline; wymaga `--features test-helpers` (security gate na `OMNIDRIVE_AUTO_RESTORE_PASSPHRASE` w release builds). Patrz `feedback_e2e_recovery_test.md` w memory.

---

## 12. Roadmap — droga do v0.4 → v5.0 → v6.0

> **Decyzje przyjęte 2026-05-10 wieczorem (Przemek + Claude).** Koniec gaszenia pożarów. Każdy etap ma jasne **Definition of Done** (DoD). Sekcja zastąpiła stary „Co przed nami" (dotyczył v0.3.18 — już osiągnięte i wyprzedzone).

#### 🧭 Drzewko orientacyjne — cała Roadmapa (you are here)

```
v0.4 → v5.0 → v6.0      (◄── = bieżący krok)
│
✅ Faza 0 — QA Foundation — ZAMKNIĘTA (6/6, perf M1–M4 PASS, marginesy 38%–500×)
│
🔄 α — Crypto Hardening — W TRAKCIE
│   ├── α.A.a  logout-locks-vault (P1-006) ......... ✅ DONE (SMOKE H1 4/4)
│   ├── α.A.b  auto-lock idle + Win+L hook ......... ✅ DONE (b1–b4 + SMOKE H2/H3 PASS, v0.3.25)
│   ├── α.A.c  Zeroize newtype KeyBytes (P2-005) ... ✅ DONE (SMOKE H4 PASS, v0.3.26)
│   ├── α.B    KDF & wrap (Argon2id + ML-KEM-768) .. 🔄 α.B.a ✅ DONE (v0.3.27), α.B.b ⏸️ (po α.C)
│   ├── α.C    Identity & device keys (P1-001/005) . 🔜 NEXT  ◄── JESTEŚMY TU
│   └── α.D    Spec + formal crypto review (QG5) ... ⏸️
│
⏸️ β — Critical Bug Fixes  (β.d Watcher CPU już ✅ PASS)
⏸️ γ — Zero Data Loss Hardening
⏸️ δ — Multi-User Infra Closure (pod maską, bez UI)
⏸️ ε — VFS Stability (pancerne O:)
⏸️ ζ — Test Automation (F1–F12 e2e)
│
🏁 v0.4.0 Release Gate (QG1–QG6) → tag + instalator + CHANGELOG
   v5.0 — Family Cloud (aktywacja UI multi-user, nadbudówka na δ)
   v6.0 — Mobile Ecosystem (Android-first, UniFFI, QR pairing)
```

(Szczegóły per faza: drzewka i tabele DoD w §12.4–12.10 niżej.)

### 12.0 Wizja docelowa (3 milestones)

| Wersja | Nazwa robocza | Zakres |
|--------|--------------|--------|
| **v0.4** | **Stabilny Fundament (Single-User, Multi-Device)** | Single-user UI, multi-device sync (Lenovo↔Dell), zero data loss, pancerne VFS, hybrid quantum-resistant crypto. **Multi-user infra (Family Cloud) gotowa pod maską w bazie/API** — ale UI pozostaje single-user. |
| **v5.0** | **Family Cloud (Aktywacja Multi-User UI)** | UI dla invite żony/dzieci, role/ACL flow, recovery dla nietechnicznych userów, dead man switch, RCE defense in depth. Nadbudówka na infrę v0.4 — żadnego przepisywania krypto/schema. |
| **v6.0** | **Mobile Ecosystem** | Android-first (UniFFI), QR pairing, SQLite snapshot read, Inbox upload, opcjonalnie iOS. WebCrypto compatibility (Epic 33 mobile). |

### 12.1 Threat Model dla v0.4 (zatwierdzony 2026-05-10)

**MUST dla v0.4:**
- (a) **Compromised provider** — full Zero-Knowledge: provider widzi tylko szyfrogram, nigdy plaintext / klucze / nazwy plików / strukturę. EC_2_1 sprawia że jeden provider = niewystarczający.
- (b) **Compromised local OS** — DPAPI / Windows Hello / TPM dla persistowanych sekretów. Pamięć user-mode procesu = **świadoma akceptacja ryzyka** (malware z user-level privilege może odczytać unwrapped Vault Key z RAM podczas unlock; mitigacja przez auto-lock po inactivity timeout).
- (d) **Recovery** — pełny działający BIP-39 mnemonik; 2-of-2 (passphrase + device) jako baseline, recovery key jako fallback.
- (e) **Brute force** — Argon2id 2026 standard params (proponuję m=47MiB, t=1, p=1 — OWASP 2025+; do potwierdzenia benchmarkiem na docelowym sprzęcie ~150ms).
- (f) **Quantum-Resistance** — **decyzja Przemka**: hybrid X25519 + ML-KEM-768 dla key encapsulation (Vault Key wrap dla devices). Symetryczne chunki AES-GCM-256 zostają — są post-quantum-safe (128-bit security level vs Grover). Schema gotowa od dnia 1, żadnej bolesnej migracji w przyszłości.

**v5.0+ (świadomie odłożone):**
- (c) Compromised endpoint (RCE w angeld) — defense in depth
- Dead Man Switch (idle X miesięcy → trigger recovery transferu)

### 12.2 SLA Performance dla v0.4 (zatwierdzone 2026-05-10)

| Komponent | SLA |
|-----------|-----|
| Watcher CPU | < 1% w spoczynku, < 5% przy 100 zmianach/min |
| VFS cold fetch (placeholder hydration) | < 2s dla pliku < 10 MB; < 10s dla pliku < 100 MB; throughput min 50 MB/s |
| VFS warm cache open | < 100ms |
| Daemon RAM idle | < 200 MB |
| Daemon cold start (boot → API ready) | < 5s |

### 12.3 Quality Gate v0.4 (zatwierdzony 2026-05-10)

| # | Kryterium | Pomiar |
|---|-----------|--------|
| QG1 | Smoke test ręczny pełen cykl (wizard + join-existing + lock/unlock + upload/download + power-cycle) na **Lenovo + Dell** bez błędów | Checklist `docs/SMOKE_CHECKLIST.md` (do utworzenia w Fazie 0) |
| QG2 | Stress test: 1000 plików małych (<1MB) + 1 plik >1GB + 24h soak watchera. Zero crashów, zero zgubionych plików, zero dataloss. | Skrypt `scripts/stress-test.ps1` (do utworzenia w Fazie ζ) |
| QG3 | `cargo test --workspace --all-features` — 100% pass. **Pokrycie kluczowych flow** (lista §12.10) — każdy ma e2e test. | CI gate `cargo test` — green required przed tagiem |
| QG4 | `docs/KNOWN_ISSUES.md` zero P1/P2 | Bug list audit (Przemek zatwierdza, Claude weryfikuje) |
| QG5 | Formalny crypto review (Claude) — patrz Faza α DoD | Dokument `docs/superpowers/specs/2026-XX-XX-crypto-review.md` |
| QG6 | Wszystkie SLA performance §12.2 spełnione | Benchmark suite `cargo bench` lub osobny PowerShell harness |

**Brak audytu zewnętrznego krypto dla v0.4** — to v5.0 gate (gdy w grę wchodzą cudze pliki). v0.4 polega na formalnym Claude review (QG5).

---

### 12.4 Faza 0 — QA Foundation *(2026-05-17 — **6/6 DONE, FAZA ZAMKNIĘTA**)*

> **Cel:** zanim cokolwiek nowego kodujemy, mamy infrastrukturę żeby _mierzyć_ jakość.

| Krok | Zakres | Status |
|------|--------|--------|
| **0.a** | Audyt kodu — pełen przegląd `angeld/src/` i `omnidrive-core/src/` pod kątem: TODOs, `unimplemented!()`, `unwrap()` na hot paths, dead code (`cargo +nightly udeps`). Każde znalezisko → wpis P3 (lub wyżej) w `KNOWN_ISSUES.md`. | ✅ DONE — raw metrics `9b874ed`, triage `cf6ae9b`; raport `docs/superpowers/specs/2026-05-11-code-audit.md` §1-4 wypełniony; **6 wpisów dodanych do KNOWN_ISSUES (P1-006, P2-003/004/005, P3-001/002).** |
| **0.b** | `docs/SMOKE_CHECKLIST.md` — manualna lista 30–50 sprawdzeń do przejścia po każdym buildzie (przed Dell smoke). | ✅ DONE — `cd7a4f2`; **50 punktów w 8 sekcjach** (A build/instalacja, B nowy vault, C join-existing z safety-numbers Dell↔Lenovo, D upload/download/sync, E UI, F recovery/maintenance, G stabilność, H zero-knowledge security). Każdy 🚨 EXPECTED-FAIL ma ref do KNOWN_ISSUES + roadmap target. |
| **0.c** | Performance baseline benchmark (watcher, VFS cold/warm fetch, RAM, cold start) — _aktualne_ wartości na Lenovo (dev box). Bez tego nie wiemy jak daleko jesteśmy od SLA z §12.2. | ✅ DONE — `d2fa947`; **M1-M4 PASS 4/4** (Faza A+B; Faza C wstrzymana per decyzja). M1 cold start **1863 ms** (<3000 ms, 38% margin), M2 RAM idle **34.2 MB** (<150 MB, 4.4× margin), M3 watcher CPU idle **0%** (<1%), M4 watcher CPU load **avg 0.01% / max 0.14%** (<5%, ~500× margin). Raport: `docs/perf-baseline-2026-05-17.md`. Faza C (M5/M6 VFS fetch) wymaga vault unlock + mount T: → osobna sesja po β.d/β.e. |
| **0.d** | CI: GitHub Actions — `cargo test --workspace`, `cargo clippy -- -D warnings`, `cargo fmt --check`. Każdy push → pipeline. Plus lokalny pre-push hook (fmt+clippy). | ✅ DONE — fix CI red `06febb1` (clippy 1.94 lints), fmt baseline `0cbee99` (63 plików), pipeline hooks + CI fmt step + Cargo.lock `a95a338`. **Pre-push hook samo-przetestowany przy własnym pushu — działa.** |
| **0.e** | Push lokalnych commitów (v0.3.19–v0.3.23) na `origin`. | ✅ DONE (już 2026-05-11 sesja "Clean Ark") |

**Wykonanie 2026-05-17 (9 commitów na main):**

| Commit | Krok | Treść |
|---|---|---|
| `9b874ed` | 0.a.1 | raw metrics baseline (clippy/fmt/udeps/grep → audit report §1) |
| `06febb1` | 0.d.1 | fix CI-red (clippy 1.94: collapsible_if + doc_lazy_continuation + misc, ~20 lintów lib+bin+testy) |
| `11b3f3f` | 0.a.2 | cleanup dead vault test helpers (`set_key_for_tests` + `UnlockedVaultKeys::new`) + P2-003 (bin/lib duplikacja 27 modułów) |
| `cf6ae9b` | 0.a.3 | Task 2 audit triage — §2-4 raportu + **3 security gaps** (P1-006/P2-004/P2-005) + AAD audit (P3-001) + unwrap triage (P3-002) |
| `cd7a4f2` | 0.b | SMOKE_CHECKLIST.md (50 punktów ready-to-tick) |
| `0cbee99` | 0.d.2 | cargo fmt --all baseline (63 plików, mechaniczny commit) |
| `a95a338` | 0.d.3 | .githooks/pre-push (bash: fmt+clippy gate) + scripts/install-git-hooks.ps1 + CI +rustfmt component +fmt --check step + Cargo.lock committed (deterministic builds) |
| `d4497d4` | 0.c.1 | perf-baseline.ps1 — isolated test daemon harness (Phase A+B M1-M4, port 8788, --no-sync, LOCALAPPDATA override) |
| `d2fa947` | 0.c.2 | perf baseline run executed: M1-M4 **PASS 4/4** + raport `docs/perf-baseline-2026-05-17.md` + script fixes (-Yes flag, TcpClient probe) |

**Bonus odkrycia (poza pierwotnym planem) — rebalansują kolejność Fazy α:**
- **P1-006:** `/api/auth/logout` (api/auth.rs:189) nie wywołuje `vault_keys.lock()`. Klucze plaintext zostają w RAM po wylogowaniu. **Zero-knowledge gap.** Hot-fix-able do v0.3.24.
- **P2-004:** Brak auto-lock po idle. 0 grep matches dla `auto_lock|idle_timeout|inactivity` w *.rs.
- **P2-005:** Brak `Zeroize` impl. `KeyBytes = [u8; 32]` (omnidrive-core/src/crypto.rs:28) bez derive. `expose_secret()` zwraca un-zeroized kopię.
- **P3-001:** AAD `&[]` na chunk encrypt — świadoma decyzja (WebCrypto compat Trybu B), brak udokumentowania w crypto-spec → §12 do dopisania.
- **P3-002:** 23 prod unwrap (nie 24 z Task 1) → 2 eskalowane do P2: `peer.rs:159` (reqwest builder) + `ingest.rs:184` (packer init).

**Szacunek pierwotny:** 2–3 sesje. **Faktyczne wykonanie:** 1 sesja (6/6 kroków) — **Faza 0 zamknięta**. Wszystkie SLA performance §12.2 (M1-M4) z marginesami 38%–500×. Bramka β.d (watcher CPU fix) — bez akcji, wynik już PASS.

---

### 12.5 Faza α — Crypto Hardening *(po Fazie 0; struktura uaktualniona 2026-05-17 wieczór na schemat α.A.b.2)*

> **Cel:** zamknąć wszystkie bramki krypto z §12.1 (a–f) zanim zaczniemy bug-fixy. Krypto = fundament; każdy fix do crypto po reszcie = ryzyko dataloss.
>
> **Grupa A (hot-fixy)** wstawiona przed B/C/D po audycie Fazy 0 — adresuje 3 security gapy (P1-006/P2-004/P2-005). Bez tego nawet perfekcyjne grupy B-D zostawiają klucze plaintext w RAM po logout.

#### Drzewko orientacyjne

```
α — Crypto Hardening
├── A. Security hot-fixes (pre-cryptodrop) ────── domyka P1/P2 z audytu Fazy 0
│   ├── α.A.a — P1-006 logout-locks-vault       ✅ DONE (ed35ecb + dc4979f, SMOKE H1 4/4)
│   ├── α.A.b — P2-004 auto-lock idle + Win+L hook ✅ DONE (SMOKE H2/H3 PASS)
│   │   ├── α.A.b.1   config `vault.auto_lock_idle_minutes` (default 15)     ✅ DONE (5dc498d)
│   │   ├── α.A.b.2   activity tracking + tick loop + lock_flow refactor      ✅ DONE 8/8 (ef5d529)
│   │   ├── α.A.b.3   hook `WM_WTSSESSION_CHANGE` / `SessionSwitch`           ✅ DONE 4/4 (ead8039)
│   │   └── α.A.b.4   UI chip countdown + settings card (+ ACL idle fix)      ✅ DONE (2031e85 + 8e0d116)
│   └── α.A.c — P2-005 Zeroize newtype dla KeyBytes  ✅ DONE (4b6415a, SMOKE H4 PASS, v0.3.26)
│
├── B. KDF & wrap upgrades ─────────────────────── re-derive existing vault data
│   ├── α.B.a — Argon2id 2026 params bump (m=256MiB, t=3, p=1)  ✅ DONE (LIVE SMOKE PASS, v0.3.27)
│   └── α.B.b — ML-KEM-768 hybrid wrap (NIST FIPS 203)  ⏸️ (zaplanowane po α.C.a)
│       ├── α.B.b.1   schema: `devices.kyber_public_key` + `wrapped_vault_key_kyber`
│       ├── α.B.b.2   unwrap: X25519 default → ML-KEM failover
│       └── α.B.b.3   e2e: hybrid wrap → 2× decrypt → ten sam VK
│
├── C. Identity & device keys ──────────────────── domyka P1-001+P1-005 (Dell ≠ Lenovo)
│   ├── α.C.a — Real X25519 keypair (zamiast `[0;32]` placeholder)  ◄── NEXT
│   └── α.C.b — Graft pełen identity bundle (db.rs:1677)
│       ├── α.C.b.1   wszystkie pola `vault_state` poza per-instance KDF
│       ├── α.C.b.2   tabela `data_encryption_keys`
│       └── α.C.b.3   tabela `recovery_keys` (jeśli istnieje)
│
└── D. Spec & formal review (QG5) ──────────────── zamknięcie fazy
    └── α.D.a — crypto-spec.md §12 AAD + §13 auto-lock/zeroize + Claude review
```

#### Tabela DoD (per zadanie)

| Krok | Zakres | DoD |
|------|--------|-----|
| **α.A.a** ✅ | **P1-006 fix: logout musi zablokować vault.** `api/auth.rs::post_auth_logout` (linia 189) dodać `state.vault_keys.lock().await` PRZED `delete_user_session`. Wzorzec do skopiowania: `api/vault.rs::post_vault_lock` (linia 915-928 — woła `state.vault_keys.lock().await` + dismount cfapi). Hot-fix-able do v0.3.24 (mały scope, security high impact, low regression risk). | **DONE 2026-05-17** — code `ed35ecb`, workspace bump v0.3.23→v0.3.24 (`dc4979f`). SMOKE H1 functional gate na Lenovo 4/4 PASS: logout HTTP 200 + `{"status":"logged_out"}`, session `valid:false` + `invalid_or_expired_session`, safety-numbers HTTP 401, O: not mounted. Memdump diff (ProcDump) odłożony do α.A.c po Zeroize newtype — bez tego diff i tak nie byłby pełen. |
| **α.A.b** ✅ | **P2-004 fix: auto-lock po idle + Windows session-lock.** α.A.b.1 ✅ (config `vault.auto_lock_idle_minutes`, default 15, `5dc498d`). α.A.b.2 ✅ 8/8 (activity tracking + ACL hooks + cfapi hooks + lock_flow refactor + tick loop + GET /status no-touch + POST /touch, `ef5d529`). α.A.b.3 ✅ 4/4 (Win+L observer, `ead8039`). α.A.b.4 ✅ (UI chip countdown + idle-timeout settings card inline w index.html, review-first + 4 a11y fixy, `2031e85`). | **DONE 2026-05-20** — SMOKE H2 (idle, `OMNIDRIVE_AUTO_LOCK_TEST_MIN=1`) + H3 (Win+L) PASS live na Lenovo, log-confirmed `reason=IdleTimeout` / `reason=WinSessionLock`. **Bug wykryty przez SMOKE i naprawiony** (`8e0d116` `fix(auto-lock): stop ACL auth checks from resetting the idle timer`): `require_session`/`require_role` touchowały timer na każdym auth-callu → dashboard `fetchAuditLog` (30s, require_role Admin) resetował idle → auto-lock nigdy nie odpalał przy otwartym dashboardzie; fix = idle activity tylko z realnego inputu (POST /touch ManualExtend) + plików (CfApi). acl 8/8 + e2e_auto_lock 7/7 green (2 testy odwrócone na strażników nowego kontraktu). Workspace bump v0.3.25. |
| **α.A.c** ✅ | **P2-005 fix: Zeroize newtype dla KeyBytes.** Dodać `zeroize = { workspace = true, features = ["zeroize_derive"] }` jako explicit dep w `omnidrive-core`. `KeyBytes` w `omnidrive-core/src/crypto.rs:28` z type alias → newtype z `#[derive(Zeroize, ZeroizeOnDrop)]`. Audit call-sites `expose_secret()` w vault.rs/downloader.rs/packer.rs/migrator.rs/sharing.rs — zamienić plain copies na `SecretBox` lub krótkożyjące referencje. | **DONE 2026-05-25** — newtype `pub struct KeyBytes([u8; KEY_LEN])` + `#[derive(Zeroize, ZeroizeOnDrop)]` + Deref/AsRef/From/redacted Debug (non-Copy). 4 commity (`d732ea8` deps + `f41815f` newtype+anchory + `531b843` sweep 13 plików + `4b6415a` test-nit) + spec `eb1be7b`/plan `084f875`, security-review CLEAN, 14/14 core. **SMOKE H4 PASS** live na Lenovo: memdump po `IdleTimeout` lock — `before.dmp`=1 trafienie known-key (kontrola OK) / `after.dmp`=0 (klucz wyzerowany z RAM). Workspace bump v0.3.26. |
| **α.B.a** ✅ | **Argon2id 2026 params bump (Desktop High Security: m=256MiB, t=3, p=1).** Atomowa re-key migracja na pierwszym unlocku istniejącego vaulta v1: derive master nowymi params → re-wrap envelope_key (envelope_key bytes UNCHANGED → DEK/dane/safety-numbers nietknięte) → re-seal device key → zachowaj stary deterministyczny vault_key jako `legacy_read_key` (sealed pod envelope, AAD=vault_id). Multi-device → Declined (per-device params w α.C). Spec `27fa7d0` + plan `abf0611` + design `docs/superpowers/specs/2026-05-25-alpha-B-a-argon2id-params-bump-design.md`. | **DONE 2026-06-04** — 9 commitów `86ac79e..33427c4`, holistic review APPROVED (zero utraty danych / all-or-nothing / trigger-safety / security), 124/126 testy zielone, clippy oba tryby + fmt + release build clean. **LIVE SMOKE PASS na Lenovo (workspace mode, ./omnidrive.db):** unlock realnym hasłem → `[VAULT] V2 envelope Vault Key unwrapped (generation 4)` → `[KDF-MIGRATION] params upgraded v1 -> v2` → `[UNLOCK] vault mounted at O:` → IdleTimeout (5min) → `[LOCK_FLOW] locked, reason=IdleTimeout` + recursive dehydration. Post-migracja DB potwierdza: vault_config = (param_set=2, m=262144 KiB=256 MiB, t=3, p=1), vault_state.legacy_read_key = 60 bytes sealed, encrypted_vault_key re-wrapped (40 B), vault_key_generation=4 unchanged. Workspace bump v0.3.27. **Uwaga:** 2× `aes-gcm operation failed` na pliku testowym smoke-5mb.bin (inode=11, rev=1267, 5MB) — pre-existing B2 bleeding corruption ery v0.3.18, NIE regresja α.B.a (DEK vault_key_gen=4 = current envelope, chunks status=COMPLETED_HEALTHY ale auth fail = klasyczna sygnatura corruption; matematycznie migracja nie może wprowadzić nowego aes-gcm fail bo envelope_key bytes IDENTYCZNE przed/po). |
| **α.B.b** | **ML-KEM-768 hybrid wrap.** Crate `ml-kem = "0.2"` (audited, NIST FIPS 203). Sub-kroki: α.B.b.1 (schema `devices.kyber_public_key BLOB` 1184 B + `devices.wrapped_vault_key_kyber BLOB` ~1100 B + AAD), α.B.b.2 (unwrap: próbuje X25519 default, failover na ML-KEM), α.B.b.3 (e2e: hybrid wrap → 2× decrypt → ten sam VK). Dla solo vault: 1 device, 1 user — wrap obu metod dla siebie. | Test e2e: vault z hybrid wrap → unlock → assert oba ciphertexty deszyfrują na ten sam VK |
| **α.C.a** | **Real X25519 keypair generation** (zamiast `[0;32]` placeholder w `migrate_single_to_multi_user` i `post_join_existing`). Klucze trzymane: public w `devices.public_key`, private w `local_device_identity.encrypted_private_key` (sealed Vault Key). | Test: świeży vault → device.public_key ≠ `[0;32]`, `validate_x25519_pubkey` accept |
| **α.C.b** | **P1-001+P1-005 fix: graft pełen identity bundle.** Lokalizacja: `angeld/src/db.rs::graft_restored_metadata_snapshot` (linia 1677). Obecnie kopiuje z `vault_state` TYLKO 3 pola (`master_key_salt`, `argon2_params`, `vault_id`) i POMIJA `encrypted_vault_key`/`vault_key_generation` + całą tabelę `data_encryption_keys` → Dell po join używa swojego gen=1 zamiast Lenovo's gen=N. Sub-kroki: α.C.b.1 (wszystkie pola `vault_state` poza per-instance KDF), α.C.b.2 (tabela `data_encryption_keys`), α.C.b.3 (tabela `recovery_keys` jeśli istnieje); plus audit pozostałych tabel w `docs/crypto-spec.md`. | P1-001+P1-005 zamknięte; SMOKE C3 (safety_numbers + mnemonic + identicon identyczne Dell↔Lenovo) przechodzi; D7 (Dell hydrate → SHA256 match z Lenovo) przechodzi. |
| **α.D.a** | **crypto-spec.md update + Formal Claude crypto review** — dopisać do crypto-spec: §12 AAD semantics (P3-001: dlaczego `&[]` dla chunków = WebCrypto Tryb B compat; dlaczego `user_id` dla OAuth = cross-user tampering protection), §13 auto-lock policy + zeroize semantics. Plus przegląd całego pipeline (passphrase → Argon2id → KEK → AES-KW → VK → AES-KW → DEK → AES-GCM). Output: `docs/superpowers/specs/2026-XX-XX-crypto-review.md`. **QG5.** | Dokument review zaakceptowany przez Przemka |

**Szacunek:** 5–8 sesji. Najcięższa faza — krypto + nowy algorytm + wiele testów.

---

### 12.6 Faza β — Critical Bug Fixes *(po Fazie α)*

> **Cel:** zamknąć wszystkie P1 z `KNOWN_ISSUES.md`. Po Fazie α mamy poprawne krypto i tożsamość — fixujemy resztę.

#### Drzewko orientacyjne

```
β — Critical Bug Fixes (po α)
├── β.a — P1-001 AES-GCM hydration fail (graft DEK z α.C.b)     ⏸️
├── β.b — P1-002 Snapshot fetch worker (refresh co 1h)          ⏸️
├── β.c — P1-003+004 Snapshot redundancy (Scaleway+R2, ≥2/3)    ⏸️
├── β.d — P2-001 Watcher CPU fix                                ✅ PASS (perf baseline 0.c, bez akcji)
└── β.e — P2-002 VFS lag fix + smart_sync.rs decompose          ⏸️ (overlap z ε.a)
```

| Krok | Zakres | DoD |
|------|--------|-----|
| **β.a** | **P1-001 AES-GCM hydration fail** — graft kopiuje DEK (zrobione w α.C.b); test: Lenovo wgra 5MB plik → Dell unlock → otwórz plik z O:\ → checksum match. | P1-001 → FIXED |
| **β.b** | **P1-002 Snapshot fetch worker** — periodic refresh snapshotu na istniejących urządzeniach (co 1h). Lock wokół DB, lamport clock per snapshot, conflict resolve = newer wins (z audit log entry). | Test: Dell join, Lenovo czeka, Lenovo MultiDevice tab pokazuje Della po max 1h |
| **β.c** | **P1-003+P1-004 Snapshot redundancy fix** — Scaleway IAM/policy debug; R2 ConnReset retry-with-fresh-pool. **QG kryterium:** snapshot _zawsze_ w ≥1 sprawnym miejscu, najlepiej w 2/3. | metadata-backup status: ≥2/3 providers zielone |
| **β.d** | **Watcher CPU fix (P2-001)** — po pomiarach z 0.c (perf baseline). Możliwe: debounce + batch + ReadDirectoryChangesW zamiast polling. | SLA `watcher idle < 1%` osiągnięty |
| **β.e** | **VFS lag fix (P2-002)** — dekompozycja `smart_sync.rs` (2197 linii) na moduły. Streaming hydration zamiast fetch-all-then-decrypt. | SLA cold fetch §12.2 osiągnięte |

**Szacunek:** 4–6 sesji.

---

### 12.7 Faza γ — Zero Data Loss Hardening *(po Fazie β)*

> **Cel:** spełnić wszystkie 5 kryteriów Zero Data Loss zaakceptowanych w decyzji 2026-05-10.

#### Drzewko orientacyjne

```
γ — Zero Data Loss Hardening (po β)
├── γ.a — Resume upload after crash (multipart state w SQLite)  ⏸️
├── γ.b — Conflict copy (2-device write → 2 revisions w O:)     ⏸️
├── γ.c — Soft-delete grace 7 dni + UI „Kosz"                   ⏸️
└── γ.d — Snapshot upload guard (3-provider outage → .bak/24h)  ⏸️
```

| Krok | Zakres | DoD |
|------|--------|-----|
| **γ.a** | **Resume upload after crash.** Multipart upload state persist w SQLite (`multipart_uploads` table z S3 upload_id, parts, completed_at). Daemon po crashu → wznowienie pending parts zamiast restart-from-zero. | Test: kill daemona w środku 1GB upload → restart → plik w chmurze kompletny |
| **γ.b** | **Conflict copy.** Modyfikacja tego samego inode z 2 urządzeń → oba revisions zachowane, materialized w O:\ jako `file (Conflict from Dell).pdf`. (Faza S w starym roadmap to mobile; tutaj desktop-first.) | Test 2-device write conflict → 2 revisions w `file_revisions` + 2 pliki w O:\ |
| **γ.c** | **Soft-delete grace period.** `inodes.deleted_at` + grace 7 dni. UI „Kosz" w sidebar. Twardy delete dopiero po grace. | Test: usuń plik → 7 dni odzyskiwalny → po 7 dniach gone |
| **γ.d** | **Snapshot upload guard.** Daemon nie wgra nowego snapshotu jeśli wszystkie 3 providery odpowiedziały błędem; trzyma stary aktualny w cache. Backup `omnidrive.db.bak.YYYYMMDD_HHMMSS` co 24h lokalnie. | Test simulated 3-provider outage → snapshot lokalny kompletny po recovery |

**Szacunek:** 4–6 sesji.

---

### 12.8 Faza δ — Multi-User Infra Closure *(pod maską, bez UI)*

> **Cel:** zamknąć Epic 34 — multi-user/Family Cloud infrastruktura w pełni działa _pod maską_, ale UI single-user. v5.0 = włączenie UI, żadnego dotykania krypto/schema.

#### Drzewko orientacyjne

```
δ — Multi-User Infra Closure (pod maską, bez UI; po γ)
├── δ.a — Per-user Vault Key wrap e2e (hybrid z α.B.b)          ⏸️
├── δ.b — Invite/accept_device flow (Member ≠ Owner)            ⏸️
├── δ.c — Recovery BIP-39 nietechniczny user (po α.B.b)         ⏸️
└── δ.d — ACL roles enforcement audit (require_role minimum)    ⏸️
```

| Krok | Zakres | DoD |
|------|--------|-----|
| **δ.a** | **Per-user Vault Key wrap end-to-end.** Owner generuje, member dostaje wrapped VK (X25519+ML-KEM hybrid, faza α). Test: 2 userów, każdy unlock swoim hasłem, oba dostają ten sam plaintext VK. | E2E test passes |
| **δ.b** | **Invite/accept_device flow** — pełen test: Owner generuje invite → kopiuje link → drugi user wkleja → wpisuje swoje hasło → device dostaje wrapped VK → unlock działa. ACL: drugi user = Member, nie Owner. | E2E test passes |
| **δ.c** | **Recovery BIP-39 dla nietechnicznego usera.** Mnemonik 24-słowny → unlock bez znanego hasła. Action: druk karty A4 (już w Sesji G.6). Czy działa po α.B.b (hybrid wrap)? | Test recovery na sklonowanym DB |
| **δ.d** | **ACL roles enforcement audit.** Każdy `acl::require_role(Role::X)` audit pod kątem: Czy wybrane role to minimum potrzebne? Czy Owner może coś czego Admin nie? Czy Viewer naprawdę tylko reads? | Audit table `docs/superpowers/specs/2026-XX-XX-acl-audit.md` |

**Szacunek:** 3–5 sesji. Większość kodu istnieje (Epic 34 Sesje A–E DONE), tu chodzi o weryfikację i domknięcie luk.

---

### 12.9 Faza ε — VFS Stability *(pancerne O:)*

> **Cel:** „Arka musi płynąć gładko" — VFS bez zająknień, native cfapi state mapping, Defender-friendly.

#### Drzewko orientacyjne

```
ε — VFS Stability (pancerne O:; po δ)
├── ε.a — Dekompozycja smart_sync.rs (2197 → 4-5 modułów <800)  ⏸️ (overlap z β.e)
├── ε.b — Native cfapi state mapping (4 stany, 0 own overlay)   ⏸️
├── ε.c — Drive O: stress (open/close storm 1000×, 0 deadlock)  ⏸️
└── ε.d — Defender exclusion guidance (instalator + README)     ⏸️
```

| Krok | Zakres | DoD |
|------|--------|-----|
| **ε.a** | Dekompozycja `smart_sync.rs` (2197 linii → 4–5 modułów: `placeholder.rs`, `hydration.rs`, `pin_state.rs`, `state_machine.rs`, `stream.rs`). Test coverage przed/po identyczne. | Compiles + tests pass + każdy moduł < 800 linii |
| **ε.b** | Native cfapi state mapping (Epic 35.2c IPC) — `CfReportProviderProgress` + `CfUpdatePlaceholderInfo` dla ikon: cloud-only / local / pinned / syncing / error. Bez własnych shell overlay (per memory `feedback_no_custom_overlays.md`). | Eksplorator pokazuje natywne ikony dla 4 stanów |
| **ε.c** | Drive O: stress test — file open/close storm (np. PowerShell `1..1000 \| % { Get-Item O:\test\$_.txt }`). Brak deadlock w cfapi. | Stress test passes 1000 cycles |
| **ε.d** | Defender exclusion guidance (instalator + dokumentacja) — instrukcja dla użytkownika jak dodać `%LOCALAPPDATA%\OmniDrive\` do exclusion list (bez tego cfapi races z Defenderem). | README sekcja + opcjonalnie skrypt PS w instalatorze |

**Szacunek:** 4–6 sesji.

---

### 12.10 Faza ζ — Test Automation 100% kluczowych flow

> **Cel:** każdy krytyczny user flow ma e2e test. „Głupie błędy podczas testów" — niedopuszczalne na etapie ręcznym.

#### Drzewko orientacyjne

```
ζ — Test Automation 100% krytycznych flow (po ε)
├── ζ.a — Stress harness (1000 plików / 1 GB / 24h soak)        ⏸️
├── ζ.b — F1–F12 e2e w angeld/tests/                            ⏸️ 0/12
└── ζ.c — Coverage report critical paths (tarpaulin/grcov)      ⏸️
```

**Lista kluczowych flow do automatyzacji:**

| # | Flow | Status |
|---|------|--------|
| F1 | Bootstrap local-only (wizard local, brak chmury, plik w O:) | ⬜ |
| F2 | Wizard cloud-enabled (3 providery, unlock, upload, download) | ⬜ |
| F3 | Join Existing Vault (Dell scenario v0.3.23) | ⬜ |
| F4 | Lock → Unlock cycle z passphrase i Windows Hello | ⬜ |
| F5 | File upload + download integrity (1MB, 100MB, 1GB) | ⬜ |
| F6 | Conflict resolution (γ.b — 2-device write) | ⬜ |
| F7 | Recovery key BIP-39 generation + restore | ⬜ |
| F8 | Multi-device add (invite + accept_device + per-user wrap, faza δ) | ⬜ |
| F9 | Change passphrase + auto-snapshot upload (v0.3.15) | ⬜ |
| F10 | Disaster recovery (kasacja DB → restore z chmury) | ⬜ |
| F11 | Soft-delete + restore from trash (γ.c) | ⬜ |
| F12 | Crypto re-key rotation (Vault Key generation bump) | ⬜ |

| Krok | Zakres | DoD |
|------|--------|-----|
| **ζ.a** | Stress test harness — `scripts/stress-test.ps1` lub `cargo test --features stress` (1000 plików, 1 GB plik, 24h soak). | Stress test runnable, baseline metrics zapisane |
| **ζ.b** | Każdy z F1–F12 → e2e test w `angeld/tests/`. | Każdy F# zielony |
| **ζ.c** | Coverage report (`cargo tarpaulin` lub `grcov`) — nie celujemy w 100% line coverage, ale w 100% pokrycia _critical paths_. | Report w `docs/coverage-vYYY-MM-DD.html` |

**Szacunek:** 6–10 sesji. To największy gap — aktualnie 13 unit + 7 integration testów na 41 638 linii kodu.

---

### 12.11 v0.4.0 Release Gate

Wszystkie QG1–QG6 spełnione → tag `v0.4.0`, instalator, CHANGELOG, push GitHub Releases.

---

### 12.12 v5.0 Family Cloud (po v0.4.0)

> **Skupienie:** UI dla nietechnicznych użytkowników. Infra już jest (faza δ).

- UI Family Cloud: invite link generation w sidebarze, pending devices view, accept/reject z safety numbers verification
- Friendly recovery flow (przewodnik krok-po-kroku dla osoby która zgubiła hasło)
- Dead Man Switch (idle X miesięcy → email do recovery contact, transfer ownership)
- Audyt zewnętrzny krypto (gate przed wpuszczeniem cudzych plików!)
- RCE defense in depth (sandbox angeld, capability-based file access)

### 12.13 v6.0 Mobile Ecosystem (po v5.0)

Patrz Fazy P/Q/R/S w starej sekcji (zachowane w archiwum). Założenia z `feedback_mobile_architecture.md`:
- Android-first (UniFFI, Kotlin + Compose)
- QR pairing z derived key (nie raw key)
- SQLite snapshot read-only (V1) → SAF write (V3)
- Inbox upload (camera, file share)
- WebCrypto compat dla web (Epic 33 mobile leg) — wymaga ML-KEM WASM polyfill (faza α już to ułatwi)

---

### 12.14 Stary „Co przed nami" — przeniesione do Backlog

Niektóre items z poprzedniej wersji STATUS.md nie wpadły do roadmapy v0.4 ale zachowujemy:

- **Batch 7 (POST-DELL):** rustls/hyper consolidation (powiązane z β.3); OAuth code-exchange refactor; Tray IPC dla recovery confirm — wszystkie do v5.0+ chyba że priorytet wzrośnie.
- **Faza O.1 Quota Fix:** raportowanie pojemności O:\ z cloud quota (B2/R2) — P3, do v0.4.x patch.
- **Epic 33 Tryb B (CF Pages share-site):** odłożone do v6.0 (mobile share = naturalny kontekst).
- **Faza O.2+ Cross-Platform VFS:** odłożone do v6.0 (Linux/macOS = post-mobile).
- **Faza P/Q/R/S** (Mobile read-only / Mobile bridge / Mobile read-write) — pełne tabele zarchiwizowane w `docs/archive/roadmap.md`. Wykonanie po v5.0 (patrz §12.13). Założenia (UniFFI, QR pairing, SQLite snapshot, SAF write) niezmienione.

---

## 13. Decyzje architektoniczne

| ID | Decyzja | Uzasadnienie |
|----|---------|--------------|
| **D1** | `skarbiec.app` = static content ONLY (CF Pages) | Cloudflare Tunnel → daemon = attack surface (session hijack, RCE). Static HTML = brak tajemnic do hackowania. |
| **D1a** | Cloudflare Pages dla decryptora (nie GH Pages) | Zero kosztu. Domena już w CF → jeden klik. Edge CDN. Fallback: `omnidrive.github.io/share`. |
| **D2** | Hybrid E2E: mockito CI + manual smoke na real B2 | Real B2 smoke = prawdziwy test; mockito = szybkie CI bez kosztów egress. |
| **D3** | Desktop 100% ready (v0.4.0) → dopiero mobile | Kolejność: Dell smoke → O.1 → Batch 7 → Epic 33 Tryb B → O.2+ → v0.4.0 → Faza P. Cryptomator latami desktop-only. Mobile zbyt wcześnie = rozproszenie. |
| **D4** | Mobile V1 = read-only SQLite snapshot (Opcja C) | Daemon writes / mobile reads = zero konfliktu. Write (CRDT) = osobna decyzja po V1. |
| **D5** | UniFFI (rekomendacja) — decyzja przed Fazą P | Native quality dla security app. Flutter = ciężki runtime Dart. |
| **D6** | Landing page `skarbiec.app` (post v0.3.0) | What is it / Screenshots / Download / GitHub. Proste. |
| **M.6.1** | CORS = loopback + RFC1918 only, zero public domains | XSS na GH Pages → fetch do daemona z ukradzioną sesją. Attack surface eliminowany. |
| **B.6** | `validate_user_session` bez constant-time | 256-bit random token + LAN-only + SQLite overhead = timing attack niewykonalny. Udokumentowane §11 `crypto-spec.md`. |
| **C.1 Wariant B** | VK Sealing dla refresh tokena | Refresh wymaga unlocked vault = zero dostępu bez hasła. Zgodne z Zero-Knowledge. |
| **C.3 POST-DELL** | rustls/hyper consolidation odłożone | Major AWS SDK bump = wysokie ryzyko regresji przed smoke testem. |
| **D7** (2026-05-10) | **v0.4 = Single-User UI, Multi-User infra pod maską** | Zero przepisywania krypto/schema przy v5.0. Decyzja Przemka: „nie chcę niczego przepisywać przy v5.0". |
| **D8** (2026-05-10) | **Quantum-Resistant hybrid (X25519 + ML-KEM-768) od dnia 1** | „Store now, decrypt later" mitigation. Tylko key encapsulation — chunki AES-GCM zostają (Grover safe). |
| **D9** (2026-05-10) | **Argon2id 2026 baseline (m=47MiB, t=1, p=1)** | OWASP 2025+ rekomendacja. Migracja: re-derive KEK przy następnym unlocku z nowymi params. |
| **D10** (2026-05-10) | **Identity grafting (Single-User-Multi-Device)** | Dell po join przyjmuje user_id ze snapshot Lenovo. Safety numbers identyczne. Implemented v0.3.23. |
| **D11** (2026-05-10) | **Audyt zewnętrzny krypto = gate v5.0**, nie v0.4 | v0.4 polega na formalnym Claude review (QG5). Zewnętrzny audyt dopiero gdy w grę wchodzą cudze pliki. |
| **D12** (2026-05-10) | **Bug tracking = `docs/KNOWN_ISSUES.md`** | Pojedynczy plik z P0/P1/P2/P3 buckets. Nie GitHub Issues. Claude wpisuje, Przemek zatwierdza. |

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
| **DEK keymap nie kopiowany w grafcie (P1-001)** | HIGH | ⬜ OPEN — `KNOWN_ISSUES.md` P1-001. Faza β.a v0.4. Skutek: hydration plików multi-device nie działa po join-existing. |
| **Snapshot fetch jednokierunkowy (P1-002)** | MEDIUM | ⬜ OPEN — Faza β.b. Lenovo nie widzi nowych devices bez restart daemona. |
| **Snapshot redundancja 1/3 providers żywych (P1-003+P1-004)** | MEDIUM | ⬜ OPEN — Faza β.c. Tylko B2 wgra, Scaleway 403, R2 ConnReset. |
| **Watcher CPU mieli (P2-001)** | MEDIUM | ⬜ OPEN — Faza β.d po pomiarach. SLA: < 1% idle. |
| **VFS lag duże pliki (P2-002)** | MEDIUM | ⬜ OPEN — Faza β.e/ε. SLA: < 10s/100MB cold fetch. |
| **ML-KEM crate maturity** | MEDIUM | Mitygacja: `ml-kem = "0.2"` to RustCrypto, audited; FIPS 203 reference. Plan: adopt + sandbox test w fazie α.B.b przed wpięciem do produkcji. |
| **Test coverage < 5% lines** | HIGH | ⬜ OPEN — 13 unit + 7 integration testów na 41 638 linii. Faza ζ celuje w 100% kluczowych flow (F1–F12). |

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
