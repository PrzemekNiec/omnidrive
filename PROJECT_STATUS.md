# OmniDrive - Project Status & Roadmap

## Vision

OmniDrive is a local-first, zero-knowledge Windows storage product that combines:
- one logical vault
- on-demand access in Explorer
- multiple cloud backends
- resilient recovery, repair, and maintenance tooling

Current product direction:
- stable desktop install on Windows
- local-only onboarding first
- cloud capabilities enabled progressively after provider configuration
- operator-grade diagnostics and maintenance built into the product UI

## Current Product State

What works today:
- per-user Windows installer
- automatic runtime bootstrap under `%LOCALAPPDATA%\OmniDrive`
- stable daemon autostart
- local-only first-run mode without configured providers
- `O:\` mounted as the main user-facing vault view
- Smart Sync / SyncRoot support when cloud mode is active
- encrypted metadata backup and disaster recovery
- scrub, repair, reconciliation, and shell self-heal flows
- maintenance dashboard and storage/cost dashboard

Known product posture:
- the product is now strong as a single-device Windows desktop vault
- the biggest remaining gaps are multi-device intelligence, conflict handling, sharing, and identity

## Completed Epics

### Epic 19.5: Virtual Drive Mapping [x] Completed
- OmniDrive vault is exposed through `O:\`
- custom drive label and icon support exist
- the product now has a clear Explorer-facing entry point

### Epic 20: Disaster Recovery [x] Completed
- encrypted metadata backups are created and tracked
- restore flow works without requiring a running daemon
- recovery status is exposed through API, CLI, and UI

### Epic 21: Deep Data Scrubbing [x] Completed
- background scrubber verifies shard health
- light and deep verification modes exist
- degraded and unreadable states are detected correctly

### Epic 22: Intelligent Local Cache & Predictive Prefetching [x] Completed
- encrypted local cache exists under `%LOCALAPPDATA%\OmniDrive\Cache`
- downloader uses cache-aware reads
- LRU eviction and predictive prefetching are implemented

### Epic 23.5: Flexible Storage & Policy Reconciliation [x] Completed
- `EC_2_1`, `SINGLE_REPLICA`, and `LOCAL_ONLY` storage modes are implemented
- read path understands each mode
- reconciliation can migrate active data between storage modes

### Epic 24: Secure Local Runtime [x] Completed
- key material is better protected in memory
- cache encryption is separated from the main vault key
- sensitive runtime directories are ACL-hardened

### Epic 26: End-to-End Test Matrix [x] Completed
- recovery, reconciliation, and self-healing are validated by E2E tests
- critical storage and repair flows are no longer only unit-tested in isolation

### Epic 27: Installer and First-Run Bootstrap [x] Completed
- per-user installer is working
- installed-mode path resolution is working
- clean-machine bootstrap is stable
- local-only onboarding works without remote providers
- reboot validation passed

### Epic 28: Self-Healing Shell Integration [x] Completed
- shell state audit exists
- shell repair and sync-root repair exist
- startup recovery exists
- second-machine validation passed

### Epic 29: Storage Cost and Policy Dashboard [x] Completed
- `/api/storage/cost` exists
- storage economics are visible in the dashboard
- policy mix, provider distribution, reconcile backlog, and GC debt are visible
- acceptance passed on the test machine

### Epic 30: Maintenance Console [x] Completed
- maintenance actions are available in the dashboard
- diagnostics are aggregated in one operator-facing view
- repair, scrub, backup, and reconciliation are triggerable from UI
- acceptance passed on the test machine

## Open Epics

### Epic 31 + Epic 32: Multi-Device Core
Goal:
- make OmniDrive safely multi-device by combining LAN-aware reads with conflict-safe revision handling

Current implementation status:
- durable local `device_id` is now persisted in SQLite
- trusted peer records are now persisted in SQLite
- LAN peer discovery and handshake service are implemented in the daemon
- downloader can now attempt peer-first chunk reads before cloud fallback
- peer selection now applies basic policy heuristics:
  - trusted peers only
  - stale-peer rejection
  - short error backoff
  - health scoring visible in diagnostics
- file revisions now carry lineage metadata:
  - `device_id`
  - `parent_revision_id`
  - `origin`
  - `conflict_reason`
- conflict-copy materialization exists in the database/API layer
- watcher write path can now materialize an automatic conflict copy when the DB head changed during a local edit
- revision application now distinguishes:
  - fast-forward lineage promotion
  - restore rewind
  - true parallel heads
- restore and local write flows only materialize conflict copies when lineage actually diverged or rewound
- multi-device status is exposed through:
  - `/api/multidevice/status`
  - dashboard `Multi-Device Core` panel

What is still open inside the epic:
- automatic conflict detection during true concurrent multi-device writes
- broader acceptance of the new winner/conflict rules across real multi-device scenarios
- full acceptance pass across two active devices in one network

Implementation plan:

#### Task 31.1: Device Identity and Trust
Goal:
- give each installation a durable device identity and a basis for trusted peer relationships

Scope:
- persistent `device_id`
- device key / identity metadata
- local device descriptor
- trusted peer model

Outcome:
- OmniDrive can distinguish devices reliably

#### Task 31.2: Peer Discovery
Goal:
- detect trusted peers in the local network

Scope:
- LAN discovery
- peer announcement
- handshake and identity verification

Outcome:
- nearby OmniDrive nodes can find each other safely

#### Task 31.3: Peer Read Path
Goal:
- prefer a trusted LAN peer before cloud for reads

Scope:
- downloader asks peer for chunk or shard first
- fallback to cloud if peer is unavailable
- read-only peer path in the first version

Outcome:
- lower egress cost and faster local-network reads

#### Task 31.4: Peer Cache Policy
Goal:
- make LAN peer usage predictable and safe

Scope:
- retry rules
- timeout rules
- peer health scoring
- source preference policy:
  - LAN
  - local cache
  - cloud

Outcome:
- peer-assisted reads behave predictably instead of opportunistically

#### Task 32.1: Revision Lineage
Goal:
- track the origin and parentage of file revisions across devices

Scope:
- `device_id` on revisions
- parent revision tracking
- timestamp and lineage metadata

Outcome:
- OmniDrive can tell linear updates from true conflicts

#### Task 32.2: Conflict Detection
Goal:
- detect when two devices create competing updates

Scope:
- identify parallel revision heads
- distinguish safe linear updates from conflicts

Outcome:
- multi-device writes stop being ambiguous

#### Task 32.3: Conflict Materialization
Goal:
- preserve both versions instead of silently overwriting one

Scope:
- conflict-copy naming
- inode/materialization strategy
- user-visible conflict files

Outcome:
- no silent overwrite on concurrent edits

#### Task 32.4: Multi-Device Policy Rules
Goal:
- define clear winner/conflict rules for revision application

Scope:
- linear lineage update rules
- competing-head conflict rules
- no aggressive content auto-merge

Outcome:
- revision behavior stays understandable and safe

#### Task 32.5: Multi-Device Diagnostics
Goal:
- expose peer and conflict state through API and UI

Scope:
- known devices
- peer health
- LAN read activity
- conflict counters
- last sync activity

Outcome:
- operators can see whether multi-device behavior is healthy

#### Task 31/32.6: Acceptance Pass
Goal:
- prove that LAN reads and conflict-safe writes work across real devices

Scope:
- two devices on one network
- LAN-assisted read path
- concurrent edit conflict scenario
- conflict copy verification

Outcome:
- OmniDrive becomes operationally credible as a multi-device system

### Epic 31/32 Bridge: Onboarding, Provider Setup & Join Existing Vault
Goal:
- replace manual `.env` setup with a real onboarding flow that can attach a second machine to the same vault without breaking the existing local-only experience

Why this bridge epic exists:
- the current product is strong in single-device local-only mode
- the current multi-device core is real code, but it cannot be validated honestly until two machines can join the same vault through a supported product flow
- this bridge epic exists to make `Epic 31 + Epic 32` testable in production-like conditions before moving to sharing and hosted identity

Constraints:
- do not remove or block the current local-only first run
- do not gate `O:\`, diagnostics, or maintenance behind wizard completion
- do not duplicate device identity storage already implemented for the multi-device core
- provider secrets must be stored securely, not as plain-text config rows

#### Task B0: Cloud Safety, Cost Control & Dry-Run Guardrails
Goal:
- introduce hard operational guardrails so onboarding and provider flows cannot silently generate runaway cloud costs

Scope:
- daily quota circuit breaker for read/write/egress operations
- single-file upload size guard
- stronger watcher debounce for bursty local edits
- startup and post-onboarding cleanup of stale multipart uploads
- `--dry-run` runtime mode with explicit API/UI visibility
- storage dashboard integration for session cloud counters and quota utilization

Outcome:
- cloud operations fail closed when limits are exceeded, with visible diagnostics instead of silent cost drift

#### Task B1: Onboarding State Persistence
Goal:
- give OmniDrive a durable application-level onboarding state

Scope:
- add `system_config` for:
  - `onboarding_state`
  - `onboarding_mode`
  - `last_onboarding_step`
  - `draft_env_detected`
  - `cloud_enabled`
- add `provider_configs` for non-secret provider metadata
- add a secure secrets layer for provider credentials

Outcome:
- onboarding and provider setup stop depending on ad-hoc environment files

#### Task B2: Safe Draft Import From `.env`
Goal:
- support existing developer/tester setups without making `.env` the product configuration model

Scope:
- detect `.env` only when onboarding is incomplete
- import found values as a draft
- expose draft presence to the onboarding API/UI
- never require `.env` for normal product usage

Outcome:
- older setups migrate cleanly into the productized config model

#### Task B3: Onboarding API
Goal:
- expose a real API for onboarding and provider setup

Scope:
- `GET /api/onboarding/status`
- `POST /api/onboarding/bootstrap-local`
- `POST /api/onboarding/setup-identity`
- `POST /api/onboarding/setup-provider`
- `POST /api/onboarding/join-existing`
- `POST /api/onboarding/complete`

Outcome:
- onboarding becomes an explicit product flow instead of manual configuration

#### Task B4: Provider Connection Validation
Goal:
- make provider setup real and trustworthy

Scope:
- test auth
- test bucket access
- test read/list
- optional small write/delete probe
- return provider-specific validation results and errors

Outcome:
- configured providers are actually usable, not just saved

Status:
- implemented in backend
- `POST /api/onboarding/setup-provider` now persists config, validates endpoint/auth/list/put/delete, stores `last_test_status`, `last_test_error`, `last_test_at`, and returns a structured validation report
- onboarding status API still exposes only `SET/MISSING` for secrets and never returns ciphertext/plaintext

#### Task B5: First-Run Wizard UI
Goal:
- add a full-screen glassmorphism wizard for first run and provider onboarding

Scope:
- steps:
  - Welcome
  - Choose Mode
  - Identity
  - Providers
  - Security
  - Finalize
- supported modes:
  - `Create New Local Vault`
  - `Connect Cloud Providers`
  - `Join Existing Vault`

Outcome:
- the user can configure OmniDrive without touching `.env`

Status:
- implemented as a full-screen glass overlay that appears whenever onboarding is not `COMPLETED`
- includes:
  - Welcome
  - Choose Mode
  - Identity
  - Providers
  - Security
  - Finalize
- `.env` draft detection is surfaced as an in-product banner and can prefill non-secret provider fields
- provider validation errors from `B4` are rendered directly on the provider card with readable detail
- back navigation preserves session state without persisting secrets
- `Join Existing Vault` now posts to the real restore endpoint and keeps the passphrase only in browser memory until the restore call completes

#### Task B6: Join Existing Vault Flow
Goal:
- allow a second computer to join the same vault through the product UI/API

Scope:
- configure shared providers
- accept passphrase
- restore metadata
- verify matching `vault_id`
- rehydrate local state for the joined device

Outcome:
- two computers can legitimately operate against the same vault

Status:
- implemented in backend and wired into the wizard
- `POST /api/onboarding/join-existing` now:
  - downloads the encrypted metadata snapshot from the selected provider
  - decrypts it locally with the supplied passphrase
  - grafts the remote `vault_id` into local SQLite while preserving local `device_id`
  - applies restored inode and revision metadata atomically through SQLite transaction boundaries
- successful restore now logs:
  - `[RESTORE] Vault ID grafted successfully: {id}`
- after restore, runtime performs immediate sync-root activation for the joined device:
  - repairs or reconnects the sync root
  - projects placeholder structure into the sync root
  - remounts `O:\` to the restored sync-root view
- join failures now return UI-readable JSON with:
  - `IncorrectPassphrase`
  - `MetadataNotFound`
  - `NetworkError`
  - `human_readable_reason`
- important boundary kept explicit:
  - `B6` restores the shared vault identity and placeholder view honestly
  - runtime provider hot-reload and worker activation behavior is handled in `B7`

#### Task B7: Runtime Integration Without Regressing Local-Only Mode
Goal:
- integrate onboarding with the daemon without breaking current bootstrap behavior

Scope:
- keep `O:\` and local-only mode available before onboarding completion
- gate only cloud-specific or join-specific actions when not configured
- reload or restart provider-backed workers after onboarding changes

Outcome:
- onboarding extends the product instead of regressing the stable local-first flow

#### Task B8: Production Bring-Up and Multi-Device Acceptance
Goal:
- connect the real providers and validate the first honest multi-device scenario

Scope:
- configure Cloudflare R2, Backblaze B2, and Scaleway
- create or restore one shared vault
- attach second machine to that vault
- rerun the `Epic 31 + Epic 32` acceptance pass with real data paths

Outcome:
- OmniDrive becomes production-testable across real devices and real providers

B8 execution pack (added for repeatable real-world validation):
- `scripts/b8-lenovo-cloud-enable.ps1`
  - enables/tests configured providers via onboarding API on the primary machine (Lenovo)
  - finalizes onboarding and writes JSON evidence to `.omnidrive/b8-lenovo-report-*.json`
- `scripts/b8-dell-join-existing.ps1`
  - configures provider on the secondary machine (Dell), executes join-existing restore, and writes `.omnidrive/b8-dell-report-*.json`
- `scripts/b8-acceptance-check.ps1`
  - runs a strict API-based acceptance gate and fails fast when core invariants are broken
  - writes `.omnidrive/b8-acceptance-*.json`

Latest B8 progress snapshot (2026-04-04):
- Lenovo (primary) is validated as the source vault for join-existing tests:
  - onboarding: `COMPLETED` + `CLOUD_ENABLED`
  - provider validation: `backblaze-b2` succeeds
  - metadata backup: successful uploads recorded (`metadata_backups` has `COMPLETED` on B2)
- Dell (secondary) initially failed join-existing with:
  - `SnapshotApply`: restored snapshot missing `vault_state`
  - later `SnapshotApply`: `database restored is locked`
- root-cause fixes implemented after those failures:
  - `vault::unlock` now guarantees local `vault_state` bootstrap for legacy DB states
  - metadata backup now requires successful `latest.db.enc` update to mark attempt `COMPLETED`
  - restore now falls back from `latest.db.enc` to recent snapshot keys and validates `vault_state` before apply
  - DB graft now uses an isolated apply-copy of staging DB + `PRAGMA busy_timeout` to reduce attach/apply lock failures
- installer rebuilt with all fixes as:
  - `dist/installer/output/OmniDrive-Setup-0.1.11.exe`
- Dell retest discovered additional root cause:
  - stale `omnidrive.db` survived cleanup because `Remove-Item` silently failed on locked files
  - wizard never appeared because daemon read old DB with `onboarding_state=COMPLETED`
  - user never got to choose "Join Existing Vault" — onboarding was already finalized
- fixes implemented (second round):
  - `cleanup_stale_restore_staging()` runs at daemon startup to remove leftover `restore-staging-*.db` files
  - `POST /api/onboarding/reset` endpoint added as a safety valve to force wizard reappearance
  - `scripts/b8-dell-clean-reset.ps1` created with retry loop, individual file cleanup, and verification
- installer rebuilt again with all second-round fixes
- Dell retest (v0.1.12) discovered: `database restored is locked` from ATTACH DATABASE on Windows
  - fix: eliminated ATTACH DATABASE entirely, rewrote `graft_restored_metadata_snapshot()` to use a separate read-only pool
  - lock error resolved
- Dell retest (v0.1.12 post-lock-fix) discovered: type mismatches in `Restored*` structs
  - error: `mismatched types; Rust type alloc::string::String (as SQL type TEXT) is not compatible with SQL type INTEGER` on `mtime` column
  - root cause: 11 `Restored*` struct definitions used `String` for fields that are actually `i64` or `Option<i64>` in SQLite
  - fix (v0.1.13): aligned all `Restored*` struct types to match actual `*Record` definitions and SQLite column types
  - affected fields: `mtime`, `mode`, `created_at`, `immutable_until`, `origin`, `pin_state`, `hydration_state`, `backup_id`, `plaintext_hash`, `checksum`, `attempts`, `last_verified_at`
  - Inno Setup `AppVersion` updated to read from Cargo.toml version, installer now produces `OmniDrive-Setup-{version}.exe`
- installer rebuilt as `OmniDrive-Setup-0.1.13.exe` — Dell retest in progress

Current bridge implementation status:
- `B1` completed:
  - `system_config`
  - `provider_configs`
  - `provider_secrets`
  - Windows DPAPI-based sealing for provider secrets
- `B2` completed in backend foundation form:
  - `.env` drafts are detected at startup
  - drafts are imported into SQLite as non-authoritative onboarding data
  - draft presence is tracked in `system_config`
- `B3` backend API is now implemented:
  - `GET /api/onboarding/status`
  - `POST /api/onboarding/bootstrap-local`
  - `POST /api/onboarding/setup-identity`
  - `POST /api/onboarding/setup-provider`
  - `POST /api/onboarding/complete`
- `B4` completed:
  - provider validation now performs reachability, auth, list, put, and delete probes
  - validation state is persisted in SQLite
- `B5` completed:
  - first-run wizard is live in the dashboard shell
  - draft `.env` import, identity setup, provider setup, and finalize flow are present
- `B6` completed at bridge level:
  - join-existing metadata restore is real
  - restored vault identity is grafted locally
  - placeholders are projected immediately after restore
- `B0` implemented (guardrail layer):
  - new runtime cloud guard module and DB `cloud_usage_daily` ledger
  - configurable daily read/write/egress limits with automatic suspension on quota breach
  - 100 MB single-file upload cap enforced in uploader path
  - watcher debounce hardened to minimum 2s to reduce write churn bursts
  - stale multipart upload cleanup executed at daemon startup and onboarding completion
  - `--dry-run` now propagates to runtime cloud guard state (no cloud side effects)
  - `/api/storage/cost` expanded with guard/session/quota telemetry
  - Storage Economics UI now shows:
    - dry-run warning banner
    - cloud guard status/message
    - session cloud operation counters
    - daily quota utilization bars
- `B7` completed:
  - daemon runtime now loads active providers from SQLite/DPAPI (SSoT), not from `.env`, for uploader/downloader/repair/scrubber/gc/metadata backup paths
  - `UploadWorker` now runs in local-only mode with empty providers and supports runtime hot-reload via in-process signal
  - `Downloader` now supports live provider reload from DB without daemon restart
  - `POST /api/onboarding/complete` and `POST /api/onboarding/join-existing` trigger runtime reload and post-onboarding reconciliation
  - startup now logs provider source as DB-backed (`Active providers loaded from DB: [...]`)
  - local-only stability is preserved while enabling cloud activation in-place after onboarding finalization
- security rule locked in for future work:
  - onboarding status API never returns provider secrets or ciphertexts
  - it returns only secret presence state such as `SET` / `MISSING`

### Phase 0: Cryptographic Checkpoint
Goal:
- produce a 1-2 page decision document that defines the single source of truth for the entire key hierarchy before any Envelope Encryption code is written

Required decisions:
- Algorithms: AES-256-GCM (DEK), X25519/ECDH P-256 (asymmetric), Argon2id (KDF)
- KDF parameters: Argon2id iterations, memory, parallelism — balance security vs. unlock time on weaker machines
- DEK wrapping format: AES-256-KW vs AES-256-GCM-SIV
- WebCrypto compatibility: if Epic 33 uses browser WebCrypto API, algorithm choices must be compatible with `window.crypto.subtle`
- Versioning strategy: `vault_format_version` scheme and forward-compatibility path

Outcome:
- ad-hoc crypto decisions during implementation are eliminated

### Epic 32.5: Cryptographic Foundation (Envelope Encryption)
Goal:
- replace the flat encryption model (Master Passphrase -> chunks) with a three-level envelope model, required before Epic 33 (sharing) and Epic 34 (multi-user vaults)

#### Task 32.5.1: Key Hierarchy (Envelope Encryption)
Goal:
- introduce DEK -> Vault Key -> KDF three-level hierarchy

Scope:
- DEK (Data Encryption Key): random AES-256 per file, encrypts data chunks
- Vault Key: master vault key, wraps ONLY DEK keys (key wrapping)
- Master Passphrase -> Argon2id -> unlocks Vault Key locally
- Wrapped DEK stored in SQLite alongside file metadata

Outcome:
- Vault Key rotation requires only re-wrapping small DEK keys, NOT re-encrypting terabytes of cloud chunks

Risk: HIGH

#### Task 32.5.2: Database Format Migration (vault_format_version)
Goal:
- safe upgrade path from current metadata format to Envelope Encryption schema

Scope:
- add `vault_format_version` field to SQLite
- migrator: decrypt chunks with old key -> generate DEK -> re-encrypt -> update metadata
- resumable migration: if machine loses power mid-migration, daemon resumes from last checkpoint
- rollback path: ability to revert to old format if migration fails

Outcome:
- zero data loss on format version change; mixed old/new format is impossible

Risk: HIGH

### Epic 35: The Ghost Shell (Native Explorer Experience)
Goal:
- integrate with Windows shell (`cfapi.dll`) so the user can right-click any file, choose a protection level, and the daemon "ingests" the file: encrypts, chunks (EC), uploads to clouds, and leaves a ghost (0-byte placeholder)

#### Task 35.0: cfapi.dll Minimal PoC (risk isolation)
Goal:
- verify Rust FFI bindings to Windows Cloud Files API BEFORE adding any cloud logic; highest technical risk in the entire roadmap

Scope:
- purely local mechanism: file -> placeholder -> hydration from hidden Cache folder
- validate progressive streaming via `CfExecute` with `CF_OPERATION_TYPE_TRANSFER_DATA`
- test interaction with Windows Defender (Mark of the Web on hydrated files)
- architecture: Shell Extension DLL as thin client (named pipe / localhost HTTP to `angeld`); crash in DLL = crash Explorer.exe, so minimum logic in DLL

Outcome:
- Go/No-Go gate: if PoC is unstable, Epic 35 requires alternate strategy (e.g. ProjFS)

Risk: HIGH

#### Task 35.1: Ingest State Machine + Erasure Coding
Goal:
- transactional file "ingestion" resilient to failures with full lifecycle

Scope:
- states: `PENDING -> CHUNKING -> UPLOADING -> GHOSTED` (+ `HYDRATING` + `FAILED` with diagnostics)
- atomic replacement of original with ghost ONLY after full confirmation of ALL chunks; error = rollback
- graceful degradation: if 2 of 3 providers unavailable, fast timeout + "unavailable" icon in Explorer
- HYDRATING: retry logic, timeout, partial-failure handling when restoring ghost from cloud

Outcome:
- full transactional file lifecycle: from raw file to ghost and back, with zero data loss possibility

Risk: HIGH

#### Task 35.2: Context Menu + Shell Extension (4 Protection Levels)
Goal:
- register Windows shell extension with context menu for 4 protection levels

Scope:
- menu: LOKALNIE (`LOCAL_ONLY`), COMBO (`SINGLE_REPLICA`), CHMURA (Sharding), FORTECA (`EC_2_1`)
- overlay icons in Explorer for each state (synced, uploading, ghost, error)
- thin client architecture: DLL does minimum — only sends commands to `angeld` daemon
- offline handling: clicking ghost without network -> "unavailable" icon, not Explorer hang

Outcome:
- user sees and controls protection policy for every file directly from Explorer

Risk: MEDIUM

### Epic 33: Zero-Knowledge Link Sharing
Goal:
- allow private file sharing without exposing keys to the server; key is part of the URI fragment (`#`) and never reaches the server

