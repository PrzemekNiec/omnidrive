# α.C.b — Graft Identity-Bundle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `graft_restored_metadata_snapshot` so a joining device adopts the source vault's full crypto state (envelope key, per-file DEKs, recovery keys), fixing the Dell↔Lenovo split-brain (P1-001 + P1-005).

**Architecture:** Pure graft-side change. The snapshot already carries everything (`VACUUM INTO` full copy). We add three reads + three applies inside the existing single `BEGIN IMMEDIATE` transaction of `db::graft_restored_metadata_snapshot`: (1) extend the `vault_state` read/apply with `encrypted_vault_key`, `vault_key_generation`, `legacy_read_key` (always from the remote snapshot); (2) wipe+copy the whole `data_encryption_keys` table; (3) wipe+copy the whole `vault_recovery_keys` table. `local_device_identity` is deliberately NOT grafted (per-device, owned by α.C.a).

**Tech Stack:** Rust (Edition 2024), `sqlx` (SQLite), Tokio. Tests are plain `#[tokio::test]` in the `db` module (file-backed SQLite via `std::env::temp_dir()`), exercising `crate::vault::VaultKeyStore` and `crate::disaster_recovery::create_metadata_snapshot`.

Spec: `docs/superpowers/specs/2026-06-04-alpha-C-b-graft-identity-bundle-design.md`

---

## File Structure

- **Modify:** `angeld/src/db.rs`
  - `graft_restored_metadata_snapshot` (fn at `db.rs:1796`):
    - local `RestoreVaultRecord` struct (`db.rs:1810-1817`) — +3 fields.
    - remote `vault_state` SELECT (`db.rs:1818-1820`) — +3 columns.
    - local `vault_state` SELECT (`db.rs:1964-1966`) — +3 columns.
    - `vault_state` apply INSERT…ON CONFLICT, both branches (`db.rs:1972-2000`) — +3 columns.
    - new reads for `data_encryption_keys` + `vault_recovery_keys` (Phase 1, near `db.rs:1928`).
    - wipe list (`db.rs:2026-2046`) — +2 `DELETE`s.
    - new INSERT loops for the two tables (Phase 2, near `db.rs:2107`).
  - new module-level row structs `RestoredDek`, `RestoredRecoveryKey` (after `RestoredVaultMember` at `db.rs:1794`).
  - new tests in the `db` test module (`mod tests` at `db.rs:8134`): one shared test helper + 5 tests.

No other files change. No snapshot/upload changes. No version bump.

---

## Shared test helper (added in Task 1, reused by later tasks)

Added once inside `mod tests` in `db.rs`. Builds a fully-initialised **source** vault on a file-backed DB and returns the handles later assertions need.

