# α.C.a — Real X25519 Keypair Generation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `[0u8;32]` placeholder public keys in the `devices` table with real per-device X25519 keypairs, generated on the first unlock (and immediately on join-existing), so `devices.public_key ≠ [0;32]` and `validate_x25519_pubkey` accepts it.

**Architecture:** Lazy generation. The startup placeholder stays (the `devices` row must exist before unlock for session creation, and `master_key` is unavailable while locked). The real keypair is produced when `master_key` becomes available: (1) on first unlock, folded into the existing post-unlock background task *after* the α.B.a KDF migration so the private key is always sealed under the final master; (2) synchronously during `post_join_existing` via a transient `VaultKeyStore`, so the joining device publishes its real public key before reporting success. All keypair crypto reuses the already-tested `identity::ensure_device_keypair` (X25519 keypair, private key sealed `AES-256-GCM` under `KEK = HKDF-SHA256(master_key, "omnidrive-identity-kek-v1")`, persisted to `local_device_identity.encrypted_private_key` + `devices.public_key`, idempotent).

**Tech Stack:** Rust (Edition 2024), `x25519_dalek`, `hkdf`/`sha2`, `aes-gcm`, `sqlx` (SQLite), Tokio, axum.

---

## Design rationale (approved 2026-06-04)

- **Seal under master-key KEK, not "Vault Key + AAD=device_id".** The α.C.a start command mentioned sealing under the Vault Key with `AAD=device_id`; that was early-phase shorthand. The established, tested implementation (`identity.rs` + α.B.a `migration_reseals_device_private_key`) seals under `HKDF(master_key)` with no AAD and is cryptographically sound (HKDF domain-separation isolates the identity KEK; `master_key` is the root). Reworking to a Vault-Key envelope would break the tested α.B.a reseal path for zero security gain. **Decision: keep the existing `identity.rs` crypto unchanged.**
- **Ordering hazard (why the unlock path is a single sequenced task).** A real legacy vault (e.g. Lenovo) is simultaneously `parameter_set_version=1` (α.B.a migrates v1→v2, which re-keys the in-memory `master_key` and reseals any existing device private key) **and** keypair-less on its first post-α.C.a unlock. Two independent background spawns (KDF migration + keypair generation) would race: the keypair could be sealed under the pre-migration master, then become unsealable after the migration's reseal step has already passed. **Fix:** one background task that runs the KDF migration first, then ensures the keypair, so the private key is always sealed under the final master.
- **Join path uses a transient `VaultKeyStore`.** `post_join_existing` has the passphrase but the shared `state.vault_keys` stays locked through join (it is unlocked later by `post_unlock`). Generating the keypair via a throwaway `VaultKeyStore` derives `master_key` from the passphrase + grafted KDF params without leaving the shared store unlocked/unmounted. The transient store drops after use (keys zeroized).

## File structure

- `angeld/src/vault.rs` — rename `spawn_kdf_migration_if_needed` → `spawn_post_unlock_maintenance`; add awaitable `run_post_unlock_maintenance` that does KDF migration **then** device-keypair ensure; rename the inflight guard field. New ordering test.
- `angeld/src/api/auth.rs` — update the two call sites (`post_unlock`, `post_windows_hello_unlock`) to the renamed method.
- `angeld/src/api/onboarding.rs` — after `ensure_local_device_in_vault` succeeds in `post_join_existing`, generate the real keypair via a transient `VaultKeyStore`.
- `angeld/src/db.rs` — drop the two stale `// Placeholder … replaced in Epic 34.1a …` task-marker comments (CLAUDE.md bans task-marker / WHAT comments; the code is self-evident).

No new crates. No schema changes (`devices.public_key`, `local_device_identity.encrypted_private_key` already exist).

---

### Task 1: Sequence keypair generation after KDF migration in the post-unlock task

**Files:**
- Modify: `angeld/src/vault.rs` (`VaultKeyStore` struct field + `spawn_kdf_migration_if_needed`, currently `vault.rs:72-75` and `vault.rs:846-874`)
- Test: `angeld/src/vault.rs` (`#[cfg(test)] mod tests`, alongside the existing migration tests)

- [ ] **Step 1: Write the failing test**

Add this test in the `tests` module in `angeld/src/vault.rs` (next to `migration_reseals_device_private_key`). It proves the post-unlock task both migrates KDF params **and** replaces the device placeholder pubkey with a real, validatable, unsealable keypair — and that the private key is sealed under the *post-migration* master.