#### Task 33.1: Fragment-Based Cryptography
Goal:
- architecture for links based on DEK in URI fragment

Scope:
- format: `https://share.omnidrive.app/{file_id}#{DEK_key}`
- URI fragment (`#`) is ignored by HTTP servers — key stays local
- design decision: per-file DEK (one key per file regardless of EC chunk count); documented for potential future per-chunk DEK change
- optional TTL (link lifetime) and one-time links (burn-after-read)

Outcome:
- link that can be safely sent by email — even if someone intercepts the URL, the server has no key

Risk: MEDIUM

#### Task 33.2: Export API + Web Receiver
Goal:
- frontend for decrypting files in recipient's browser (WebCrypto API)

Scope:
- JavaScript in recipient's browser uses WebCrypto API and DEK key from URL to decrypt
- streaming: `ReadableStream` + `TransformStream` for progressive decryption of large files (RAM limit)
- size limit: explicit cap or chunked download with progressive decryption for files >500 MB
- UX: decryption progress bar + "Save as..." button

Outcome:
- recipient without OmniDrive account can download and decrypt file entirely in browser

Risk: MEDIUM

### Epic 34: The Family Cloud (Shared Vaults & Identity)
Goal:
- transition from purely local app to a product with identity while maintaining full separation of authentication identity (OAuth) from cryptographic identity (X25519); OmniDrive server distributes encrypted blobs — never sees keys