```rust
// ── α.C.b graft test helpers ──
async fn build_source_vault(
    dir: &std::path::Path,
) -> Result<
    (
        sqlx::SqlitePool,         // source pool
        std::path::PathBuf,       // snapshot path (VACUUM INTO target)
        Vec<u8>,                  // source envelope key (plaintext, 32B)
        String,                   // safety numbers for USER_FIXTURE
        i64,                      // inode id that has a wrapped DEK
        Vec<u8>,                  // that wrapped DEK bytes (as stored)
        String,                   // vault_id
    ),
    Box<dyn std::error::Error>,
> {
    use crate::disaster_recovery::create_metadata_snapshot;
    use crate::vault::VaultKeyStore;

    let source_path = dir.join("source.db");
    let snapshot_path = dir.join("snapshot.db");
    let source_url = format!("sqlite://{}", source_path.to_string_lossy().replace('\\', "/"));

    let source_pool = init_db(&source_url).await?;

    // Real unlock creates vault_config + a random V2 envelope key + vault_state row.
    let store = VaultKeyStore::new();
    store.unlock(&source_pool, "test-pass").await?;
    let envelope_key = store.require_envelope_key().await?.to_vec();
    let safety = store
        .safety_numbers(USER_FIXTURE)
        .await
        .expect("source must produce safety numbers");

    // One inode with a real wrapped DEK (wrapped under the envelope key).
    let inode_id = create_inode(&source_pool, None, "graft-test.txt", "FILE", 42).await?;
    store.get_or_create_dek(&source_pool, inode_id).await?;
    let wrapped_dek: Vec<u8> = sqlx::query_scalar(
        "SELECT wrapped_dek FROM data_encryption_keys WHERE inode_id = ?",
    )
    .bind(inode_id)
    .fetch_one(&source_pool)
    .await?;

    let vault_id: String =
        sqlx::query_scalar("SELECT vault_id FROM vault_state WHERE id = 1")
            .fetch_one(&source_pool)
            .await?;

    // A recovery key (opaque wrapped bytes — graft copies bytes verbatim).
    insert_recovery_key(&source_pool, &vault_id, &[0xABu8; 40], 1, Some("test")).await?;

    // A non-NULL legacy_read_key (only set after α.B.a migration in production;
    // set explicitly here so the test proves it is grafted).
    sqlx::query("UPDATE vault_state SET legacy_read_key = ? WHERE id = 1")
        .bind(vec![0x5Au8; 60])
        .execute(&source_pool)
        .await?;

    create_metadata_snapshot(&source_pool, &snapshot_path).await?;

    Ok((source_pool, snapshot_path, envelope_key, safety, inode_id, wrapped_dek, vault_id))
}

const USER_FIXTURE: &str = "user-fixture";

fn temp_test_dir(tag: &str) -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    std::env::temp_dir().join(format!(
        "omnidrive-acb-{}-{}",
        tag,
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    ))
}
```

> Note: `VaultKeyStore::require_envelope_key`, `safety_numbers`, `get_or_create_dek` already exist and are `pub` (`vault.rs:341`, `vault.rs:403`). `insert_recovery_key` exists (`db.rs:7882`). `create_inode` exists. The `db` module is `crate::db`, so inside `db.rs` tests these are bare (`init_db`, `create_inode`, `insert_recovery_key`) and `crate::vault::…` / `crate::disaster_recovery::…` for the rest.

---

## Task 1: Graft `vault_state` envelope key, generation, legacy_read_key

**Files:**
- Modify: `angeld/src/db.rs` (struct `db.rs:1810-1817`, remote SELECT `db.rs:1818-1820`, local SELECT `db.rs:1964-1966`, apply `db.rs:1972-2000`)
- Test: `angeld/src/db.rs` (`mod tests`, `db.rs:8134`)

- [ ] **Step 1: Add the shared test helper**

Paste the **Shared test helper** block above (incl. `USER_FIXTURE`, `temp_test_dir`, `build_source_vault`) into `mod tests` in `db.rs`, just after the `use super::*;` line.

- [ ] **Step 2: Write the failing test**

Add to `mod tests`:

```rust
#[tokio::test]
async fn graft_copies_encrypted_vault_key_generation_and_legacy_read_key()
-> Result<(), Box<dyn std::error::Error>> {
    use tokio::fs;
    let dir = temp_test_dir("vaultstate");
    fs::create_dir_all(&dir).await?;

    let (source_pool, snapshot_path, _evk, _safety, _inode, _dek, _vid) =
        build_source_vault(&dir).await?;
    let source_evk: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT encrypted_vault_key FROM vault_state WHERE id = 1")
            .fetch_one(&source_pool)
            .await?;
    let source_gen: Option<i64> =
        sqlx::query_scalar("SELECT vault_key_generation FROM vault_state WHERE id = 1")
            .fetch_one(&source_pool)
            .await?;
    assert!(source_evk.is_some(), "source must have an envelope key");

    // Target = a DIFFERENT vault that already unlocked once (its own EVK gen=1).
    let target_url = format!(
        "sqlite://{}",
        dir.join("target.db").to_string_lossy().replace('\\', "/")
    );
    let target_pool = init_db(&target_url).await?;
    crate::vault::VaultKeyStore::new()
        .unlock(&target_pool, "test-pass")
        .await?;
    let dell_evk_before: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT encrypted_vault_key FROM vault_state WHERE id = 1")
            .fetch_one(&target_pool)
            .await?;

    graft_restored_metadata_snapshot(&target_pool, &snapshot_path).await?;

    let after_evk: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT encrypted_vault_key FROM vault_state WHERE id = 1")
            .fetch_one(&target_pool)
            .await?;
    let after_gen: Option<i64> =
        sqlx::query_scalar("SELECT vault_key_generation FROM vault_state WHERE id = 1")
            .fetch_one(&target_pool)
            .await?;
    let after_legacy: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT legacy_read_key FROM vault_state WHERE id = 1")
            .fetch_one(&target_pool)
            .await?;

    assert_eq!(after_evk, source_evk, "EVK must be adopted from snapshot");
    assert_ne!(after_evk, dell_evk_before, "EVK must overwrite the device's own");
    assert_eq!(after_gen, source_gen, "generation must be adopted");
    assert_eq!(after_legacy, Some(vec![0x5Au8; 60]), "legacy_read_key must be grafted");

    let _ = fs::remove_dir_all(&dir).await;
    Ok(())
}
```