```rust
    #[tokio::test]
    async fn post_unlock_maintenance_migrates_then_generates_keypair() -> Result<(), VaultError> {
        let pool = test_pool_v1().await;
        let store = VaultKeyStore::new();
        store.unlock(&pool, "pass-123").await?;

        // Bootstrap the multi-user device row with the [0;32] placeholder, exactly
        // as production does at startup via migrate_single_to_multi_user.
        db::upsert_local_device_identity(&pool, "dev-fresh", "Fresh Device", "peer-tok")
            .await
            .unwrap();
        let vault = db::get_vault_params(&pool).await?.unwrap();
        let migrated = db::migrate_single_to_multi_user(&pool, &vault.vault_id)
            .await
            .unwrap();
        assert!(migrated, "single→multi migration should run on a fresh vault");
        let before = db::get_device(&pool, "dev-fresh").await.unwrap().unwrap();
        assert_eq!(
            before.public_key,
            vec![0u8; 32],
            "device must start with the placeholder pubkey"
        );

        // Run the post-unlock maintenance (awaitable form) deterministically.
        store
            .run_post_unlock_maintenance(&pool, "pass-123")
            .await?;

        // KDF params upgraded v1 → v2 (α.B.a behaviour preserved).
        assert_eq!(
            db::get_vault_config(&pool).await?.unwrap().parameter_set_version,
            2,
            "KDF params must be upgraded to v2"
        );

        // devices.public_key is now a real, non-placeholder X25519 key.
        let after = db::get_device(&pool, "dev-fresh").await.unwrap().unwrap();
        assert_ne!(after.public_key, vec![0u8; 32], "pubkey must no longer be the placeholder");
        assert_eq!(after.public_key.len(), 32);

        // Private key unseals under the FINAL (post-migration) master and matches the pubkey.
        let master = store.require_master_key().await?;
        let priv_key = identity::get_device_private_key(&pool, master.as_ref())
            .await
            .expect("device private key must unseal under the post-migration master");
        let derived_pub = x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(priv_key));
        assert_eq!(
            derived_pub.to_bytes().to_vec(),
            after.public_key,
            "stored public key must match the sealed private key"
        );
        Ok(())
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p angeld --lib post_unlock_maintenance_migrates_then_generates_keypair`
Expected: FAIL to compile — `no method named run_post_unlock_maintenance found for ... VaultKeyStore`.

- [ ] **Step 3: Rename the inflight guard field**

In the `VaultKeyStore` struct (`vault.rs:72-75`), rename the field:

```rust
pub struct VaultKeyStore {
    inner: Arc<RwLock<Option<UnlockedVaultKeys>>>,
    post_unlock_inflight: Arc<AtomicBool>,
}
```

Then update every other reference to `kdf_migration_inflight` in this file (the constructor that builds `VaultKeyStore { ... }`, and the two uses inside the spawn method) to `post_unlock_inflight`. Find them with: `grep -n kdf_migration_inflight angeld/src/vault.rs`.

- [ ] **Step 4: Replace `spawn_kdf_migration_if_needed` with the awaitable + wrapper pair**

Replace the whole `spawn_kdf_migration_if_needed` method (`vault.rs:846-874`) with:

```rust
    /// Runs the post-unlock maintenance sequence: first the α.B.a KDF-params
    /// migration (re-keys the in-memory master), then device-keypair generation —
    /// in that order so the X25519 private key is sealed under the *final* master.
    pub async fn run_post_unlock_maintenance(
        &self,
        pool: &SqlitePool,
        passphrase: &str,
    ) -> Result<(), VaultError> {
        match self.migrate_kdf_params_if_needed(pool, passphrase).await {
            Ok(MigrationOutcome::Migrated { from, to }) => {
                info!("[KDF-MIGRATION] params upgraded v{from} -> v{to}");
            }
            Ok(MigrationOutcome::Declined { reason }) => {
                warn!("[KDF-MIGRATION] declined: {reason}");
            }
            Ok(MigrationOutcome::Skipped) => {}
            Err(e) => warn!("[KDF-MIGRATION] failed (will retry next unlock): {e}"),
        }

        let master_key = self.require_master_key().await?;
        match crate::identity::ensure_device_keypair(pool, master_key.as_ref()).await {
            Ok(_) => info!("[DEVICE-KEY] X25519 keypair ensured for local device"),
            Err(e) => warn!("[DEVICE-KEY] keypair generation failed (will retry next unlock): {e}"),
        }
        Ok(())
    }

    pub fn spawn_post_unlock_maintenance(&self, pool: &SqlitePool, passphrase: &str) {
        if self
            .post_unlock_inflight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let store = self.clone();
        let pool = pool.clone();
        let passphrase = SecretString::from(passphrase.to_owned());
        tokio::spawn(async move {
            let _ = store
                .run_post_unlock_maintenance(&pool, passphrase.expose_secret())
                .await;
            store.post_unlock_inflight.store(false, Ordering::Release);
        });
    }
```

