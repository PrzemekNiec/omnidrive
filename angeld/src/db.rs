use sqlx::{FromRow, Row, SqlitePool};

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct VaultRecord {
    pub id: i64,
    pub master_key_salt: Vec<u8>,
    pub argon2_params: String,
    pub vault_id: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct InodeRecord {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub kind: String,
    pub size: i64,
    pub mode: Option<i64>,
    pub mtime: Option<i64>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ChunkRecord {
    pub id: i64,
    pub inode_id: i64,
    pub chunk_id: Vec<u8>,
    pub file_offset: i64,
    pub size: i64,
}

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
pub async fn init_db(db_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let pool = SqlitePool::connect(db_url).await?;

    sqlx::query(
        r#"
        DROP TABLE IF EXISTS files
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS vault_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            master_key_salt BLOB NOT NULL,
            argon2_params TEXT NOT NULL,
            vault_id TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS inodes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            parent_id INTEGER REFERENCES inodes(id),
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            size INTEGER DEFAULT 0,
            mode INTEGER,
            mtime INTEGER,
            UNIQUE(parent_id, name)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chunk_refs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            inode_id INTEGER REFERENCES inodes(id) ON DELETE CASCADE,
            chunk_id BLOB NOT NULL,
            file_offset INTEGER NOT NULL,
            size INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS pack_locations (
            chunk_id BLOB PRIMARY KEY,
            pack_id TEXT NOT NULL,
            pack_offset INTEGER NOT NULL,
            encrypted_size INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS upload_jobs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            pack_id TEXT UNIQUE NOT NULL,
            status TEXT NOT NULL,
            attempts INTEGER DEFAULT 0
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS upload_job_targets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            job_id INTEGER NOT NULL REFERENCES upload_jobs(id) ON DELETE CASCADE,
            provider TEXT NOT NULL,
            status TEXT NOT NULL,
            attempts INTEGER DEFAULT 0,
            last_error TEXT,
            bucket TEXT,
            object_key TEXT,
            etag TEXT,
            version_id TEXT,
            last_attempt_at INTEGER,
            updated_at INTEGER,
            completed_at INTEGER,
            UNIQUE(job_id, provider)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    ensure_column_exists(&pool, "upload_job_targets", "last_attempt_at", "INTEGER").await?;
    ensure_column_exists(&pool, "upload_job_targets", "updated_at", "INTEGER").await?;

    Ok(pool)
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
        SELECT id, master_key_salt, argon2_params, vault_id
        FROM vault_state
        WHERE id = 1
        "#,
    )
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn create_inode(
    pool: &SqlitePool,
    parent_id: Option<i64>,
    name: &str,
    kind: &str,
    size: i64,
) -> Result<i64, sqlx::Error> {
    validate_inode_kind(kind)?;

    let result = sqlx::query(
        r#"
        INSERT INTO inodes (parent_id, name, kind, size)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(parent_id)
    .bind(name)
    .bind(kind)
    .bind(size)
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

#[allow(dead_code)]
pub async fn upsert_inode(
    pool: &SqlitePool,
    parent_id: Option<i64>,
    name: &str,
    kind: &str,
    size: i64,
    mtime: Option<i64>,
) -> Result<i64, sqlx::Error> {
    validate_inode_kind(kind)?;

    if let Some(existing) = get_inode_by_path(pool, parent_id, name).await? {
        if existing.kind != kind {
            return Err(sqlx::Error::InvalidArgument(format!(
                "inode kind mismatch for '{name}': existing={}, new={kind}",
                existing.kind
            )));
        }

        sqlx::query(
            r#"
            UPDATE inodes
            SET size = ?, mtime = ?
            WHERE id = ?
            "#,
        )
        .bind(size)
        .bind(mtime)
        .bind(existing.id)
        .execute(pool)
        .await?;

        return Ok(existing.id);
    }

    let result = sqlx::query(
        r#"
        INSERT INTO inodes (parent_id, name, kind, size, mtime)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(parent_id)
    .bind(name)
    .bind(kind)
    .bind(size)
    .bind(mtime)
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

#[allow(dead_code)]
pub async fn get_inode_by_path(
    pool: &SqlitePool,
    parent_id: Option<i64>,
    name: &str,
) -> Result<Option<InodeRecord>, sqlx::Error> {
    sqlx::query_as::<_, InodeRecord>(
        r#"
        SELECT id, parent_id, name, kind, size, mode, mtime
        FROM inodes
        WHERE ((parent_id IS NULL AND ? IS NULL) OR parent_id = ?)
          AND name = ?
        "#,
    )
    .bind(parent_id)
    .bind(parent_id)
    .bind(name)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn resolve_path(pool: &SqlitePool, path: &str) -> Result<Option<i64>, sqlx::Error> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return Ok(None);
    }

    let mut current_parent_id = None;

    for segment in trimmed.split('/').filter(|segment| !segment.is_empty()) {
        let inode = match get_inode_by_path(pool, current_parent_id, segment).await? {
            Some(inode) => inode,
            None => return Ok(None),
        };

        current_parent_id = Some(inode.id);
    }

    Ok(current_parent_id)
}

#[allow(dead_code)]
pub async fn register_chunk(
    pool: &SqlitePool,
    inode_id: i64,
    chunk_id: &[u8],
    offset: i64,
    size: i64,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        INSERT INTO chunk_refs (inode_id, chunk_id, file_offset, size)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(inode_id)
    .bind(chunk_id)
    .bind(offset)
    .bind(size)
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

#[allow(dead_code)]
pub async fn delete_file_chunks(pool: &SqlitePool, inode_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        DELETE FROM chunk_refs
        WHERE inode_id = ?
        "#,
    )
    .bind(inode_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn link_chunk_to_pack(
    pool: &SqlitePool,
    chunk_id: &[u8],
    pack_id: &str,
    pack_offset: i64,
    enc_size: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO pack_locations (chunk_id, pack_id, pack_offset, encrypted_size)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(chunk_id) DO UPDATE SET
            pack_id = excluded.pack_id,
            pack_offset = excluded.pack_offset,
            encrypted_size = excluded.encrypted_size
        "#,
    )
    .bind(chunk_id)
    .bind(pack_id)
    .bind(pack_offset)
    .bind(enc_size)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_file_chunks(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Vec<ChunkRecord>, sqlx::Error> {
    sqlx::query_as::<_, ChunkRecord>(
        r#"
        SELECT id, inode_id, chunk_id, file_offset, size
        FROM chunk_refs
        WHERE inode_id = ?
        ORDER BY file_offset ASC
        "#,
    )
    .bind(inode_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn queue_pack_for_upload(pool: &SqlitePool, pack_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO upload_jobs (pack_id, status)
        VALUES (?, 'PENDING')
        ON CONFLICT(pack_id) DO NOTHING
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

fn validate_inode_kind(kind: &str) -> Result<(), sqlx::Error> {
    match kind {
        "FILE" | "DIR" => Ok(()),
        _ => Err(sqlx::Error::InvalidArgument(format!(
            "invalid inode kind '{kind}', expected FILE or DIR"
        ))),
    }
}

async fn ensure_column_exists(
    pool: &SqlitePool,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), sqlx::Error> {
    let pragma = format!("PRAGMA table_info({table})");
    let columns = sqlx::query(&pragma).fetch_all(pool).await?;
    let exists = columns.iter().any(|row| {
        row.try_get::<String, _>("name")
            .map(|name| name == column)
            .unwrap_or(false)
    });

    if !exists {
        let alter = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
        sqlx::query(&alter).execute(pool).await?;
    }

    Ok(())
}