- [ ] **Step 3: Run the test, verify it FAILS**

Run: `cargo test -p angeld --lib graft_copies_encrypted_vault_key_generation_and_legacy_read_key`
Expected: FAIL — `after_evk` equals `dell_evk_before` (or `assert_eq!(after_evk, source_evk)` fails), because the current graft does not copy these columns.

- [ ] **Step 4: Extend the local `RestoreVaultRecord` struct**

In `graft_restored_metadata_snapshot`, replace the struct (`db.rs:1810-1817`):

```rust
    #[allow(dead_code)]
    #[derive(sqlx::FromRow)]
    struct RestoreVaultRecord {
        id: i64,
        master_key_salt: Vec<u8>,
        argon2_params: String,
        vault_id: String,
        encrypted_vault_key: Option<Vec<u8>>,
        vault_key_generation: Option<i64>,
        legacy_read_key: Option<Vec<u8>>,
    }
```

- [ ] **Step 5: Extend the remote `vault_state` SELECT**

Replace the remote read query string (`db.rs:1819`):

```rust
        "SELECT id, master_key_salt, argon2_params, vault_id, encrypted_vault_key, \
         vault_key_generation, legacy_read_key FROM vault_state WHERE id = 1",
```

- [ ] **Step 6: Extend the local `vault_state` SELECT**

Replace the local read query string (`db.rs:1965`):

```rust
            "SELECT id, master_key_salt, argon2_params, vault_id, encrypted_vault_key, \
             vault_key_generation, legacy_read_key FROM vault_state WHERE id = 1",
```

- [ ] **Step 7: Extend the apply — `Some(local)` branch**

Replace the `Some(local) => { … }` INSERT (`db.rs:1972-1985`):

```rust
            Some(local) => {
                sqlx::query(
                    "INSERT INTO vault_state \
                     (id, master_key_salt, argon2_params, vault_id, encrypted_vault_key, \
                      vault_key_generation, legacy_read_key) \
                     VALUES (1, ?, ?, ?, ?, ?, ?) \
                     ON CONFLICT(id) DO UPDATE SET \
                         master_key_salt = excluded.master_key_salt, \
                         argon2_params = excluded.argon2_params, \
                         vault_id = excluded.vault_id, \
                         encrypted_vault_key = excluded.encrypted_vault_key, \
                         vault_key_generation = excluded.vault_key_generation, \
                         legacy_read_key = excluded.legacy_read_key",
                )
                .bind(local.master_key_salt)
                .bind(local.argon2_params)
                .bind(&remote_vault.vault_id)
                .bind(&remote_vault.encrypted_vault_key)
                .bind(remote_vault.vault_key_generation)
                .bind(&remote_vault.legacy_read_key)
                .execute(&mut *conn)
                .await?;
            }
```

- [ ] **Step 8: Extend the apply — `None` branch**

Replace the `None => { … }` INSERT (`db.rs:1986-2000`):

