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

## CURRENT FOCUS

### Epic 8: ADR and target storage model
Goal:
- formally close the architectural decisions for the EC model

Scope:
- lock in **single encrypted chunk** as the EC unit
- define the shard model:
  - `2 data shards`
  - `1 parity shard`
- define status semantics:
  - `COMPLETED_HEALTHY` = `3/3`
  - `COMPLETED_DEGRADED` = `2/3`
  - `UNREADABLE` = `<2/3`
- define repair lifecycle behavior
- define the relationship:
  - `inode/revision -> encrypted chunk`
  - `encrypted chunk -> shard set`
  - `shard set -> provider objects`
- define identifiers and metadata:
  - `chunk_id`
  - `shard_id`
  - checksums and reconstruction metadata
- define the migration plan from the current prototype model

Outcome:
- one technical design that becomes the implementation baseline

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