Note: `crate::identity` must be reachable from `vault.rs`. If the file does not already `use crate::identity;`, the test module references `identity::...` so it is in scope for tests; for the non-test method use the fully-qualified `crate::identity::ensure_device_keypair` shown above (no new `use` needed). Verify `identity` is imported at module scope with `grep -n "use crate::identity" angeld/src/vault.rs`; if absent the fully-qualified path already used above is sufficient.

- [ ] **Step 5: Update the existing migration spawn test to the new name**

The test at `vault.rs:1578` calls `store.spawn_kdf_migration_if_needed(&pool, "pass-123");`. Rename that single call to `store.spawn_post_unlock_maintenance(&pool, "pass-123");`. Its assertions (poll until `parameter_set_version == 2`) remain valid because `run_post_unlock_maintenance` runs the migration first.

- [ ] **Step 6: Run the new + existing tests to verify they pass**

Run: `cargo test -p angeld --lib post_unlock_maintenance_migrates_then_generates_keypair migration_reseals_device_private_key spawn`
Expected: PASS (new ordering test + the α.B.a reseal test + the renamed spawn test).

- [ ] **Step 7: Commit**

```bash
git add angeld/src/vault.rs
git commit -m "feat(vault): α.C.a generate device X25519 keypair in post-unlock maintenance"
```

---

### Task 2: Wire the renamed method into the unlock handlers

**Files:**
- Modify: `angeld/src/api/auth.rs` (`post_unlock` at `auth.rs:56-58`, `post_windows_hello_unlock` at `auth.rs:424-426`)

- [ ] **Step 1: Update the `post_unlock` call site**

In `angeld/src/api/auth.rs`, replace:

```rust
    state
        .vault_keys
        .spawn_kdf_migration_if_needed(&state.pool, request.passphrase.expose_secret());
```

with:

```rust
    state
        .vault_keys
        .spawn_post_unlock_maintenance(&state.pool, request.passphrase.expose_secret());
```

- [ ] **Step 2: Update the `post_windows_hello_unlock` call site**

In the same file, replace:

```rust
    state
        .vault_keys
        .spawn_kdf_migration_if_needed(&state.pool, &passphrase);
```

with:

```rust
    state
        .vault_keys
        .spawn_post_unlock_maintenance(&state.pool, &passphrase);
```

- [ ] **Step 3: Verify the workspace builds and no stale references remain**

Run: `grep -rn spawn_kdf_migration_if_needed angeld/src; cargo build -p angeld`
Expected: `grep` returns nothing; build succeeds.

- [ ] **Step 4: Commit**

```bash
git add angeld/src/api/auth.rs
git commit -m "refactor(auth): α.C.a route both unlock paths through post-unlock maintenance"
```

---

### Task 3: Generate a real keypair during join-existing

**Files:**
- Modify: `angeld/src/api/onboarding.rs` (`post_join_existing`, inside the `if let Ok(Some(local_dev)) = ...` block, after the `ensure_local_device_in_vault` match at `onboarding.rs:722-733`)
- Test: `angeld/src/api/onboarding.rs` (`#[cfg(test)] mod tests`) — a focused unit test of a small extracted helper.

The handler is hard to exercise end-to-end in a unit test (network restore, providers). Extract the keypair step into a small, directly testable helper and call it from the handler.

- [ ] **Step 1: Write the failing test**

Add (or extend) a `#[cfg(test)] mod tests` at the bottom of `angeld/src/api/onboarding.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::identity;

    #[tokio::test]
    async fn join_keypair_replaces_placeholder_with_real_key() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();

        // Simulate a freshly-initialised + single→multi-migrated local vault:
        // the device row exists with the [0;32] placeholder, vault is on disk.
        let store = crate::vault::VaultKeyStore::new();
        store.unlock(&pool, "join-pass").await.unwrap();
        db::upsert_local_device_identity(&pool, "dev-join", "JoinPC", "peer-tok")
            .await
            .unwrap();
        let vault = db::get_vault_params(&pool).await.unwrap().unwrap();
        db::migrate_single_to_multi_user(&pool, &vault.vault_id)
            .await
            .unwrap();
        assert_eq!(
            db::get_device(&pool, "dev-join").await.unwrap().unwrap().public_key,
            vec![0u8; 32]
        );

        // The join handler does NOT keep state.vault_keys unlocked, so the helper
        // must derive master_key itself from the passphrase via a transient store.
        generate_local_device_keypair(&pool, "join-pass")
            .await
            .expect("keypair generation must succeed");

        let dev = db::get_device(&pool, "dev-join").await.unwrap().unwrap();
        assert_ne!(dev.public_key, vec![0u8; 32], "join must publish a real pubkey");
        assert_eq!(dev.public_key.len(), 32);

        // Private key unseals under master derived from the same passphrase
        // (reuse the setup store — same passphrase + params yield the same master).
        let master = store.require_master_key().await.unwrap();
        let priv_key = identity::get_device_private_key(&pool, master.as_ref())
            .await
            .expect("private key must unseal");
        let derived =
            x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(priv_key));
        assert_eq!(derived.to_bytes().to_vec(), dev.public_key);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p angeld --lib join_keypair_replaces_placeholder_with_real_key`
