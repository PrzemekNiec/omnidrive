use sqlx::FromRow;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct CacheEntryRecord {
    pub cache_key: String,
    pub inode_id: i64,
    pub revision_id: i64,
    pub chunk_index: i64,
    pub pack_id: String,
    pub file_path: String,
    pub cache_path: String,
    pub size: i64,
    pub created_at: i64,
    pub last_accessed_at: i64,
    pub access_count: i64,
    pub is_prefetched: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct CacheStatusSummary {
    pub total_entries: i64,
    pub total_bytes: i64,
    pub prefetched_entries: i64,
}

#[allow(dead_code)]
pub async fn get_cache_entry(
    pool: &SqlitePool,
    cache_key: &str,
) -> Result<Option<CacheEntryRecord>, sqlx::Error> {
    sqlx::query_as::<_, CacheEntryRecord>(
        r#"
        SELECT
            cache_key,
            inode_id,
            revision_id,
            chunk_index,
            pack_id,
            file_path,
            cache_path,
            size,
            created_at,
            last_accessed_at,
            access_count,
            is_prefetched
        FROM cache_entries
        WHERE cache_key = ?
        "#,
    )
    .bind(cache_key)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn upsert_cache_entry(
    pool: &SqlitePool,
    cache_key: &str,
    inode_id: i64,
    revision_id: i64,
    chunk_index: i64,
    pack_id: &str,
    file_path: &str,
    cache_path: &str,
    size: i64,
    is_prefetched: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO cache_entries (
            cache_key,
            inode_id,
            revision_id,
            chunk_index,
            pack_id,
            file_path,
            cache_path,
            size,
            created_at,
            last_accessed_at,
            access_count,
            is_prefetched
        )
        VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?,
            CAST(strftime('%s','now') AS INTEGER),
            CAST(strftime('%s','now') AS INTEGER),
            1,
            ?
        )
        ON CONFLICT(cache_key) DO UPDATE SET
            inode_id = excluded.inode_id,
            revision_id = excluded.revision_id,
            chunk_index = excluded.chunk_index,
            pack_id = excluded.pack_id,
            file_path = excluded.file_path,
            cache_path = excluded.cache_path,
            size = excluded.size,
            last_accessed_at = CAST(strftime('%s','now') AS INTEGER),
            access_count = cache_entries.access_count + 1,
            is_prefetched = excluded.is_prefetched
        "#,
    )
    .bind(cache_key)
    .bind(inode_id)
    .bind(revision_id)
    .bind(chunk_index)
    .bind(pack_id)
    .bind(file_path)
    .bind(cache_path)
    .bind(size)
    .bind(if is_prefetched { 1 } else { 0 })
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn touch_cache_entry(pool: &SqlitePool, cache_key: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE cache_entries
        SET last_accessed_at = CAST(strftime('%s','now') AS INTEGER),
            access_count = access_count + 1
        WHERE cache_key = ?
        "#,
    )
    .bind(cache_key)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn list_cache_entries_by_lru(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<CacheEntryRecord>, sqlx::Error> {
    sqlx::query_as::<_, CacheEntryRecord>(
        r#"
        SELECT
            cache_key,
            inode_id,
            revision_id,
            chunk_index,
            pack_id,
            file_path,
            cache_path,
            size,
            created_at,
            last_accessed_at,
            access_count,
            is_prefetched
        FROM cache_entries
        ORDER BY last_accessed_at ASC, created_at ASC, cache_key ASC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_total_cache_size(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(SUM(size), 0)
        FROM cache_entries
        "#,
    )
    .fetch_one(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_cache_status_summary(
    pool: &SqlitePool,
) -> Result<CacheStatusSummary, sqlx::Error> {
    sqlx::query_as::<_, CacheStatusSummary>(
        r#"
        SELECT
            COUNT(*) AS total_entries,
            COALESCE(SUM(size), 0) AS total_bytes,
            COALESCE(SUM(CASE WHEN is_prefetched = 1 THEN 1 ELSE 0 END), 0) AS prefetched_entries
        FROM cache_entries
        "#,
    )
    .fetch_one(pool)
    .await
}

#[allow(dead_code)]
pub async fn delete_cache_entry(pool: &SqlitePool, cache_key: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        DELETE FROM cache_entries
        WHERE cache_key = ?
        "#,
    )
    .bind(cache_key)
    .execute(pool)
    .await?;

    Ok(())
}
