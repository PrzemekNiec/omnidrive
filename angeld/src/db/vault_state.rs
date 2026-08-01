use crate::db::*;
use sqlx::FromRow;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct VaultRecord {
    pub id: i64,
    pub master_key_salt: Vec<u8>,
    pub argon2_params: String,
    pub vault_id: String,
    pub vault_format_version: Option<i64>,
    pub encrypted_vault_key: Option<Vec<u8>>,
    pub vault_key_generation: Option<i64>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct VaultConfigRecord {
    pub id: i64,
    pub salt: Vec<u8>,
    pub parameter_set_version: i64,
    pub memory_cost_kib: i64,
    pub time_cost: i64,
    pub lanes: i64,
}

#[allow(dead_code)]
pub async fn set_vault_params(
    pool: &SqlitePool,
    salt: &[u8],
    params_json: &str,
    vault_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO vault_state (id, master_key_salt, argon2_params, vault_id)
        VALUES (1, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            master_key_salt = excluded.master_key_salt,
            argon2_params = excluded.argon2_params,
            vault_id = excluded.vault_id
        "#,
    )
    .bind(salt)
    .bind(params_json)
    .bind(vault_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_vault_params(pool: &SqlitePool) -> Result<Option<VaultRecord>, sqlx::Error> {
    sqlx::query_as::<_, VaultRecord>(
        r#"
        SELECT id, master_key_salt, argon2_params, vault_id,
               vault_format_version, encrypted_vault_key, vault_key_generation
        FROM vault_state
        WHERE id = 1
        "#,
    )
    .fetch_optional(pool)
    .await
}

pub async fn store_encrypted_vault_key(
    pool: &SqlitePool,
    encrypted_vault_key: &[u8],
    vault_key_generation: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE vault_state SET encrypted_vault_key = ?, vault_key_generation = ?, \
         vault_format_version = 2 WHERE id = 1",
    )
    .bind(encrypted_vault_key)
    .bind(vault_key_generation)
    .execute(pool)
    .await?;
    Ok(())
}

// ── DEK (Data Encryption Key) persistence ───────────────────────────────

#[allow(dead_code)]
#[derive(Clone, Debug, FromRow)]
pub struct WrappedDekRecord {
    pub dek_id: i64,
    pub inode_id: i64,
    pub wrapped_dek: Vec<u8>,
    pub key_version: i64,
    pub vault_key_gen: i64,
    pub created_at: i64,
}

/// Fetch the latest wrapped DEK for a given inode (highest key_version).
pub async fn get_wrapped_dek(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Option<WrappedDekRecord>, sqlx::Error> {
    sqlx::query_as::<_, WrappedDekRecord>(
        "SELECT dek_id, inode_id, wrapped_dek, key_version, vault_key_gen, created_at \
         FROM data_encryption_keys \
         WHERE inode_id = ? \
         ORDER BY key_version DESC \
         LIMIT 1",
    )
    .bind(inode_id)
    .fetch_optional(pool)
    .await
}

/// Insert a new wrapped DEK for an inode. Returns the assigned dek_id.
pub async fn insert_wrapped_dek(
    pool: &SqlitePool,
    inode_id: i64,
    wrapped_dek: &[u8],
    key_version: i64,
    vault_key_gen: i64,
) -> Result<i64, sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let result = sqlx::query(
        "INSERT INTO data_encryption_keys (inode_id, wrapped_dek, key_version, vault_key_gen, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(inode_id)
    .bind(wrapped_dek)
    .bind(key_version)
    .bind(vault_key_gen)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

/// Binds a pack to the DEK that encrypted it. The pack — not the inode — owns the key,
/// so every reference to the pack (dedup, conflict copy, restore) resolves the same one.
pub async fn set_pack_dek(
    pool: &SqlitePool,
    pack_id: &str,
    dek_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO pack_deks (pack_id, dek_id) VALUES (?, ?) \
         ON CONFLICT(pack_id) DO NOTHING",
    )
    .bind(pack_id)
    .bind(dek_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_pack_dek_id(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, i64>("SELECT dek_id FROM pack_deks WHERE pack_id = ?")
        .bind(pack_id)
        .fetch_optional(pool)
        .await
}

pub async fn get_wrapped_dek_by_id(
    pool: &SqlitePool,
    dek_id: i64,
) -> Result<Option<WrappedDekRecord>, sqlx::Error> {
    sqlx::query_as::<_, WrappedDekRecord>(
        "SELECT dek_id, inode_id, wrapped_dek, key_version, vault_key_gen, created_at \
         FROM data_encryption_keys WHERE dek_id = ?",
    )
    .bind(dek_id)
    .fetch_optional(pool)
    .await
}

/// Next free `key_version` for an inode. `data_encryption_keys` enforces
/// `UNIQUE(inode_id, key_version)` and that constraint cannot be dropped under the
/// additive-migration model, so one inode creating several pack DEKs must count up.
pub async fn next_dek_key_version(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<i64, sqlx::Error> {
    let current: Option<i64> = sqlx::query_scalar(
        "SELECT MAX(key_version) FROM data_encryption_keys WHERE inode_id = ?",
    )
    .bind(inode_id)
    .fetch_one(pool)
    .await?;
    Ok(current.unwrap_or(0) + 1)
}

/// Resolves which inode's write created `pack_id`, by the earliest revision that
/// references any of its chunks. Used to backfill `pack_deks` for packs written
/// before the key moved from the inode to the pack.
pub async fn creating_inode_for_pack(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT fr.inode_id \
         FROM pack_locations pl \
         JOIN chunk_refs cr ON cr.chunk_id = pl.chunk_id \
         JOIN file_revisions fr ON fr.revision_id = cr.revision_id \
         WHERE pl.pack_id = ? \
         ORDER BY fr.revision_id ASC LIMIT 1",
    )
    .bind(pack_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_vault_config(pool: &SqlitePool) -> Result<Option<VaultConfigRecord>, sqlx::Error> {
    sqlx::query_as::<_, VaultConfigRecord>(
        r#"
        SELECT id, salt, parameter_set_version, memory_cost_kib, time_cost, lanes
        FROM vault_config
        WHERE id = 1
        "#,
    )
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn set_vault_config(
    pool: &SqlitePool,
    salt: &[u8],
    parameter_set_version: i64,
    memory_cost_kib: i64,
    time_cost: i64,
    lanes: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO vault_config (
            id,
            salt,
            parameter_set_version,
            memory_cost_kib,
            time_cost,
            lanes
        )
        VALUES (1, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            salt = excluded.salt,
            parameter_set_version = excluded.parameter_set_version,
            memory_cost_kib = excluded.memory_cost_kib,
            time_cost = excluded.time_cost,
            lanes = excluded.lanes
        "#,
    )
    .bind(salt)
    .bind(parameter_set_version)
    .bind(memory_cost_kib)
    .bind(time_cost)
    .bind(lanes)
    .execute(pool)
    .await?;

    Ok(())
}

// ── Vault Key rotation queries ────────────────────────────────────────────

/// Fetch all wrapped DEKs (for re-wrapping during key rotation).
#[allow(dead_code)]
pub async fn get_all_wrapped_deks(pool: &SqlitePool) -> Result<Vec<WrappedDekRecord>, sqlx::Error> {
    sqlx::query_as::<_, WrappedDekRecord>(
        "SELECT dek_id, inode_id, wrapped_dek, key_version, vault_key_gen, created_at \
         FROM data_encryption_keys \
         ORDER BY dek_id ASC",
    )
    .fetch_all(pool)
    .await
}

/// Update a single DEK's wrapped blob and vault_key_gen after rotation.
#[allow(dead_code)]
pub async fn update_wrapped_dek(
    pool: &SqlitePool,
    dek_id: i64,
    new_wrapped_dek: &[u8],
    new_vault_key_gen: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE data_encryption_keys \
         SET wrapped_dek = ?, vault_key_gen = ? \
         WHERE dek_id = ?",
    )
    .bind(new_wrapped_dek)
    .bind(new_vault_key_gen)
    .bind(dek_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update vault_state with new salt, argon2_params, encrypted_vault_key and bumped generation.
#[allow(dead_code)]
pub async fn rotate_vault_state(
    pool: &SqlitePool,
    new_salt: &[u8],
    new_argon2_params: &str,
    new_encrypted_vault_key: &[u8],
    new_vault_key_generation: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE vault_state SET \
         master_key_salt = ?, \
         argon2_params = ?, \
         encrypted_vault_key = ?, \
         vault_key_generation = ? \
         WHERE id = 1",
    )
    .bind(new_salt)
    .bind(new_argon2_params)
    .bind(new_encrypted_vault_key)
    .bind(new_vault_key_generation)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update only encrypted_vault_key and generation (no salt/params change).
/// Used by VK rotation triggered by device revocation (passphrase unchanged).
pub async fn rotate_vault_key_only(
    pool: &SqlitePool,
    new_encrypted_vault_key: &[u8],
    new_vault_key_generation: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE vault_state SET \
         encrypted_vault_key = ?, \
         vault_key_generation = ? \
         WHERE id = 1",
    )
    .bind(new_encrypted_vault_key)
    .bind(new_vault_key_generation)
    .execute(pool)
    .await?;
    Ok(())
}

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
thread_local! {
    static MIGRATION_FAILPOINT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(feature = "test-helpers")]
pub fn set_migration_failpoint(on: bool) {
    MIGRATION_FAILPOINT.with(|f| f.set(on));
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
    if MIGRATION_FAILPOINT.with(|f| f.get()) {
        return Err(sqlx::Error::Protocol("migration failpoint".into()));
    }

    tx.commit().await
}

// ── DEK re-wrap queue (Epic 34.2b) ──────────────────────────────────

#[derive(Debug, Clone, FromRow)]
pub struct RewrapQueueItem {
    pub dek_id: i64,
    pub source_vk_generation: i64,
    pub target_vk_generation: i64,
    pub status: String,
    pub attempted_at: Option<i64>,
    pub error: Option<String>,
}

/// Enqueue all DEKs with vault_key_gen < target_generation for re-wrapping.
pub async fn enqueue_deks_for_rewrap(
    pool: &SqlitePool,
    target_vk_generation: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT OR IGNORE INTO dek_rewrap_queue (dek_id, source_vk_generation, target_vk_generation, status) \
         SELECT dek_id, vault_key_gen, ?, 'PENDING' \
         FROM data_encryption_keys \
         WHERE vault_key_gen < ?",
    )
    .bind(target_vk_generation)
    .bind(target_vk_generation)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Fetch a batch of PENDING re-wrap items (with their current wrapped DEK).
pub async fn get_pending_rewrap_batch(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<(RewrapQueueItem, Vec<u8>)>, sqlx::Error> {
    let items = sqlx::query_as::<_, RewrapQueueItem>(
        "SELECT dek_id, source_vk_generation, target_vk_generation, status, attempted_at, error \
         FROM dek_rewrap_queue WHERE status = 'PENDING' ORDER BY dek_id ASC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut result = Vec::with_capacity(items.len());
    for item in items {
        let dek = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT wrapped_dek FROM data_encryption_keys WHERE dek_id = ?",
        )
        .bind(item.dek_id)
        .fetch_one(pool)
        .await?;
        result.push((item, dek));
    }
    Ok(result)
}

/// Remove a successfully re-wrapped DEK from the queue.
pub async fn complete_rewrap_item(pool: &SqlitePool, dek_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM dek_rewrap_queue WHERE dek_id = ?")
        .bind(dek_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Mark a re-wrap item as FAILED with an error message.
pub async fn fail_rewrap_item(
    pool: &SqlitePool,
    dek_id: i64,
    error: &str,
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "UPDATE dek_rewrap_queue SET status = 'FAILED', attempted_at = ?, error = ? WHERE dek_id = ?",
    )
    .bind(now)
    .bind(error)
    .bind(dek_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get queue status: total, pending, failed counts.
pub async fn get_rewrap_status(pool: &SqlitePool) -> Result<(i64, i64, i64), sqlx::Error> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dek_rewrap_queue")
        .fetch_one(pool)
        .await?;
    let pending: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM dek_rewrap_queue WHERE status = 'PENDING'")
            .fetch_one(pool)
            .await?;
    let failed: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM dek_rewrap_queue WHERE status = 'FAILED'")
            .fetch_one(pool)
            .await?;
    Ok((total, pending, failed))
}

/// Get DEKs with a specific vault_key_gen (for lookup during dual-VK read).
pub async fn get_deks_by_generation(
    pool: &SqlitePool,
    vault_key_gen: i64,
) -> Result<Vec<WrappedDekRecord>, sqlx::Error> {
    sqlx::query_as::<_, WrappedDekRecord>(
        "SELECT dek_id, inode_id, wrapped_dek, key_version, vault_key_gen, created_at \
         FROM data_encryption_keys WHERE vault_key_gen = ? ORDER BY dek_id ASC",
    )
    .bind(vault_key_gen)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "test-helpers")]
    use super::*;

    #[cfg(feature = "test-helpers")]
    async fn test_pool() -> SqlitePool {
        init_db("sqlite::memory:").await.unwrap()
    }

    #[cfg(feature = "test-helpers")]
    async fn seed_vault_state_v1(pool: &SqlitePool) {
        set_vault_config(pool, &[1u8; 16], 1, 65_536, 3, 1)
            .await
            .unwrap();
        set_vault_params(
            pool,
            &[2u8; 32],
            r#"{"mode":"LOCAL_VAULT","parameter_set_version":1,"memory_cost_kib":65536,"time_cost":3,"lanes":1}"#,
            "vault-test-001",
        )
        .await
        .unwrap();
    }

    #[cfg(feature = "test-helpers")]
    #[tokio::test]
    async fn migrate_kdf_params_tx_writes_all_fields() {
        use omnidrive_core::crypto::WRAPPED_KEY_LEN;

        let pool = test_pool().await;
        seed_vault_state_v1(&pool).await;

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
        assert_eq!(
            get_legacy_read_key(&pool).await.unwrap().unwrap(),
            vec![5u8; 60]
        );
    }

    #[cfg(feature = "test-helpers")]
    #[tokio::test]
    async fn migrate_kdf_params_tx_rolls_back_on_failure() {
        use omnidrive_core::crypto::WRAPPED_KEY_LEN;

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
        assert_eq!(
            cfg.parameter_set_version, 1,
            "version must be unchanged after rollback"
        );
        assert!(
            get_legacy_read_key(&pool).await.unwrap().is_none(),
            "no legacy key written on rollback"
        );
        let v = get_vault_params(&pool).await.unwrap().unwrap();
        assert_eq!(
            v.master_key_salt,
            vec![2u8; 32],
            "salt unchanged after rollback"
        );
        assert!(
            v.encrypted_vault_key.is_none(),
            "encrypted_vault_key untouched after rollback"
        );
    }
}
