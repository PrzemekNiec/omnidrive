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
- `Join Existing Vault` is staged honestly in the UI, but final metadata restore activation still belongs to `B6`

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
- `B3` backend API is now partially implemented:
  - `GET /api/onboarding/status`
  - `POST /api/onboarding/bootstrap-local`
  - `POST /api/onboarding/setup-identity`
  - `POST /api/onboarding/setup-provider`
  - `POST /api/onboarding/complete`
- security rule locked in for future work:
  - onboarding status API never returns provider secrets or ciphertexts
  - it returns only secret presence state such as `SET` / `MISSING`

### Epic 33: Zero-Knowledge Link Sharing
Goal:
- allow private file sharing without exposing keys to the server

Scope:
- share token generation
- dedicated download page
- URL-fragment key delivery
- browser-side decrypt / decode

Outcome:
- private zero-knowledge sharing

### Epic 34: Secure Authentication and Google Login
Goal:
- add account-backed identity only if OmniDrive grows beyond the current local-daemon product model

Scope:
- secure auth model
- Google Login / OAuth
- session lifecycle
- device association
- onboarding integration

Outcome:
- hosted or multi-user identity layer when product direction requires it

## Recommended Order

1. `Epic 31 + Epic 32: Multi-Device Core`
2. `Epic 31/32 Bridge: Onboarding, Provider Setup & Join Existing Vault`
3. `Epic 33: Zero-Knowledge Link Sharing`
4. `Epic 34: Secure Authentication and Google Login`

Why this order:
- `Epic 31` and `Epic 32` are strongest when delivered together as one multi-device foundation
- the bridge epic is required to validate that foundation honestly on real providers and shared vaults
- `Epic 33` expands product value after the multi-device model is safer
- `Epic 34` is the most optional and should only happen when account-backed product direction is confirmed

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
1. implement the bridge epic:
   - provider connection validation
   - wizard UI
   - join existing vault
2. connect the three real providers:
   - Cloudflare R2
   - Backblaze B2
   - Scaleway
3. use the bridge flow to attach the second machine to the same vault
4. rerun the real acceptance pass for `Epic 31 + Epic 32`

Working rule for future sessions on this project:
- always use `jcodemunch` at the beginning of the session for repo context, symbol lookup, and code navigation before making implementation decisions

## Historical / Superseded Planning

The old roadmap sections for earlier epics `9-19`, duplicate implementation plans, and older phased planning were intentionally removed from this file.

Reason:
- they are now either already reflected in shipped architecture
- or they are no longer the active execution plan
- keeping them here created noise, duplication, and a misleading picture of what is still actually left to build

If needed, the old detailed plans still exist in git history.
