# OmniDrive - Project Status & Architecture Roadmap v3.3

## Vision

OmniDrive is a fully encrypted, zero-knowledge distributed storage system for personal use. It combines multiple cloud providers into one logical vault and is evolving toward a **Reed-Solomon Erasure Coding (2+1)** architecture to maximize effective capacity while preserving tolerance to the loss of one provider.

Core assumptions:
- the current full-replication model is a **working transitional prototype**
- the target model is **encrypted chunks -> shard sets -> provider objects**
- the EC unit will be a **single encrypted chunk**, not a whole pack
- a logical write is considered successful at **2/3 shards**, resulting in `COMPLETED_DEGRADED`
- full `3/3` coverage means `COMPLETED_HEALTHY`

## DONE

### Core cryptography
- AES-256-GCM chunk encryption is implemented in `omnidrive-core`.
- The local encrypt/decrypt pipeline already works end-to-end.

### Vault master key lifecycle
- Argon2 KDF is implemented.
- `vault_config` stores KDF salt and parameters in SQLite.
- `VaultKeyStore` holds the unlocked key in RAM only.
- `POST /api/unlock` unlocks the vault with a passphrase.

### Local data engine
- `Packer` splits files into chunks and creates local pack data.
- `Downloader` reconstructs files from SQLite metadata and remote providers.
- The read path works end-to-end in the current prototype model.

### SQLite and vault tree model
- The filesystem tree is modeled with `inodes`.
- Chunk, pack, and upload state are persisted in the local database.

### Watcher and background workers
- The watcher runs recursively.
- Debounce and dedup logic collapse repeated filesystem events.
- Upload processing runs in the background.

### Multi-provider integration
- Cloudflare R2, Scaleway, and Backblaze B2 are integrated.
- Per-provider tracking is implemented.
- Retry logic already handles partial provider failures.

### Local API bridge
- A local HTTP API is implemented with `axum`.
- Transfer and provider health endpoints are available.
- The API layer is ready to back the future UI.

### Smart Sync / Files On-Demand
- Windows `Cloud Files API` integration is implemented.
- Sync root bootstrap, placeholder projection, hydration callbacks, range-based reads, and OS writeback are working.
- Pin / unpin, hydration state tracking, CLI commands, API endpoints, and web UI controls are implemented.
- Smart Sync is complete enough to expose the vault as on-demand files in Windows Explorer.

