# PROJECT STATUS

## COMPLETED
- Epic 1: Crypto baseline in `omnidrive-core` is implemented with Argon2id root key derivation, HKDF-based key expansion, HMAC-SHA256 chunk IDs, and AES-256-GCM chunk encryption/decryption.
- Epic 2: SQLite state store in `angeld` is implemented with schema initialization, vault state persistence, inode creation and lookup, recursive path resolution, chunk-to-pack mapping, and upload queue handoff.

## CURRENT STATE
- `angeld/src/db.rs` initializes the SQLite schema and exposes helpers for `vault_state`, `inodes`, `chunk_refs`, `pack_locations`, and `upload_jobs`.
- The DB layer currently supports vault parameter set/get, inode insertion, single-directory lookup, recursive path resolution, chunk registration, pack linkage, ordered chunk retrieval for a file, queueing a pack for upload, and claiming the next pending upload job by marking it `IN_PROGRESS`.

## NEXT STEPS
- Epic 3: Implement the uploader layer for S3-compatible object storage, including multipart upload flow and scheduler/worker integration.
- Epic 4: Build out the client-facing control surface, including `angelctl` CLI commands and the daemon interaction layer.

## NOTES
- `cargo check --workspace` is currently green for `omnidrive-core`, `angeld`, and `angelctl`.
- `jcodemunch-mcp` was temporarily unavailable in the last run, so targeted local reads were used instead of MCP symbol tools.
