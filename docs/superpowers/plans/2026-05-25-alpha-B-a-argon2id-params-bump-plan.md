# α.B.a — Argon2id Params Bump (Atomic Re-Key on Unlock) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When an existing vault is unlocked under params `version<2`, transparently and atomically re-key its KDF parameters to the Desktop High Security profile (m=256 MiB, t=3, p=1) without re-encrypting bulk data, preserving the random envelope Vault Key, device identity, and read access to legacy V1 chunks.

**Architecture:** A spawned post-unlock task runs `VaultKeyStore::migrate_kdf_params_if_needed`. It re-derives `master_key` with a fresh salt + new params, re-wraps the unchanged `envelope_key` under the new KEK, re-seals the local device X25519 private key under the new master, captures the old deterministic V1 `vault_key` sealed under the (params-independent) `envelope_key` as `legacy_read_key`, and writes everything in a **single SQLite transaction** (`db::migrate_kdf_params_tx`) — all-or-nothing. The deterministic MAC keys are dead weight (no consumers) so nothing is re-MAC'd; the local cache is rebuildable so it is invalidated.

**Tech Stack:** Rust (edition 2024), `sqlx`/SQLite, `argon2`, `aes-kw`, `aes-gcm`, `hkdf`, `zeroize` (`KeyBytes`), `tokio`. Spec: `docs/superpowers/specs/2026-05-25-alpha-B-a-argon2id-params-bump-design.md`.

**Conventions for every task:** `cargo` runs from repo root `C:\Users\Przemek\Desktop\aplikacje\omnidrive`. Pre-push hook runs `cargo fmt --all -- --check` + `cargo clippy --workspace -- -D warnings`; never `--no-verify`. No comments in production code (CLAUDE.md). All transient keys are `KeyBytes`/`SecretBox` (zeroized on drop).

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `angeld/src/vault.rs` | `TARGET_*` consts, `needs_kdf_migration`, `target_kdf_params`, `LEGACY_READ_KEY_INFO`, `seal_legacy_read_key`/`open_legacy_read_key`, `VaultKeyStore::migrate_kdf_params_if_needed`, `vault_key_for_v1_read`, spawn in `unlock()` | Modify |
| `angeld/src/identity.rs` | `reseal_local_device_private_key` | Modify |
| `angeld/src/db.rs` | `legacy_read_key` column (init), `get_legacy_read_key`, `count_active_devices`, `migrate_kdf_params_tx` (+`test-helpers` failpoint) | Modify |
| `angeld/src/downloader.rs` | Use `vault_key_for_v1_read` for the V1 decryption key at chunk-decrypt sites | Modify |

Migration parameter set is daemon policy → lives in `vault.rs` beside the existing `DEFAULT_*` consts (`vault.rs:17-20`). Sealing helpers live in `vault.rs` (they compose `omnidrive-core` primitives). No `omnidrive-core` change is required.

---

## Task 1: Target params, migration predicate, and `legacy_read_key` column

**Files:**
- Modify: `angeld/src/vault.rs` (after the `DEFAULT_*` consts, `vault.rs:17-23`)
- Modify: `angeld/src/db.rs` (the `init_db` column-ensure block near `db.rs:1022`)
- Test: `angeld/src/vault.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test** (append to `vault.rs` tests module)

```rust
#[test]
fn target_params_are_desktop_high_security() {
    assert_eq!(TARGET_PARAMETER_SET_VERSION, 2);
    assert_eq!(TARGET_MEMORY_COST_KIB, 262_144);
    assert_eq!(TARGET_TIME_COST, 3);
    assert_eq!(TARGET_LANES, 1);
}