#### Task 34.1: OAuth2 Identity Layer
Goal:
- authenticate users via Google OAuth, completely independent from cryptographic key derivation

Scope:
- Google Login = "this is the user", NOT "this is their key"
- session and JWT management at daemon dashboard level
- X25519 private key is generated locally and derived from user's own password — NOT from Google token
- Google account takeover does NOT grant access to vault data

Outcome:
- user logs in conveniently, but cryptographic keys are fully independent from identity provider

Risk: MEDIUM

#### Task 34.2: Asymmetric Key Wrapping (Zero-Knowledge Handoff)
Goal:
- securely pass vault access between users without breaking Zero-Knowledge principle

Scope:
- each user/device generates X25519 key pair (or ECDH P-256 for WebCrypto compatibility)
- key wrapping: `HKDF(ECDH(sender_priv, recipient_pub)) -> AES-256-KW(Vault_Key)`
- OmniDrive server stores and distributes ONLY encrypted key blobs
- Vault Key is never transmitted in plaintext

Outcome:
- server is a "blind intermediary" — passes envelopes but has no access to contents

Risk: HIGH

#### Task 34.3: ACL, Revocation & Recovery
Goal:
- permissions system plus secure procedure for removing access and recovery on key loss

Scope:
- invitations: owner generates wrapped Vault Key for new member
- revocation: user removal -> automatic Vault Key rotation -> re-wrap ONLY DEK keys; data chunks unchanged
- recovery: Shamir's Secret Sharing or paper recovery keys (e.g. 24-word BIP-39) — optional but clearly communicated in onboarding
- UX: clear message "save this key, nobody can help you" when creating vault