```rust
            None => {
                sqlx::query(
                    "INSERT INTO vault_state \
                     (id, master_key_salt, argon2_params, vault_id, encrypted_vault_key, \
                      vault_key_generation, legacy_read_key) \
                     VALUES (1, ?, ?, ?, ?, ?, ?) \
                     ON CONFLICT(id) DO UPDATE SET \
                         master_key_salt = excluded.master_key_salt, \
                         argon2_params = excluded.argon2_params, \
                         vault_id = excluded.vault_id, \
                         encrypted_vault_key = excluded.encrypted_vault_key, \
                         vault_key_generation = excluded.vault_key_generation, \
                         legacy_read_key = excluded.legacy_read_key",
                )
                .bind(remote_vault.master_key_salt)
                .bind(remote_vault.argon2_params)
                .bind(&remote_vault.vault_id)
                .bind(&remote_vault.encrypted_vault_key)
                .bind(remote_vault.vault_key_generation)
                .bind(&remote_vault.legacy_read_key)
                .execute(&mut *conn)
                .await?;
            }
```

> `remote_vault.vault_id` is now borrowed (`&`) in both branches; `master_key_salt`/`argon2_params` in the `None` branch are still moved out of `remote_vault` (fine — used once). `encrypted_vault_key`/`legacy_read_key` are borrowed so they can be bound in either branch without partial-move conflicts.

- [ ] **Step 9: Run the test, verify it PASSES**

Run: `cargo test -p angeld --lib graft_copies_encrypted_vault_key_generation_and_legacy_read_key`
Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add angeld/src/db.rs
git commit -m "feat(db): α.C.b graft vault_state envelope key + generation + legacy_read_key"
```

---

## Task 2: Graft the `data_encryption_keys` table

**Files:**
- Modify: `angeld/src/db.rs` (new struct after `db.rs:1794`; new read near `db.rs:1928`; wipe list `db.rs:2026-2046`; new insert loop near `db.rs:2107`)
- Test: `angeld/src/db.rs` (`mod tests`)

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
#[tokio::test]
async fn graft_copies_data_encryption_keys() -> Result<(), Box<dyn std::error::Error>> {
    use tokio::fs;
    let dir = temp_test_dir("deks");
    fs::create_dir_all(&dir).await?;

    let (source_pool, snapshot_path, _evk, _safety, inode_id, wrapped_dek, _vid) =
        build_source_vault(&dir).await?;
    let source_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM data_encryption_keys")
            .fetch_one(&source_pool)
            .await?;
    assert_eq!(source_count, 1, "source has exactly one DEK");

    let target_url = format!(
        "sqlite://{}",
        dir.join("target.db").to_string_lossy().replace('\\', "/")
    );
    let target_pool = init_db(&target_url).await?;

    graft_restored_metadata_snapshot(&target_pool, &snapshot_path).await?;

    let got = get_wrapped_dek(&target_pool, inode_id).await?;
    let got = got.expect("DEK must be grafted for the inode");
    assert_eq!(got.wrapped_dek, wrapped_dek, "wrapped DEK bytes must match source");

    let _ = fs::remove_dir_all(&dir).await;
    Ok(())
}
```

- [ ] **Step 2: Run the test, verify it FAILS**

Run: `cargo test -p angeld --lib graft_copies_data_encryption_keys`
Expected: FAIL — `get_wrapped_dek` returns `None` (graft does not copy the table).

- [ ] **Step 3: Add the `RestoredDek` row struct**

Insert after `RestoredVaultMember` (`db.rs:1794`):

```rust
#[derive(sqlx::FromRow)]
struct RestoredDek {
    dek_id: i64,
    inode_id: i64,
    wrapped_dek: Vec<u8>,
    key_version: i64,
    vault_key_gen: i64,
    created_at: i64,
}
```

- [ ] **Step 4: Read DEKs from the snapshot (Phase 1)**

In `graft_restored_metadata_snapshot`, after the `r_vault_config` read (`db.rs:1933-1939`) and before `restored_pool.close()` (`db.rs:1945`), add:

```rust
    let r_deks = sqlx::query_as::<_, RestoredDek>(
        "SELECT dek_id, inode_id, wrapped_dek, key_version, vault_key_gen, created_at \
         FROM data_encryption_keys",
    )
    .fetch_all(&restored_pool)
    .await
    .unwrap_or_default();
```