#[test]
fn needs_migration_compares_version() {
    assert!(needs_kdf_migration(1));
    assert!(!needs_kdf_migration(2));
    assert!(!needs_kdf_migration(3));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p angeld --lib target_params_are_desktop_high_security`
Expected: FAIL — `cannot find value TARGET_PARAMETER_SET_VERSION`.

- [ ] **Step 3: Write minimal implementation** (in `vault.rs`, after `DEFAULT_LANES`, `vault.rs:20`)

```rust
const TARGET_PARAMETER_SET_VERSION: u32 = 2;
const TARGET_MEMORY_COST_KIB: u32 = 262_144;
const TARGET_TIME_COST: u32 = 3;
const TARGET_LANES: u32 = 1;

fn needs_kdf_migration(current_parameter_set_version: i64) -> bool {
    current_parameter_set_version < i64::from(TARGET_PARAMETER_SET_VERSION)
}

fn target_kdf_params(new_salt: Vec<u8>) -> RootKdfParams {
    RootKdfParams::new(
        TARGET_PARAMETER_SET_VERSION,
        new_salt,
        TARGET_MEMORY_COST_KIB,
        TARGET_TIME_COST,
        TARGET_LANES,
    )
}
```

- [ ] **Step 4: Add the `legacy_read_key` column** (in `db.rs::init_db`, alongside the existing `ensure_column_exists(&pool, "vault_state", "encrypted_vault_key", "BLOB")` at `db.rs:1022`)

```rust
ensure_column_exists(&pool, "vault_state", "legacy_read_key", "BLOB").await?;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p angeld --lib needs_migration_compares_version target_params_are_desktop_high_security`
Expected: PASS (2 tests).

- [ ] **Step 6: Verify it compiles workspace-wide**

Run: `cargo check --workspace`
Expected: Finished, no errors.

- [ ] **Step 7: Commit**

```bash
git add angeld/src/vault.rs angeld/src/db.rs
git commit -m "feat(crypto): α.B.a target KDF params + legacy_read_key column"
```

---

## Task 2: `legacy_read_key` seal / open helpers

The old deterministic V1 `vault_key` is sealed under a subkey of the random `envelope_key` (params-independent), with `vault_id` as AAD, using AES-256-GCM (`crypto::encrypt_secret`).

**Files:**
- Modify: `angeld/src/vault.rs` (free functions near `to_root_kdf_params`, `vault.rs:753`)
- Test: `angeld/src/vault.rs` tests module

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn legacy_read_key_round_trips_under_envelope() {
    let envelope = generate_random_key();
    let old_vault_key = generate_random_key();
    let blob = seal_legacy_read_key(&envelope, &old_vault_key, "vault-abc").unwrap();
    let opened = open_legacy_read_key(&envelope, &blob, "vault-abc").unwrap();
    assert_eq!(opened.as_ref() as &[u8], old_vault_key.as_ref() as &[u8]);
}

#[test]
fn legacy_read_key_rejects_wrong_aad() {
    let envelope = generate_random_key();
    let old_vault_key = generate_random_key();
    let blob = seal_legacy_read_key(&envelope, &old_vault_key, "vault-abc").unwrap();
    assert!(open_legacy_read_key(&envelope, &blob, "vault-XYZ").is_err());
}

#[test]
fn legacy_read_key_rejects_wrong_envelope() {
    let envelope = generate_random_key();
    let other = generate_random_key();
    let old_vault_key = generate_random_key();
    let blob = seal_legacy_read_key(&envelope, &old_vault_key, "vault-abc").unwrap();
    assert!(open_legacy_read_key(&other, &blob, "vault-abc").is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p angeld --lib legacy_read_key_round_trips_under_envelope`
Expected: FAIL — `cannot find function seal_legacy_read_key`.

- [ ] **Step 3: Write minimal implementation** (in `vault.rs`; add `LEGACY_READ_KEY_INFO` beside the other `*_INFO` consts and import `derive_subkey`, `encrypt_secret`, `decrypt_secret`, `generate_random_key` which are already imported at `vault.rs:4-7`)

```rust
const LEGACY_READ_KEY_INFO: &[u8] = b"legacy-read-key-v1";

fn seal_legacy_read_key(
    envelope_key: &KeyBytes,
    old_vault_key: &KeyBytes,
    vault_id: &str,
) -> Result<Vec<u8>, VaultError> {
    let subkey = derive_subkey(envelope_key, LEGACY_READ_KEY_INFO)?;
    let blob = encrypt_secret(&subkey, old_vault_key.as_ref(), vault_id.as_bytes())?;
    Ok(blob)
}

fn open_legacy_read_key(
    envelope_key: &KeyBytes,
    blob: &[u8],
    vault_id: &str,
) -> Result<KeyBytes, VaultError> {
    let subkey = derive_subkey(envelope_key, LEGACY_READ_KEY_INFO)?;
    let plaintext = decrypt_secret(&subkey, blob, vault_id.as_bytes())?;
    let bytes: [u8; KEY_LEN] = plaintext
        .as_slice()
        .try_into()
        .map_err(|_| VaultError::InvalidConfig("legacy_read_key has invalid length"))?;
    Ok(KeyBytes::from(bytes))
}
```

(`KEY_LEN` is already imported from `omnidrive_core::crypto`. If not, add it to the `use` at `vault.rs:4-7`.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p angeld --lib legacy_read_key_`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add angeld/src/vault.rs
git commit -m "feat(crypto): α.B.a seal/open legacy_read_key under envelope key"
```

---

## Task 3: Re-seal the local device private key under a new master

**Files:**
- Modify: `angeld/src/identity.rs` (after `get_device_private_key`, `identity.rs:158`)
- Test: `angeld/src/identity.rs` tests module

- [ ] **Step 1: Write the failing test** (append to `identity.rs` tests)

```rust
#[test]
fn reseal_private_key_rebinds_to_new_master() {
    let old_master = [0x11u8; 32];
    let new_master = [0x22u8; 32];
    let private_key = [0x33u8; 32];

    let old_kek = derive_identity_kek(&old_master).unwrap();
    let old_blob = encrypt_private_key(&old_kek, &private_key).unwrap();

    let new_blob = reseal_local_device_private_key(&old_master, &new_master, &old_blob).unwrap();

    let new_kek = derive_identity_kek(&new_master).unwrap();
    assert_eq!(decrypt_private_key(&new_kek, &new_blob).unwrap(), private_key);
    assert!(decrypt_private_key(&old_kek, &new_blob).is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p angeld --lib reseal_private_key_rebinds_to_new_master`
Expected: FAIL — `cannot find function reseal_local_device_private_key`.

- [ ] **Step 3: Write minimal implementation** (in `identity.rs`, after `get_device_private_key`)

```rust
pub fn reseal_local_device_private_key(
    old_master: &[u8],
    new_master: &[u8],
    current_blob: &[u8],
) -> Result<Vec<u8>, IdentityError> {
    let old_kek = derive_identity_kek(old_master)?;
    let private_key = decrypt_private_key(&old_kek, current_blob)?;
    let new_kek = derive_identity_kek(new_master)?;
    encrypt_private_key(&new_kek, &private_key)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p angeld --lib reseal_private_key_rebinds_to_new_master`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add angeld/src/identity.rs
git commit -m "feat(identity): α.B.a reseal local device private key to new master"
```

---

## Task 4: Transactional DB orchestrator `db::migrate_kdf_params_tx`

All four writes in one `pool.begin()` transaction. A `test-helpers`-gated failpoint forces a pre-commit error so the rollback path is deterministically testable.

**Files:**
- Modify: `angeld/src/db.rs` (after `rotate_vault_key_only`, `db.rs:6618`)
- Modify: `angeld/Cargo.toml` (only if a `test-helpers` feature does not already exist — it does, per α.A.b)
- Test: `angeld/src/db.rs` tests module

- [ ] **Step 1: Write the failing tests** (append to `db.rs` tests; uses the existing in-memory pool helper pattern — follow a neighboring `db.rs` test for `init_db` setup)

```rust
#[tokio::test]
async fn migrate_kdf_params_tx_writes_all_fields() {
    let pool = test_pool().await; // existing helper: init_db on a temp/in-memory DB
    seed_vault_state_v1(&pool).await; // helper: insert vault_state id=1 + vault_config id=1 (version 1)

    let writes = KdfMigrationWrites {
        new_salt: &[7u8; 16],
        new_argon2_params_json: r#"{"mode":"LOCAL_VAULT","parameter_set_version":2,"memory_cost_kib":262144,"time_cost":3,"lanes":1}"#,
        new_param_version: 2,
        new_memory_cost_kib: 262_144,
        new_time_cost: 3,
        new_lanes: 1,
        new_encrypted_vault_key: &[9u8; WRAPPED_KEY_LEN],
        legacy_read_key_blob: &[5u8; 60],
        new_encrypted_device_private_key: Some(&[6u8; 60]),
    };
    migrate_kdf_params_tx(&pool, writes).await.unwrap();

    let cfg = get_vault_config(&pool).await.unwrap().unwrap();
    assert_eq!(cfg.parameter_set_version, 2);
    assert_eq!(cfg.memory_cost_kib, 262_144);
    assert_eq!(cfg.salt, vec![7u8; 16]);
    let v = get_vault_params(&pool).await.unwrap().unwrap();
    assert_eq!(v.encrypted_vault_key.unwrap(), vec![9u8; WRAPPED_KEY_LEN]);
    assert_eq!(get_legacy_read_key(&pool).await.unwrap().unwrap(), vec![5u8; 60]);
}

#[tokio::test]
async fn migrate_kdf_params_tx_rolls_back_on_failure() {
    let pool = test_pool().await;
    seed_vault_state_v1(&pool).await;

    set_migration_failpoint(true);
    let writes = KdfMigrationWrites {
        new_salt: &[7u8; 16],
        new_argon2_params_json: "{}",
        new_param_version: 2,
        new_memory_cost_kib: 262_144,
        new_time_cost: 3,
        new_lanes: 1,
        new_encrypted_vault_key: &[9u8; WRAPPED_KEY_LEN],
        legacy_read_key_blob: &[5u8; 60],
        new_encrypted_device_private_key: Some(&[6u8; 60]),
    };
    let result = migrate_kdf_params_tx(&pool, writes).await;
    set_migration_failpoint(false);

    assert!(result.is_err());
    let cfg = get_vault_config(&pool).await.unwrap().unwrap();
    assert_eq!(cfg.parameter_set_version, 1, "version must be unchanged after rollback");
    assert!(get_legacy_read_key(&pool).await.unwrap().is_none(), "no legacy key written on rollback");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p angeld --lib --features test-helpers migrate_kdf_params_tx`
Expected: FAIL — `cannot find ... KdfMigrationWrites / migrate_kdf_params_tx`.

- [ ] **Step 3: Write minimal implementation** (in `db.rs`, after `rotate_vault_key_only`)

```rust
pub struct KdfMigrationWrites<'a> {
    pub new_salt: &'a [u8],
    pub new_argon2_params_json: &'a str,
    pub new_param_version: i64,
    pub new_memory_cost_kib: i64,
    pub new_time_cost: i64,
    pub new_lanes: i64,
    pub new_encrypted_vault_key: &'a [u8],
    pub legacy_read_key_blob: &'a [u8],
    pub new_encrypted_device_private_key: Option<&'a [u8]>,
}

#[cfg(feature = "test-helpers")]
static MIGRATION_FAILPOINT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "test-helpers")]
pub fn set_migration_failpoint(on: bool) {
    MIGRATION_FAILPOINT.store(on, std::sync::atomic::Ordering::SeqCst);
}

