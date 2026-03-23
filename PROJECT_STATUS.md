# OmniDrive - Project Status & Architecture Roadmap v3

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

### Disaster Recovery
- Live metadata snapshotting is implemented through safe SQLite `VACUUM INTO`.
- Metadata backups are encrypted in the `OMNIDRIVE-META1` format with a dedicated recovery key derived via HKDF.
- Off-site metadata backup upload and tracking are implemented across configured providers.
- A periodic metadata backup worker now runs in the daemon background.
- Recovery bootstrap is implemented through `omnidrive recovery restore`, without requiring a running daemon.
- Recovery visibility is exposed through API, CLI, and the local web UI.

## CURRENT FOCUS

### Epic 21: Deep Data Scrubbing
Goal:
- detect and repair silent shard corruption in cloud storage

Scope:
- sampled or scheduled shard verification
- checksum validation
- object size validation
- shard-set consistency checks
- triggering the existing repair flow for corrupted shards

Outcome:
- the system detects bitrot, truncation, and silent corruption instead of only missing objects

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