Outcome:
- full access lifecycle: invitation -> usage -> removal -> emergency recovery

Risk: HIGH

## Recommended Execution Order

**Epics 32.5 -> 35 -> 33 -> 34**

| Phase | Epic/Task | Scope | Estimate |
|-------|-----------|-------|----------|
| Phase 0 | Checkpoint 0 | Cryptographic specification document | 1 week |
| Phase 1 | Epic 32.5 | Envelope Encryption + DB format migration | 2-3 weeks |
| Phase 2a | Task 35.0 | cfapi.dll PoC — isolated local test | 1-2 weeks |
| Phase 2b | Tasks 35.1-35.2 | Full Ghost Shell: Ingest, EC, Context Menu, Overlays | 4-6 weeks |
| Phase 3 | Epic 33 | Zero-Knowledge Link Sharing | 3-4 weeks |
| Phase 4 | Epic 34 | Family Cloud: OAuth2, Key Wrapping, ACL, Recovery | 4-6 weeks |

Why this order:
- Envelope Encryption (32.5) is the cryptographic foundation required by ALL subsequent epics
- Ghost Shell (35) depends on Envelope Encryption for per-file DEK handling
- Zero-Knowledge Sharing (33) depends on DEK architecture from 32.5 for fragment URI links
- Family Cloud (34) depends on both Envelope Encryption and sharing infrastructure
- changing this order risks cryptography refactoring under pressure

