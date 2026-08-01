use crate::db::*;
use sqlx::FromRow;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct PackRecord {
    pub pack_id: String,
    pub chunk_id: Vec<u8>,
    pub plaintext_hash: Option<String>,
    pub storage_mode: String,
    pub encryption_version: i64,
    pub ec_scheme: String,
    pub logical_size: i64,
    pub cipher_size: i64,
    pub shard_size: i64,
    pub nonce: Vec<u8>,
    pub gcm_tag: Vec<u8>,
    pub status: String,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct VaultHealthSummary {
    pub total_packs: i64,
    pub healthy_packs: i64,
    pub degraded_packs: i64,
    pub unreadable_packs: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ScrubStatusSummary {
    pub total_shards: i64,
    pub verified_shards: i64,
    pub healthy_shards: i64,
    pub corrupted_or_missing: i64,
    pub verified_light_shards: i64,
    pub verified_deep_shards: i64,
    pub last_scrub_timestamp: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ScrubErrorRecord {
    pub pack_id: String,
    pub provider: String,
    pub shard_index: i64,
    pub last_verified_at: Option<i64>,
    pub last_verification_status: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ActiveStorageModeSummary {
    pub storage_mode: String,
    pub active_packs: i64,
    pub logical_bytes: i64,
    pub cipher_bytes: i64,
    pub total_shard_bytes: i64,
    pub physical_bytes: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct OrphanedPackSummary {
    pub pack_count: i64,
    pub physical_bytes: i64,
}

#[allow(dead_code)]
pub async fn create_pack(
    pool: &SqlitePool,
    pack_id: &str,
    chunk_id: &[u8],
    plaintext_hash: &str,
    storage_mode: StorageMode,
    encryption_version: i64,
    ec_scheme: &str,
    logical_size: i64,
    cipher_size: i64,
    shard_size: i64,
    nonce: &[u8],
    gcm_tag: &[u8],
    status: PackStatus,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO packs (
            pack_id,
            chunk_id,
            plaintext_hash,
            storage_mode,
            encryption_version,
            ec_scheme,
            logical_size,
            cipher_size,
            shard_size,
            nonce,
            gcm_tag,
            status
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(pack_id) DO UPDATE SET
            chunk_id = excluded.chunk_id,
            plaintext_hash = excluded.plaintext_hash,
            storage_mode = excluded.storage_mode,
            encryption_version = excluded.encryption_version,
            ec_scheme = excluded.ec_scheme,
            logical_size = excluded.logical_size,
            cipher_size = excluded.cipher_size,
            shard_size = excluded.shard_size,
            nonce = excluded.nonce,
            gcm_tag = excluded.gcm_tag,
            status = excluded.status
        "#,
    )
    .bind(pack_id)
    .bind(chunk_id)
    .bind(plaintext_hash)
    .bind(storage_mode.as_str())
    .bind(encryption_version)
    .bind(ec_scheme)
    .bind(logical_size)
    .bind(cipher_size)
    .bind(shard_size)
    .bind(nonce)
    .bind(gcm_tag)
    .bind(status.as_str())
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn update_pack_status(
    pool: &SqlitePool,
    pack_id: &str,
    status: PackStatus,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE packs
        SET status = ?
        WHERE pack_id = ?
        "#,
    )
    .bind(status.as_str())
    .bind(pack_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_pack(pool: &SqlitePool, pack_id: &str) -> Result<Option<PackRecord>, sqlx::Error> {
    sqlx::query_as::<_, PackRecord>(
        r#"
        SELECT
            pack_id,
            chunk_id,
            plaintext_hash,
            storage_mode,
            encryption_version,
            ec_scheme,
            logical_size,
            cipher_size,
            shard_size,
            nonce,
            gcm_tag,
            status
        FROM packs
        WHERE pack_id = ?
        "#,
    )
    .bind(pack_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn find_pack_by_plaintext_hash(
    pool: &SqlitePool,
    plaintext_hash: &str,
    storage_mode: StorageMode,
) -> Result<Option<PackRecord>, sqlx::Error> {
    sqlx::query_as::<_, PackRecord>(
        r#"
        SELECT
            pack_id,
            chunk_id,
            plaintext_hash,
            storage_mode,
            encryption_version,
            ec_scheme,
            logical_size,
            cipher_size,
            shard_size,
            nonce,
            gcm_tag,
            status
        FROM packs
        WHERE plaintext_hash = ?
          AND storage_mode = ?
          AND status != 'UNREADABLE'
        ORDER BY
            CASE status
                WHEN 'COMPLETED_HEALTHY' THEN 0
                WHEN 'COMPLETED_DEGRADED' THEN 1
                WHEN 'UPLOADING' THEN 2
                ELSE 3
            END,
            pack_id ASC
        LIMIT 1
        "#,
    )
    .bind(plaintext_hash)
    .bind(storage_mode.as_str())
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_orphaned_pack_ids(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT p.pack_id
        FROM packs p
        LEFT JOIN pack_locations pl
            ON pl.pack_id = p.pack_id
        WHERE pl.pack_id IS NULL
          AND p.status != 'UPLOADING'
        ORDER BY p.pack_id ASC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_next_degraded_pack(pool: &SqlitePool) -> Result<Option<PackRecord>, sqlx::Error> {
    sqlx::query_as::<_, PackRecord>(
        r#"
        SELECT
            pack_id,
            chunk_id,
            plaintext_hash,
            storage_mode,
            encryption_version,
            ec_scheme,
            logical_size,
            cipher_size,
            shard_size,
            nonce,
            gcm_tag,
            status
        FROM packs
        WHERE status = 'COMPLETED_DEGRADED'
          AND (
            repair_next_attempt_at IS NULL
            OR repair_next_attempt_at <= CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
          )
        ORDER BY pack_id ASC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn defer_pack_repair(
    pool: &SqlitePool,
    pack_id: &str,
    base_delay_ms: i64,
    max_delay_ms: i64,
) -> Result<i64, sqlx::Error> {
    let attempts = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(repair_attempts, 0) FROM packs WHERE pack_id = ?",
    )
    .bind(pack_id)
    .fetch_optional(pool)
    .await?
    .unwrap_or(0);

    let delay_ms = base_delay_ms
        .saturating_mul(1i64 << attempts.clamp(0, 8))
        .min(max_delay_ms);

    sqlx::query(
        r#"
        UPDATE packs
        SET repair_attempts = COALESCE(repair_attempts, 0) + 1,
            repair_next_attempt_at =
                CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) + ?
        WHERE pack_id = ?
        "#,
    )
    .bind(delay_ms)
    .bind(pack_id)
    .execute(pool)
    .await?;

    Ok(attempts + 1)
}

#[allow(dead_code)]
pub async fn clear_pack_repair_backoff(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE packs
        SET repair_attempts = 0,
            repair_next_attempt_at = NULL
        WHERE pack_id = ?
        "#,
    )
    .bind(pack_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn pack_repair_is_deferred(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<bool, sqlx::Error> {
    let deferred = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM packs
        WHERE pack_id = ?
          AND repair_next_attempt_at IS NOT NULL
          AND repair_next_attempt_at > CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        "#,
    )
    .bind(pack_id)
    .fetch_one(pool)
    .await?;

    Ok(deferred > 0)
}

#[allow(dead_code)]
pub async fn get_vault_health_summary(
    pool: &SqlitePool,
) -> Result<VaultHealthSummary, sqlx::Error> {
    sqlx::query_as::<_, VaultHealthSummary>(
        r#"
        SELECT
            COUNT(*) AS total_packs,
            COALESCE(SUM(CASE WHEN status = 'COMPLETED_HEALTHY' THEN 1 ELSE 0 END), 0) AS healthy_packs,
            COALESCE(SUM(CASE WHEN status = 'COMPLETED_DEGRADED' THEN 1 ELSE 0 END), 0) AS degraded_packs,
            COALESCE(SUM(CASE WHEN status = 'UNREADABLE' THEN 1 ELSE 0 END), 0) AS unreadable_packs
        FROM packs
        "#,
    )
    .fetch_one(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_scrub_status_summary(
    pool: &SqlitePool,
) -> Result<ScrubStatusSummary, sqlx::Error> {
    sqlx::query_as::<_, ScrubStatusSummary>(
        r#"
        SELECT
            COUNT(*) AS total_shards,
            COALESCE(SUM(CASE WHEN last_verified_at IS NOT NULL THEN 1 ELSE 0 END), 0) AS verified_shards,
            COALESCE(SUM(CASE WHEN last_verification_status = 'HEALTHY' THEN 1 ELSE 0 END), 0) AS healthy_shards,
            COALESCE(SUM(CASE WHEN last_verification_status IN ('MISSING', 'SIZE_MISMATCH', 'CORRUPTED') THEN 1 ELSE 0 END), 0) AS corrupted_or_missing,
            COALESCE(SUM(CASE WHEN last_verification_method = 'LIGHT' THEN 1 ELSE 0 END), 0) AS verified_light_shards,
            COALESCE(SUM(CASE WHEN last_verification_method = 'DEEP' THEN 1 ELSE 0 END), 0) AS verified_deep_shards,
            MAX(last_verified_at) AS last_scrub_timestamp
        FROM pack_shards
        "#,
    )
    .fetch_one(pool)
    .await
}

#[allow(dead_code)]
pub async fn list_scrub_errors(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<ScrubErrorRecord>, sqlx::Error> {
    sqlx::query_as::<_, ScrubErrorRecord>(
        r#"
        SELECT
            pack_id,
            provider,
            shard_index,
            last_verified_at,
            last_verification_status
        FROM pack_shards
        WHERE last_verification_status IS NOT NULL
          AND last_verification_status != 'HEALTHY'
        ORDER BY COALESCE(last_verified_at, 0) DESC, pack_id ASC, shard_index ASC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_physical_usage_for_provider(
    pool: &SqlitePool,
    provider_name: &str,
) -> Result<u64, sqlx::Error> {
    let total = sqlx::query_scalar::<_, Option<i64>>(
        r#"
        SELECT COALESCE(SUM(size), 0)
        FROM pack_shards
        WHERE provider = ?
          AND status IN ('PENDING', 'IN_PROGRESS', 'COMPLETED')
        "#,
    )
    .bind(provider_name)
    .fetch_one(pool)
    .await?
    .unwrap_or(0);

    Ok(u64::try_from(total).unwrap_or(0))
}

#[allow(dead_code)]
pub async fn get_active_storage_mode_summaries(
    pool: &SqlitePool,
) -> Result<Vec<ActiveStorageModeSummary>, sqlx::Error> {
    sqlx::query_as::<_, ActiveStorageModeSummary>(
        r#"
        WITH active_packs AS (
            SELECT DISTINCT
                p.pack_id,
                p.storage_mode,
                p.logical_size,
                p.cipher_size,
                p.shard_size
            FROM packs p
            INNER JOIN pack_locations pl
                ON pl.pack_id = p.pack_id
        ),
        physical_by_pack AS (
            SELECT
                pack_id,
                COALESCE(SUM(size), 0) AS physical_bytes
            FROM pack_shards
            WHERE status IN ('PENDING', 'IN_PROGRESS', 'COMPLETED')
            GROUP BY pack_id
        )
        SELECT
            ap.storage_mode,
            COUNT(*) AS active_packs,
            COALESCE(SUM(ap.logical_size), 0) AS logical_bytes,
            COALESCE(SUM(ap.cipher_size), 0) AS cipher_bytes,
            COALESCE(SUM(ap.shard_size), 0) AS total_shard_bytes,
            COALESCE(SUM(COALESCE(pb.physical_bytes, 0)), 0) AS physical_bytes
        FROM active_packs ap
        LEFT JOIN physical_by_pack pb
            ON pb.pack_id = ap.pack_id
        GROUP BY ap.storage_mode
        ORDER BY ap.storage_mode ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_orphaned_pack_summary(
    pool: &SqlitePool,
) -> Result<OrphanedPackSummary, sqlx::Error> {
    sqlx::query_as::<_, OrphanedPackSummary>(
        r#"
        WITH orphaned AS (
            SELECT p.pack_id
            FROM packs p
            LEFT JOIN pack_locations pl
                ON pl.pack_id = p.pack_id
            WHERE pl.pack_id IS NULL
              AND p.status != 'UPLOADING'
        )
        SELECT
            COUNT(*) AS pack_count,
            COALESCE((
                SELECT SUM(ps.size)
                FROM pack_shards ps
                INNER JOIN orphaned o
                    ON o.pack_id = ps.pack_id
                WHERE ps.status IN ('PENDING', 'IN_PROGRESS', 'COMPLETED')
            ), 0) AS physical_bytes
        FROM orphaned
        "#,
    )
    .fetch_one(pool)
    .await
}

#[allow(dead_code)]
pub async fn delete_pack_metadata(pool: &SqlitePool, pack_id: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        DELETE FROM upload_jobs
        WHERE pack_id = ?
        "#,
    )
    .bind(pack_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        DELETE FROM pack_locations
        WHERE pack_id = ?
        "#,
    )
    .bind(pack_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        DELETE FROM packs
        WHERE pack_id = ?
        "#,
    )
    .bind(pack_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

#[allow(dead_code)]
pub fn resolve_pack_status(summary: PackShardSummary) -> PackStatus {
    resolve_pack_status_for_mode(StorageMode::Ec2_1, summary)
}

#[allow(dead_code)]
pub fn resolve_pack_status_for_mode(
    storage_mode: StorageMode,
    summary: PackShardSummary,
) -> PackStatus {
    match storage_mode {
        StorageMode::Ec2_1 => {
            if summary.completed >= 3 {
                PackStatus::Healthy
            } else if summary.completed >= 2 {
                PackStatus::Degraded
            } else if summary.pending > 0 || summary.in_progress > 0 {
                PackStatus::Uploading
            } else {
                PackStatus::Unreadable
            }
        }
        StorageMode::SingleReplica => {
            if summary.completed >= 1 {
                PackStatus::Healthy
            } else if summary.pending > 0 || summary.in_progress > 0 {
                PackStatus::Uploading
            } else {
                PackStatus::Unreadable
            }
        }
        StorageMode::LocalOnly => PackStatus::Healthy,
    }
}

#[allow(dead_code)]
pub async fn list_active_packs(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<PackRecord>, sqlx::Error> {
    sqlx::query_as::<_, PackRecord>(
        r#"
        SELECT DISTINCT
            p.pack_id,
            p.chunk_id,
            p.plaintext_hash,
            p.storage_mode,
            p.encryption_version,
            p.ec_scheme,
            p.logical_size,
            p.cipher_size,
            p.shard_size,
            p.nonce,
            p.gcm_tag,
            p.status
        FROM packs p
        INNER JOIN pack_locations pl
            ON pl.pack_id = p.pack_id
        ORDER BY p.pack_id ASC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_desired_storage_mode_for_pack(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<StorageMode, sqlx::Error> {
    let inode_ids = get_referencing_inode_ids_for_pack(pool, pack_id).await?;
    if inode_ids.is_empty() {
        return Ok(StorageMode::Ec2_1);
    }

    let mut desired = StorageMode::LocalOnly;
    for inode_id in inode_ids {
        let inode_path = get_inode_path(pool, inode_id)
            .await?
            .unwrap_or_else(|| format!("inode/{inode_id}"));
        let policy_type = find_sync_policy_for_path(pool, &inode_path)
            .await?
            .map(|policy| policy.policy_type)
            .unwrap_or_else(|| "PARANOIA".to_string());
        match StorageMode::from_policy_type(&policy_type) {
            StorageMode::Ec2_1 => return Ok(StorageMode::Ec2_1),
            StorageMode::SingleReplica => desired = StorageMode::SingleReplica,
            StorageMode::LocalOnly => {}
        }
    }

    Ok(desired)
}

#[allow(dead_code)]
pub async fn get_next_pack_requiring_reconciliation(
    pool: &SqlitePool,
) -> Result<Option<PackRecord>, sqlx::Error> {
    for pack in list_active_packs(pool, 256).await? {
        let desired = get_desired_storage_mode_for_pack(pool, &pack.pack_id).await?;
        if StorageMode::from_str(&pack.storage_mode) != desired
            && !pack_repair_is_deferred(pool, &pack.pack_id).await?
        {
            return Ok(Some(pack));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;
    use crate::db::link_chunk_to_pack;

    async fn insert_pack(pool: &SqlitePool, pack_id: &str, mode: StorageMode, status: PackStatus) {
        create_pack(
            pool,
            pack_id,
            pack_id.as_bytes(),
            "",
            mode,
            1,
            "rs_2_1",
            0,
            0,
            0,
            b"",
            b"",
            status,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn deferred_pack_does_not_starve_later_degraded_packs() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        insert_pack(&pool, "pack-a", StorageMode::Ec2_1, PackStatus::Degraded).await;
        insert_pack(&pool, "pack-b", StorageMode::Ec2_1, PackStatus::Degraded).await;

        let first = get_next_degraded_pack(&pool).await.unwrap().unwrap();
        assert_eq!(first.pack_id, "pack-a");

        let attempts = defer_pack_repair(&pool, "pack-a", 60_000, 3_600_000)
            .await
            .unwrap();
        assert_eq!(attempts, 1);

        let second = get_next_degraded_pack(&pool)
            .await
            .unwrap()
            .expect("odroczony pack nie moze blokowac kolejnych");
        assert_eq!(
            second.pack_id, "pack-b",
            "nienaprawialny pack glodzi cala kolejke napraw"
        );
    }

    #[tokio::test]
    async fn deferred_pack_returns_once_delay_elapsed() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        insert_pack(&pool, "pack-a", StorageMode::Ec2_1, PackStatus::Degraded).await;

        defer_pack_repair(&pool, "pack-a", 0, 0).await.unwrap();

        let again = get_next_degraded_pack(&pool).await.unwrap().unwrap();
        assert_eq!(again.pack_id, "pack-a");
    }

    #[tokio::test]
    async fn successful_repair_clears_the_backoff() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        insert_pack(&pool, "pack-a", StorageMode::Ec2_1, PackStatus::Degraded).await;

        defer_pack_repair(&pool, "pack-a", 3_600_000, 3_600_000)
            .await
            .unwrap();
        assert!(get_next_degraded_pack(&pool).await.unwrap().is_none());

        clear_pack_repair_backoff(&pool, "pack-a").await.unwrap();
        let attempts = defer_pack_repair(&pool, "pack-a", 0, 0).await.unwrap();
        assert_eq!(attempts, 1, "licznik prob musi ruszac od zera po sukcesie");
    }

    #[tokio::test]
    async fn deferred_pack_is_skipped_by_reconciliation_queue() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        insert_pack(
            &pool,
            "pack-a",
            StorageMode::SingleReplica,
            PackStatus::Healthy,
        )
        .await;
        link_chunk_to_pack(&pool, b"pack-a", "pack-a", 0, 0)
            .await
            .unwrap();

        let candidate = get_next_pack_requiring_reconciliation(&pool).await.unwrap();
        assert_eq!(candidate.map(|pack| pack.pack_id).as_deref(), Some("pack-a"));

        defer_pack_repair(&pool, "pack-a", 3_600_000, 3_600_000)
            .await
            .unwrap();

        assert!(
            get_next_pack_requiring_reconciliation(&pool)
                .await
                .unwrap()
                .is_none(),
            "odroczona rekoncyliacja musi wypuscic kolejke"
        );
    }
}

#[allow(dead_code)]
pub async fn pack_requires_healthy(pool: &SqlitePool, pack_id: &str) -> Result<bool, sqlx::Error> {
    let inode_ids = get_referencing_inode_ids_for_pack(pool, pack_id).await?;
    if inode_ids.is_empty() {
        return Ok(false);
    }

    let mut saw_policy = false;
    for inode_id in inode_ids {
        let Some(path) = get_inode_path(pool, inode_id).await? else {
            continue;
        };
        match find_sync_policy_for_path(pool, &path).await? {
            Some(policy) => {
                saw_policy = true;
                if policy.require_healthy != 0 {
                    return Ok(true);
                }
            }
            None => return Ok(true),
        }
    }

    Ok(!saw_policy)
}
