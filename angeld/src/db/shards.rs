use crate::db::*;
use sqlx::FromRow;
use sqlx::Row;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct PackShardRecord {
    pub id: i64,
    pub pack_id: String,
    pub shard_index: i64,
    pub shard_role: String,
    pub provider: String,
    pub object_key: String,
    pub size: i64,
    pub checksum: String,
    pub status: String,
    pub attempts: Option<i64>,
    pub last_error: Option<String>,
    pub last_verified_at: Option<i64>,
    pub last_verification_method: Option<String>,
    pub last_verification_status: Option<String>,
    pub last_verified_size: Option<i64>,
    pub verification_failures: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ScrubShardRecord {
    pub id: i64,
    pub pack_id: String,
    pub shard_index: i64,
    pub provider: String,
    pub object_key: String,
    pub size: i64,
    pub checksum: String,
    pub status: String,
    pub last_verified_at: Option<i64>,
    pub last_verification_method: Option<String>,
    pub last_verification_status: Option<String>,
    pub last_verified_size: Option<i64>,
    pub verification_failures: i64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackShardSummary {
    pub total: i64,
    pub completed: i64,
    pub pending: i64,
    pub in_progress: i64,
    pub failed: i64,
}

#[allow(dead_code)]
pub async fn register_pack_shard(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
    shard_role: ShardRole,
    provider: &str,
    object_key: &str,
    size: i64,
    checksum: &str,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO pack_shards (
            pack_id,
            shard_index,
            shard_role,
            provider,
            object_key,
            size,
            checksum,
            status
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(pack_id, shard_index) DO UPDATE SET
            shard_role = excluded.shard_role,
            provider = excluded.provider,
            object_key = excluded.object_key,
            size = excluded.size,
            checksum = excluded.checksum,
            status = excluded.status,
            last_error = NULL
        "#,
    )
    .bind(pack_id)
    .bind(shard_index)
    .bind(shard_role.as_str())
    .bind(provider)
    .bind(object_key)
    .bind(size)
    .bind(checksum)
    .bind(status)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_pack_shards(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Vec<PackShardRecord>, sqlx::Error> {
    sqlx::query_as::<_, PackShardRecord>(
        r#"
        SELECT
            id,
            pack_id,
            shard_index,
            shard_role,
            provider,
            object_key,
            size,
            checksum,
            status,
            attempts,
            last_error,
            last_verified_at,
            last_verification_method,
            last_verification_status,
            last_verified_size,
            COALESCE(verification_failures, 0) AS verification_failures
        FROM pack_shards
        WHERE pack_id = ?
        ORDER BY shard_index ASC
        "#,
    )
    .bind(pack_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_incomplete_pack_shards(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Vec<PackShardRecord>, sqlx::Error> {
    sqlx::query_as::<_, PackShardRecord>(
        r#"
        SELECT
            id,
            pack_id,
            shard_index,
            shard_role,
            provider,
            object_key,
            size,
            checksum,
            status,
            attempts,
            last_error,
            last_verified_at,
            last_verification_method,
            last_verification_status,
            last_verified_size,
            COALESCE(verification_failures, 0) AS verification_failures
        FROM pack_shards
        WHERE pack_id = ?
          AND status NOT IN ('COMPLETED', 'PERMANENTLY_FAILED')
        ORDER BY shard_index ASC
        "#,
    )
    .bind(pack_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn mark_pack_shard_in_progress(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE pack_shards
        SET status = 'IN_PROGRESS',
            last_error = NULL
        WHERE pack_id = ?
          AND shard_index = ?
        "#,
    )
    .bind(pack_id)
    .bind(shard_index)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn mark_pack_shard_completed(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE pack_shards
        SET status = 'COMPLETED',
            last_error = NULL
        WHERE pack_id = ?
          AND shard_index = ?
        "#,
    )
    .bind(pack_id)
    .bind(shard_index)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn requeue_pack_shard(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
    error_message: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE pack_shards
        SET status = 'PENDING',
            attempts = COALESCE(attempts, 0) + 1,
            last_error = ?
        WHERE pack_id = ?
          AND shard_index = ?
        "#,
    )
    .bind(error_message)
    .bind(pack_id)
    .bind(shard_index)
    .execute(pool)
    .await?;

    let attempts = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(attempts, 0)
        FROM pack_shards
        WHERE pack_id = ?
          AND shard_index = ?
        "#,
    )
    .bind(pack_id)
    .bind(shard_index)
    .fetch_one(pool)
    .await?;

    Ok(attempts)
}

#[allow(dead_code)]
pub async fn mark_pack_shard_failed(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE pack_shards
        SET status = 'FAILED',
            attempts = COALESCE(attempts, 0) + 1,
            last_error = ?
        WHERE pack_id = ?
          AND shard_index = ?
        "#,
    )
    .bind(error_message)
    .bind(pack_id)
    .bind(shard_index)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn mark_pack_shard_permanently_failed(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE pack_shards
        SET status = 'PERMANENTLY_FAILED',
            last_error = ?
        WHERE pack_id = ?
          AND shard_index = ?
        "#,
    )
    .bind(error_message)
    .bind(pack_id)
    .bind(shard_index)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_next_shards_for_scrub(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<ScrubShardRecord>, sqlx::Error> {
    sqlx::query_as::<_, ScrubShardRecord>(
        r#"
        SELECT
            id,
            pack_id,
            shard_index,
            provider,
            object_key,
            size,
            checksum,
            status,
            last_verified_at,
            last_verification_method,
            last_verification_status,
            last_verified_size,
            COALESCE(verification_failures, 0) AS verification_failures
        FROM pack_shards
        ORDER BY
            CASE WHEN last_verified_at IS NULL THEN 0 ELSE 1 END ASC,
            COALESCE(last_verified_at, 0) ASC,
            COALESCE(verification_failures, 0) DESC,
            id ASC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn update_shard_verification_status(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
    verification_method: &str,
    verification_status: &str,
    verified_size: Option<i64>,
    increment_failures: bool,
    last_error: Option<&str>,
) -> Result<(), sqlx::Error> {
    let operational_status = if verification_status == "HEALTHY" {
        "COMPLETED"
    } else {
        "FAILED"
    };

    sqlx::query(
        r#"
        UPDATE pack_shards
        SET status = ?,
            last_verified_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            last_verification_method = ?,
            last_verification_status = ?,
            last_verified_size = ?,
            verification_failures = COALESCE(verification_failures, 0) + CASE WHEN ? THEN 1 ELSE 0 END,
            last_error = ?
        WHERE pack_id = ?
          AND shard_index = ?
        "#,
    )
    .bind(operational_status)
    .bind(verification_method)
    .bind(verification_status)
    .bind(verified_size)
    .bind(increment_failures)
    .bind(last_error)
    .bind(pack_id)
    .bind(shard_index)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn reset_in_progress_pack_shards(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE pack_shards
        SET status = 'PENDING'
        WHERE status = 'IN_PROGRESS'
        "#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

#[allow(dead_code)]
pub async fn summarize_pack_shards(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<PackShardSummary, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT status, COUNT(*) AS count
        FROM pack_shards
        WHERE pack_id = ?
        GROUP BY status
        "#,
    )
    .bind(pack_id)
    .fetch_all(pool)
    .await?;

    let mut summary = PackShardSummary::default();
    for row in rows {
        let status: String = row.try_get("status")?;
        let count: i64 = row.try_get("count")?;
        summary.total += count;
        match status.as_str() {
            "COMPLETED" => summary.completed += count,
            "PENDING" => summary.pending += count,
            "IN_PROGRESS" => summary.in_progress += count,
            "FAILED" => summary.failed += count,
            _ => {}
        }
    }

    Ok(summary)
}