pub async fn get_legacy_read_key(pool: &SqlitePool) -> Result<Option<Vec<u8>>, sqlx::Error> {
    let row: Option<(Option<Vec<u8>>,)> =
        sqlx::query_as("SELECT legacy_read_key FROM vault_state WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|r| r.0))
}

pub async fn migrate_kdf_params_tx(
    pool: &SqlitePool,
    w: KdfMigrationWrites<'_>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        "UPDATE vault_config SET salt = ?, parameter_set_version = ?, \
         memory_cost_kib = ?, time_cost = ?, lanes = ? WHERE id = 1",
    )
    .bind(w.new_salt)
    .bind(w.new_param_version)
    .bind(w.new_memory_cost_kib)
    .bind(w.new_time_cost)
    .bind(w.new_lanes)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE vault_state SET master_key_salt = ?, argon2_params = ?, \
         encrypted_vault_key = ?, legacy_read_key = ? WHERE id = 1",
    )
    .bind(w.new_salt)
    .bind(w.new_argon2_params_json)
    .bind(w.new_encrypted_vault_key)
    .bind(w.legacy_read_key_blob)
    .execute(&mut *tx)
    .await?;

    if let Some(blob) = w.new_encrypted_device_private_key {
        sqlx::query("UPDATE local_device_identity SET encrypted_private_key = ? WHERE id = 1")
            .bind(blob)
            .execute(&mut *tx)
            .await?;
    }

    #[cfg(feature = "test-helpers")]
    if MIGRATION_FAILPOINT.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(sqlx::Error::Protocol("migration failpoint".into()));
    }

    tx.commit().await
}
```

(On the `return Err` path, `tx` is dropped without `commit`, which rolls back — sqlx default. `vault_key_generation` is intentionally not written: the envelope key is unchanged, so DEKs keep pointing at the right generation.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p angeld --lib --features test-helpers migrate_kdf_params_tx`
Expected: PASS (2 tests).

