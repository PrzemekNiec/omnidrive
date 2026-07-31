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
pub struct FileRevisionRecord {
    pub revision_id: i64,
    pub inode_id: i64,
    pub created_at: i64,
    pub size: i64,
    pub is_current: i64,
    pub immutable_until: Option<i64>,
    pub device_id: Option<String>,
    pub parent_revision_id: Option<i64>,
    pub origin: String,
    pub conflict_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevisionLineageRelation {
    Same,
    CandidateDescendsFromCurrent,
    CurrentDescendsFromCandidate,
    Parallel,
}

#[allow(dead_code)]
pub async fn create_file_revision(
    pool: &SqlitePool,
    inode_id: i64,
    size: i64,
    immutable_until: Option<i64>,
    device_id: Option<&str>,
    parent_revision_id: Option<i64>,
    origin: &str,
    conflict_reason: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        UPDATE file_revisions
        SET is_current = 0
        WHERE inode_id = ?
        "#,
    )
    .bind(inode_id)
    .execute(&mut *tx)
    .await?;

    let result = sqlx::query(
        r#"
        INSERT INTO file_revisions (
            inode_id,
            created_at,
            size,
            is_current,
            immutable_until,
            device_id,
            parent_revision_id,
            origin,
            conflict_reason
        )
        VALUES (
            ?,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            ?,
            1,
            ?,
            ?,
            ?,
            ?,
            ?
        )
        "#,
    )
    .bind(inode_id)
    .bind(size)
    .bind(immutable_until)
    .bind(device_id)
    .bind(parent_revision_id)
    .bind(origin)
    .bind(conflict_reason)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(result.last_insert_rowid())
}

#[allow(dead_code)]
pub async fn get_current_file_revision(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Option<FileRevisionRecord>, sqlx::Error> {
    sqlx::query_as::<_, FileRevisionRecord>(
        r#"
        SELECT revision_id, inode_id, created_at, size, is_current, immutable_until, device_id, parent_revision_id, origin, conflict_reason
        FROM file_revisions
        WHERE inode_id = ?
          AND is_current = 1
        ORDER BY revision_id DESC
        LIMIT 1
        "#,
    )
    .bind(inode_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_storage_mode_for_inode(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<StorageMode, sqlx::Error> {
    let inode_path = get_inode_path(pool, inode_id)
        .await?
        .unwrap_or_else(|| format!("inode/{inode_id}"));
    let policy_type = find_sync_policy_for_path(pool, &inode_path)
        .await?
        .map(|policy| policy.policy_type)
        .unwrap_or_else(|| "PARANOIA".to_string());
    Ok(StorageMode::from_policy_type(&policy_type))
}

#[allow(dead_code)]
pub async fn get_file_revision(
    pool: &SqlitePool,
    inode_id: i64,
    revision_id: i64,
) -> Result<Option<FileRevisionRecord>, sqlx::Error> {
    sqlx::query_as::<_, FileRevisionRecord>(
        r#"
        SELECT revision_id, inode_id, created_at, size, is_current, immutable_until, device_id, parent_revision_id, origin, conflict_reason
        FROM file_revisions
        WHERE inode_id = ?
          AND revision_id = ?
        LIMIT 1
        "#,
    )
    .bind(inode_id)
    .bind(revision_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn list_file_revisions(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Vec<FileRevisionRecord>, sqlx::Error> {
    sqlx::query_as::<_, FileRevisionRecord>(
        r#"
        SELECT revision_id, inode_id, created_at, size, is_current, immutable_until, device_id, parent_revision_id, origin, conflict_reason
        FROM file_revisions
        WHERE inode_id = ?
        ORDER BY created_at DESC, revision_id DESC
        "#,
    )
    .bind(inode_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_referencing_inode_ids_for_pack(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT DISTINCT fr.inode_id
        FROM pack_locations pl
        INNER JOIN chunk_refs cr
            ON cr.chunk_id = pl.chunk_id
        INNER JOIN file_revisions fr
            ON fr.revision_id = cr.revision_id
        WHERE pl.pack_id = ?
        ORDER BY fr.inode_id ASC
        "#,
    )
    .bind(pack_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn promote_revision_to_current(
    pool: &SqlitePool,
    revision_id: i64,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    let inode_id = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT inode_id
        FROM file_revisions
        WHERE revision_id = ?
        "#,
    )
    .bind(revision_id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE file_revisions
        SET is_current = 0
        WHERE inode_id = ?
        "#,
    )
    .bind(inode_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE file_revisions
        SET is_current = 1
        WHERE revision_id = ?
        "#,
    )
    .bind(revision_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn classify_revision_lineage(
    pool: &SqlitePool,
    candidate_revision_id: i64,
    current_revision_id: i64,
) -> Result<RevisionLineageRelation, sqlx::Error> {
    if candidate_revision_id == current_revision_id {
        return Ok(RevisionLineageRelation::Same);
    }

    if is_revision_ancestor(pool, current_revision_id, candidate_revision_id).await? {
        return Ok(RevisionLineageRelation::CandidateDescendsFromCurrent);
    }

    if is_revision_ancestor(pool, candidate_revision_id, current_revision_id).await? {
        return Ok(RevisionLineageRelation::CurrentDescendsFromCandidate);
    }

    Ok(RevisionLineageRelation::Parallel)
}

async fn is_revision_ancestor(
    pool: &SqlitePool,
    ancestor_revision_id: i64,
    descendant_revision_id: i64,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar::<_, i64>(
        r#"
        WITH RECURSIVE lineage(revision_id, parent_revision_id) AS (
            SELECT revision_id, parent_revision_id
            FROM file_revisions
            WHERE revision_id = ?

            UNION ALL

            SELECT fr.revision_id, fr.parent_revision_id
            FROM file_revisions fr
            INNER JOIN lineage l
                ON fr.revision_id = l.parent_revision_id
        )
        SELECT 1
        FROM lineage
        WHERE revision_id = ?
        LIMIT 1
        "#,
    )
    .bind(descendant_revision_id)
    .bind(ancestor_revision_id)
    .fetch_optional(pool)
    .await?;

    Ok(found.is_some())
}
