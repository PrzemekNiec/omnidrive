# PROJECT STATUS

## DONE
- Epic 1: Core encryption in `omnidrive-core` is implemented with Argon2id root key derivation, HKDF-based key expansion, HMAC-SHA256 chunk IDs, and AES-256-GCM chunk encryption/decryption.
- Epic 2: SQLite inode-based state store in `angeld` is implemented with schema initialization, vault state persistence, inode creation and lookup, recursive path resolution, chunk-to-pack mapping, and upload queue handoff.
- Epic 3: Local Packaging Engine and Upload Worker are implemented. `angeld` reads local files, splits them into 4 MiB chunks, encrypts them into local packs, queues uploads, and uploads packs to S3-compatible providers.
- Epic 4: File Watcher is implemented with recursive directory monitoring, inode tree updates, debounce, and deduplication before packaging.
- Epic 4.5: Resilient Uploader is implemented with per-provider tracking via `upload_job_targets`, allowing R2 failures or transient backend errors without blocking successful uploads to Scaleway or Backblaze B2.

## IN PROGRESS
- Epic 5: API / Bridge Layer. `angeld` now exposes a local HTTP server with `GET /api/transfers` and `GET /api/health`, and this layer is being expanded into the frontend bridge surface.

## TO DO
- Epic 6: Read Path. Implement downloader, pack reader, chunk reconstruction, and decryptor flow for restoring file content from remote providers.
- Epic 7: Vault Master Key management. Replace the current development key fallback with proper persistent vault key lifecycle and recovery-safe key handling.
- Epic 8: Garbage Collection. Implement cleanup and reconciliation for deleted files, stale packs, and cloud-side object retention across providers.

## CURRENT STATE
- `angeld/src/db.rs` manages SQLite schema, inode tree state, chunk references, pack locations, upload jobs, and per-provider upload target tracking.
- `angeld/src/packer.rs` builds encrypted local packs from watched filesystem changes using 4 MiB chunks by default.
- `angeld/src/watcher.rs` monitors the configured watch directory recursively and collapses duplicate OS events before packaging.
- `angeld/src/uploader.rs` uploads pending packs to Cloudflare R2, Scaleway, and Backblaze B2 with retries and per-provider persistence.
- `angeld/src/api.rs` exposes local JSON endpoints for transfer state and provider health.

## NOTES
- `cargo check --workspace` is currently green.
- GitHub CLI board sync could not be executed from this environment because outbound GitHub API access for `gh` is blocked, so the local status tracker was updated instead.