- [ ] **Step 5: Verify default build still compiles (failpoint is feature-gated)**

Run: `cargo check -p angeld`
Expected: Finished, no errors (no `test-helpers`).

- [ ] **Step 6: Commit**

```bash
git add angeld/src/db.rs
git commit -m "feat(db): α.B.a atomic migrate_kdf_params_tx + legacy_read_key getter"
```

---

## Task 5: `VaultKeyStore::migrate_kdf_params_if_needed` (orchestration)

Idempotent, single-flight, multi-device-safe. Reads old material from `self.inner`, derives the new master from the passphrase, calls `db::migrate_kdf_params_tx`, then updates in-memory keys.

**Files:**
- Modify: `angeld/src/vault.rs` (new method on `impl VaultKeyStore`, near `rotate_vault_key`, `vault.rs:433`; add `count_active_devices` to `db.rs`)
- Modify: `angeld/src/db.rs` (`count_active_devices`)
- Test: `angeld/src/vault.rs` tests module

- [ ] **Step 1: Add `db::count_active_devices`** (in `db.rs`, near the other `devices` queries)

```rust
pub async fn count_active_devices(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM devices WHERE revoked_at IS NULL")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}
```

- [ ] **Step 2: Write the failing tests** (append to `vault.rs` tests; mirror the existing unlock-based tests around `vault.rs:810-1000` for setup — `VaultKeyStore::new()`, `unlock`, an in-memory pool seeded to params version 1 via `init_db`)

