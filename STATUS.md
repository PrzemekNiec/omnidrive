# OmniDrive — Status & Plan (Single Source of Truth)

> **Ostatnia aktualizacja:** 2026-04-27
> **Aktualna wersja kodu:** `v0.2.0` (commit `7819811`) — release `v0.3.0` zablokowany do końca Fazy N.5 + Dell smoke testu
> **Konsolidacja:** ten plik zastępuje `plan.md`, `PROJECT_STATUS.md`, `roadmap.md`, `ARCHITECT_STATE.md`, `spec_review.md` (zarchiwizowane w `docs/archive/`).
> **Plan taktyczny per-batch:** sekcja [§5 Faza N.5](#5-aktualna-faza-n5--pre-dell-hardening) + memory `project_faza_n_next.md`.

---

## Spis treści

1. [Wizja i stack](#1-wizja-i-stack)
2. [Stan produktu dziś](#2-stan-produktu-dziś)
3. [Roadmapa wysokiego poziomu](#3-roadmapa-wysokiego-poziomu)
4. [Decyzje architektoniczne (D1–D6, M.6.1, N.5)](#4-decyzje-architektoniczne)
5. [Aktualna faza: N.5 — Pre-Dell Hardening](#5-aktualna-faza-n5--pre-dell-hardening)
6. [Następne fazy (po N.5)](#6-następne-fazy-po-n5)
7. [Backlog i świadome pominięcia](#7-backlog-i-świadome-pominięcia)
8. [Architektura — krytyczne pliki](#8-architektura--krytyczne-pliki)
9. [Historia (ukończone epiki w skrócie)](#9-historia-ukończone-epiki-w-skrócie)
10. [Risk register](#10-risk-register)

---

## 1. Wizja i stack

**OmniDrive** — local-first, zero-knowledge Windows storage:
- jeden logiczny skarbiec (Vault) widoczny jako `O:\`
- on-demand access przez Eksplorator (cfapi/Cloud Files API)
- multi-cloud backend (S3-compatible: B2, R2, Scaleway)
- envelope encryption (KEK → Vault Key → DEK), AES-256-KW + AES-256-GCM
- recovery, scrub, repair, reconciliation built-in

**Stack:**
- Backend: Rust Edition 2024, Tokio
- DB: SQLite (`sqlx` / `rusqlite`)
- OS integration: `windows-rs`, `cfapi.dll` (Cloud Files)
- Cloud: S3-compatible (B2, R2, Scaleway)
- Frontend: Vanilla JS + Tailwind, glassmorphism, serwowane lokalnie z daemona
- Mobile (planowane): UniFFI + Kotlin (Android first)

---

## 2. Stan produktu dziś

### Co działa
- Per-user Windows installer + autostart, runtime pod `%LOCALAPPDATA%\OmniDrive`
- Local-only first-run bez konfiguracji providerów
- `O:\` jako wirtualny dysk (cfapi placeholder + smart sync)
- Encrypted metadata backup + disaster recovery
- Scrub, repair, reconciliation, shell self-heal
- Maintenance dashboard + storage/cost dashboard
- Multi-device core (LAN peer discovery, conflict-aware revisions, bridge B0–B8 zamknięty na Lenovo+Dell)
- Envelope encryption v2 (DEK per-plik, Vault Key rotation)
- Ghost Shell: cfapi PoC + ingest state machine + hydration (35.0–35.1e + 35.2a-b + 35.3 tray)
- Zero-Knowledge link sharing (Tryb A — LAN/loopback)
- Family Cloud (Epic 34): audit trail, recovery keys BIP-39, OAuth2 (PKCE), Safety Numbers (Identicon + mnemonik)
- UI Redesign (Epic 36 Sesja F+G — Stitch layout, 7 zakładek, hash-router)
- Faza N.5 Batche 1–5: graft hardening, OsRng, CORS exact-match, recovery rate-limit, replaceState, restore state markery, low-order pubkey defense, vault_id consistency check, OAUTH gate

### Co zostało / nadchodzi
- ✅ **Faza N.5 Batch 6** — C.1 refresh-token VK sealing + C.2 SecretString migration DONE
- **Dell smoke test** — gate przed releasem v0.3.0 (graft + watcher DRY_RUN + share LAN end-to-end)
- **Faza N.3+N.4** — bump 0.2.0 → 0.3.0, payload, instalator, tag, CHANGELOG, SHA-256
- **Po v0.3.0:** Epic 33 Tryb B + Faza O.1 + Faza O.2+ → v0.4.0 → mobile (P→Q→R→S)

---

## 3. Roadmapa wysokiego poziomu

| Faza | Zakres | Status | Priorytet | Szacunek |
|------|--------|--------|-----------|----------|
| Phase 0 | Crypto checkpoint + `docs/crypto-spec.md` | ✅ DONE | — | — |
| Epic 32.5 | Envelope Encryption (KEK→VK→DEK, migracja, rotacja) | ✅ DONE | — | — |
| Epic 35 | Ghost Shell (cfapi, ingest, hydration, shell ext, tray) | ✅ DONE | — | — |
| Epic 33 (A) | Zero-Knowledge Link Sharing — Tryb A (LAN) | ✅ DONE | — | — |
| Epic 34 | Family Cloud (audit, recovery, OAuth, safety numbers) | ✅ DONE | — | — |
| Epic 36 Sesja F+G | UI Redesign Stitch layout | ✅ DONE | — | — |
| Faza H–M.6 | UI quick-wins, OAuth UI, Safety Numbers, Local-First lockin | ✅ DONE | — | — |
| **Faza N.1+N.2** | Dead code audyt + hybrid E2E | ✅ DONE `7819811` | — | — |
| **Faza N.5 Batch 1–5** | Pre-Dell hardening (Paczki A+B core) | ✅ DONE | — | — |
| **Faza N.5 Batch 6** | C.1 refresh-token VK + C.2 SecretString | ✅ DONE | **P0** | 1 sesja |
| **Dell Smoke Test** | Cross-device acceptance v0.3.0 | ⬜ TODO | **P0** | 0.5 dnia |
| **Faza N.3+N.4** | Bump → 0.3.0 + payload + instalator + release | ⬜ TODO | **P0** | 0.5 dnia |
| Faza O.1 | Quota fix (cloud quota zamiast C: dla O:) | ⬜ TODO | P1 | 1 dzień |
| Epic 33 Tryb B | Public shares przez CF Pages (`skarbiec.app/s/…`) | ⬜ BACKLOG | P2 | 2-3 tyg |
| Faza O.2+ | Cross-platform VFS (FileSystemAdapter + FUSE) | ⬜ BACKLOG | P2 | 2-4 tyg |
| Faza N.5 Batch 7 (POST-DELL) | C.3 rustls/hyper consolidation + B.3 Krok 2 + B.2 tray IPC | ⬜ BACKLOG | P2 | — |
| Faza P | Core Extraction (UniFFI, NDK, aarch64) | ⬜ BACKLOG | P3 | 2-3 dni |
| Faza Q | Mobile Bridge & Handshake (Android + QR ECDH+SAS) | ⬜ BACKLOG | P3 | 5-7 dni |
| Faza R | Mobile V1 Read-Only (BiometricPrompt, snapshot, decrypt) | ⬜ BACKLOG | P3 | 7-10 dni |
| Faza S | Mobile V2 Read-Write (Inbox, share links, camera) | ⬜ BACKLOG | P4 | 5-7 dni |

**Critical path do v0.3.0:** Batch 6 → Dell smoke → N.3+N.4 release → tag.

---

## 4. Decyzje architektoniczne

### Local-First (zatwierdzone 2026-04-19, M.6 ZROBIONE)
- **D1 + D1a:** `skarbiec.app` wraca do użycia, ale **wyłącznie** jako CF Pages static content (decryptor Trybu B + landing). Daemon nadal `127.0.0.1` only.
- **D6:** Prosty landing na root `skarbiec.app` razem z share-site.
- **M.6.1 KRYTYCZNA:** **Nie dodajemy** `skarbiec.app` do CORS allowlist. Daemon głuchy na publiczny internet. Zatwierdzone w `B.1` (CORS exact-match z `host_from_http_origin` + `IpAddr::parse`).

### Sieć i CORS
- Daemon słucha tylko `127.0.0.1:8787` (opcjonalnie LAN dla Trybu A share).
- **CORS allowlist:** loopback + RFC1918 LAN tylko. Public domains zabronione (atak surface przez XSS na GH Pages → fetch do LAN-owego daemona z ukradzioną sesją).
- OAuth redirect wyłącznie loopback (RFC 8252).
- Share link Tryb A: dynamic host z `Host:` headera lub env `OMNIDRIVE_SHARE_HOST`.

### Krypto (Phase 0 + decyzje N.5)
- **Hierarchia 3-warstwowa:** passphrase → KEK (HKDF Argon2id) → Vault Key (random AES-KW wrapped) → DEK (per-plik, AES-KW wrapped) → AES-256-GCM dla chunków.
- **AES-256-KW (RFC 3394)** dla wrappingu (nie AES-GCM — brak nonce, WebCrypto-kompatybilny).
- **DEK per-plik** (nie per-chunk) — jeden secret w share URL dla Epic 33.
- **Lazy migration V1→V2** — nowe pliki V2, stare czytane V1, opcjonalny batch re-encryption (DONE).
- **Refresh token Google (C.1):** wybrany Wariant B = VK Sealing (AES-GCM z kluczem `derived_from_VK("oauth-refresh-tokens", user_id)`). Refresh wymaga unlocked vault.
- **B.6 (timing side-channel):** `validate_user_session` zostaje bez constant-time — udokumentowane w `docs/crypto-spec.md` §11 (256-bitowy random token + LAN/loopback only + SQLite query overhead → atak niewykonalny).

### Mobile (zatwierdzone 2026-04-19/04-20)
- **D3 = Desktop polish first** — mobile zaczyna się dopiero po v0.4.0 (Epic 33 Tryb B + O.2+).
- **D4 = Opcja C** — mobile V1 czyta `vault_snapshot.db` wygenerowany przez daemon (read-only, daemon writes / mobile reads).
- **D5** — UniFFI vs Flutter: rekomendacja **UniFFI** (native quality dla security app), finalna decyzja przed startem Fazy P.
- **QR Pairing:** ECDH X25519 + SAS (4-cyfrowy kod), spec `docs/superpowers/specs/2026-04-20-mobile-qr-pairing-design.md`.
- **Mobile identity:** osobna tożsamość urządzenia + własny key wrapped przez vault key Alice (revokable).

### Hardening N.5 (decyzje user 2026-04-27)
- **Akceptacja całości** — wszystkie 20 itemów audytu (security-reviewer + tech-lead-reviewer).
- **B.2 recovery:** tylko rate-limit + state-guard. Tray confirmation IPC → Task 35.3 (System Tray Companion).
- **B.3 OAuth fragment:** tylko Krok 1 (`history.replaceState` + CSP). Krok 2 (code-exchange + HttpOnly cookie) → POST-DELL.
- **C.1 refresh token:** Wariant B (VK Sealing) — najbardziej zgodny z Zero-Knowledge.
- **C.3 rustls/hyper consolidation:** POST-DELL (major bump AWS SDK = za duże ryzyko regresji).

---

## 5. Aktualna faza: N.5 — Pre-Dell Hardening

**Geneza:** audyt security-reviewer (`a47d15de04fc9599d`) + tech-lead-reviewer (`a315d4485f94cd0f5`) — 2026-04-27. Wykrył **20 znalezisk** (7 HIGH + 7 MEDIUM + 6 LOW) przed Dell smoke testem.

**Cel:** Skarbiec hermetyczny przed wgraniem na drugą maszynę produkcyjną. Zero leftoverów, zero ataków na zera, zero plaintextów w logach/URL/DB.

### ✅ Batch 1 — Foundation + Cross-Device Critical (DONE)
| Item | Commit | Co zrobione |
|------|--------|-------------|
| `A.0` | `bb6e596` | `retry_io` helper w `secure_fs.rs` + `secure_delete` go używa |
| `A.2` | `796180e` | Staging file `secure_delete` (zero-overwrite + retry 5×500ms) |
| `A.4` | `f55d810` | `drop(restored_pool) + yield_now()` w `db.rs` |

### ✅ Batch 2 — Watcher + Pubkey Defense (DONE)
| Item | Commit | Co zrobione |
|------|--------|-------------|
| `A.1` | `5c31ec4` | Watcher DRY_RUN + pre-onboarding passive gate |
| `A.3` | `4f949bb` | X25519 low-order point defense + `devices.enrolled_at` |

### ✅ Batch 3 — Crypto Quick Wins (DONE)
| Item | Commit | Co zrobione |
|------|--------|-------------|
| `B.4` | `ebd3220` | `thread_rng` → `OsRng` w `db.rs` + `oauth.rs` |
| `B.1` | `ebd3220` | CORS exact-match: `host_from_http_origin` + `IpAddr::parse` |

### ✅ Batch 4 — Auth Surface Hardening (DONE)
| Item | Commit | Co zrobione |
|------|--------|-------------|
| `B.2` | `35a95bb` | `recovery/restore`: rate-limit (DashMap) + state-guard (24h) + audit |
| `B.5` | `0803908` | `join-existing`: state-guard + progressive delay (1s→5s→30s) |
| `B.3 K1` | `a6446db` | `Referrer-Policy: no-referrer` + `X-Frame-Options: DENY` |

### ✅ Batch 5 — Polish / Diagnostyka (DONE)
| Item | Commit | Co zrobione |
|------|--------|-------------|
| `A.5` | `348ed0d` | Restore state markery + `GET /api/diagnostics/restore` |
| `A.6` | `348ed0d` | `provider_configs` graft z lokalnym `epoch_secs()` |
| `A.7` | `2a0a763` | `migrate_single_to_multi`: `target_user_id` + `target_device_id` |
| `A.8` | `517b5a0` | `CONNECTION_KEY.lock().unwrap_or_else(...into_inner())` (5 miejsc) |
| `A.9` | `9e42575` | `verify_vault_device_binding` przy starcie + panic na niezgodności |
| `B.6` | `fda2cec` | Komentarz w `validate_user_session` + §11 w `crypto-spec.md` |
| `B.7` | `fda2cec` | `OMNIDRIVE_AUTO_RESTORE_PASSPHRASE` ignorowany w release + warn |

### ✅ Batch 6 — Defense in Depth (DONE)

#### `C.1` Refresh-token Google: VK Sealing (Wariant B) 🟡
- **Stan obecny:** `db.rs:1133` `users.google_refresh_token TEXT` — plaintext. `api/oauth.rs:175-191` zapisuje wprost.
- **Plan:**
  - Nowa kolumna `google_refresh_token_ciphertext BLOB`.
  - AES-GCM z kluczem `derived_from_VK("oauth-refresh-tokens", user_id)` jako AAD.
  - Wymaga aktywnego unlocked Vault dla każdego refresha access-tokena.
  - Migracja: stare plaintext → przy następnym unlock → szyfrowane → kolumna plaintext nullowana.
- **Exit:** `cargo audit` clean + lock vault → próba refresh OAuth → fail z czytelnym `vault_locked`. Unlock → refresh działa.

#### `C.2` SecretString migration ✅ DONE
- **Wykonano:** `api/auth.rs`, `api/recovery.rs`, `api/onboarding.rs`, `api/vault.rs` — `passphrase: String` → `SecretString`. `Cargo.toml` workspace: `secrecy` + `features = ["serde"]`. `ExposeSecret` dodano we wszystkich 4 handlerach.
- **Exit spełniony:** `grep -rn 'passphrase: String' angeld/src` → brak. `cargo audit`: 8 pre-existing vulns (AWS SDK + sqlx/rsa chain) — bez nowych. Secrecy 0.10.3 czyste. C.3 rustls consolidation czeka POST-DELL.

### 🚪 Dell Smoke Test Gate (po Batchu 6)
1. Świeża instalacja v0.3.0 na Dellu (po payload + instalatorze).
2. `Join Existing Vault` z passphrase + provider config.
3. **Asserty:**
   - `%LOCALAPPDATA%\OmniDrive\restore-staging-*` pusty po graftcie.
   - Watcher pokazuje `[DRY_RUN]` w logach albo jest pasywny.
   - `accept_device` z `[0;32]` → `400 BadRequest "low_order_pubkey"`.
   - `curl -H "Origin: http://localhost.evil.com"` → CORS reject.
   - Brute-force `/api/recovery/restore` 10× → po 3. próbie 429.
   - OAuth callback → `window.location.hash === ""` po replaceState.
   - Cross-device Identicon + mnemonik test (Lenovo ↔ Dell, byte-identyczny SVG + ten sam mnemonik).
   - Crash daemona w trakcie graftu (`kill -9`), restart → `restore_state = last_failed`, retry działa.

### ⏸️ Batch 7 — POST-DELL
- **C.3** rustls/hyper consolidation (`aws-config`/`aws-sdk-s3` → hyper-1, `sqlx` → rustls, `cargo audit`).
- **B.3 Krok 2** OAuth code-exchange + HttpOnly cookie.
- **B.2 Tray confirmation** (Task 35.3 IPC).

### Risk register Faza N.5

| Item | Severity | Ryzyko regresji | Pre-Dell? |
|------|----------|-----------------|-----------|
| A.0 retry helper | HIGH (foundation) | Niskie | ✅ DONE |
| A.1 watcher DRY_RUN | HIGH | Niskie | ✅ DONE |
| A.2 staging delete | HIGH | Niskie | ✅ DONE |
| A.3 zero-pubkey | HIGH | Średnie (schema) | ✅ DONE |
| A.4 close-and-settle | HIGH | Niskie | ✅ DONE |
| B.1 CORS | HIGH | Średnie | ✅ DONE |
| B.2 recovery limit | HIGH | Niskie | ✅ DONE |
| B.3 OAuth Krok 1 | HIGH | Niskie | ✅ DONE |
| B.4 OsRng | HIGH | Zerowe | ✅ DONE |
| A.5–A.9, B.5–B.7, A.6–A.8 | MEDIUM/LOW | Niskie | ✅ DONE |
| **C.1 refresh-token VK** | MEDIUM | Średnie (migracja) | **NEXT (zalecane)** |
| **C.2 SecretString** | MEDIUM | Średnie (refactor 6-8 plików) | ✅ DONE |
| C.3 rustls consolidation | MEDIUM | **Wysokie** | ❌ POST-DELL |
| B.3 Krok 2 (OAuth) | — | Średnie | ❌ POST-DELL |
| B.2 Tray confirmation | — | Wysokie (IPC) | ❌ Task 35.3 |

---

## 6. Następne fazy (po N.5)

### Faza N — Release v0.3.0 (po Dell smoke)
- **N.3:** Bump `0.2.0 → 0.3.0` we wszystkich `Cargo.toml` (angeld, omnidrive-core, angelctl) + `installer/omnidrive.iss`.
- `cargo build --release --workspace` → `cp target/release/*.exe dist/installer/payload/` → `cp angeld/static/* dist/installer/payload/static/` → Inno Setup.
- `CHANGELOG.md` wpis v0.3.0 (M.5 BIP-39+Identicon, M.6 Local-First lockin, N.5 hardening).
- **N.4:** Pełny smoke flow (unlock → share LAN → join → verify Identicon+mnemonik match → lock). Commit `release: v0.3.0`, push, tag `v0.3.0`.
- **N.5:** SHA-256 instalatora w GitHub Releases + `README.md`.

### Faza O.1 — Quota Fix (1 dzień, P1)
- Raportowanie pojemności `O:\` z faktycznego cloud quota (B2/R2) zamiast z `C:`.
- Może być razem z v0.3.0 albo jako 0.3.1 hotfix.

### Epic 33 Tryb B — Public Shares przez CF Pages (2-3 tyg, P2)
- **Architektura:** `skarbiec.app/s/{id}#{dek}@{b2_base}` — static decryptor na CF Pages, daemon NIE uczestniczy w downloadzie.
- **Upload:** Duplicate encrypted chunków pod publicznym prefixem (`shares/{share_id}/chunk-*.bin`) — łatwe revocation przez delete prefix.
- **Password:** `wrapped_dek = AES-KW(dek, PBKDF2(password, salt))` w manifeście — Bob wpisuje hasło → unwrap.
- **Revocation:** UI „Wycofaj link" → DELETE prefix w B2 → Bob 404. SQLite mapping `share_id → [object keys]`.
- **Repo:** osobny `omnidrive/share-site` — `index.html` + `decryptor.js` + SHA-256 publikowany w README dla weryfikacji.
- **CI:** Cloudflare Pages Git integration (push do `main` = deploy automatyczny).
- **Fallback:** `omnidrive.github.io/share` jeśli CF odpadnie.

### Faza O.2+ — Cross-Platform VFS Foundation (2-4 tyg, P2)
- Trait `FileSystemAdapter`, refactor `cfapi` → implementacja traitu.
- Prototyp FUSE adaptera dla Linux/macOS.
- ENABLER dla Fazy P-R (mobile share core dependencies).

### Mobile (Fazy P → Q → R → S)

#### Faza P — Core Extraction (2-3 dni)
- Konfiguracja Android NDK + toolchain `aarch64-linux-android` + `x86_64-linux-android`.
- `uniffi` w `omnidrive-core/Cargo.toml`.
- UDL/proc-macro: eksport `decrypt_chunk`, `verify_vault_identity`.
- Build → `libomni_core.so`, generowanie Kotlin bindingów.

#### Faza Q — Mobile Bridge & Handshake (5-7 dni)
- Q.1 Android skeleton (Kotlin + Compose, `minSdkVersion 26`)
- Q.2 ML Kit QR scanning + parsowanie `omnidrive://pair?...`
- Q.3 ECDH X25519 (Tink Android) + SAS 4-cyfrowy kod (7-step protocol z spec)
- Q.4 Android Keystore AES-256 wrapping VK
- Nowe endpointy: `/api/mobile/pair-{start,init,confirm,cancel}`
- Nowe kolumny `devices`: `platform`, `paired_at`, `pairing_status`, `vault_key_generation`

#### Faza R — Read-Only Vault Browser V1 (7-10 dni)
- BiometricPrompt unlock → VK w pamięci (zeroize po 5 min)
- `GET /api/snapshot/latest` → `vault_snapshot.db` lokalnie
- Compose file browser (katalogi, metadane, search)
- Streaming decrypt (chunki LAN → `decrypt_chunk` UniFFI → Intent)
- Offline pinning (long-press → cache w internal storage)

#### Faza S — Read-Write V2 (5-7 dni)
- Inbox upload (ShareSheet → `POST /api/mobile/inbox/upload` → daemon szyfruje V2 + B2/R2)
- Share links via Epic 33 Tryb B
- Camera upload (WorkManager + `READ_MEDIA_IMAGES`) — opcjonalnie
- Conflict resolution dla S = osobna decyzja (rekomendacja: Opcja c — version branching)

---

## 7. Backlog i świadome pominięcia

### Backlog (świadomy)
- **Epic 34 dług:** `docs/THREAT_MODEL.md` formal review, e2e multi-user test extension.
- **Faza O.3** Cache management (częściowo już w `smart_sync.rs`).
- **Sharing Tryb B audit beacon** (opcjonalny opt-in `share_downloaded` event).

### Świadome pominięcia (POST v1.0)
- Bezpieczeństwo operacyjne (formal pen-test, external audit).
- Płatności / monetyzacja (projekt osobisty).
- i18n / l10n (UI tylko PL, ENG i inne — nie priorytet).
- Accessibility (a11y) — P4 backlog.
- Spec v1 formal compliance: `superblock`, `manifest envelope`, `canonical MsgPack object graph`, `lease/fencing token single-writer model`. Decyzja: **nie wracamy** — produkt-first wygrał. Szczegóły w zarchiwizowanym `spec_review.md`.

---

## 8. Architektura — krytyczne pliki

| Moduł / Plik | Rola |
|--------------|------|
| `omnidrive-core/` | Silnik krypto (EC_2_1, AES-KW, AES-GCM, Argon2id, BIP-39, identity) |
| `angeld/src/db.rs` | SQLite + migracje schema |
| `angeld/src/onboarding.rs` | `Join Existing Vault`, graft metadanych |
| `angeld/src/cfapi/` + `smart_sync.rs` | Windows Cloud Files (Ghost Shell) |
| `angeld/src/api/` | REST API split (auth, vault, files, sharing, onboarding, oauth, recovery, audit, stats, diagnostics, maintenance, multidevice) |
| `angeld/src/api_error.rs` | Unified `ApiError` (10 variants) |
| `angeld/src/secure_fs.rs` | `retry_io`, `secure_delete`, `overwrite_with_zeros` |
| `angeld/src/watcher.rs` | DRY_RUN gate (A.1) |
| `angeld/src/migrator.rs` | V1→V2 envelope batch migration |
| `angeld/src/scrubber.rs` | Background shard verification |
| `angeld/static/index.html` | Stitch UI shell (sidebar 240 + header 64 + 7 widoków) |
| `dist/installer/payload/` | Inno Setup payload (kopia binarek z `target/release/`) |
| `docs/crypto-spec.md` | Single Source of Truth dla envelope encryption + format V2 + B.6 §11 |
| `docs/superpowers/specs/2026-04-20-mobile-qr-pairing-design.md` | QR Pairing ECDH+SAS spec |
| `omnidrive-tray/` | System Tray Companion (Task 35.3) |
| `omnidrive-shell-ext/` | Shell Extension DLL (35.2a-b thin client) |

### Bezpieczne ścieżki testowe (CLAUDE.md Święta Zasada)
- **`O:\`** — wirtualny dysk skarbca (produkcja).
- **SyncRoot:** `C:\Users\{User}\AppData\Local\OmniDrive\OmniSync`.
- **`SYNC_PATH`** — dedykowany folder testowy. Watcher zabroniony poza nim.

---

## 9. Historia (ukończone epiki w skrócie)

### Epic 19.5 — Virtual Drive Mapping (`O:\`)
Custom drive label + icon; ten dysk = entry point dla Eksploratora.

### Epic 20–24 — Storage Engine
Disaster Recovery (encrypted metadata backup), Deep Scrubbing (light/deep), Local Cache (LRU + predictive prefetch), Flexible Storage (`EC_2_1` / `SINGLE_REPLICA` / `LOCAL_ONLY`), Secure Local Runtime (key memory protection + ACL hardening).

### Epic 26 — E2E Test Matrix
Recovery, reconciliation, self-healing pokryte E2E.

### Epic 27–28 — Installer + Self-Healing Shell
Per-user installer, autostart, clean-machine bootstrap, shell self-heal, second-machine validation.

### Epic 29–30 — Cost & Maintenance Dashboards
`/api/storage/cost`, policy mix, provider distribution, GC debt, maintenance actions w UI.

### Epic 31+32 — Multi-Device Core
Persistent `device_id`, trusted peer registry, LAN peer discovery + handshake, peer-first downloader, conflict-aware revisions (`device_id`, `parent_revision_id`, `origin`, `conflict_reason`), `/api/multidevice/status`, dashboard panel.

### Epic 31/32 Bridge B0–B8 — Onboarding + Multi-Device Acceptance ✅ (v0.1.20, 2026-04-06)
- B0 cloud guard + dry-run + cloud_usage_daily ledger
- B1 system_config + provider_configs + DPAPI provider_secrets
- B2 .env draft import (non-authoritative)
- B3 onboarding API (`/api/onboarding/{status,bootstrap-local,setup-identity,setup-provider,join-existing,complete}`)
- B4 provider validation (auth/list/put/delete probe)
- B5 first-run wizard UI (glassmorphism overlay)
- B6 join-existing metadata restore + vault_id graft + sync-root activation
- B7 daemon runtime DB-backed providers + hot-reload
- B8 Lenovo + Dell `dir O:\` instant (3 cfapi fixes w `smart_sync.rs`)

### Phase 0 — Crypto Checkpoint (DONE, `docs/crypto-spec.md`)
3-warstwowa hierarchia, AES-256-KW (RFC 3394), DEK per-plik, ChunkRecordPrefix V2 (80B, `record_version=2`).

### Epic 32.5 — Envelope Encryption (DONE, 2026-04-07)
- 32.5.1a-b KEK + Vault Key — `9ded01a`
- 32.5.1c-d DEK per-file + chunk encrypt V2 — `9ded01a`
- 32.5.2a-c Batch Migrator V1→V2 — `f6286dc`
- 32.5.2d Vault Key Rotation — `ad65cc2`

### Epic 35 — Ghost Shell (DONE, large)
- 35.0a-d cfapi PoC ✅ (zrobione w B8 — SyncRoot, hydracja, streaming, dehydracja w `smart_sync.rs`)
- 35.1a-e Ingest State Machine + chunking + DEK + atomic swap + hydration + failure recovery
- 35.2a Shell Extension DLL (thin client, IContextMenu)
- 35.2b Context menu — 4 poziomy (LOKALNIE / COMBO / CHMURA / FORTECA)
- 35.2c Natywne stany cfapi (decyzja 2026-04-13: **zero custom overlays**, używamy wyłącznie `CfSetPlaceholderState` + `CfSetPinState`)
- 35.3 System Tray Companion (`omnidrive-tray` crate, `tray-item` lub `windows-rs Shell_NotifyIcon`, polling `/api/health` co 5s)

### Epic 33 Tryb A — LAN Share (DONE)
Dynamic host w generowaniu linku (z `Host:` headera lub `OMNIDRIVE_SHARE_HOST`).

### Epic 34 — Family Cloud (DONE)
- 34.0–34.4a schema + asymmetric crypto + invite + revocation + ACL
- Sesja A: 34.5a+b Audit trail
- Sesja B: 34.6a Recovery Keys BIP-39
- Sesja Pre-C: User ID UUID v4 (Faza J `13177b6`)
- Sesja C: Google OAuth2 backend (PKCE, state DB, callback, refresh_token) — Faza K `667b0d5`
- Sesja D: OAuth frontend (przycisk Google, profil w topbarze, `#oauth_token`) — Faza L `6530194`
- Sesja E: Safety Numbers (60-digit Signal-style) — Faza M `a267cf8`

### Epic 36 — UI Redesign (DONE)
- Sesja F (F.1–F.8): Stitch layout (sidebar 240 + header 64 + 7 widoków), hash router, `/legacy` fallback, audit log + recovery alert + shard status pill — `5d1527d`
- Sesja G (G.1–G.11): stats endpoints + 5 widoków + Settings — wersja v0.2.0

### Faza H–M.6 (DONE)
- H: QR code, logout, audit fetch z Bearer, recovery CTA, link „Pełny log" — `e4ea91f`
- I: `/api/vault/lock`, `/api/vault/rotate-key`, `/api/filesystem/policies`, sysinfo CPU — `de0ce1b`
- J: UUID v4 user_id + backfill legacy `owner-{device_id}` — `13177b6`
- K: Google OAuth2 backend (PKCE, S256, oauth_states TTL 10 min) — `667b0d5`
- L: OAuth frontend — `6530194`
- M: Safety Numbers + QR code — `a267cf8`
- M.5: BIP-39 mnemonik (12 słów z hash[..16]) + Identicon (jdenticon) + 4×3 grid — `45a9b89` + `29dded3`
- M.6: Local-First lockin (CORS cleanup + dynamic share host + docs purge) — `4cfca26`–`0433bbc`

### Faza N (IN PROGRESS)
- N.1+N.2 dead code audit (`#[allow(dead_code)]` z komentarzami `// reserved for Epic X` w 10 plikach) — `7819811`
- N.2 Hybrid E2E tests (`roundtrip_pack_upload_download_restore_file` w `downloader::tests`, mockito S3 3 providery) — `0f1af36`
- 87/87 angeld + 11/11 omnidrive-core = **98 zielonych testów**

### Refactoring: Unified ApiError + API Module Split (DONE, 2026-04-09→04-11)
- Monolityczny `api.rs` (5026 linii) → `api/` directory (8 handler modules)
- `ApiError` enum (10 variants) z `IntoResponse` impl
- `acl::require_role()` → `Result<_, ApiError>` z `?`
- GitHub Actions CI (windows-latest), clippy zero warnings

---

## 10. Risk register

| Risk | Level | Mitigation |
|------|-------|------------|
| Refresh-token plaintext w DB (C.1) | MEDIUM | VK Sealing — Batch 6 |
| Passphrase residue w pamięci (C.2) | MEDIUM | ✅ SecretString + Zeroize on drop — Batch 6 DONE |
| Dell graft fail (Defender + cfapi races) | MEDIUM | A.0 retry helper + A.2 zero-overwrite + A.4 yield_now (DONE) |
| cfapi.dll bindings unstable | HIGH | B8 zamknął — `dir O:\` instant na Lenovo+Dell |
| Ingest race conditions | HIGH | Transactional state machine + rollback |
| Shell Extension crash = Explorer crash | HIGH | Thin client architecture (35.2a) — wszystko w angeld |
| Migration interrupted (power loss) | HIGH | Resumable V1→V2 z checkpointami |
| WebCrypto OOM duże pliki | MEDIUM | ReadableStream + TransformStream + size limit |
| Windows Defender blokuje hydrated files | MEDIUM | Early MotW testing + placeholder signature |
| Cloud costs surprise user | MEDIUM | Cloud Guard (B0) + threshold alerts |
| Private key loss (sole owner) | HIGH | Recovery Keys BIP-39 (Sesja B) |
| Hydration timeout | MEDIUM | EC_2_1 graceful degradation + adaptive timeouts |
| Multi-device formal lease/fencing brak | LOW | Świadoma decyzja: produkt-first nad spec v1. Szczegóły: `docs/archive/spec_review.md` |
| C.3 rustls/hyper bump regression | HIGH | POST-DELL + osobny test integracyjny B2/R2/Scaleway |
| Tray IPC complexity (B.2 Tray confirm) | HIGH | Odłożone do Task 35.3 |

---

## Workflow przypomnienie (z CLAUDE.md)

1. **Kompilacja:** `cargo check` → przed instalatorem `cargo build --release --workspace`.
2. **Payload:** **MUSISZ** `cp target/release/*.exe dist/installer/payload/` przed Inno Setup.
3. **Wersja:** podbij we wszystkich `Cargo.toml` (angeld, omnidrive-core, angelctl) + `installer/omnidrive.iss`.
4. **Zero-Knowledge:** zero plaintext haseł / kluczy / tokenów w logach. Używaj `[REDACTED]`.
5. **Święta Zasada Integralności:** żadnych operacji zapisu/szyfrowania/przesuwania/usuwania plików poza `SYNC_PATH`. Watcher domyślnie DRY_RUN albo wyłączony podczas pracy nad UI/API.
6. **Token budget:** po każdym mikro-kroku (A.1, A.2, …) pytaj „kontynuujemy czy commit+push?".

---

*Stare pliki planowania zarchiwizowane w `docs/archive/`. Rozproszenie zlikwidowane. Ten plik = jedno źródło prawdy.*