### Epic 19.5: Virtual Drive Mapping
- The Smart Sync sync root is now exposed as a dedicated virtual drive mapped to `O:\`.
- The physical sync root directory is hidden so the user naturally interacts with OmniDrive through the drive letter instead of the backing folder path.
- Startup now mounts the virtual drive after Smart Sync bootstrap, and shutdown unmounts it cleanly.
- Sync root preflight now recreates an empty stale directory in the current user context and verifies write access before CFAPI registration.
- The `O:\` drive now has a custom per-user label (`OmniDrive`) and a dedicated custom icon in Windows Explorer.

### Disaster Recovery
- Live metadata snapshotting is implemented through safe SQLite `VACUUM INTO`.
- Metadata backups are encrypted in the `OMNIDRIVE-META1` format with a dedicated recovery key derived via HKDF.
- Off-site metadata backup upload and tracking are implemented across configured providers.
- A periodic metadata backup worker now runs in the daemon background.
- Recovery bootstrap is implemented through `omnidrive recovery restore`, without requiring a running daemon.
- Recovery visibility is exposed through API, CLI, and the local web UI.

### Epic 21: Deep Data Scrubbing [x] Completed
- `pack_shards` now track verification timestamps, methods, statuses, verified sizes, and verification failure counts.
- A background `ScrubberWorker` performs low-cost `LIGHT` verification via `HEAD` requests and marks shards as `HEALTHY`, `MISSING`, or `SIZE_MISMATCH`.
- Selective `DEEP` verification now downloads shard blobs, computes SHA-256, and detects true binary corruption with `CORRUPTED` status.
- Logical pack state is automatically degraded to `COMPLETED_DEGRADED` or `UNREADABLE` when scrub results invalidate shard health.
- Visibility is exposed through API, CLI, and the dashboard, including audit views of currently problematic shards.

### Epic 22: Intelligent Local Cache & Predictive Prefetching [x] Completed
- A plaintext chunk cache now lives under `%LOCALAPPDATA%\OmniDrive\Cache`, keyed by `revision_id + chunk_index` and backed by SQLite metadata in `cache_entries`.
- `downloader.read_range(...)` is now cache-aware, so cache hits bypass cloud fetches and cache misses write decrypted chunks back through the cache automatically.
- LRU eviction keeps the cache within a fixed byte budget using `last_accessed_at`, `access_count`, and on-disk cache entry cleanup.
- Sequential look-ahead now prefetches upcoming chunks in the background after adjacent reads are detected.
- Small-file warmup proactively caches the rest of files smaller than `8 MiB` after the first chunk read.
- Cache visibility is exposed through API, CLI, and the dashboard, including hit/miss counters and cache usage.

### Epic 23.5: Flexible Storage & Policy Reconciliation [x] Completed
- `packs` now carry an explicit `storage_mode`, with working modes for `EC_2_1`, `SINGLE_REPLICA`, and `LOCAL_ONLY`.
- New writes inherit their storage mode from the effective filesystem policy, so protection levels now affect actual storage behavior instead of only metadata intent.
- `SINGLE_REPLICA` stores a single encrypted shard on the primary provider, while `LOCAL_ONLY` keeps only local metadata and manifest state without remote shard upload.
- `downloader.read_range(...)` now reconstructs data according to the pack mode, using EC decode for `EC_2_1`, direct decrypt for `SINGLE_REPLICA`, and local manifest reads for `LOCAL_ONLY`.
- The background repair/reconciliation flow can now convert active packs between storage modes when a policy changes, then re-point live chunk mappings so old physical variants can be collected by normal GC.

### Epic 24: Secure Local Runtime [x] Completed
- In-memory vault keys are now wrapped in `secrecy`, so master and vault keys are zeroized on drop instead of living as plain arrays in RAM.
- A dedicated `cache_key` is derived from the master key via HKDF using a separate context string, so local cache encryption is cryptographically separated from the main vault key.
- `%LOCALAPPDATA%\\OmniDrive\\Cache` no longer stores plaintext chunks; cache entries are encrypted at rest with `AES-256-GCM` and automatically treated as cache misses if decryption fails.
- Sensitive spool and temporary artifacts now use ephemeral-file handling where possible, and secure cleanup overwrites then removes temporary files during disaster-recovery and upload cleanup paths.
- Runtime directories for cache, spool, download spool, and the SQLite database are now ACL-hardened on Windows for the current user and `SYSTEM`, with `0700` fallback on Unix-like targets.

## CURRENT FOCUS

### Epic 27: Installer and First-Run Bootstrap [x] Completed
Goal:
- turn OmniDrive into an installable desktop product and make installed-mode runtime bootstrap reliable

Delivered:
- per-user installer to `%LOCALAPPDATA%\Programs\OmniDrive`
- unified installed-mode runtime paths under `%LOCALAPPDATA%\OmniDrive`
- autostart and headless daemon bootstrap
- automatic local-vault bootstrap on a fresh machine
- stable `setup/local-only mode` without configured remote providers
- `O:\` mounted as a plain local vault view until real cloud providers are configured
- working diagnostics API on clean-machine installs
- successful restart validation: daemon, autostart, API, and `O:\` survive reboot

Outcome:
- OmniDrive is now installable and operational on a clean Windows machine without manual terminal setup

### Epic 28: Self-Healing Shell Integration [x] Completed
Goal:
- make Windows shell integration self-healing and resilient to system drift, stale registry state, and partial shell failures

Delivered:
- `Task 28.1: Shell State Audit`
  - new diagnostics endpoint for shell state:
    - `/api/diagnostics/shell`
- `Task 28.2: Virtual Drive Self-Heal`
  - duplicate/stale drive mapping cleanup
  - automatic `O:\` remount to the expected target
- `Task 28.3: SyncRoot Self-Heal`
  - new diagnostics endpoint for sync-root state:
    - `/api/diagnostics/sync-root`
  - repair endpoint:
    - `/api/maintenance/repair-sync-root`
  - startup audit/recovery path for cloud-mode `SyncRoot`
- `Task 28.4: Explorer Integration Repair`
  - repair endpoint:
    - `/api/maintenance/repair-shell`
  - restores drive appearance and Explorer context menu entries
- `Task 28.5: Startup Recovery`
  - daemon now performs startup shell/sync-root audit and logs whether recovery actions were needed
- `Task 28.6: Recovery Matrix`
  - local validation passed for audit + repair APIs
  - dedicated `e2e_shell_recovery` harness added for unrestricted desktop sessions
  - `e2e_sync` extended to cover sync-root audit/repair endpoints

Outcome:
- OmniDrive can now audit and repair its Windows shell state and sync-root state locally, with startup recovery hooks and API-triggered repair paths
- Operational validation passed on the second Windows machine with installer `0.1.6`:
  - daemon autostart remained stable after reboot
  - `O:\` stayed browseable as the local vault view
  - `/api/diagnostics/shell` reported a healthy shell state
  - `/api/maintenance/repair-shell` and `/api/maintenance/repair-sync-root` responded successfully
  - no duplicate drive mappings or shell drift remained after validation

### Epic 29: Storage Cost and Policy Dashboard [x] Completed
Goal:
- expose the cost and behavior of storage policies in a product-readable way

Delivered:
- `/api/storage/cost` aggregates:
  - logical bytes
  - physical bytes
  - physical-to-logical ratio
  - per-provider physical usage
  - active pack counts by `storage_mode`
  - estimated monthly cost
  - reconcile backlog
  - orphaned / GC-candidate pack counts
- the dashboard now includes a dedicated `Storage Economics` section
- the UI now shows:
  - logical footprint
  - physical footprint
  - policy efficiency
  - estimated monthly cost
  - bytes avoided
  - provider distribution
  - reconciliation / GC backlog
- acceptance validation passed on the test machine:
  - `/api/storage/cost` returned a correct empty-vault envelope in `local-only`
  - the `Storage Economics` panel rendered correctly
  - values remained stable after restart

Outcome:
- storage policy consequences are now visible and decision-grade instead of hidden in worker internals

### Epic 30: Maintenance Console [x] Completed
Goal:
- expose maintenance and support flows cleanly so repair, scrub, backup, and diagnostics can be triggered without manual registry or filesystem intervention

Implementation summary:

#### Task 30.1: Maintenance API Consolidation
Goal:
- unify maintenance and diagnostics endpoints into a coherent operator-facing API surface

Scope:
- standardize the response format for:
  - health
  - shell
  - sync-root
  - backup-now
  - scrub-now
  - repair-shell
  - repair-sync-root
  - optional repair/reconcile actions
- normalize fields such as:
  - `status`
  - `actions`
  - `warnings`
  - `errors`
  - `timestamp`

Outcome:
- one predictable API contract for all maintenance actions

Current progress:
- `Task 30.1` is implemented.
- A unified maintenance envelope now wraps key diagnostics with:
  - `status`
  - `message`
  - `last_run`
- The following endpoints now expose a consistent operator-facing status model:
  - `/api/diagnostics/health`
  - `/api/diagnostics/shell`
  - `/api/diagnostics/sync-root`
  - `/api/recovery/status`
- A new aggregate maintenance endpoint now exists:
  - `/api/maintenance/status`
- Maintenance actions now also return the same top-level status fields:
  - `/api/maintenance/repair-shell`
  - `/api/maintenance/repair-sync-root`
  - `/api/maintenance/scrub-now`
  - `/api/recovery/backup-now`

#### Task 30.2: Maintenance UI Page
Goal:
- add a dedicated web UI page for maintenance and support operations

Scope:
- add a `Maintenance` screen with sections for:
  - system health
  - shell integration
  - smart sync
  - data integrity
  - backups
- surface the latest action result inline in the UI

Outcome:
- maintenance actions become discoverable and usable without terminal commands

Current progress:
- `Task 30.2` is implemented.
- The local dashboard now includes a dedicated `Maintenance Console` section above the main vault widgets.
- The maintenance console uses a graphitic glassmorphism visual style:
  - deep radial dark background
  - translucent graphite cards
  - soft blur and crisp glass borders
  - monochrome typography with restrained amber/red state accents
- The UI now surfaces live maintenance cards for:
  - system health
  - shell integration
  - sync-root
  - backup readiness
- The first interactive maintenance actions are now available directly from the UI:
  - `Repair Drive`
  - `Repair SyncRoot`
  - `Backup Metadata Now`
  - `Run Light Scrub`
- Action results are shown inline in the maintenance panel instead of requiring terminal inspection

#### Task 30.3: Repair Actions
Goal:
- expose Windows integration repair actions as first-class UI controls

Scope:
- add UI actions for:
  - `Repair Drive`
  - `Repair SyncRoot`
  - `Repair Explorer Integration`
  - optional `Full Shell Repair`
- show what was repaired and whether follow-up action is needed

Outcome:
- shell and drive recovery can be triggered safely from the UI

Current progress:
- `Task 30.3` is implemented.
- The maintenance console now exposes direct UI actions for:
  - `Repair Drive`
  - `Repair SyncRoot`
- Repair actions now return structured maintenance results with:
  - `status`
  - `message`
  - `last_run`
  - action counts / action lists
- Shell and sync-root repair continue to use the existing API-backed self-heal logic, but are now visible and triggerable from the dashboard.

#### Task 30.4: Integrity Actions
Goal:
- expose integrity and reconciliation operations through the same console

Scope:
- add actions for:
  - `Run Scrub Now`
  - `Run Repair Now`
  - `Run Reconcile Now`
- display last run result and current activity state

Outcome:
- integrity workflows no longer require direct API or CLI calls

Current progress:
- `Task 30.4` is implemented.
- The maintenance console now exposes:
  - `Run Light Scrub`
  - `Run Repair Now`
  - `Run Reconcile Now`
- New maintenance endpoints now exist for on-demand integrity operations:
  - `/api/maintenance/repair-now`
  - `/api/maintenance/reconcile-now`
- Manual repair/reconciliation runs now reuse the existing repair worker logic instead of introducing a separate maintenance-only code path.

#### Task 30.5: Backup & Recovery Actions
Goal:
- expose metadata backup and recovery-readiness controls

Scope:
- add:
  - `Backup Metadata Now`
  - last-backup visibility
  - recovery-readiness summary

Outcome:
- backup operations become visible and manually triggerable from the product UI

Current progress:
- `Task 30.5` is implemented.
- The maintenance console now surfaces backup readiness directly from:
  - `/api/recovery/status`
- `Backup Metadata Now` is available as a first-class maintenance action with inline status feedback.

#### Task 30.6: Operator Diagnostics
Goal:
- present the most important operational state in one place

Scope:
- aggregate:
  - worker states
  - queue sizes
  - cache stats
  - last errors
  - shell state
  - sync-root state

Outcome:
- operators can assess daemon health quickly without piecing together multiple endpoints

Current progress:
- `Task 30.6` is implemented.
- A new aggregate diagnostics endpoint now exists:
  - `/api/maintenance/diagnostics`
- The maintenance console now renders a dedicated operator diagnostics area showing:
  - vault health summary
  - pending uploads
  - last upload error
  - cache usage
  - scrub coverage
  - backup readiness
  - worker state grid
- This gives operators one maintenance-focused surface instead of forcing them to manually combine separate health and diagnostics endpoints.

#### Task 30.7: E2E / Acceptance Pass
Goal:
- validate that maintenance actions work end-to-end in the real product flow

Scope:
- verify locally and on the test machine that:
  - actions execute successfully
  - UI status updates reflect reality
  - no maintenance action silently fails

Outcome:
- the Maintenance Console is product-ready, not just internally wired

Current progress:
- `Task 30.7` passed on the test machine.
- Verified in practice:
  - `/api/maintenance/status`
  - `/api/maintenance/diagnostics`
  - `Repair Drive`
  - `Repair SyncRoot`
  - `Backup Metadata Now`
  - `Run Light Scrub`
  - `Run Repair Now`
  - `Run Reconcile Now`
- The maintenance dashboard stayed responsive during the whole pass.
- `O:\` remained browseable and writable.
- Reboot validation passed:
  - daemon came back
  - dashboard loaded
  - maintenance actions remained available
- The only non-blocking polish items discovered were:
  - post-install launch should stay headless
  - remaining legacy dashboard cards should match the new graphite glass style

Outcome:
- OmniDrive now has a working maintenance console for real repair, backup, and diagnostics flows.

### Next Epic
Goal:
- add LAN-aware multi-device acceleration so reads can prefer trusted local peers before cloud downloads

## ROADMAP

### Epic 9: New EC core
Goal:
- implement the new storage model end-to-end for writes

Scope:
- new DB schema:
  - `packs`
  - `pack_shards`
  - optional `repair_jobs`
- new logical-object and physical-shard state model
- refactor the packer to:
  - encrypt each chunk
  - shard the encrypted chunk
- refactor the uploader to:
  - upload shards as separate physical objects
  - retry per shard
  - persist `2/3` as `COMPLETED_DEGRADED`
  - persist `3/3` as `COMPLETED_HEALTHY`
- update health reporting for the new model

Outcome:
- working EC write path with no transitional replication-only rewrite

### Epic 10: EC read path
Goal:
- reconstruct data from any available `2/3` shards

Scope:
- resolve the shard set for requested chunks
- choose available shards
- download at least `2/3`
- decode
- decrypt
- reassemble the file

Outcome:
- full read path in the EC model

### Epic 11: Vault health score and self-healing repair
Goal:
- keep data healthy without manual intervention

Scope:
- shard-set health model
- `DEGRADED` detection
- repair worker
- missing-shard reconstruction from the remaining two shards
- upload of the repaired shard to the missing provider
- transition back to `COMPLETED_HEALTHY`

Outcome:
- automatic repair from `2/3` back to `3/3`

### Epic 12: Garbage collection for shards
Goal:
- safely delete logical and physical data in a shard-aware model

Scope:
- delete pipeline that understands shard sets
- cleanup after deleted files
- cleanup after logical references disappear
- delete physical shards only when no logical object or revision still needs them

Outcome:
- no orphaned shards and predictable space reclamation

### Epic 13: File versioning and immutable snapshots
Goal:
- preserve history and protect against ransomware

Scope:
- `file_revisions`
- revision history attached to each `inode`
- restore of previous versions
- immutable snapshots with retention windows

Outcome:
- safe rollback and overwrite protection

### Epic 14: Chunk-level deduplication
Goal:
- avoid storing the same data repeatedly

Scope:
- fingerprints for encrypted chunks
- detect existing data before shard generation and upload
- create new logical references instead of writing identical data again
- keep dedup aligned with the `chunk -> shard set` model

Outcome:
- lower transfer and storage cost

### Epic 15: Quota management
Goal:
- control cost and effective capacity

Scope:
- per-provider limits
- reserved capacity for in-flight jobs
- monitoring of `logical size` vs `physical size`
- blocking or slowing uploads near quota limits

Outcome:
- predictable cost control

### Epic 16: Multi-folder sync and policy engine
Goal:
- support different rules for different folders and data classes

Scope:
- `sync_policies`
- multiple watcher roots
- rules per path or inode
- policies such as:
  - require `HEALTHY`
  - allow `DEGRADED`
  - snapshot required
  - throttling enabled

Outcome:
- OmniDrive becomes a controllable data engine, not just a daemon

### Epic 17: Upload policies, scheduling, and bandwidth control
Goal:
- control when and how the daemon uses the network

Scope:
- throttling
- night sync windows
- pause and resume
- transfer priorities

Outcome:
- more predictable background behavior

### Epic 18: Expanded API and UI integration
Goal:
- expose the full workflow to the UI

Scope:
- `GET /api/files`
- `POST /api/download`
- API coverage for policies, health, revisions, and repair
- integration with the planned frontend

Outcome:
- the daemon is fully operable through the UI

### Epic 19: Smart Sync / Files On-Demand
Goal:
- expose files locally without requiring a full local copy

Scope:
- placeholder files
- lazy download
- pin and unpin
- cache policy
- integration with Windows `Cloud Files API`

Outcome:
- full cloud-drive behavior at the OS level

### Epic 22: Intelligent Local Cache & Predictive Prefetching
Goal:
- make `O:\` feel as fast and responsive as possible by caching decrypted chunks locally and prefetching predictable reads

Scope:
- local cache directory under `%LOCALAPPDATA%\OmniDrive\Cache`
- cache entries tracked in SQLite
- read-through / write-through cache integrated into `downloader.read_range(...)`
- LRU eviction when the cache exceeds its byte budget
- predictive prefetch for sequential reads and small-file warmup
- API / CLI / UI visibility for cache health and usage

Outcome:
- repeat reads avoid cloud fetches, hydration latency drops, and Smart Sync feels much closer to a native local drive

## EPIC 22 IMPLEMENTATION PLAN

### Objective
- Make `O:\` behave like a fast local disk by introducing a cache layer between Smart Sync hydration and the cloud providers.
- Minimize repeated shard downloads and reduce first-read latency for common access patterns.

### Core design
- Cache at the **chunk** level, not at the whole-file level.
- Start by caching **plaintext chunks** after decrypt, because this gives the fastest repeat reads and fits naturally into `downloader.read_range(...)`.
- Keep cache logic inside `downloader.rs`, not in CFAPI callbacks, so Smart Sync gets cache benefits automatically.

### Cache store layout
- Local cache root:
  - `%LOCALAPPDATA%\OmniDrive\Cache`
- Cache files stored on disk in hash-based subdirectories, for example:
  - `Cache\ab\cd\<cache_key>.bin`
- First cache key recommendation:
  - `revision_id + chunk_index`
- This is simple, deterministic, and safe for versioned files.

### New table: `cache_entries`
- `cache_key TEXT PRIMARY KEY`
- `inode_id INTEGER NOT NULL`
- `revision_id INTEGER NOT NULL`
- `chunk_index INTEGER NOT NULL`
- `pack_id TEXT NOT NULL`
- `file_path TEXT NOT NULL`
- `cache_path TEXT NOT NULL`
- `size INTEGER NOT NULL`
- `created_at INTEGER NOT NULL`
- `last_accessed_at INTEGER NOT NULL`
- `access_count INTEGER NOT NULL DEFAULT 0`
- `is_prefetched INTEGER NOT NULL DEFAULT 0`

### Required DB helpers
- `get_cache_entry(cache_key)`
- `upsert_cache_entry(...)`
- `touch_cache_entry(cache_key)`
- `list_cache_entries_by_lru(limit)`
- `get_total_cache_size()`
- `delete_cache_entry(cache_key)`

### Phase 1: Basic Cache Store
Goal:
- Add a read-through / write-through cache for chunk reads.

Scope:
- new module `angeld/src/cache.rs`
- helpers:
  - `get_cached_chunk(...)`
  - `put_cached_chunk(...)`
- integrate into `downloader.read_range(...)`
- cache lookup happens before cloud fetch
- cache write happens after successful EC reconstruction + decrypt

Deliverable:
- repeated reads of the same chunk no longer hit the cloud providers

### Phase 2: LRU Eviction
Goal:
- keep cache size under a fixed limit such as `50 GB`

Scope:
- new config:
  - `OMNIDRIVE_CACHE_MAX_BYTES=53687091200`
- on cache insert:
  - compute total cache size
  - evict least-recently-used entries by `last_accessed_at` until under budget
- prefer simple LRU first
- avoid evicting entries actively being read if practical

Deliverable:
- bounded cache size with predictable local disk usage

### Phase 3: Hydration Hook Integration
Goal:
- let Smart Sync hydration benefit from the cache automatically

Scope:
- keep CFAPI callback flow unchanged
- `smart_sync.rs` continues to call `downloader.read_range(...)`
- `downloader.read_range(...)` becomes cache-aware:
  - cache hit -> return bytes immediately
  - cache miss -> fetch, decrypt, cache, return

Deliverable:
- `O:\` hydration gets local-cache acceleration without extra CFAPI complexity

### Phase 4: Predictive Prefetching
Goal:
- proactively fetch likely-next chunks to reduce visible latency during sequential access

Scope:
- sequential-read detection:
  - if the user reads chunk `N`, then `N+1`, then `N+2`, prefetch `N+3...`
- first-read warmup for file opens near offset `0`
- small-file optimization:
  - if a file is smaller than a threshold, prefetch the rest after the first read
- run prefetch in low-priority background tasks
- only prefetch chunks not already cached

Deliverable:
- common sequential reads feel smoother and closer to a local disk

### Phase 5: Cache Observability
Goal:
- expose cache usage and effectiveness through operators and UI

Scope:
- API:
  - `GET /api/cache/status`
- CLI:
  - `omnidrive cache status`
  - optional `omnidrive cache clear`
- UI:
  - cache usage
  - hit ratio
  - recent evictions
  - prefetched entries

Deliverable:
- cache behavior is measurable and tunable

### Architectural recommendations
- start with **chunk cache**, not subrange cache
- cache **plaintext chunks**, not encrypted shards
- keep cache logic in `downloader.rs`
- implement simple **LRU** first, then add predictive prefetch
- postpone folder thumbnail / metadata prefetch until the chunk cache is stable

### Minimal first milestone
- `cache_entries` table
- `cache.rs`
- cache lookup / insert in `downloader.read_range(...)`
- `OMNIDRIVE_CACHE_MAX_BYTES`
- LRU eviction

This milestone alone should noticeably improve `O:\` responsiveness and reduce repeated cloud reads.

## EPIC 19 IMPLEMENTATION PLAN

### Objective
- Integrate OmniDrive with the Windows `Cloud Files API` so the vault appears locally as a native filesystem tree while file contents are hydrated only on demand.

### Platform approach
- Target Windows first.
- Use the Windows `Cloud Files API (CFAPI)` through the `windows` or `windows-sys` crate.
- Do **not** build a custom filesystem driver.
- Treat Smart Sync as a projection layer on top of the existing:
  - `db.rs`
  - `downloader.rs`
  - EC shard model
  - revision model

### Core design
- A local `sync root` is registered with Windows as a Cloud Files provider root.
- Every active file in the vault is represented locally as a placeholder entry.
- The placeholder stores file identity metadata:
  - `inode_id`
  - `revision_id`
  - optional policy / pin flags
- When Windows requests file content, OmniDrive resolves the request through:
  - `inode_id`
  - current or pinned `revision_id`
  - EC downloader reconstruction

### Data model additions
- Add table `smart_sync_state`:
  - `inode_id PRIMARY KEY`
  - `revision_id`
  - `placeholder_path`
  - `pin_state`
  - `hydration_state`
  - `last_hydrated_at`
  - `last_error`
- Optional table `sync_root_config`:
  - `root_path`
  - `provider_name`
  - `registered_at`
  - `cfapi_version`

### Phase 1: Sync Root bootstrap
- Create module `angeld/src/smart_sync.rs`.
- Register the selected local folder as a CFAPI sync root.
- Persist sync root configuration in SQLite.
- Add daemon startup wiring so Smart Sync can run beside:
  - watcher
  - uploader
  - repair
  - GC
  - API

Deliverable:
- Windows recognizes the OmniDrive sync directory as a managed cloud root.

### Phase 2: Placeholder projection
- Extend `db.rs` with inventory queries specialized for placeholder generation:
  - active files
  - active directories
  - current revision metadata
  - logical file size
- Project the vault tree into the sync root as placeholders.
- Use `inode_id + revision_id` as the placeholder file identity payload.
- Preserve timestamps and logical size from the current revision.

Deliverable:
- Explorer shows the full vault tree even when file data is not hydrated locally.

### Phase 3: Hydration callback registration
- Register CFAPI callbacks for:
  - fetch data
  - cancel fetch
  - fetch placeholders
- Build a callback dispatcher that converts CFAPI file identity back into:
  - `inode_id`
  - `revision_id`
- Keep callback logic thin; delegate real work to downloader services.

Deliverable:
- OmniDrive is notified when Windows or a user process opens a placeholder file.

### Phase 4: Downloader range API
- Refactor `downloader.rs` to expose range-based reconstruction:
  - `read_range(inode_id, revision_id, offset, length)`
- Reuse existing EC logic:
  - resolve chunk refs for the revision
  - locate shard sets
  - reconstruct missing shard if needed
  - decode ciphertext
  - decrypt plaintext
- Avoid always restoring the full file to disk.

Deliverable:
- Downloader can produce byte ranges on demand for OS hydration requests.

### Phase 5: OS hydration writeback
- Stream reconstructed bytes back into the placeholder through CFAPI.
- Support partial hydration for requested ranges.
- Handle cancellation cleanly if Windows aborts the request.
- Keep hydration incremental rather than forcing a full-file materialization.

Deliverable:
- Double-clicking or opening a placeholder file hydrates it through OmniDrive.

### Phase 6: Pinning and cache policy
- Introduce pin states:
  - `ONLINE_ONLY`
  - `PINNED`
  - `HYDRATED`
- Add local cache management:
  - pin file
  - unpin file
  - evict hydrated content when allowed
- Respect versioning:
  - placeholders point to `current revision` by default
  - restore changes the current revision and future hydrations follow it

Deliverable:
- Users can choose which files remain fully local and which stay on-demand.

### Phase 7: API, CLI, and UI surface
- Add API endpoints:
  - `GET /api/files/:inode_id/status`
  - `POST /api/files/:inode_id/pin`
  - `POST /api/files/:inode_id/unpin`
- Extend CLI:
  - `omnidrive pin <inode_id>`
  - `omnidrive unpin <inode_id>`
- Extend web UI:
  - placeholder / hydrated / pinned badges
  - pin / unpin actions

Deliverable:
- Smart Sync is controllable from API, CLI, and web UI.

### Runtime architecture
- `smart_sync.rs`
  - CFAPI registration
  - placeholder reconciliation
  - hydration callbacks
- `db.rs`
  - file inventory and sync state queries
- `downloader.rs`
  - range-based EC reconstruction
- `api.rs`
  - sync status and pin management
- `omnidrive-cli`
  - pin / unpin commands

### Key technical rules
- Placeholder identity must be stable and map directly to `inode_id` and `revision_id`.
- Smart Sync should default to the `current revision`, not historical revisions.
- Restore stays a metadata operation; hydration follows the newly promoted revision.
- Hydration must be range-capable, not just whole-file restore.
- Cancellation and partial reads are first-class requirements.

### Risks
- CFAPI callbacks are low-level and require strict error handling.
- Range-based reconstruction is a deeper downloader change than simple file restore.
- Concurrent readers may trigger overlapping hydration requests for the same file.
- Placeholder reconciliation must stay consistent with watcher, versioning, and policy engine.

### Recommended rollout order
1. Register sync root and create a single test placeholder.
2. Support hydration for one small full-file test case.
3. Add range hydration for real application access patterns.
4. Add pin / unpin and local cache policy.
5. Add API / CLI / UI controls and diagnostics.

## PHASE 6: OMNIDRIVE ULTIMATE v2

### Epic 20: Disaster Recovery
Goal:
- recover the entire vault after loss of the local machine and local SQLite state

Scope:
- `Metadata Backup Worker`
- periodic snapshots of `omnidrive.db`
- encryption of metadata snapshots with a recovery key or a key derived from the Vault Master Key
- upload of encrypted metadata backups to cloud storage
- recovery manifest
- `Restore from Cloud` flow on first startup on a new machine

Key decisions:
- whether metadata backup is stored on one provider or multiple providers
- snapshot rotation policy
- recovery workflow when no local database exists

Outcome:
- local SQLite is no longer a single point of failure

### Epic 20 Implementation Plan

#### Objective
- eliminate the local SQLite database as a single point of failure for vault metadata
- allow a fresh installation to restore the complete vault structure using only:
  - the Master Password
  - provider credentials
  - cloud-stored encrypted metadata backups

#### Core design
- back up the full SQLite metadata database rather than exporting logical tables
- create a consistent live snapshot while the daemon is running
- encrypt the snapshot before upload using a dedicated recovery key derived from the unlocked vault key
- store the encrypted metadata backup as a reserved system object in cloud storage
- support a clean `Restore from Cloud` bootstrap path on a new machine

#### Snapshot strategy
- use the SQLite Online Backup API as the primary snapshot mechanism
- avoid raw file copying of `omnidrive.db` while WAL activity is in flight
- write the temporary snapshot into a local recovery spool directory first

Why:
- it preserves relational integrity
- it avoids pausing the daemon for a long time
- it is safer than copying the live `.db` file directly

#### Encryption strategy
- do not encrypt the metadata snapshot directly with the generic `vault_key`
- derive a dedicated recovery key, for example:
  - `metadata_backup_key = HKDF(vault_key, "omnidrive-metadata-backup-v1")`
- encrypt the snapshot with AES-256-GCM
- wrap the encrypted file in a versioned backup format with:
  - magic header
  - format version
  - created timestamp
  - db schema version
  - nonce
  - plaintext size
  - ciphertext checksum

Why:
- key separation is cleaner and safer
- backup format versioning makes future migrations manageable

#### Cloud storage strategy
- store metadata backups under a reserved system prefix, for example:
  - `_omnidrive/system/metadata/latest.db.enc`
  - `_omnidrive/system/metadata/manifest.json`
  - `_omnidrive/system/metadata/snapshots/<timestamp>.db.enc`
- prefer storing backups on at least `2` providers, ideally all `3`, because metadata backups are relatively small
- keep a rolling retention window instead of only one latest copy

Why:
- removing the local SPOF should not introduce a single-provider SPOF

#### New module
- create `angeld/src/disaster_recovery.rs`

Responsibilities:
- metadata snapshot creation
- metadata backup encryption and packaging
- cloud upload for recovery artifacts
- manifest generation and validation
- recovery download, decrypt, and restore flow

#### New worker
- create `MetadataBackupWorker`

Responsibilities:
- periodic metadata backup
- backup retry handling
- retention cleanup for old metadata snapshots
- optional idle-aware or change-aware scheduling

#### Proposed DB additions

##### Table: `metadata_backups`
- `backup_id TEXT PRIMARY KEY`
- `created_at INTEGER NOT NULL`
- `snapshot_version INTEGER NOT NULL`
- `object_key TEXT NOT NULL`
- `provider TEXT NOT NULL`
- `encrypted_size INTEGER NOT NULL`
- `plaintext_size INTEGER NOT NULL`
- `checksum TEXT NOT NULL`
- `status TEXT NOT NULL`
- `last_error TEXT NULL`

Purpose:
- local visibility into created backups
- diagnostics and history
- tracking of the latest valid recovery point

##### Table: `metadata_backup_targets` (recommended)
- `backup_id TEXT NOT NULL`
- `provider TEXT NOT NULL`
- `object_key TEXT NOT NULL`
- `status TEXT NOT NULL`
- `attempts INTEGER NOT NULL DEFAULT 0`
- `last_error TEXT NULL`
- `etag TEXT NULL`

Purpose:
- per-provider tracking for metadata backup uploads
- consistency with the rest of the daemon architecture

#### Recovery manifest
- store a small manifest object in cloud containing:
  - `snapshot_version`
  - `created_at`
  - `providers`
  - `object_keys`
  - `checksum`
  - `encryption_scheme`
  - `db_schema_version`
  - optional `backup_id`

Purpose:
- enable a fresh client to discover the newest valid metadata backup without a local database

#### Phase 1: Snapshot engine
- implement a safe live snapshot function
- source: active SQLite database
- output: temporary snapshot file in recovery spool
- prefer SQLite Online Backup API over raw file copy

Deliverable:
- the daemon can generate a consistent metadata snapshot while still running normally

#### Phase 2: Encryption and backup artifact format
- derive `metadata_backup_key`
- encrypt the snapshot using AES-256-GCM
- produce a versioned `.db.enc` artifact
- include metadata needed for safe restore and compatibility checks

Deliverable:
- metadata snapshot becomes a portable encrypted recovery artifact

#### Phase 3: Cloud upload and tracking
- upload the encrypted metadata artifact under `_omnidrive/system/metadata/...`
- create or update local `metadata_backups`
- create or update `metadata_backup_targets`
- add retry behavior and mark failures cleanly

Deliverable:
- encrypted metadata backups are persisted in cloud storage and tracked locally

#### Phase 4: Metadata Backup Worker
- run periodically, for example every 24 hours
- optionally trigger on:
  - daemon idle
  - significant metadata changes
  - explicit backup-now action
- maintain a retention policy, for example last `7` snapshots
- clean up stale remote recovery artifacts

Deliverable:
- metadata backups happen automatically without operator action

#### Phase 5: Restore from Cloud bootstrap
- support a startup mode where no local database exists yet
- read provider config and recovery manifest
- pick the newest valid backup
- download the encrypted metadata snapshot
- derive `metadata_backup_key` from the passphrase lineage
- decrypt and validate the snapshot
- write restored `omnidrive.db`
- continue daemon startup normally

Deliverable:
- a brand-new machine can recover the entire vault structure from cloud metadata backups

#### Phase 6: Operator surface and validation
- add CLI and/or API for:
  - recovery status
  - backup now
  - restore from cloud
- add an end-to-end disaster recovery test flow:
  - create vault state
  - run metadata backup
  - remove local database
  - restore from cloud
  - verify the vault structure and file access still work

Deliverable:
- disaster recovery is not just implemented, but testable and operable

#### Failure cases to design for
- backup upload succeeds on only a subset of providers
- latest cloud backup is corrupted
- schema version in backup does not match daemon expectations
- vault is still locked when a scheduled backup window arrives
- passphrase lineage changes after older backups were created

Expected handling:
- per-provider status tracking
- fallback to older valid backups
- strict schema compatibility checks
- worker waits for unlock before encrypting backups
- recovery format carries enough metadata for safe refusal when incompatible

#### Recommended rollout order
1. safe SQLite snapshot engine
2. backup encryption format and `metadata_backup_key`
3. cloud upload and local tracking
4. periodic `MetadataBackupWorker`
5. restore bootstrap path
6. CLI/API recovery controls
7. end-to-end disaster recovery validation

#### Minimal first milestone
- manual `backup-now`
- safe snapshot
- encrypt locally
- upload to cloud
- manual restore on a clean database path

This first milestone already removes the most important architectural risk: losing the only metadata database.

### Epic 21: Deep Data Scrubbing
Goal:
- detect and repair silent shard corruption in cloud storage

Scope:
- `Scrubbing Worker`
- sampled or scheduled shard verification
- validation of:
  - checksum
  - object size
  - shard-set consistency
- marking corrupted shards as `FAILED`
- automatic trigger of the existing `Repair Worker`

Key decisions:
- sampled scrubbing versus full sweep
- scrub frequency
- whether to prioritize `DEGRADED` packs

Outcome:
- the system detects bitrot, not just missing objects

### Epic 21 Implementation Plan

#### Objective
- detect and repair:
  - missing shards
  - silent data corruption
  - size mismatches
  - shard-set inconsistencies
- degrade logical packs early and let the existing repair path rebuild missing or corrupted physical shards

#### Core design
- scrubbing operates at the **shard level**, not only the pack level
- verification is split into two tiers:
  - `Light verification`
  - `Deep verification`
- the scrubber detects and marks corruption
- the existing `RepairWorker` remains responsible for reconstruction

#### Verification model

##### Light verification
- cheap, continuous checks
- uses `HEAD`
- verifies:
  - object existence
  - `content-length`
  - optional `etag` or provider metadata

If a light check finds:
- `404 / NoSuchKey` -> mark shard as `MISSING`
- wrong size -> mark shard as `SIZE_MISMATCH`

##### Deep verification
- selective full byte download
- compute local SHA-256
- compare with `pack_shards.checksum`

If a deep check finds:
- checksum mismatch -> mark shard as `CORRUPTED`

#### Important operational rule
- do **not** mark shards as corrupted for transient network failures
- distinguish:
  - integrity failures:
    - `404`
    - bad size
    - bad checksum
  - operational failures:
    - timeout
    - DNS
    - TLS
    - `5xx`

Only integrity failures should change shard health state.

#### DB changes

##### Extend `pack_shards`
Add:
- `last_verified_at INTEGER NULL`
- `last_verification_method TEXT NULL`
  - `HEAD`
  - `FULL`
- `last_verification_status TEXT NULL`
  - `HEALTHY`
  - `MISSING`
  - `CORRUPTED`
  - `SIZE_MISMATCH`
  - `CHECKSUM_MISMATCH`
  - `UNKNOWN`
- `last_verified_size INTEGER NULL`
- `last_verified_etag TEXT NULL`
- `verification_failures INTEGER NOT NULL DEFAULT 0`
- optional `last_scrub_error TEXT NULL`

Purpose:
- keep an audit trail
- support scheduling by age
- expose scrub state through API and CLI

##### Optional future table: `scrub_jobs`
Not required for the first iteration.

Possible shape:
- `id`
- `pack_id`
- `shard_index`
- `scheduled_at`
- `started_at`
- `completed_at`
- `status`
- `reason`

Recommendation:
- skip this for the first implementation
- keep the first version worker-driven and query-based

#### Worker design

##### `ScrubberWorker`
- runs continuously in the background
- low priority
- processes a small batch each cycle

##### Scheduling strategy
Prioritize:
1. shards never verified
2. shards with the oldest `last_verified_at`
3. shards with previous verification failures
4. packs in `COMPLETED_DEGRADED`

This balances safety and egress cost.

##### Core query helper
- `get_next_shards_for_scrub(limit)`

Sort order:
- `last_verified_at IS NULL` first
- then oldest verified
- optional bias toward problematic providers or degraded packs

#### Corruption handling
If scrubber detects:
- `MISSING`
- `SIZE_MISMATCH`
- `CHECKSUM_MISMATCH`
- `CORRUPTED`

Then it should:
1. update `pack_shards`
   - `status = FAILED`
   - `last_verification_status = ...`
   - `verification_failures += 1`
2. recompute logical pack status
   - `2/3` remaining -> `COMPLETED_DEGRADED`
   - `<2/3` remaining -> `UNREADABLE`
3. let the existing `RepairWorker` rebuild the shard through EC

#### Provider-aware strategy
Scrubbing policy should be provider-sensitive.

Recommended default:
- `Cloudflare R2`
  - allow more aggressive full verification if egress is acceptable
- `Backblaze B2`
  - mostly `HEAD`
  - periodic full sampling
- `Scaleway`
  - mainly `HEAD`
  - selective full verification

Reason:
- the cost profile differs per provider
- scrub policy should reflect that instead of using one global rule

#### API visibility

##### `GET /api/maintenance/scrub-status`
Return:
- `last_scrub_started_at`
- `last_scrub_completed_at`
- `total_shards`
- `never_verified`
- `healthy_verified`
- `verification_failures`
- `corrupted_or_missing`
- optional per-provider summary

##### Optional `POST /api/maintenance/scrub-now`
- manual trigger for one immediate batch

##### Optional `GET /api/maintenance/scrub-log`
- recent verification events and failures

Recommendation:
- implement `scrub-status` first

#### CLI visibility

##### `omnidrive maintenance scrub-status`
Display:
- last scrub time
- shards never verified
- healthy verified shards
- missing / corrupted shards
- verification failures

##### Optional `omnidrive maintenance scrub-now`
- manual one-shot scrub trigger

#### Recommended phases

##### Phase 1: Schema and tracking
- extend `pack_shards`
- add DB helpers:
  - `get_next_shards_for_scrub(limit)`
  - `mark_shard_verified_head(...)`
  - `mark_shard_verified_full(...)`
  - `mark_shard_missing(...)`
  - `mark_shard_corrupted(...)`

Deliverable:
- verification history exists in the database

##### Phase 2: Light scrubber
- implement `ScrubberWorker`
- `HEAD` verification only
- detect:
  - missing objects
  - size mismatch
- update shard state and logical pack state

Deliverable:
- low-cost continuous integrity monitoring

##### Phase 3: Deep verification
- selective full downloads
- checksum verification
- mark shards as corrupted on mismatch
- rely on the existing repair worker afterward

Deliverable:
- actual bitrot detection, not just presence checks

##### Phase 4: API and CLI visibility
- `GET /api/maintenance/scrub-status`
- `omnidrive maintenance scrub-status`

Deliverable:
- operator can observe scrub coverage and failures

##### Phase 5: Policy tuning
- per-provider scrub rules
- sampling ratios
- scheduling windows
- cost-aware verification tuning

Deliverable:
- scrubber is practical to run continuously without exploding egress costs

#### Architectural recommendations
- reuse the existing `RepairWorker`; do not build a second repair path inside the scrubber
- track verification at shard level, because corruption is a physical-object problem
- use `HEAD` as the default scrub path and full-download verification selectively
- treat network outages as operational failures, not corruption

#### Minimal first milestone
- schema extensions on `pack_shards`
- `ScrubberWorker`
- `HEAD` verification only
- marking `MISSING` and `SIZE_MISMATCH`
- update `packs.status`
- basic `GET /api/maintenance/scrub-status`

This first milestone already delivers real value while keeping egress cost low.

### Epic 22: P2P LAN Cache
Goal:
- avoid unnecessary internet downloads when another machine on the local network already has the required data

Scope:
- peer discovery via `mDNS / Bonjour`
- identification of peers sharing the same `Vault ID`
- downloader checks LAN peers before cloud providers
- direct transfer of encrypted shards or encrypted chunks between devices

Key decisions:
- whether peers expose shards or encrypted chunks
- mutual authentication between peers
- whether LAN peers act only as opportunistic cache or as a full read-path source

Outcome:
- faster local recovery and lower internet bandwidth usage in home networks

### Epic 23: Zero-Knowledge Link Sharing
Goal:
- share files safely without exposing the decryption key to the server

Scope:
- API endpoint for public share token generation
- dedicated web download view
- decryption key stored in the URL fragment
- browser-side workflow:
  - fetch at least `2/3` shards
  - reconstruct data
  - decrypt locally using WebCrypto or WASM

Key decisions:
- token expiry
- token revocation
- maximum shared file size
- whether the browser performs full EC decode or consumes a prepared stream

Outcome:
- private zero-knowledge file sharing

## PHASE 7: HARDENING AND PRODUCTION READINESS

### Epic 24: Secure Local Runtime
Goal:
- close the remaining local security gaps on the user machine

Scope:
- encrypt the local plaintext cache
- audit and reduce plaintext lifetime for spool and temporary artifacts
- secure deletion / cleanup of temporary recovery and snapshot artifacts
- tighten local directory permissions for cache, spool, and sync metadata
- review master-key-adjacent runtime surfaces and sensitive local state
- review what remains on disk after upload, hydration, restore, and reconciliation

Outcome:
- the local machine no longer keeps avoidable plaintext artifacts in normal operation

### Epic 25: Observability and Diagnostics
Goal:
- make the daemon easy to understand and debug in production

Scope:
- structured logs
- log levels and log rotation
- diagnostics surface for:
  - worker liveness
  - queue depth
  - last upload error
  - last repair
  - last scrub
  - last metadata backup
  - cache hit ratio
- optional support bundle export

Outcome:
- failures can be diagnosed without manual SQLite inspection

### Epic 26: End-to-End Test Matrix
Goal:
- validate the full lifecycle with repeatable system-level tests

Scope:
- E2E tests for:
  - upload and download
  - policy changes between `PARANOIA`, `STANDARD`, and `LOCAL`
  - disaster recovery backup and restore
  - Smart Sync hydration
  - cache reuse and eviction
  - GC after pack reconciliation
  - scrubber -> degrade -> repair flow
- provider failure scenarios
- daemon restart during in-flight operations

Outcome:
- release confidence based on real lifecycle coverage, not only smoke tests

Summary:
- Fully validated via E2E: Recovery, Reconciliation, and Self-Healing.

Current progress:
- `Task 26.1` is implemented.
- A dedicated integration harness now exists in `angeld/tests/e2e_basic.rs`.
- The daemon supports `--no-sync` and `OMNIDRIVE_E2E_TEST_MODE=1` so tests can exercise the HTTP API and worker lifecycle without CFAPI bootstrap.
- The first happy-path test launches a fresh daemon, queues a minimal `LOCAL_ONLY` upload job, polls `GET /api/diagnostics/health`, and verifies the uploader transitions `idle -> active -> idle`.
- The baseline E2E test passes under:
  - `cargo test -p angeld --test e2e_basic -- --nocapture`
- `Task 26.3` is implemented.
- A dedicated full-stack SyncRoot harness now exists in `angeld/tests/e2e_sync.rs`.
- The Smart Sync bootstrap now performs a non-destructive CFAPI state probe with `CfGetSyncRootInfoByPath` before attempting registration.
- In E2E mode, the HTTP API starts even when Smart Sync only reports a bootstrap warning, which keeps diagnostics reachable during driver-state investigations.
- The SyncRoot E2E test now passes under:
  - `cargo test -p angeld --test e2e_sync -- --nocapture`
- `Task 26.5` is implemented.
- A dedicated full-stack disaster-recovery harness now exists in `angeld/tests/e2e_recovery.rs`.
- The daemon can now auto-restore the metadata database on startup when the local SQLite file is missing and a recovery passphrase is provided.
- The recovery E2E flow now validates:
  - metadata backup creation
  - deletion of the local SQLite database and cache
  - automatic metadata restore from the provider-backed recovery store
  - full SyncRoot re-registration and recursive placeholder projection after restore
- Smart Sync projection was hardened for recovery by:
  - creating physical parent directories inside the SyncRoot
  - creating file placeholders relative to their immediate parent directory, as expected by `CfCreatePlaceholders`
  - treating modern Cloud Files placeholder attributes such as `UNPINNED` and `RECALL_ON_DATA_ACCESS` as valid placeholder state in the Windows test assertions
- The full-stack DR E2E test now passes under:
  - `cargo test -p angeld --test e2e_recovery -- --nocapture`
- `Task 26.6` is implemented.
- A dedicated reconciliation harness now exists in `angeld/tests/e2e_reconciliation.rs`.
- The reconciliation E2E flow now validates:
  - linear policy transitions `PARANOIA -> STANDARD -> LOCAL -> PARANOIA`
  - continuous 100 ms heartbeat reads during each transition
  - explicit `SWAP start` / `SWAP complete` logging at the live `pack_locations` repoint moment
  - old packs becoming inactive-orphaned GC candidates only after the new pack is ready
- The reconciliation harness uses a local mock S3 provider stack and `--no-sync` full daemon mode so uploader, repair, downloader, and watcher can be exercised deterministically without CFAPI.
- The reconciliation E2E test now passes under:
  - `cargo test -p angeld --test e2e_reconciliation -- --nocapture`
- `Task 26.7` is implemented.
- A dedicated chaos harness now exists in `angeld/tests/e2e_scrubber_repair.rs`.
- The scrubber / repair E2E flow now validates:
  - creation of a healthy `PARANOIA` (`EC_2_1`) pack for a `1 MiB` file
  - physical sabotage by deleting one remote shard from the mock S3 object store
  - scrubber detection of the missing shard and transition of the pack to `COMPLETED_DEGRADED`
  - repair reconstruction and re-upload of the missing shard from the remaining shard set
  - return of the pack to `COMPLETED_HEALTHY` after re-scrub verification
  - continuous 100 ms heartbeat reads that never fail during the full degrade-and-heal cycle
- The repair path now emits explicit degraded-pack lifecycle logs:
  - `repair degraded pack start`
  - `repair degraded pack reconstructing shard`
  - `repair degraded pack complete`
- The scrubber / repair E2E test now passes under:
  - `cargo test -p angeld --test e2e_scrubber_repair -- --nocapture`
- Next recommended step:
  - `Epic 27: Installer and First-Run Bootstrap`

## PHASE 8: EXPLORER RELIABILITY AND OPERATIONS

### Epic 27: Installer and First-Run Bootstrap
Goal:
- turn OmniDrive into a normal installable desktop application

Scope:
- Windows installer
- runtime directory setup
- daemon autostart
- `O:\` bootstrap
- shell registration
- first-run wizard for:
  - vault creation
  - unlock
  - provider credential setup
  - optional restore from cloud

Outcome:
- the user can install and start OmniDrive without terminal setup

Implementation plan:

#### Task 27.1: Packaging and Installer Baseline
Goal:
- produce a Windows installer that lays down all required OmniDrive binaries and runtime assets

Scope:
- choose installer technology:
  - WiX
  - Inno Setup
  - MSIX
- package:
  - `angeld`
  - `omnidrive-cli`
  - static web assets
  - required icons and shell assets
- define installation directories under `Program Files` and per-user runtime directories under `%LOCALAPPDATA%\OmniDrive`
- create uninstall entry and clean upgrade-safe layout

Deliverable:
- a repeatable installer that places OmniDrive on a clean Windows machine without manual file copying

Current progress:
- `Task 27.1` is implemented.
- OmniDrive now ships with an Inno Setup installer that packages:
  - `angeld`
  - `omnidrive-cli`
  - static web assets
  - shell and icon assets
- The installer model was refined to a per-user architecture:
  - install root: `%LOCALAPPDATA%\Programs\OmniDrive`
  - runtime root: `%LOCALAPPDATA%\OmniDrive`
- The installer now:
  - registers uninstall metadata
  - supports optional user PATH integration through `HKCU\Environment`
  - avoids the earlier admin/per-user mismatch warning by using `PrivilegesRequired=lowest`
- A repeatable installer build pipeline now exists through:
  - `installer/omnidrive.iss`
  - `scripts/build-installer.ps1`
- Current packaged output:
  - `dist\installer\output\OmniDrive-Setup-0.1.0.exe`

#### Task 27.2: Runtime Bootstrap and First-Run State
Goal:
- ensure the installed app can initialize its local runtime safely on first launch

Scope:
- detect runtime mode:
  - development / workspace mode
  - installed mode under `Program Files`
- in installed mode, use `%LOCALAPPDATA%\OmniDrive` as the base path for:
  - SQLite DB
  - cache directory
  - spool directory
  - download spool
  - logs directory
- create and validate all required runtime directories on startup if they are missing
- ensure the base runtime directory and its sensitive children are ACL-hardened through `win_acl.rs`
- ensure installed-mode logging is redirected to `%LOCALAPPDATA%\OmniDrive\logs`
- initialize local configuration if missing
- add a first-run marker / bootstrap state so the daemon and UI know whether onboarding is still required

Deliverable:
- installed OmniDrive can bootstrap its runtime directories and local DB state without terminal setup

Implementation plan for Task 27.2:

1. Runtime mode detection
- add a small environment-resolution layer in `main.rs` / config bootstrap
- determine whether OmniDrive is running from:
  - a developer worktree
  - an installed location under `Program Files`
- prefer explicit env overrides first, then installed-mode defaults

2. Unified runtime path resolver
- introduce one source of truth for:
  - DB path
  - logs path
  - cache path
  - spool path
  - download spool path
- in installed mode, all of them resolve under `%LOCALAPPDATA%\OmniDrive`
- keep current env overrides working for tests and development

3. Startup directory bootstrap
- before DB init and worker startup:
  - create `%LOCALAPPDATA%\OmniDrive`
  - create `logs`
  - create `Cache`
  - create `Spool`
  - create `download-spool`
- ensure bootstrap is idempotent and safe to run on every start

4. ACL hardening for installed mode
- call `secure_directory(...)` on the runtime base directory and sensitive subdirectories
- treat ACL failure as a startup error in installed mode
- keep debug/development guardrails intact so local development is not bricked again

5. Installed-mode logging
- route default logs to `%LOCALAPPDATA%\OmniDrive\logs`
- keep stdout available in development mode
- confirm the daemon remains diagnosable when started headlessly by the installer/autostart path

6. First-run marker
- persist a simple local marker indicating:
  - runtime bootstrapped
  - onboarding still required or already completed
- this marker will feed `Task 27.4`

Exit criteria:
- starting `angeld` from an installed layout without env vars creates and uses `%LOCALAPPDATA%\OmniDrive`
- DB, cache, spool, and logs all resolve there automatically
- the runtime directories are ACL-hardened
- logs are written to the file logger path in installed mode
- development mode and E2E harnesses still work without regression

Current progress:
- `Task 27.2` is implemented.
- A shared runtime path resolver now detects:
  - workspace mode
  - installed mode under `Program Files`
- In installed mode, daemon and CLI now resolve their runtime paths from `%LOCALAPPDATA%\OmniDrive`.
- The daemon now bootstraps:
  - runtime base directory
  - DB directory
  - cache
  - spool
  - download spool
  - logs
- Installed-mode runtime directories are ACL-hardened through `win_acl.rs`.
- Logging now uses the shared runtime path resolver and writes rotating `angeld.log*` files under the installed runtime log directory.
- The CLI recovery path now resolves the same SQLite location as the daemon by default.
- Clean-machine hotfix:
  - installed-mode startup no longer fails when no remote providers are configured
  - OmniDrive now enters a `setup/local-only` mode and keeps the daemon, API, and runtime bootstrap alive
  - runtime ACLs now explicitly grant full access to the current user and `SYSTEM`
  - installed-mode logs stay readable while `angeld` is running
  - a best-effort panic hook now flushes logging output before process exit

#### Task 27.3: Daemon Autostart and Process Lifecycle
Goal:
- make `angeld` behave like a real background desktop daemon

Scope:
- register daemon autostart for the current user
- define start / stop / restart behavior during:
  - install
  - update
  - uninstall
- ensure logs and diagnostics survive background execution
- verify the daemon can start headlessly without stdout dependence

Deliverable:
- OmniDrive starts automatically after install and after user logon, with stable lifecycle management

Current progress:
- `Task 27.3` is implemented.
- The daemon now exposes current-user autostart registry helpers in `angeld/src/autostart.rs`.
- OmniDrive registers itself under:
  - `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
  - value name: `OmniDriveAngeld`