## Session Continuation Notes

Current saved progress for `Epic 31 + Epic 32`:
- `cargo check --workspace` is green after the first multi-device integration pass
- implemented and wired:
  - persistent local device identity
  - trusted peer registry in SQLite
  - LAN peer discovery and handshake
  - peer-first downloader read path with cloud fallback
  - peer eligibility heuristics with stale rejection, backoff, and health scoring
  - revision lineage fields on `file_revisions`
  - conflict-copy materialization in DB and API
  - lineage-aware winner/conflict rules for restore and local-write flows
  - `/api/multidevice/status`
  - dashboard `Multi-Device Core` panel
- files touched in the current pass:
  - `angeld/src/device_identity.rs`
  - `angeld/src/peer.rs`
  - `angeld/src/db.rs`
  - `angeld/src/api.rs`
  - `angeld/src/main.rs`
  - `angeld/src/downloader.rs`
  - `angeld/src/diagnostics.rs`
  - `angeld/src/config.rs`
  - `angeld/src/lib.rs`
  - `angeld/static/index.html`
  - `angeld/Cargo.toml`

Next execution plan:
1. complete B8 retest on Dell using `OmniDrive-Setup-0.1.13.exe`:
   - run `scripts/b8-dell-clean-reset.ps1` on Dell — verify all green
   - restart Dell, install fresh 0.1.13
   - run wizard path: `Join Existing Vault` on `backblaze-b2`
   - if wizard does not appear, use `POST http://127.0.0.1:8787/api/onboarding/reset` then reload
   - verify join result is true join mode, not fallback local-only cloud-enabled mode
2. acceptance criteria for B8 pass:
   - Dell `onboarding_mode` = `JOIN_EXISTING`
   - Dell `multidevice.vault_id` is not local default (`local-vault-*`)
   - Dell diagnostics shell/sync-root stay healthy in cloud mode after finalize
3. once B8 is green, move to new roadmap sequence:
   - Phase 0: Cryptographic Checkpoint (decision document)
   - Phase 1: Epic 32.5 (Envelope Encryption)
   - Phase 2: Epic 35 (Ghost Shell)
   - Phase 3: Epic 33 (Zero-Knowledge Link Sharing)
   - Phase 4: Epic 34 (Family Cloud)

Working rule for future sessions on this project:
- always use `jcodemunch` at the beginning of the session for repo context, symbol lookup, and code navigation before making implementation decisions

## Historical / Superseded Planning

The old roadmap sections for earlier epics `9-19`, duplicate implementation plans, and older phased planning were intentionally removed from this file.

Reason:
- they are now either already reflected in shipped architecture
- or they are no longer the active execution plan
- keeping them here created noise, duplication, and a misleading picture of what is still actually left to build

If needed, the old detailed plans still exist in git history.