- [ ] **Step 5: Add `data_encryption_keys` to the wipe list**

In the `for statement in [ … ]` wipe array (`db.rs:2026-2046`), add `"DELETE FROM data_encryption_keys",` immediately before `"DELETE FROM inodes",` (DEKs reference inodes):

```rust
            "DELETE FROM chunk_refs",
            "DELETE FROM conflict_events",
            "DELETE FROM file_revisions",
            "DELETE FROM metadata_backups",
            "DELETE FROM sync_policies",
            "DELETE FROM data_encryption_keys",
            "DELETE FROM inodes",
```

- [ ] **Step 6: Insert DEKs (Phase 2)**

After the `r_sync_state` insert loop completes (the loop ending near `db.rs:2170`) and before the `r_backups` loop — anywhere among the data inserts is fine, but place it right after the inode insert loop (`db.rs:2123`) so inodes exist first. Add:

```rust
        for row in &r_deks {
            sqlx::query(
                "INSERT INTO data_encryption_keys \
                 (dek_id, inode_id, wrapped_dek, key_version, vault_key_gen, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(row.dek_id)
            .bind(row.inode_id)
            .bind(&row.wrapped_dek)
            .bind(row.key_version)
            .bind(row.vault_key_gen)
            .bind(row.created_at)
            .execute(&mut *conn)
            .await?;
        }
```

- [ ] **Step 7: Run the test, verify it PASSES**

Run: `cargo test -p angeld --lib graft_copies_data_encryption_keys`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add angeld/src/db.rs
git commit -m "feat(db): α.C.b graft data_encryption_keys table"
```

---

## Task 3: Graft the `vault_recovery_keys` table

**Files:**
- Modify: `angeld/src/db.rs` (new struct after `RestoredDek`; new read near `db.rs:1939`; wipe list; new insert loop)
- Test: `angeld/src/db.rs` (`mod tests`)

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
#[tokio::test]
async fn graft_copies_vault_recovery_keys() -> Result<(), Box<dyn std::error::Error>> {
    use tokio::fs;
    let dir = temp_test_dir("recovery");
    fs::create_dir_all(&dir).await?;

    let (_source_pool, snapshot_path, _evk, _safety, _inode, _dek, vault_id) =
        build_source_vault(&dir).await?;

    let target_url = format!(
        "sqlite://{}",
        dir.join("target.db").to_string_lossy().replace('\\', "/")
    );
    let target_pool = init_db(&target_url).await?;

    graft_restored_metadata_snapshot(&target_pool, &snapshot_path).await?;

    let active = list_active_recovery_keys(&target_pool, &vault_id).await?;
    assert_eq!(active.len(), 1, "the source recovery key must be grafted");
    assert_eq!(active[0].wrapped_vault_key, vec![0xABu8; 40]);
    assert_eq!(active[0].vk_generation, 1);

    let _ = fs::remove_dir_all(&dir).await;
    Ok(())
}
```

- [ ] **Step 2: Run the test, verify it FAILS**

Run: `cargo test -p angeld --lib graft_copies_vault_recovery_keys`
Expected: FAIL — `active.len()` is 0 (graft does not copy the table).

- [ ] **Step 3: Add the `RestoredRecoveryKey` row struct**

Insert after `RestoredDek`:

```rust
#[derive(sqlx::FromRow)]
struct RestoredRecoveryKey {
    id: i64,
    vault_id: String,
    wrapped_vault_key: Vec<u8>,
    vk_generation: i64,
    created_at: i64,
    created_by: Option<String>,
    revoked_at: Option<i64>,
}
```

- [ ] **Step 4: Read recovery keys from the snapshot (Phase 1)**

Immediately after the `r_deks` read added in Task 2, add:

```rust
    let r_recovery_keys = sqlx::query_as::<_, RestoredRecoveryKey>(
        "SELECT id, vault_id, wrapped_vault_key, vk_generation, created_at, created_by, \
         revoked_at FROM vault_recovery_keys",
    )
    .fetch_all(&restored_pool)
    .await
    .unwrap_or_default();
```

- [ ] **Step 5: Add `vault_recovery_keys` to the wipe list**