- Installed-mode autostart now prefers a headless launcher script:
  - `angeld-autostart.vbs`
  - launched through `wscript.exe //B`
- The installer now:
  - installs the headless launcher script
  - registers the autostart entry during installation
  - removes the autostart entry during uninstall
- The CLI now provides a manual fallback for lifecycle control:
  - `omnidrive service register`
  - `omnidrive service unregister`
- Verification:
  - `cargo check --workspace`

#### Task 27.4: First-Run Wizard
Goal:
- guide the user through vault creation or restore on a clean machine

Scope:
- first-run UI flow for:
  - create new vault
  - unlock existing vault
  - set master passphrase
  - configure provider credentials
  - choose sync root location if needed
  - optional metadata restore from cloud
- clear success / failure states and retry UX
- persist configuration only after validation succeeds

Deliverable:
- a non-technical user can reach a usable OmniDrive state without touching CLI commands

Current progress:
- `Task 27.4` is partially implemented through daemon-side auto-bootstrap.
- On a clean install with no local SQLite database, `angeld` now:
  - initializes default local-vault metadata
  - creates the installed runtime directory layout under `%LOCALAPPDATA%\OmniDrive`
  - bootstraps a default local watch root under the current user profile
- Clean-machine hotfix:
  - a fresh install with no configured providers now boots into a usable local-only setup mode instead of exiting
  - the default watch-root policy is initialized as `LOCAL`, so the first machine can come up before cloud credentials are added
  - CFAPI callback paths are now guarded against panics in local-only setup mode
  - Smart Sync now treats `0` projected files as an explicit no-op instead of entering any placeholder path assumptions