```rust
#[tokio::test]
async fn migration_upgrades_params_and_preserves_envelope() -> Result<(), VaultError> {
    let pool = test_pool_v1().await; // init_db; first unlock will create the v2 envelope key at gen 1
    let store = VaultKeyStore::new();
    store.unlock(&pool, "pass-123").await?;

    let evk_before = store.require_envelope_key().await?;
    let outcome = store.migrate_kdf_params_if_needed(&pool, "pass-123").await?;
    assert!(matches!(outcome, MigrationOutcome::Migrated { from: 1, to: 2 }));

    let cfg = db::get_vault_config(&pool).await?.unwrap();
    assert_eq!(cfg.parameter_set_version, 2);
    assert_eq!(cfg.memory_cost_kib, 262_144);

    // Re-unlock with the SAME passphrase under the new params must succeed and yield the SAME envelope key.
    let store2 = VaultKeyStore::new();
    store2.unlock(&pool, "pass-123").await?;
    assert_eq!(
        store2.require_envelope_key().await?.as_ref() as &[u8],
        evk_before.as_ref() as &[u8]
    );
    Ok(())
}

#[tokio::test]
async fn migration_is_idempotent() -> Result<(), VaultError> {
    let pool = test_pool_v1().await;
    let store = VaultKeyStore::new();
    store.unlock(&pool, "pass-123").await?;
    assert!(matches!(store.migrate_kdf_params_if_needed(&pool, "pass-123").await?, MigrationOutcome::Migrated { .. }));
    assert!(matches!(store.migrate_kdf_params_if_needed(&pool, "pass-123").await?, MigrationOutcome::Skipped));
    Ok(())
}

#[tokio::test]
async fn migration_preserves_legacy_v1_read_key() -> Result<(), VaultError> {
    let pool = test_pool_v1().await;
    let store = VaultKeyStore::new();
    store.unlock(&pool, "pass-123").await?;
    let v1_key_before = store.require_key().await?; // deterministic vault_key under old master

    store.migrate_kdf_params_if_needed(&pool, "pass-123").await?;

    let recovered = store.vault_key_for_v1_read(&pool).await?;
    assert_eq!(recovered.as_ref() as &[u8], v1_key_before.as_ref() as &[u8]);
    Ok(())
}

#[tokio::test]
async fn migration_declines_on_multi_device() -> Result<(), VaultError> {
    let pool = test_pool_v1().await;
    seed_two_active_devices(&pool).await; // helper: 2 rows in devices, revoked_at NULL
    let store = VaultKeyStore::new();
    store.unlock(&pool, "pass-123").await?;
    let outcome = store.migrate_kdf_params_if_needed(&pool, "pass-123").await?;
    assert!(matches!(outcome, MigrationOutcome::Declined { .. }));
    let cfg = db::get_vault_config(&pool).await?.unwrap();
    assert_eq!(cfg.parameter_set_version, 1, "declined migration must not touch params");
    Ok(())
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p angeld --lib migration_`
Expected: FAIL — `cannot find type MigrationOutcome` / method `migrate_kdf_params_if_needed`.

