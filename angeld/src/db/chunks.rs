use crate::db::*;
use serde::Serialize;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::FromRow;
use sqlx::Row;
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;
use uuid::Uuid;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ChunkRecord {
    pub id: i64,
    pub revision_id: i64,
    pub chunk_id: Vec<u8>,
    pub file_offset: i64,
    pub size: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ChunkLookupRecord {
    pub inode_id: i64,
    pub revision_id: i64,
    pub chunk_id: Vec<u8>,
    pub chunk_index: i64,
    pub file_offset: i64,
    pub size: i64,
    pub pack_id: String,
    pub pack_offset: i64,
    pub encrypted_size: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct FileChunkLocation {
    pub chunk_id: Vec<u8>,
    pub chunk_index: i64,
    pub file_offset: i64,
    pub size: i64,
    pub pack_id: String,
    pub pack_offset: i64,
    pub encrypted_size: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct ChunkRefRecord {
    pub id: i64,
    pub revision_id: i64,
    pub chunk_id: Vec<u8>,
    pub file_offset: i64,
    pub size: i64,
}

#[allow(dead_code)]
pub async fn register_chunk(
    pool: &SqlitePool,
    revision_id: i64,
    chunk_id: &[u8],
    offset: i64,
    size: i64,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        INSERT INTO chunk_refs (revision_id, chunk_id, file_offset, size)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(revision_id)
    .bind(chunk_id)
    .bind(offset)
    .bind(size)
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

#[allow(dead_code)]
pub async fn copy_chunk_refs(
    pool: &SqlitePool,
    from_revision_id: i64,
    to_revision_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO chunk_refs (revision_id, chunk_id, file_offset, size)
        SELECT ?, chunk_id, file_offset, size
        FROM chunk_refs
        WHERE revision_id = ?
        ORDER BY file_offset ASC
        "#,
    )
    .bind(to_revision_id)
    .bind(from_revision_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_chunk_lookup_by_chunk_id(
    pool: &SqlitePool,
    chunk_id: &[u8],
) -> Result<Option<ChunkLookupRecord>, sqlx::Error> {
    sqlx::query_as::<_, ChunkLookupRecord>(
        r#"
        WITH ordered_chunks AS (
            SELECT
                fr.inode_id,
                fr.revision_id,
                cr.chunk_id,
                cr.file_offset,
                cr.size,
                ROW_NUMBER() OVER (
                    PARTITION BY fr.revision_id
                    ORDER BY cr.file_offset ASC
                ) - 1 AS chunk_index
            FROM chunk_refs cr
            INNER JOIN file_revisions fr
                ON fr.revision_id = cr.revision_id
            WHERE cr.chunk_id = ?
            ORDER BY fr.is_current DESC, fr.created_at DESC, fr.revision_id DESC
        )
        SELECT
            oc.inode_id,
            oc.revision_id,
            oc.chunk_id,
            oc.chunk_index,
            oc.file_offset,
            oc.size,
            pl.pack_id,
            pl.pack_offset,
            pl.encrypted_size
        FROM ordered_chunks oc
        INNER JOIN pack_locations pl
            ON pl.chunk_id = oc.chunk_id
        LIMIT 1
        "#,
    )
    .bind(chunk_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn delete_file_chunks(pool: &SqlitePool, inode_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        DELETE FROM chunk_refs
        WHERE revision_id IN (
            SELECT revision_id
            FROM file_revisions
            WHERE inode_id = ?
        )
        "#,
    )
    .bind(inode_id)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        DELETE FROM file_revisions
        WHERE inode_id = ?
        "#,
    )
    .bind(inode_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_file_chunk_locations(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Vec<FileChunkLocation>, sqlx::Error> {
    sqlx::query_as::<_, FileChunkLocation>(
        r#"
        WITH ordered_chunks AS (
            SELECT
                cr.chunk_id,
                cr.file_offset,
                cr.size,
                ROW_NUMBER() OVER (ORDER BY cr.file_offset ASC) - 1 AS chunk_index
            FROM chunk_refs cr
            INNER JOIN file_revisions fr
                ON fr.revision_id = cr.revision_id
            WHERE fr.inode_id = ?
              AND fr.is_current = 1
        )
        SELECT
            oc.chunk_id,
            oc.chunk_index,
            oc.file_offset,
            oc.size,
            pl.pack_id,
            pl.pack_offset,
            pl.encrypted_size
        FROM ordered_chunks oc
        INNER JOIN pack_locations pl
            ON pl.chunk_id = oc.chunk_id
        ORDER BY oc.file_offset ASC
        "#,
    )
    .bind(inode_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_revision_chunk_locations_in_range(
    pool: &SqlitePool,
    inode_id: i64,
    revision_id: i64,
    start_offset: i64,
    end_offset: i64,
) -> Result<Vec<FileChunkLocation>, sqlx::Error> {
    sqlx::query_as::<_, FileChunkLocation>(
        r#"
        WITH ordered_chunks AS (
            SELECT
                cr.chunk_id,
                cr.file_offset,
                cr.size,
                ROW_NUMBER() OVER (ORDER BY cr.file_offset ASC) - 1 AS chunk_index
            FROM chunk_refs cr
            INNER JOIN file_revisions fr
                ON fr.revision_id = cr.revision_id
            WHERE fr.inode_id = ?
              AND fr.revision_id = ?
        )
        SELECT
            oc.chunk_id,
            oc.chunk_index,
            oc.file_offset,
            oc.size,
            pl.pack_id,
            pl.pack_offset,
            pl.encrypted_size
        FROM ordered_chunks oc
        INNER JOIN pack_locations pl
            ON pl.chunk_id = oc.chunk_id
        WHERE (oc.file_offset + oc.size) > ?
          AND oc.file_offset < ?
        ORDER BY oc.file_offset ASC
        "#,
    )
    .bind(inode_id)
    .bind(revision_id)
    .bind(start_offset)
    .bind(end_offset)
    .fetch_all(pool)
    .await
}

/// Get chunk locations for a specific revision (for sharing).
pub async fn get_chunk_locations_for_revision(
    pool: &SqlitePool,
    revision_id: i64,
) -> Result<Vec<FileChunkLocation>, sqlx::Error> {
    sqlx::query_as::<_, FileChunkLocation>(
        r#"
        WITH ordered_chunks AS (
            SELECT
                cr.chunk_id,
                cr.file_offset,
                cr.size,
                ROW_NUMBER() OVER (ORDER BY cr.file_offset ASC) - 1 AS chunk_index
            FROM chunk_refs cr
            WHERE cr.revision_id = ?
        )
        SELECT
            oc.chunk_id,
            oc.chunk_index,
            oc.file_offset,
            oc.size,
            pl.pack_id,
            pl.pack_offset,
            pl.encrypted_size
        FROM ordered_chunks oc
        INNER JOIN pack_locations pl
            ON pl.chunk_id = oc.chunk_id
        ORDER BY oc.file_offset ASC
        "#,
    )
    .bind(revision_id)
    .fetch_all(pool)
    .await
}

/// Get chunk refs for a specific revision, ordered by file_offset.
#[allow(dead_code)]
pub async fn get_chunk_refs_for_revision(
    pool: &SqlitePool,
    revision_id: i64,
) -> Result<Vec<ChunkRefRecord>, sqlx::Error> {
    sqlx::query_as::<_, ChunkRefRecord>(
        "SELECT id, revision_id, chunk_id, file_offset, size \
         FROM chunk_refs WHERE revision_id = ? ORDER BY file_offset ASC",
    )
    .bind(revision_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_chunks_for_pack(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Vec<ChunkRecord>, sqlx::Error> {
    sqlx::query_as::<_, ChunkRecord>(
        r#"
        SELECT cr.id, cr.revision_id, cr.chunk_id, cr.file_offset, cr.size
        FROM pack_locations pl
        INNER JOIN chunk_refs cr
            ON cr.chunk_id = pl.chunk_id
        WHERE pl.pack_id = ?
        ORDER BY cr.file_offset ASC, cr.id ASC
        "#,
    )
    .bind(pack_id)
    .fetch_all(pool)
    .await
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
        SELECT cr.id, cr.revision_id, cr.chunk_id, cr.file_offset, cr.size
        FROM chunk_refs cr
        INNER JOIN file_revisions fr
            ON fr.revision_id = cr.revision_id
        WHERE fr.inode_id = ?
          AND fr.is_current = 1
        ORDER BY file_offset ASC
        "#,
    )
    .bind(inode_id)
    .fetch_all(pool)
    .await
}