Expected: FAIL to compile — `cannot find function generate_local_device_keypair`.

- [ ] **Step 3: Write the helper**

Add this free function in `angeld/src/api/onboarding.rs` (module scope, above `post_join_existing`):

```rust
/// Generates this device's real X25519 keypair after join, replacing the `[0;32]`
/// bootstrap placeholder in `devices.public_key`.  The shared `state.vault_keys`
/// stays locked through join, so a transient `VaultKeyStore` derives `master_key`
/// from the passphrase (+ grafted KDF params) to seal the private key at rest.
async fn generate_local_device_keypair(
    pool: &sqlx::SqlitePool,
    passphrase: &str,
) -> Result<(), String> {
    let store = crate::vault::VaultKeyStore::new();
    store
        .unlock(pool, passphrase)
        .await
        .map_err(|e| format!("transient unlock failed: {e}"))?;
    let master_key = store
        .require_master_key()
        .await
        .map_err(|e| format!("master key unavailable: {e}"))?;
    crate::identity::ensure_device_keypair(pool, master_key.as_ref())
        .await
        .map_err(|e| format!("keypair generation failed: {e}"))?;
    Ok(())
}
```

- [ ] **Step 4: Call the helper from `post_join_existing`**

Inside `post_join_existing`, in the `if let Ok(Some(local_dev)) = db::get_local_device_identity(&state.pool).await` block, immediately after the `ensure_local_device_in_vault` match arm closes (`onboarding.rs:733`, before the `// Resolve the adopted user_id …` comment), insert:

```rust
        if let Err(err) = generate_local_device_keypair(&state.pool, passphrase).await {
            warn!("[join-existing] device keypair generation failed (non-fatal): {err}");
        } else {
            info!(
                "[join-existing] generated real X25519 keypair for device {}",
                local_dev.device_id
            );
        }
```

(`passphrase` is already bound at `onboarding.rs:550` as `request.passphrase.expose_secret()` and is in scope here.)

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p angeld --lib join_keypair_replaces_placeholder_with_real_key`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add angeld/src/api/onboarding.rs
git commit -m "feat(onboarding): α.C.a generate real device X25519 keypair on join-existing"
```

---

### Task 4: Remove stale placeholder task-marker comments

**Files:**
- Modify: `angeld/src/db.rs` (`db.rs:7543` in `migrate_single_to_multi_user`, `db.rs:7634` in `ensure_local_device_in_vault`)

- [ ] **Step 1: Remove the two task-marker comments**

In `migrate_single_to_multi_user`, delete the line:

```rust
    // Placeholder 32-byte zero public key — replaced in Epic 34.1a with real X25519 keypair
```

leaving `let placeholder_pubkey = vec![0u8; 32];` (self-documenting via the variable name).

In `ensure_local_device_in_vault`, the placeholder line `let placeholder_pubkey = vec![0u8; 32];` has no comment above it — leave it as-is (nothing to remove there). Re-grep to confirm no other `// Placeholder … Epic …` markers remain near these functions: `grep -n "replaced in Epic" angeld/src/db.rs`.

- [ ] **Step 2: Verify build + full lib test suite**

Run: `cargo test -p angeld --lib`
Expected: PASS (no regressions).

- [ ] **Step 3: Commit**

```bash
git add angeld/src/db.rs
git commit -m "chore(db): α.C.a drop stale placeholder task-marker comment"
```

---

## Final verification (after all tasks)

- [ ] **Full workspace gate** (mirrors the pre-push hook so the push won't be rejected):

Run:
```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features test-helpers -- -D warnings
cargo build --release --workspace
cargo test -p angeld --lib
```
Expected: all green.

- [ ] **DoD confirmation:**
  - Fresh vault → first unlock → `devices.public_key ≠ [0;32]` (Task 1 ordering test).
  - `validate_x25519_pubkey` accepts the generated key (guaranteed: `ensure_device_keypair` generates via `x25519_dalek`, and `validate_x25519_pubkey` only rejects all-zero; covered transitively by `identity::ensure_device_keypair_generates_and_persists` asserting `pubkey != [0u8;32]`).
  - Private key seal/unseal round-trip under the final master (Task 1 + Task 3 tests assert `get_device_private_key` succeeds and the derived public key matches).
  - Join-existing → real pubkey published (Task 3 test).

## Out of scope (do NOT do here)

- No version bump (per start command — bump happens after DoD + optional smoke).
- No change to `identity.rs` crypto (seal stays under master-key KEK).
- No multi-device VK wrapping for the new pubkeys (that is α.B.b / Epic 33).
- No graft identity-bundle fix (that is α.C.b).