In the wipe array, add `"DELETE FROM vault_recovery_keys",` right after the `"DELETE FROM data_encryption_keys",` line from Task 2 (no FK constraints, order is not critical):

```rust
            "DELETE FROM data_encryption_keys",
            "DELETE FROM vault_recovery_keys",
            "DELETE FROM inodes",
```

- [ ] **Step 6: Insert recovery keys (Phase 2)**

Immediately after the `r_deks` insert loop added in Task 2, add:

```rust
        for row in &r_recovery_keys {
            sqlx::query(
                "INSERT INTO vault_recovery_keys \
                 (id, vault_id, wrapped_vault_key, vk_generation, created_at, created_by, \
                  revoked_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(row.id)
            .bind(&row.vault_id)
            .bind(&row.wrapped_vault_key)
            .bind(row.vk_generation)
            .bind(row.created_at)
            .bind(&row.created_by)
            .bind(row.revoked_at)
            .execute(&mut *conn)
            .await?;
        }
```

- [ ] **Step 7: Run the test, verify it PASSES**

Run: `cargo test -p angeld --lib graft_copies_vault_recovery_keys`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add angeld/src/db.rs
git commit -m "feat(db): α.C.b graft vault_recovery_keys table"
```

---

## Task 4: End-to-end crypto round-trip + V1 backward-compat

**Files:**
- Test only: `angeld/src/db.rs` (`mod tests`)

These tests exercise the whole chain. They should PASS immediately on top of Tasks 1-3; if either fails, the bug is in the graft and must be fixed before moving on.

- [ ] **Step 1: Write the round-trip e2e test**

Add to `mod tests`:

```rust
#[tokio::test]
async fn graft_makes_joining_device_derive_same_evk_safety_and_dek()
-> Result<(), Box<dyn std::error::Error>> {
    use tokio::fs;
    let dir = temp_test_dir("roundtrip");
    fs::create_dir_all(&dir).await?;

    let (source_pool, snapshot_path, source_evk, source_safety, inode_id, _dek, _vid) =
        build_source_vault(&dir).await?;
    // Capture the source's plaintext DEK for the round-trip comparison.
    let source_store = crate::vault::VaultKeyStore::new();
    source_store.unlock(&source_pool, "test-pass").await?;
    let (_id, source_dek) = source_store.get_or_create_dek(&source_pool, inode_id).await?;
    let source_dek_bytes = source_dek.expose_secret().as_ref().to_vec();

    // Fresh joining device that unlocked once on its own (split-brain EVK).
    let target_url = format!(
        "sqlite://{}",
        dir.join("target.db").to_string_lossy().replace('\\', "/")
    );
    let target_pool = init_db(&target_url).await?;
    crate::vault::VaultKeyStore::new()
        .unlock(&target_pool, "test-pass")
        .await?;

    graft_restored_metadata_snapshot(&target_pool, &snapshot_path).await?;

    // After graft, a fresh unlock with the same passphrase must reproduce the
    // source vault's envelope key, safety numbers, and unwrap the grafted DEK.
    let joined = crate::vault::VaultKeyStore::new();
    joined.unlock(&target_pool, "test-pass").await?;

    let joined_evk = joined.require_envelope_key().await?.to_vec();
    assert_eq!(joined_evk, source_evk, "joined EVK must equal source EVK");

    let joined_safety = joined.safety_numbers(USER_FIXTURE).await.unwrap();
    assert_eq!(joined_safety, source_safety, "safety numbers must match (P1-005)");

    let (_id2, joined_dek) = joined.get_or_create_dek(&target_pool, inode_id).await?;
    assert_eq!(
        joined_dek.expose_secret().as_ref().to_vec(),
        source_dek_bytes,
        "grafted DEK must unwrap to the same plaintext (P1-001)"
    );

    let _ = fs::remove_dir_all(&dir).await;
    Ok(())
}
```

> `SecretBox<KeyBytes>` exposes via `.expose_secret()` (re-exported `secrecy::ExposeSecret`); `KeyBytes` derefs to `[u8]`. If `expose_secret()` is not in scope in the test module, add `use secrecy::ExposeSecret;` at the top of the test fn.

- [ ] **Step 2: Run the e2e test, verify it PASSES**

Run: `cargo test -p angeld --lib graft_makes_joining_device_derive_same_evk_safety_and_dek`
Expected: PASS.

- [ ] **Step 3: Write the V1 backward-compat test**

Add to `mod tests`:

```rust
#[tokio::test]
async fn graft_from_legacy_v1_snapshot_does_not_panic()
-> Result<(), Box<dyn std::error::Error>> {
    use tokio::fs;
    let dir = temp_test_dir("v1compat");
    fs::create_dir_all(&dir).await?;

    // A source DB that has a vault_state row but no envelope key / DEKs /
    // recovery keys (NULL columns + empty tables) — simulates a pre-α.B.a vault.
    let source_url = format!(
        "sqlite://{}",
        dir.join("source.db").to_string_lossy().replace('\\', "/")
    );
    let source_pool = init_db(&source_url).await?;
    sqlx::query(
        "INSERT INTO vault_state (id, master_key_salt, argon2_params, vault_id) \
         VALUES (1, ?, ?, ?)",
    )
    .bind(vec![1u8; 16])
    .bind("v1-params")
    .bind("vault-legacy")
    .execute(&source_pool)
    .await?;

    let snapshot_path = dir.join("snapshot.db");
    crate::disaster_recovery::create_metadata_snapshot(&source_pool, &snapshot_path).await?;

    let target_url = format!(
        "sqlite://{}",
        dir.join("target.db").to_string_lossy().replace('\\', "/")
    );
    let target_pool = init_db(&target_url).await?;

    // Must succeed (no panic, no error) and leave NULL crypto columns.
    graft_restored_metadata_snapshot(&target_pool, &snapshot_path).await?;

    let evk: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT encrypted_vault_key FROM vault_state WHERE id = 1")
            .fetch_one(&target_pool)
            .await?;
    assert!(evk.is_none(), "V1 snapshot has no envelope key — must stay NULL");
    let dek_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM data_encryption_keys")
            .fetch_one(&target_pool)
            .await?;
    assert_eq!(dek_count, 0);

    let _ = fs::remove_dir_all(&dir).await;
    Ok(())
}
```

- [ ] **Step 4: Run the V1 test, verify it PASSES**

Run: `cargo test -p angeld --lib graft_from_legacy_v1_snapshot_does_not_panic`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add angeld/src/db.rs
git commit -m "test(db): α.C.b e2e graft round-trip + V1 backward-compat"
```

