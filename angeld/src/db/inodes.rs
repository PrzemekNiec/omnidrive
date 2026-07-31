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
pub struct InodeRecord {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub kind: String,
    pub size: i64,
    pub mode: Option<i64>,
    pub mtime: Option<i64>,
    pub deleted_at: Option<i64>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct FileInventoryRecord {
    pub inode_id: i64,
    pub path: String,
    pub size: i64,
    pub current_revision_id: Option<i64>,
    pub current_revision_created_at: Option<i64>,
    pub smart_sync_pin_state: Option<i64>,
    pub smart_sync_hydration_state: Option<i64>,
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
        SELECT id, parent_id, name, kind, size, mode, mtime, deleted_at
        FROM inodes
        WHERE ((parent_id IS NULL AND ? IS NULL) OR parent_id = ?)
          AND name = ?
          AND deleted_at IS NULL
        "#,
    )
    .bind(parent_id)
    .bind(parent_id)
    .bind(name)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_inode_by_id(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Option<InodeRecord>, sqlx::Error> {
    sqlx::query_as::<_, InodeRecord>(
        r#"
        SELECT id, parent_id, name, kind, size, mode, mtime, deleted_at
        FROM inodes
        WHERE id = ?
        "#,
    )
    .bind(inode_id)
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
pub async fn delete_inode_record(pool: &SqlitePool, inode_id: i64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        DELETE FROM inodes
        WHERE id = ?
        "#,
    )
    .bind(inode_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn soft_delete_inode(
    pool: &SqlitePool,
    inode_id: i64,
    now_ms: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE inodes SET deleted_at = ? WHERE id = ? AND kind = 'FILE' AND deleted_at IS NULL",
    )
    .bind(now_ms)
    .bind(inode_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

#[derive(Clone, Debug, FromRow)]
pub struct SoftDeletedInode {
    pub inode_id: i64,
    pub name: String,
    pub deleted_at: i64,
    pub size: i64,
}

pub async fn list_soft_deleted(pool: &SqlitePool) -> Result<Vec<SoftDeletedInode>, sqlx::Error> {
    sqlx::query_as::<_, SoftDeletedInode>(
        "SELECT id AS inode_id, name, deleted_at, size \
         FROM inodes WHERE deleted_at IS NOT NULL AND kind = 'FILE' \
         ORDER BY deleted_at DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn restore_soft_deleted_inode(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<String, sqlx::Error> {
    let inode = get_inode_by_id(pool, inode_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    let mut final_name = inode.name.clone();
    let mut attempt = 1;
    while get_inode_by_path(pool, inode.parent_id, &final_name)
        .await?
        .is_some()
    {
        final_name = restored_name(&inode.name, attempt);
        attempt += 1;
    }
    sqlx::query("UPDATE inodes SET deleted_at = NULL, name = ? WHERE id = ?")
        .bind(&final_name)
        .bind(inode_id)
        .execute(pool)
        .await?;
    Ok(final_name)
}

fn restored_name(original: &str, attempt: usize) -> String {
    let (stem, ext) = match original.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() => (s, format!(".{e}")),
        _ => (original, String::new()),
    };
    if attempt == 1 {
        format!("{stem} (restored){ext}")
    } else {
        format!("{stem} (restored {attempt}){ext}")
    }
}

pub async fn list_expired_soft_deleted(
    pool: &SqlitePool,
    cutoff_ms: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT id FROM inodes WHERE deleted_at IS NOT NULL AND deleted_at < ? ORDER BY id ASC",
    )
    .bind(cutoff_ms)
    .fetch_all(pool)
    .await
}

fn validate_inode_kind(kind: &str) -> Result<(), sqlx::Error> {
    match kind {
        "FILE" | "DIR" => Ok(()),
        _ => Err(sqlx::Error::InvalidArgument(format!(
            "invalid inode kind '{kind}', expected FILE or DIR"
        ))),
    }
}
