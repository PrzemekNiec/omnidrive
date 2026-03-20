# OmniDrive Project Status

## DONE

### Epic 1: Core Encryption
- AES-256-GCM encryption pipeline implemented in `omnidrive-core`.
- Deterministic chunk encryption and hashing flow integrated with the daemon.

### Epic 2: SQLite Inodes Schema
- Local state store implemented in SQLite.
- Vault tree modeled through `inodes` with `parent_id` and `name`.
- Chunk, pack, and upload tracking persisted in the daemon database.

### Epic 3: Packer and Uploader Worker
- Local packaging engine implemented with configurable 4 MiB chunking.
- Encrypted `.odpk` pack generation integrated with SQLite chunk registration.
- Background uploader worker implemented for Cloudflare R2, Scaleway, and Backblaze B2.

### Epic 4: File Watcher
- Recursive local file watcher implemented with `notify`.
- New and modified files are registered in SQLite and packed automatically.
- Debounce and deduplication logic prevents duplicate packaging from repeated OS events.

### Epic 4.5: Resilient Uploader
- Per-provider upload tracking implemented through `upload_job_targets`.
- Partial provider failures no longer block successful uploads to other backends.
- Retry logic handles transient provider errors such as Cloudflare R2 `502` responses.

### Epic 5: API / Bridge Layer
- Local HTTP API server implemented with `axum`.
- `GET /api/transfers` exposes transfer state with per-provider progress.
- `GET /api/health` exposes provider connection status for the frontend bridge.

### Epic 6: Read Path (Downloader & Decryptor)
- File reconstruction flow implemented from SQLite chunk metadata.
- Downloader retrieves encrypted packs from completed provider targets.
- Latency-based provider selection uses `HEAD` probes to choose the fastest source.
- Decrypt and reassembly path restores original files to the requested output path.

### Epic 7: Vault Master Key Management
- Secure Argon2-based key derivation implemented for the Vault Master Key.
- `vault_config` stores the KDF salt and configuration parameters in SQLite.
- In-memory `VaultKeyStore` manages the unlocked key lifecycle.
- `POST /api/unlock` allows the daemon vault to be unlocked with a passphrase.
- Packer and Downloader now require the vault to be unlocked before processing files.

## CURRENT FOCUS

### Epic 8: Garbage Collection & Deletes
- Monitor local file deletions through the watcher pipeline.
- Remove deleted file state from `inodes` and `chunk_refs` in SQLite.
- Dispatch delete operations to Cloudflare R2, Scaleway, and Backblaze B2.
- Prevent orphaned chunks and stale packs across local and remote storage.

## UPCOMING ROADMAP

### Epic 9: Expanded API Bridge
- Add `GET /api/files` to expose the vault tree to the frontend.
- Add `POST /api/download` to trigger the downloader from the UI.
- Continue shaping the daemon API around frontend consumption patterns.

### Epic 10: E2E Integration Testing
- Build full dry-run test coverage through the API layer.
- Validate flows such as unlock, upload, simulated provider failure, download, and delete.
- Use the integration suite to harden daemon behavior before UI rollout.

### Epic 11: Frontend UI Implementation
- Build the desktop and mobile interface from the approved Google Stitch fintech designs.
- Connect the UI to the local `axum` API bridge.
- Surface vault state, transfers, health, and download actions through the frontend.