---

## Final verification (after all tasks)

- [ ] **Full workspace gate** (mirrors the pre-push hook so the push won't be rejected):

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features test-helpers -- -D warnings
cargo build --release --workspace
cargo test -p angeld --lib
```
Expected: all green. The five new tests live in the `db` module → covered by `--lib` (no `--features test-helpers` needed, unlike α.C.a's join test).

- [ ] **DoD confirmation (Rust gate):**
  - `graft_copies_encrypted_vault_key_generation_and_legacy_read_key` — vault_state crypto columns adopted + overwrite own.
  - `graft_copies_data_encryption_keys` — per-file DEKs adopted byte-for-byte.
  - `graft_copies_vault_recovery_keys` — recovery keys adopted.
  - `graft_makes_joining_device_derive_same_evk_safety_and_dek` — EVK + safety numbers identical + DEK unwraps (P1-001/005 closed in-process).
  - `graft_from_legacy_v1_snapshot_does_not_panic` — backward-compat preserved.

- [ ] **Live acceptance (separate, does NOT gate code DONE):** SMOKE C3 (safety numbers identical Dell↔Lenovo) + D7 (Dell hydrate → SHA256 match with Lenovo). Requires Dell; run after the Rust gate is green.

## Out of scope (do NOT do here)

- Version bump (after DoD + optional smoke).
- Per-device adaptive KDF params (deferred from α.B.a — separate task).
- α.B.b ML-KEM hybrid wrap.
- Snapshot creation/upload/encryption changes.
- Grafting `local_device_identity` (per-device, deliberately excluded).
- `db.rs` decomposition (Phase γ).
