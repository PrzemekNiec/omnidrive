use crate::db::*;
use sqlx::FromRow;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ConflictEventRecord {
    pub conflict_id: i64,
    pub inode_id: i64,
    pub winning_revision_id: i64,
    pub losing_revision_id: i64,
    pub reason: String,
    pub materialized_inode_id: Option<i64>,
    pub materialized_revision_id: Option<i64>,
    pub created_at: i64,
}

#[allow(dead_code)]
pub async fn create_conflict_event(
    pool: &SqlitePool,
    inode_id: i64,
    winning_revision_id: i64,
    losing_revision_id: i64,
    reason: &str,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        INSERT INTO conflict_events (
            inode_id,
            winning_revision_id,
            losing_revision_id,
            reason,
            created_at
        )
        VALUES (
            ?,
            ?,
            ?,
            ?,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        )
        "#,
    )
    .bind(inode_id)
    .bind(winning_revision_id)
    .bind(losing_revision_id)
    .bind(reason)
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

#[allow(dead_code)]
pub async fn materialize_conflict_copy_from_revision(
    pool: &SqlitePool,
    source_revision_id: i64,
    device_id: Option<&str>,
    device_name: &str,
    reason: &str,
) -> Result<(i64, i64, String, i64), sqlx::Error> {
    let source_revision = sqlx::query_as::<_, FileRevisionRecord>(
        r#"
        SELECT revision_id, inode_id, created_at, size, is_current, immutable_until, device_id, parent_revision_id, origin, conflict_reason
        FROM file_revisions
        WHERE revision_id = ?
        LIMIT 1
        "#,
    )
    .bind(source_revision_id)
    .fetch_one(pool)
    .await?;

    let source_inode = get_inode_by_id(pool, source_revision.inode_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

    let timestamp = source_revision.created_at;
    let base_name = build_conflict_copy_name(&source_inode.name, device_name, timestamp);

    let mut created_inode_id = None;
    let mut final_name = base_name.clone();
    for attempt in 0..16 {
        let candidate = if attempt == 0 {
            base_name.clone()
        } else {
            disambiguate_conflict_copy_name(&base_name, attempt)
        };

        match create_inode(
            pool,
            source_inode.parent_id,
            &candidate,
            &source_inode.kind,
            source_revision.size,
        )
        .await
        {
            Ok(inode_id) => {
                created_inode_id = Some(inode_id);
                final_name = candidate;
                break;
            }
            Err(sqlx::Error::Database(err)) if err.is_unique_violation() => continue,
            Err(err) => return Err(err),
        }
    }

    let inode_id = created_inode_id.ok_or(sqlx::Error::RowNotFound)?;
    let revision_id = create_file_revision(
        pool,
        inode_id,
        source_revision.size,
        source_revision.immutable_until,
        device_id,
        Some(source_revision.revision_id),
        "conflict_copy",
        Some(reason),
    )
    .await?;
    copy_chunk_refs(pool, source_revision.revision_id, revision_id).await?;
    let conflict_id = create_conflict_event(
        pool,
        source_revision.inode_id,
        source_revision.revision_id,
        revision_id,
        reason,
    )
    .await?;
    attach_conflict_materialization(pool, conflict_id, inode_id, revision_id).await?;

    Ok((inode_id, revision_id, final_name, conflict_id))
}

#[allow(dead_code)]
pub async fn attach_conflict_materialization(
    pool: &SqlitePool,
    conflict_id: i64,
    materialized_inode_id: i64,
    materialized_revision_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE conflict_events
        SET materialized_inode_id = ?,
            materialized_revision_id = ?
        WHERE conflict_id = ?
        "#,
    )
    .bind(materialized_inode_id)
    .bind(materialized_revision_id)
    .bind(conflict_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn list_recent_conflicts(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<ConflictEventRecord>, sqlx::Error> {
    sqlx::query_as::<_, ConflictEventRecord>(
        r#"
        SELECT
            conflict_id,
            inode_id,
            winning_revision_id,
            losing_revision_id,
            reason,
            materialized_inode_id,
            materialized_revision_id,
            created_at
        FROM conflict_events
        ORDER BY created_at DESC, conflict_id DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

fn build_conflict_copy_name(original_name: &str, device_name: &str, timestamp_ms: i64) -> String {
    let (stem, extension) = split_file_name(original_name);
    format!(
        "{stem} (conflict - {} - {timestamp_ms}){extension}",
        sanitize_conflict_component(device_name)
    )
}

fn disambiguate_conflict_copy_name(base_name: &str, attempt: usize) -> String {
    let (stem, extension) = split_file_name(base_name);
    format!("{stem} [{attempt}]{extension}")
}

fn split_file_name(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, &name[stem.len()..]),
        _ => (name, ""),
    }
}

fn sanitize_conflict_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ if ch.is_control() => '_',
            _ => ch,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn materialize_conflict_copy_creates_inode_copies_chunks_and_records_event()
    -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let inode = create_inode(&pool, None, "report.txt", "FILE", 20).await?;
        let source = create_file_revision(
            &pool,
            inode,
            20,
            None,
            Some("dev-a"),
            None,
            "local_write",
            None,
        )
        .await?;
        register_chunk(&pool, source, &[1u8; 32], 0, 20).await?;

        let (copy_inode, copy_rev, name, conflict_id) = materialize_conflict_copy_from_revision(
            &pool,
            source,
            Some("dev-a"),
            "Laptop",
            "parallel_local_edit",
        )
        .await?;

        assert_ne!(copy_inode, inode, "conflict copy must be a distinct inode");
        assert!(
            name.starts_with("report (conflict - Laptop - "),
            "unexpected name: {name}"
        );
        assert!(
            name.ends_with(").txt"),
            "extension must be preserved: {name}"
        );

        let copied = get_chunk_refs_for_revision(&pool, copy_rev).await?;
        assert_eq!(
            copied.len(),
            1,
            "chunk refs must be copied so the conflict copy is recoverable"
        );
        assert_eq!(copied[0].size, 20);

        let events = list_recent_conflicts(&pool, 10).await?;
        let event = events
            .iter()
            .find(|e| e.conflict_id == conflict_id)
            .expect("conflict event surfaced");
        assert_eq!(event.reason, "parallel_local_edit");
        assert_eq!(event.inode_id, inode);
        assert_eq!(event.materialized_inode_id, Some(copy_inode));
        assert_eq!(event.materialized_revision_id, Some(copy_rev));
        Ok(())
    }

    #[tokio::test]
    async fn materialize_conflict_copy_disambiguates_name_on_collision()
    -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let inode = create_inode(&pool, None, "notes.md", "FILE", 5).await?;
        let source = create_file_revision(
            &pool,
            inode,
            5,
            None,
            Some("dev-a"),
            None,
            "local_write",
            None,
        )
        .await?;
        register_chunk(&pool, source, &[2u8; 32], 0, 5).await?;

        let (_i1, _r1, name1, _c1) = materialize_conflict_copy_from_revision(
            &pool,
            source,
            Some("dev-a"),
            "PC",
            "stale_local_base",
        )
        .await?;
        let (_i2, _r2, name2, _c2) = materialize_conflict_copy_from_revision(
            &pool,
            source,
            Some("dev-a"),
            "PC",
            "stale_local_base",
        )
        .await?;

        assert_ne!(name1, name2, "second copy must not collide with the first");
        assert!(
            name2.contains(" [1]"),
            "second copy must be disambiguated: {name2}"
        );
        Ok(())
    }
}
