use sqlx::{FromRow, SqlitePool};

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
pub async fn init_db(db_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let pool = SqlitePool::connect(db_url).await?;

    sqlx::query!(
        r#"
        CREATE TABLE IF NOT EXISTS vault_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            master_key_salt BLOB NOT NULL,
            argon2_params TEXT NOT NULL,
            vault_id TEXT NOT NULL
        )
        "#
    )
    .execute(&pool)
    .await?;

    sqlx::query!(
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
        "#
    )
    .execute(&pool)
    .await?;

    sqlx::query!(
        r#"
        CREATE TABLE IF NOT EXISTS chunk_refs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            inode_id INTEGER REFERENCES inodes(id) ON DELETE CASCADE,
            chunk_id BLOB NOT NULL,
            file_offset INTEGER NOT NULL,
            size INTEGER NOT NULL
        )
        "#
    )
    .execute(&pool)
    .await?;

    sqlx::query!(
        r#"
        CREATE TABLE IF NOT EXISTS pack_locations (
            chunk_id BLOB PRIMARY KEY,
            pack_id TEXT NOT NULL,
            pack_offset INTEGER NOT NULL,
            encrypted_size INTEGER NOT NULL
        )
        "#
    )
    .execute(&pool)
    .await?;

    sqlx::query!(
        r#"
        CREATE TABLE IF NOT EXISTS upload_jobs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            pack_id TEXT UNIQUE NOT NULL,
            status TEXT NOT NULL,
            attempts INTEGER DEFAULT 0
        )
        "#
    )
    .execute(&pool)
    .await?;

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

fn validate_inode_kind(kind: &str) -> Result<(), sqlx::Error> {
    match kind {
        "FILE" | "DIR" => Ok(()),
        _ => Err(sqlx::Error::InvalidArgument(format!(
            "invalid inode kind '{kind}', expected FILE or DIR"
        ))),
    }
}