- This gives OmniDrive a usable empty local vault immediately after installation, even before the dedicated first-run UI is added.
- Remaining scope for the future UI layer:
  - explicit create / unlock / restore screens
  - passphrase entry and validation UX
  - provider credential onboarding UX

#### Task 27.5: SyncRoot and `O:\` Bootstrap
Goal:
- make installed OmniDrive bring up the actual Windows integration automatically

Scope:
- register or reuse Smart Sync `SyncRoot`
- bootstrap placeholder projection after onboarding or restore
- map the virtual drive to `O:\`
- apply:
  - drive label
  - drive icon
  - shell menu registration
- verify startup ordering:
  - unlock
  - DB ready
  - SyncRoot ready
  - `O:\` mount

Deliverable:
- after first-run completion, the user sees a working `O:\` drive and Explorer integration automatically

Current progress:
- `Task 27.5` is implemented.
- On normal daemon startup, OmniDrive now:
  - automatically registers or reuses the Smart Sync `SyncRoot`
  - refreshes placeholder projection through the existing recursive projection path
  - mounts the virtual drive to `O:\` or the first available drive letter if `O:` is already occupied
  - applies the existing drive label, drive icon, and Explorer shell integration
- Clean-machine hotfix:
  - Smart Sync projection and virtual-drive mount now continue to initialize even when no remote providers are active yet
  - this keeps the installed experience centered on a visible working `O:\` drive while onboarding is still incomplete
  - SyncRoot ACLs now explicitly preserve full access for `SYSTEM`, which the Windows Cloud Files driver relies on
  - Smart Sync now flushes logs after major registration and projection milestones for easier crash forensics
- This means an installed OmniDrive instance can self-initialize into a working Windows-integrated vault without manual terminal setup.

#### Task 27.6: Clean-Machine Validation Matrix
Goal:
- prove the installer flow works on a fresh environment, not just in dev worktrees

Scope:
- validate on a clean Windows user profile:
  - fresh install
  - first vault creation
  - provider credential setup
  - restore path
  - reboot / relogon autostart
  - uninstall / reinstall
- verify:
  - daemon starts
  - API responds
  - `SyncRoot` works
  - `O:\` appears
  - logs are written

Deliverable:
- a reproducible install / first-run / reboot validation checklist for release readiness

#### Recommended execution order
1. `Task 27.1: Packaging and Installer Baseline`
2. `Task 27.2: Runtime Bootstrap and First-Run State`
3. `Task 27.3: Daemon Autostart and Process Lifecycle`
4. `Task 27.4: First-Run Wizard`
5. `Task 27.5: SyncRoot and O:\ Bootstrap`
6. `Task 27.6: Clean-Machine Validation Matrix`

#### Suggested first milestone
- installer places binaries
- runtime directories are created
- daemon autostarts
- basic first-run screen appears
- user can create or restore a vault

This milestone is enough to move OmniDrive from a developer-operated system to an installable desktop product.

### Epic 28: Self-Healing Shell Integration
Goal:
- make Windows shell integration resilient

Scope:
- re-registration and repair for:
  - sync root
  - `O:\`
  - shell menu
  - drive icon and label
- recovery from stale shell, drive, Explorer, or CFAPI state
- resilience after upgrades and Windows drift

Outcome:
- Explorer and Smart Sync integration survive upgrades and shell restarts reliably

### Epic 29: Storage Cost and Policy Dashboard
Goal:
- expose the cost and behavior of storage policies in a product-readable way

Scope:
- usage per provider
- logical vs physical storage
- estimated savings from:
  - deduplication
  - cache
  - `SINGLE_REPLICA`
- distribution of files across:
  - `PARANOIA`
  - `STANDARD`
  - `LOCAL`
- reconciliation and cleanup visibility

Outcome:
- policy decisions become explainable in terms of storage and cost

Implementation plan:

#### Task 29.1: Storage Cost API
Goal:
- aggregate storage usage, policy mix, and cost-relevant telemetry into one operator-friendly API surface

Scope:
- add an API endpoint that reports:
  - logical bytes
  - physical bytes
  - per-provider physical usage
  - active pack counts by `storage_mode`
  - estimated savings from local-only and single-replica modes
  - reconcile backlog and orphaned-pack / GC-candidate counts

Outcome:
- one API response describes the current storage shape and cost posture of the vault

Current progress:
- `Task 29.1` is implemented.
- A dedicated storage dashboard endpoint now exists:
  - `/api/storage/cost`
- The API now aggregates:
  - logical bytes
  - physical bytes
  - physical-to-logical ratio
  - per-provider physical usage
  - active pack counts by `storage_mode`
  - estimated provider bytes avoided by `STANDARD` and `LOCAL`
  - reconciliation backlog
  - orphaned / GC-candidate pack counts
- Cost estimation is now driven by configurable per-provider or default `cost-per-GiB-per-month` rates from environment-backed config.

#### Task 29.2: Policy Dashboard UI
Goal:
- visualize storage mode distribution and policy consequences clearly in the dashboard

Scope:
- add cards and charts for:
  - `PARANOIA`
  - `STANDARD`
  - `LOCAL`
- show logical vs physical footprint
- show provider distribution and active policy mix

Outcome:
- the user can see how storage policy choices translate into actual storage behavior

Current progress:
- `Task 29.2` is implemented.
- The dashboard now includes a dedicated `Storage Economics` section.
- It surfaces:
  - logical footprint
  - physical footprint
  - policy efficiency ratio
  - estimated monthly cost
- It also visualizes active pack distribution by:
  - `PARANOIA`
  - `STANDARD`
  - `LOCAL`
- Provider distribution is now shown with both footprint and estimated monthly cost.

#### Task 29.3: Reconciliation and GC Visibility
Goal:
- make policy transitions and cleanup debt visible

Scope:
- show:
  - packs waiting for reconciliation
  - packs already reconciled but left as orphaned physical variants
  - GC-ready candidates
- expose these counts through both API and UI

Outcome:
- storage drift and cleanup backlog become measurable

Current progress:
- `Task 29.3` is implemented.
- The storage dashboard now exposes:
  - reconcile backlog packs
  - orphaned packs
  - orphaned physical bytes
  - GC-candidate pack count
- This makes storage drift and cleanup debt visible without reading raw worker logs.

#### Task 29.4: Estimated Cost Model
Goal:
- present a simple but useful estimate of ongoing storage cost

Scope:
- derive rough monthly usage estimates from:
  - per-provider physical usage
  - current storage-mode mix
- keep the first model simple and transparent
- present cost as guidance, not as billing-grade truth

Outcome:
- the user can compare the expected cost impact of `PARANOIA`, `STANDARD`, and `LOCAL`

Current progress:
- `Task 29.4` is implemented.
- OmniDrive now computes a lightweight estimated monthly cost model from:
  - per-provider physical usage
  - configurable provider rates
  - current storage-mode mix
- The first model is intentionally simple and transparent, intended for operator guidance rather than billing-grade accounting.

#### Task 29.5: Acceptance Pass
Goal:
- validate that the dashboard numbers match the actual storage model closely enough to trust operationally

Scope:
- verify on real data that:
  - totals are internally consistent
  - provider usage matches known state
  - policy mix reflects actual packs
  - reconciliation and GC counters are believable

Outcome:
- the storage dashboard becomes decision-grade rather than decorative

### Epic 30: Maintenance Console
Goal:
- centralize all maintenance actions for the user and operator

Scope:
- `Backup now`
- `Scrub now`
- `Repair now`
- `Evict cache`
- `Reconcile now`
- `Rebuild sync root`
- `Re-register O:\`

Outcome:
- maintenance workflows become accessible without manual command sequences

## PHASE 10: MULTI-DEVICE AND NETWORK INTELLIGENCE

### Epic 31: P2P LAN Cache
Goal:
- avoid unnecessary internet downloads when another trusted local device already has the data

Scope:
- peer discovery
- mutual authentication
- transfer of encrypted chunks or shards over LAN
- downloader preference for LAN cache before cloud fetch

Outcome:
- lower egress costs and faster reads on home or office networks

### Epic 32: Sync Conflict Handling
Goal:
- handle concurrent updates across multiple devices safely

Scope:
- conflict detection
- conflict naming / conflict copies
- revision ordering rules
- optional lock / lease semantics where needed

Outcome:
- multi-device usage no longer risks silent overwrite ambiguity

## PHASE 11: SHARING AND IDENTITY

### Epic 33: Zero-Knowledge Link Sharing
Goal:
- let users share files safely without exposing decryption keys to the server

Scope:
- share token generation
- dedicated download page
- URL-fragment key delivery
- browser-side decrypt / decode

Outcome:
- private zero-knowledge file sharing

### Epic 34: Secure Authentication and Google Login
Goal:
- add account-based identity only when OmniDrive grows beyond a purely local daemon model

Scope:
- secure authentication model
- Google Login / OAuth
- session lifecycle
- device association
- installer onboarding integration

Outcome:
- account-backed identity for hosted or multi-user product scenarios

## NEXT RECOMMENDED ORDER

1. `Epic 24: Secure Local Runtime`
2. `Epic 25: Observability and Diagnostics`
3. `Epic 26: End-to-End Test Matrix`
4. `Epic 27: Installer and First-Run Bootstrap`
5. `Epic 28: Self-Healing Shell Integration`
6. `Epic 29: Storage Cost and Policy Dashboard`
7. `Epic 30: Maintenance Console`
8. `Epic 31: P2P LAN Cache`
9. `Epic 32: Sync Conflict Handling`
10. `Epic 33: Zero-Knowledge Link Sharing`
11. `Epic 34: Secure Authentication and Google Login`

## WHY THIS ORDER

- It closes local security before adding more product surface.
- It improves diagnostics and testability before broader rollout.
- It makes installation and Explorer reliability more urgent than sharing or identity features.
- It keeps Google Login as a later product-layer decision, not a premature infrastructure commitment.

## PHASE 6 RECOMMENDED ORDER

1. `Epic 20: Disaster Recovery`
2. `Epic 21: Deep Data Scrubbing`
3. `Epic 22: P2P LAN Cache`
4. `Epic 23: Zero-Knowledge Link Sharing`

## WHY THIS PHASE 6 STRUCTURE IS BETTER

- It does not mix four different systems into one oversized epic.
- It closes resilience and integrity before adding network convenience and sharing features.
- It is easier to implement, test, and track on the roadmap.
- It keeps `Smart Sync` as the immediate next delivery while making the post-Smart-Sync direction explicit.

## Architectural Notes

### Current replication is a reference prototype
- The current system already solved provider integration, vault unlock, background workers, API bridging, downloader structure, and retry behavior.
- It is not throwaway work. It is the reference implementation we will evolve away from.

### The EC unit is the encrypted chunk
- This keeps memory usage low.
- This preserves streaming write and read behavior.
- This makes retry and repair easier.
- This keeps failure domains small.

### Pack becomes a logical layer, not the EC durability unit
- After the EC transition, durability depends on shard sets for encrypted chunks.
- Packs remain useful as a metadata or grouping layer.

### `2/3` only works with repair lifecycle
- If `2/3` is treated as logical success, the system must also support:
  - `COMPLETED_DEGRADED`
  - health reporting
  - repair worker
  - diagnostics and observability

### No transitional dead code
- We will not build a temporary phase with new tables but the old write model.
- We will close the ADR, introduce the new schema, and move directly into the EC core.