- [ ] **Step 4: Write minimal implementation** (in `vault.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationOutcome {
    Migrated { from: i64, to: i64 },
    Skipped,
    Declined { reason: &'static str },
}

impl VaultKeyStore {
    pub async fn migrate_kdf_params_if_needed(
        &self,
        pool: &SqlitePool,
        passphrase: &str,
    ) -> Result<MigrationOutcome, VaultError> {
        let cfg = db::get_vault_config(pool)
            .await?
            .ok_or(VaultError::InvalidConfig("no vault_config found"))?;
        if !needs_kdf_migration(cfg.parameter_set_version) {
            return Ok(MigrationOutcome::Skipped);
        }
        if db::count_active_devices(pool).await? > 1 {
            return Ok(MigrationOutcome::Declined {
                reason: "multi-device vault: per-device KDF params deferred to α.C",
            });
        }

        let old_master = self.require_master_key().await?;
        let envelope_key = self.require_envelope_key().await?;
        let old_vault_key = self.require_key().await?;
        let vault = db::get_vault_params(pool)
            .await?
            .ok_or(VaultError::InvalidConfig("no vault_state found"))?;

        let new_salt = RootKdfParams::random_salt();
        let new_params = target_kdf_params(new_salt.to_vec());
        let new_root = derive_root_keys(passphrase.as_bytes(), &new_params)?;
        let new_encrypted_vault_key = wrap_key(&new_root.kek, &envelope_key)?;
        let legacy_blob = seal_legacy_read_key(&envelope_key, &old_vault_key, &vault.vault_id)?;

        let new_device_blob = match db::get_local_device_identity(pool).await? {
            Some(d) => match d.encrypted_private_key {
                Some(old_blob) => Some(
                    identity::reseal_local_device_private_key(
                        old_master.as_ref(),
                        new_root.master_key.as_ref(),
                        &old_blob,
                    )
                    .map_err(|_| VaultError::InvalidConfig("device key reseal failed"))?,
                ),
                None => None,
            },
            None => None,
        };

        let argon2_params_json = format!(
            r#"{{"mode":"LOCAL_VAULT","parameter_set_version":{},"memory_cost_kib":{},"time_cost":{},"lanes":{}}}"#,
            new_params.parameter_set_version,
            new_params.memory_cost_kib,
            new_params.time_cost,
            new_params.lanes
        );

        db::migrate_kdf_params_tx(
            pool,
            db::KdfMigrationWrites {
                new_salt: &new_salt,
                new_argon2_params_json: &argon2_params_json,
                new_param_version: i64::from(new_params.parameter_set_version),
                new_memory_cost_kib: i64::from(new_params.memory_cost_kib),
                new_time_cost: i64::from(new_params.time_cost),
                new_lanes: i64::from(new_params.lanes),
                new_encrypted_vault_key: &new_encrypted_vault_key,
                legacy_read_key_blob: &legacy_blob,
                new_encrypted_device_private_key: new_device_blob.as_deref(),
            },
        )
        .await?;

        *self.inner.write().await = Some(UnlockedVaultKeys::with_envelope_key(
            new_root.master_key,
            new_root.vault_key,
            envelope_key,
        ));

        Ok(MigrationOutcome::Migrated {
            from: cfg.parameter_set_version,
            to: i64::from(TARGET_PARAMETER_SET_VERSION),
        })
    }
}
```

(`identity` is reachable as `crate::identity`; add `use crate::identity;` if not already present in `vault.rs` — `vault.rs:1` imports `crate::{db, identity}` already.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p angeld --lib migration_`
Expected: PASS (4 tests). (`vault_key_for_v1_read` is implemented in Task 6; if `migration_preserves_legacy_v1_read_key` fails to compile here, implement Task 6 Step 3 first, then re-run — these two tasks are adjacent.)

- [ ] **Step 6: Commit**

```bash
git add angeld/src/vault.rs angeld/src/db.rs
git commit -m "feat(vault): α.B.a migrate_kdf_params_if_needed orchestration"
```

---

## Task 6: Legacy V1 read fallback in the downloader

After migration the in-memory deterministic `vault_key` is the *new* one, which cannot read pre-migration V1 chunks. `vault_key_for_v1_read` returns the captured `legacy_read_key` when present, else the current `vault_key`.

**Files:**
- Modify: `angeld/src/vault.rs` (accessor on `impl VaultKeyStore`)
- Modify: `angeld/src/downloader.rs` (the 5 chunk-decrypt sites that call `require_key()`: lines 281, 377, 497, 672, 791 — replace with the new accessor)
- Test: `angeld/src/vault.rs` tests module (the accessor is exercised by `migration_preserves_legacy_v1_read_key` from Task 5)

- [ ] **Step 1: Write the failing test** (a pre-migration case ensuring the accessor returns the current key when no legacy key exists)

```rust
#[tokio::test]
async fn vault_key_for_v1_read_falls_back_to_current_when_no_legacy() -> Result<(), VaultError> {
    let pool = test_pool_v1().await;
    let store = VaultKeyStore::new();
    store.unlock(&pool, "pass-123").await?;
    let current = store.require_key().await?;
    let v1 = store.vault_key_for_v1_read(&pool).await?;
    assert_eq!(v1.as_ref() as &[u8], current.as_ref() as &[u8]);
    Ok(())
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p angeld --lib vault_key_for_v1_read_falls_back_to_current_when_no_legacy`
Expected: FAIL — method `vault_key_for_v1_read` not found.

- [ ] **Step 3: Write minimal implementation** (in `vault.rs`, on `impl VaultKeyStore`)

```rust
pub async fn vault_key_for_v1_read(&self, pool: &SqlitePool) -> Result<KeyBytes, VaultError> {
    let blob = db::get_legacy_read_key(pool).await?;
    match blob {
        Some(blob) => {
            let envelope_key = self.require_envelope_key().await?;
            let vault = db::get_vault_params(pool)
                .await?
                .ok_or(VaultError::InvalidConfig("no vault_state found"))?;
            open_legacy_read_key(&envelope_key, &blob, &vault.vault_id)
        }
        None => self.require_key().await,
    }
}
```

- [ ] **Step 4: Swap the downloader read sites** (`downloader.rs`; at each of lines ~281, 377, 497, 672, 791 replace)

```rust
let vault_key = self.vault_keys.require_key().await?;
```
with
```rust
let vault_key = self.vault_keys.vault_key_for_v1_read(&self.pool).await?;
```

- [ ] **Step 5: Run tests + workspace check**

Run: `cargo test -p angeld --lib vault_key_for_v1_read_falls_back_to_current_when_no_legacy migration_preserves_legacy_v1_read_key`
Expected: PASS (2 tests).
Run: `cargo check --workspace`
Expected: Finished, no errors.

- [ ] **Step 6: Commit**

```bash
git add angeld/src/vault.rs angeld/src/downloader.rs
git commit -m "feat(downloader): α.B.a read legacy V1 chunks via legacy_read_key"
```

---

## Task 7: Spawn the migration after a successful unlock

The migration runs as a non-blocking background task at the end of `unlock()`, so every unlock path (lock-screen, boot restore, Windows Hello) benefits. Failure logs and is retried next unlock; it never affects unlock success.

**Files:**
- Modify: `angeld/src/vault.rs` (`unlock()`, before `Ok(UnlockResult { .. })` at `vault.rs:250`)
- Test: `angeld/tests/e2e_basic.rs` or `angeld/src/vault.rs` tests (integration: unlock a v1 vault, await migration, assert params=2)

- [ ] **Step 1: Write the failing test** (append to `vault.rs` tests; awaits the spawned task by polling the param version with a bounded loop — mirrors the α.A.b bounded-poll pattern)

```rust
#[tokio::test]
async fn unlock_spawns_background_migration() -> Result<(), VaultError> {
    let pool = test_pool_v1().await;
    let store = VaultKeyStore::new();
    store.unlock(&pool, "pass-123").await?; // returns immediately

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let v = db::get_vault_config(&pool).await?.unwrap().parameter_set_version;
        if v == 2 {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!("background migration did not complete: params still v{v}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Ok(())
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p angeld --lib unlock_spawns_background_migration`
Expected: FAIL — params stay v1 (no spawn yet).

- [ ] **Step 3: Write minimal implementation** (in `unlock()`, immediately before the final `Ok(UnlockResult { initialized, unlocked: true })` at `vault.rs:250`)

```rust
if db::get_vault_config(pool)
    .await
    .ok()
    .flatten()
    .map(|c| needs_kdf_migration(c.parameter_set_version))
    .unwrap_or(false)
{
    let store = self.clone();
    let pool = pool.clone();
    let passphrase = secrecy::SecretString::from(passphrase.to_owned());
    tokio::spawn(async move {
        use secrecy::ExposeSecret;
        match store
            .migrate_kdf_params_if_needed(&pool, passphrase.expose_secret())
            .await
        {
            Ok(MigrationOutcome::Migrated { from, to }) => {
                info!("[KDF-MIGRATION] params upgraded v{from} -> v{to}");
            }
            Ok(MigrationOutcome::Declined { reason }) => {
                warn!("[KDF-MIGRATION] declined: {reason}");
            }
            Ok(MigrationOutcome::Skipped) => {}
            Err(e) => warn!("[KDF-MIGRATION] failed (will retry next unlock): {e}"),
        }
    });
}
```

(`VaultKeyStore` is `#[derive(Clone)]` at `vault.rs:25`; `SqlitePool` is `Clone`. `secrecy::{SecretString, ExposeSecret}` — `ExposeSecret` is already imported at `vault.rs:9`; add `SecretString` to that `use` if absent. The passphrase copy lives in a `SecretString` and is zeroized on drop.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p angeld --lib unlock_spawns_background_migration`
Expected: PASS.

- [ ] **Step 5: Full suite + lints (pre-push parity)**

Run: `cargo test -p angeld --lib` then `cargo test -p angeld --lib --features test-helpers` then `cargo clippy --workspace -- -D warnings` then `cargo fmt --all -- --check`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add angeld/src/vault.rs
git commit -m "feat(vault): α.B.a spawn background KDF migration after unlock"
```

---

## Spec Coverage Check (self-review)

| Spec requirement | Task |
|---|---|
| §5.1 target param-set + version predicate | Task 1 |
| §6 `legacy_read_key` nullable column | Task 1 |
| §5.3/§5.6 capture + seal legacy_read_key | Task 2 |
| §3 re-seal device private key | Task 3 |
| §5.4 single atomic SQLite transaction | Task 4 |
| §8.1 rollback on failure (deterministic test) | Task 4 |
| §5.2 trigger / idempotency / single-device decline | Task 5 |
| §3 re-wrap envelope_key, generation unchanged | Task 5 |
| §5.6 legacy V1 read path | Task 6 |
| §5.2 spawned post-unlock task (non-blocking) | Task 7 |
| §8.2 acceptance: envelope preserved, device reseals, V1 readable, idempotent, rollback | Tasks 4–7 |

**Deferred (out of scope, per spec §1.2 / §9):** bulk V1→V2 re-encryption (α.B.c), ML-KEM (α.B.b), per-device params (α.C). MAC recompute is a documented no-op (spec §4). Local cache invalidation (spec §5.5) is non-fatal and rebuildable — not a separate task; if cache decryption errors after migration the existing cache-miss path re-hydrates. The migration does not need to proactively clear it for correctness.

**Test-helper notes:** `test_pool()`, `test_pool_v1()`, `seed_vault_state_v1()`, `seed_two_active_devices()` follow the existing `db.rs`/`vault.rs` test harness conventions (in-memory or temp SQLite via `init_db`; `test_pool_v1` then performs a first `unlock` so the v2 envelope key exists at generation 1 with `parameter_set_version=1`). Reuse the nearest existing helper rather than duplicating.

---

## Open follow-ups (NOT in this plan)
- Workspace version bump for α.B.a closure happens after acceptance (per project release convention), not inside these tasks.
- α.B.c (bulk V1→V2 re-encryption) consumes `legacy_read_key` and ultimately drops the column when zero V1 chunks remain.
