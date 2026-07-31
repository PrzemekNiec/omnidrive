use sqlx::FromRow;
use sqlx::Row;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct UploadJob {
    pub id: i64,
    pub pack_id: String,
    pub status: String,
    pub attempts: Option<i64>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct UploadTargetRecord {
    pub id: i64,
    pub job_id: i64,
    pub provider: String,
    pub status: String,
    pub attempts: Option<i64>,
    pub last_error: Option<String>,
    pub bucket: Option<String>,
    pub object_key: Option<String>,
    pub etag: Option<String>,
    pub version_id: Option<String>,
    pub last_attempt_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub completed_at: Option<i64>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct PackDownloadTarget {
    pub provider: String,
    pub bucket: String,
    pub object_key: String,
    pub attempts: Option<i64>,
    pub last_error: Option<String>,
    pub last_attempt_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub completed_at: Option<i64>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize)]
pub struct GcOrphanReport {
    pub orphan_pack_ids: Vec<String>,
    pub deleted_packs: u64,
    pub deleted_pack_shards: u64,
    pub deleted_pack_locations: u64,
    pub deleted_upload_jobs: u64,
    pub deleted_upload_job_targets: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct RetryStormTargetRecord {
    pub job_id: i64,
    pub pack_id: String,
    pub provider: String,
    pub status: String,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub last_attempt_at: Option<i64>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct UploadTargetSyncReport {
    pub synced_targets: u64,
    pub cleared_errors: u64,
}

#[allow(dead_code)]
pub async fn queue_pack_for_upload(pool: &SqlitePool, pack_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO upload_jobs (pack_id, status)
        VALUES (?, 'PENDING')
        ON CONFLICT(pack_id) DO UPDATE SET
            status = CASE
                WHEN upload_jobs.status = 'COMPLETED' THEN 'PENDING'
                ELSE upload_jobs.status
            END
        "#,
    )
    .bind(pack_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_next_upload_job(pool: &SqlitePool) -> Result<Option<UploadJob>, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let pending_job = sqlx::query_as::<_, UploadJob>(
        r#"
        SELECT id, pack_id, status, attempts
        FROM upload_jobs
        WHERE status = 'PENDING'
        ORDER BY id ASC
        LIMIT 1
        "#,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(mut job) = pending_job else {
        tx.commit().await?;
        return Ok(None);
    };

    sqlx::query(
        r#"
        UPDATE upload_jobs
        SET status = 'IN_PROGRESS'
        WHERE id = ?
        "#,
    )
    .bind(job.id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    job.status = "IN_PROGRESS".to_string();

    Ok(Some(job))
}

#[allow(dead_code)]
pub async fn mark_upload_job_completed(pool: &SqlitePool, job_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_jobs
        SET status = 'COMPLETED'
        WHERE id = ?
        "#,
    )
    .bind(job_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_upload_job_by_pack_id(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Option<UploadJob>, sqlx::Error> {
    sqlx::query_as::<_, UploadJob>(
        r#"
        SELECT id, pack_id, status, attempts
        FROM upload_jobs
        WHERE pack_id = ?
        LIMIT 1
        "#,
    )
    .bind(pack_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn ensure_upload_targets(
    pool: &SqlitePool,
    job_id: i64,
    providers: &[&str],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    for provider in providers {
        sqlx::query(
            r#"
            INSERT INTO upload_job_targets (job_id, provider, status)
            VALUES (?, ?, 'PENDING')
            ON CONFLICT(job_id, provider) DO NOTHING
            "#,
        )
        .bind(job_id)
        .bind(*provider)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn get_incomplete_upload_targets(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<Vec<UploadTargetRecord>, sqlx::Error> {
    sqlx::query_as::<_, UploadTargetRecord>(
        r#"
        SELECT
            id,
            job_id,
            provider,
            status,
            attempts,
            last_error,
            bucket,
            object_key,
            etag,
            version_id,
            last_attempt_at,
            updated_at,
            completed_at
        FROM upload_job_targets
        WHERE job_id = ?
          AND status != 'COMPLETED'
        ORDER BY id ASC
        "#,
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn mark_upload_target_in_progress(
    pool: &SqlitePool,
    job_id: i64,
    provider: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_job_targets
        SET status = 'IN_PROGRESS',
            last_attempt_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        WHERE job_id = ?
          AND provider = ?
        "#,
    )
    .bind(job_id)
    .bind(provider)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn mark_upload_target_completed(
    pool: &SqlitePool,
    job_id: i64,
    provider: &str,
    bucket: &str,
    object_key: &str,
    etag: Option<&str>,
    version_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_job_targets
        SET status = 'COMPLETED',
            last_error = NULL,
            bucket = ?,
            object_key = ?,
            etag = ?,
            version_id = ?,
            last_attempt_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            completed_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        WHERE job_id = ?
          AND provider = ?
        "#,
    )
    .bind(bucket)
    .bind(object_key)
    .bind(etag)
    .bind(version_id)
    .bind(job_id)
    .bind(provider)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn requeue_upload_target(
    pool: &SqlitePool,
    job_id: i64,
    provider: &str,
    error_message: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_job_targets
        SET status = 'PENDING',
            attempts = COALESCE(attempts, 0) + 1,
            last_error = ?,
            last_attempt_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            completed_at = NULL
        WHERE job_id = ?
          AND provider = ?
        "#,
    )
    .bind(error_message)
    .bind(job_id)
    .bind(provider)
    .execute(pool)
    .await?;

    let attempts = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(attempts, 0)
        FROM upload_job_targets
        WHERE job_id = ?
          AND provider = ?
        "#,
    )
    .bind(job_id)
    .bind(provider)
    .fetch_one(pool)
    .await?;

    Ok(attempts)
}

#[allow(dead_code)]
pub async fn mark_upload_target_failed(
    pool: &SqlitePool,
    job_id: i64,
    provider: &str,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_job_targets
        SET status = 'FAILED',
            attempts = COALESCE(attempts, 0) + 1,
            last_error = ?,
            last_attempt_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            completed_at = NULL
        WHERE job_id = ?
          AND provider = ?
        "#,
    )
    .bind(error_message)
    .bind(job_id)
    .bind(provider)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn gc_orphan_packs(pool: &SqlitePool) -> Result<GcOrphanReport, sqlx::Error> {
    let orphan_pack_ids: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT p.pack_id
        FROM packs p
        WHERE NOT EXISTS (
            SELECT 1
            FROM pack_locations pl
            INNER JOIN chunk_refs cr ON cr.chunk_id = pl.chunk_id
            WHERE pl.pack_id = p.pack_id
        )
        "#,
    )
    .fetch_all(pool)
    .await?;

    if orphan_pack_ids.is_empty() {
        return Ok(GcOrphanReport {
            orphan_pack_ids,
            deleted_packs: 0,
            deleted_pack_shards: 0,
            deleted_pack_locations: 0,
            deleted_upload_jobs: 0,
            deleted_upload_job_targets: 0,
        });
    }

    let mut tx = pool.begin().await?;
    let mut deleted_targets: u64 = 0;
    let mut deleted_jobs: u64 = 0;
    let mut deleted_locations: u64 = 0;
    let mut deleted_shards: u64 = 0;
    let mut deleted_packs: u64 = 0;

    for pack_id in &orphan_pack_ids {
        let job_ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM upload_jobs WHERE pack_id = ?")
            .bind(pack_id)
            .fetch_all(&mut *tx)
            .await?;
        for job_id in job_ids {
            let r = sqlx::query("DELETE FROM upload_job_targets WHERE job_id = ?")
                .bind(job_id)
                .execute(&mut *tx)
                .await?;
            deleted_targets = deleted_targets.saturating_add(r.rows_affected());
            let r = sqlx::query("DELETE FROM upload_jobs WHERE id = ?")
                .bind(job_id)
                .execute(&mut *tx)
                .await?;
            deleted_jobs = deleted_jobs.saturating_add(r.rows_affected());
        }
        let r = sqlx::query("DELETE FROM pack_locations WHERE pack_id = ?")
            .bind(pack_id)
            .execute(&mut *tx)
            .await?;
        deleted_locations = deleted_locations.saturating_add(r.rows_affected());
        // pack_shards cascade-delete via FK ON DELETE CASCADE — count first for report.
        let count_shards: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pack_shards WHERE pack_id = ?")
                .bind(pack_id)
                .fetch_one(&mut *tx)
                .await?;
        deleted_shards = deleted_shards.saturating_add(count_shards.max(0) as u64);
        let r = sqlx::query("DELETE FROM packs WHERE pack_id = ?")
            .bind(pack_id)
            .execute(&mut *tx)
            .await?;
        deleted_packs = deleted_packs.saturating_add(r.rows_affected());
    }

    tx.commit().await?;
    Ok(GcOrphanReport {
        orphan_pack_ids,
        deleted_packs,
        deleted_pack_shards: deleted_shards,
        deleted_pack_locations: deleted_locations,
        deleted_upload_jobs: deleted_jobs,
        deleted_upload_job_targets: deleted_targets,
    })
}

#[allow(dead_code)]
pub async fn list_retry_storm_targets(
    pool: &SqlitePool,
    attempts_threshold: i64,
) -> Result<Vec<RetryStormTargetRecord>, sqlx::Error> {
    sqlx::query_as::<_, RetryStormTargetRecord>(
        r#"
        SELECT
            ut.job_id   AS job_id,
            uj.pack_id  AS pack_id,
            ut.provider AS provider,
            ut.status   AS status,
            COALESCE(ut.attempts, 0) AS attempts,
            ut.last_error AS last_error,
            ut.last_attempt_at AS last_attempt_at
        FROM upload_job_targets ut
        INNER JOIN upload_jobs uj ON uj.id = ut.job_id
        WHERE COALESCE(ut.attempts, 0) >= ?
          AND ut.status NOT IN ('COMPLETED')
        ORDER BY ut.attempts DESC, ut.job_id ASC
        "#,
    )
    .bind(attempts_threshold)
    .fetch_all(pool)
    .await
}

/// Reconcile `upload_job_targets.status` against the source-of-truth
/// `pack_shards.status`. When a shard reaches `COMPLETED` the target row
/// sometimes lingers in `PENDING/FAILED` (historic transient errors that
/// later resolved on retry from a different worker). That ghost state inflates
/// `attempts` counters, fires retry-storm alerts, and pollutes `last_error`.
///
/// Idempotent. Safe to call at startup and from periodic maintenance.
#[allow(dead_code)]
pub async fn sync_upload_targets_from_shards(
    pool: &SqlitePool,
) -> Result<UploadTargetSyncReport, sqlx::Error> {
    let mut tx = pool.begin().await?;

    // 1. Targets that should be COMPLETED because the matching shard is.
    let synced = sqlx::query(
        r#"
        UPDATE upload_job_targets
        SET status = 'COMPLETED',
            completed_at = COALESCE(completed_at, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)),
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        WHERE id IN (
            SELECT ut.id
            FROM upload_job_targets ut
            JOIN upload_jobs uj ON uj.id = ut.job_id
            JOIN pack_shards ps ON ps.pack_id = uj.pack_id AND ps.provider = ut.provider
            WHERE ut.status NOT IN ('COMPLETED', 'PERMANENTLY_FAILED')
              AND ps.status = 'COMPLETED'
        )
        "#,
    )
    .execute(&mut *tx)
    .await?;

    // 2. Stale `last_error` strings for COMPLETED targets — `get_latest_upload_error`
    // reads from `last_error` and is unaware of status, so cached errors keep
    // diagnostics in WARN state forever.
    let cleared = sqlx::query(
        r#"
        UPDATE upload_job_targets
        SET last_error = NULL
        WHERE status = 'COMPLETED'
          AND last_error IS NOT NULL
        "#,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(UploadTargetSyncReport {
        synced_targets: synced.rows_affected(),
        cleared_errors: cleared.rows_affected(),
    })
}

#[allow(dead_code)]
pub async fn mark_upload_target_permanently_failed(
    pool: &SqlitePool,
    job_id: i64,
    provider: &str,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_job_targets
        SET status = 'PERMANENTLY_FAILED',
            last_error = ?,
            last_attempt_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            completed_at = NULL
        WHERE job_id = ?
          AND provider = ?
        "#,
    )
    .bind(error_message)
    .bind(job_id)
    .bind(provider)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn has_incomplete_upload_targets(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<bool, sqlx::Error> {
    let count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM upload_job_targets
        WHERE job_id = ?
          AND status != 'COMPLETED'
        "#,
    )
    .bind(job_id)
    .fetch_one(pool)
    .await?;

    Ok(count > 0)
}

#[allow(dead_code)]
pub async fn requeue_upload_job(pool: &SqlitePool, job_id: i64) -> Result<i64, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_jobs
        SET status = 'PENDING',
            attempts = COALESCE(attempts, 0) + 1
        WHERE id = ?
        "#,
    )
    .bind(job_id)
    .execute(pool)
    .await?;

    let attempts = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(attempts, 0)
        FROM upload_jobs
        WHERE id = ?
        "#,
    )
    .bind(job_id)
    .fetch_one(pool)
    .await?;

    Ok(attempts)
}

#[allow(dead_code)]
pub async fn mark_upload_job_failed(pool: &SqlitePool, job_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_jobs
        SET status = 'FAILED'
        WHERE id = ?
        "#,
    )
    .bind(job_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn reset_in_progress_upload_targets(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE upload_job_targets
        SET status = 'PENDING',
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        WHERE status = 'IN_PROGRESS'
        "#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

#[allow(dead_code)]
pub async fn get_upload_targets_for_job(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<Vec<UploadTargetRecord>, sqlx::Error> {
    sqlx::query_as::<_, UploadTargetRecord>(
        r#"
        SELECT
            id,
            job_id,
            provider,
            status,
            attempts,
            last_error,
            bucket,
            object_key,
            etag,
            version_id,
            last_attempt_at,
            updated_at,
            completed_at
        FROM upload_job_targets
        WHERE job_id = ?
        ORDER BY provider ASC
        "#,
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn list_recent_upload_jobs(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<UploadJob>, sqlx::Error> {
    sqlx::query_as::<_, UploadJob>(
        r#"
        SELECT id, pack_id, status, attempts
        FROM upload_jobs
        ORDER BY
            CASE WHEN status IN ('PENDING', 'IN_PROGRESS') THEN 0 ELSE 1 END,
            id DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_pending_upload_queue_size(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT COUNT(*) AS count
        FROM upload_jobs
        WHERE status = 'PENDING'
        "#,
    )
    .fetch_one(pool)
    .await?;

    row.try_get("count")
}

#[allow(dead_code)]
pub async fn get_latest_upload_error(pool: &SqlitePool) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT last_error
        FROM upload_job_targets
        WHERE last_error IS NOT NULL
          AND last_error != ''
        ORDER BY COALESCE(last_attempt_at, updated_at, completed_at, 0) DESC, id DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.and_then(|row| row.try_get("last_error").ok()))
}

#[allow(dead_code)]
pub async fn get_latest_upload_target_for_provider(
    pool: &SqlitePool,
    provider: &str,
) -> Result<Option<UploadTargetRecord>, sqlx::Error> {
    sqlx::query_as::<_, UploadTargetRecord>(
        r#"
        SELECT
            id,
            job_id,
            provider,
            status,
            attempts,
            last_error,
            bucket,
            object_key,
            etag,
            version_id,
            last_attempt_at,
            updated_at,
            completed_at
        FROM upload_job_targets
        WHERE provider = ?
        ORDER BY COALESCE(last_attempt_at, updated_at, completed_at, 0) DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(provider)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_completed_pack_targets(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Vec<PackDownloadTarget>, sqlx::Error> {
    sqlx::query_as::<_, PackDownloadTarget>(
        r#"
        SELECT
            ut.provider,
            ut.bucket,
            ut.object_key,
            ut.attempts,
            ut.last_error,
            ut.last_attempt_at,
            ut.updated_at,
            ut.completed_at
        FROM upload_jobs uj
        INNER JOIN upload_job_targets ut
            ON ut.job_id = uj.id
        WHERE uj.pack_id = ?
          AND ut.status = 'COMPLETED'
          AND ut.bucket IS NOT NULL
          AND ut.object_key IS NOT NULL
        ORDER BY COALESCE(ut.completed_at, ut.updated_at, ut.last_attempt_at, 0) DESC, ut.id ASC
        "#,
    )
    .bind(pack_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn reset_in_progress_upload_jobs(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE upload_jobs
        SET status = 'PENDING'
        WHERE status = 'IN_PROGRESS'
        "#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}
