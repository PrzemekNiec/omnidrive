use crate::db::*;
use sqlx::FromRow;
use sqlx::SqlitePool;

// ── Ingest Jobs persistence ───────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct IngestJobRow {
    pub id: i64,
    pub file_path: String,
    pub file_size: i64,
    pub state: String,
    pub bytes_processed: i64,
    pub attempt_count: i64,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[allow(dead_code)]
pub async fn create_ingest_job(
    pool: &SqlitePool,
    file_path: &str,
    file_size: i64,
) -> Result<i64, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "INSERT INTO ingest_jobs (file_path, file_size, state, created_at, updated_at) \
         VALUES (?, ?, 'PENDING', ?, ?)",
    )
    .bind(file_path)
    .bind(file_size)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn get_next_pending_ingest_job(
    pool: &SqlitePool,
) -> Result<Option<IngestJobRow>, sqlx::Error> {
    sqlx::query_as::<_, IngestJobRow>(
        "SELECT id, file_path, file_size, state, bytes_processed, attempt_count, \
         error_message, created_at, updated_at \
         FROM ingest_jobs WHERE state = 'PENDING' ORDER BY created_at ASC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
}

pub async fn transition_ingest_job(
    pool: &SqlitePool,
    job_id: i64,
    from_state: &str,
    to_state: &str,
) -> Result<bool, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "UPDATE ingest_jobs SET state = ?, updated_at = ? \
         WHERE id = ? AND state = ?",
    )
    .bind(to_state)
    .bind(now)
    .bind(job_id)
    .bind(from_state)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn update_ingest_progress(
    pool: &SqlitePool,
    job_id: i64,
    bytes_processed: i64,
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query("UPDATE ingest_jobs SET bytes_processed = ?, updated_at = ? WHERE id = ?")
        .bind(bytes_processed)
        .bind(now)
        .bind(job_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn fail_ingest_job(
    pool: &SqlitePool,
    job_id: i64,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "UPDATE ingest_jobs SET state = 'FAILED', error_message = ?, \
         attempt_count = attempt_count + 1, updated_at = ? WHERE id = ?",
    )
    .bind(error_message)
    .bind(now)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn reset_interrupted_ingest_jobs(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "UPDATE ingest_jobs SET state = 'PENDING', error_message = 'interrupted by restart', \
         attempt_count = attempt_count + 1, updated_at = ? \
         WHERE state IN ('CHUNKING', 'UPLOADING')",
    )
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[allow(dead_code)]
pub async fn list_ingest_jobs(pool: &SqlitePool) -> Result<Vec<IngestJobRow>, sqlx::Error> {
    sqlx::query_as::<_, IngestJobRow>(
        "SELECT id, file_path, file_size, state, bytes_processed, attempt_count, \
         error_message, created_at, updated_at \
         FROM ingest_jobs ORDER BY created_at DESC LIMIT 100",
    )
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_ingest_job(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<Option<IngestJobRow>, sqlx::Error> {
    sqlx::query_as::<_, IngestJobRow>(
        "SELECT id, file_path, file_size, state, bytes_processed, attempt_count, \
         error_message, created_at, updated_at \
         FROM ingest_jobs WHERE id = ?",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn requeue_failed_ingest_job(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<bool, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "UPDATE ingest_jobs SET state = 'PENDING', error_message = NULL, \
         bytes_processed = 0, updated_at = ? WHERE id = ? AND state = 'FAILED'",
    )
    .bind(now)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_ingest_job(pool: &SqlitePool, job_id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM ingest_jobs WHERE id = ? AND state = 'GHOSTED'")
        .bind(job_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_failed_ingest_job(pool: &SqlitePool, job_id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM ingest_jobs WHERE id = ? AND state = 'FAILED'")
        .bind(job_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Find all pack_ids associated with an inode via its file_revisions → chunk_refs → pack_locations.
pub async fn get_pack_ids_for_inode(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT DISTINCT pl.pack_id
        FROM file_revisions fr
        INNER JOIN chunk_refs cr ON cr.revision_id = fr.revision_id
        INNER JOIN pack_locations pl ON pl.chunk_id = cr.chunk_id
        WHERE fr.inode_id = ?
        "#,
    )
    .bind(inode_id)
    .fetch_all(pool)
    .await
}

/// Reset a FAILED ingest job to PENDING, clearing error and resetting attempt_count.
pub async fn retry_ingest_job(pool: &SqlitePool, job_id: i64) -> Result<bool, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "UPDATE ingest_jobs SET state = 'PENDING', error_message = NULL, \
         attempt_count = 0, bytes_processed = 0, updated_at = ? \
         WHERE id = ? AND state = 'FAILED'",
    )
    .bind(now)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
