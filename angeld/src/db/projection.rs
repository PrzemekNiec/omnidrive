use crate::db::sync_policies::normalize_policy_path;
use crate::db::*;
use sqlx::FromRow;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ProjectionFileRecord {
    pub inode_id: i64,
    pub path: String,
    pub revision_id: i64,
    pub size: i64,
    pub created_at: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct SmartSyncStateRecord {
    pub inode_id: i64,
    pub revision_id: i64,
    pub pin_state: i64,
    pub hydration_state: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct SmartSyncEvictionRecord {
    pub inode_id: i64,
    pub revision_id: i64,
    pub path: String,
}

#[allow(dead_code)]
pub async fn ensure_smart_sync_state(
    pool: &SqlitePool,
    inode_id: i64,
    revision_id: i64,
) -> Result<SmartSyncStateRecord, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO smart_sync_state (inode_id, revision_id, pin_state, hydration_state)
        VALUES (?, ?, 0, 0)
        ON CONFLICT(inode_id) DO UPDATE SET
            revision_id = excluded.revision_id
        "#,
    )
    .bind(inode_id)
    .bind(revision_id)
    .execute(pool)
    .await?;

    get_smart_sync_state(pool, inode_id)
        .await?
        .ok_or_else(|| sqlx::Error::RowNotFound)
}

#[allow(dead_code)]
pub async fn get_smart_sync_state(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Option<SmartSyncStateRecord>, sqlx::Error> {
    sqlx::query_as::<_, SmartSyncStateRecord>(
        r#"
        SELECT inode_id, revision_id, pin_state, hydration_state
        FROM smart_sync_state
        WHERE inode_id = ?
        "#,
    )
    .bind(inode_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn set_pin_state(
    pool: &SqlitePool,
    inode_id: i64,
    pin_state: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE smart_sync_state
        SET pin_state = ?
        WHERE inode_id = ?
        "#,
    )
    .bind(pin_state)
    .bind(inode_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn set_hydration_state(
    pool: &SqlitePool,
    inode_id: i64,
    hydration_state: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE smart_sync_state
        SET hydration_state = ?
        WHERE inode_id = ?
        "#,
    )
    .bind(hydration_state)
    .bind(inode_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_active_files_for_projection(
    pool: &SqlitePool,
) -> Result<Vec<ProjectionFileRecord>, sqlx::Error> {
    let mut records = sqlx::query_as::<_, ProjectionFileRecord>(
        r#"
        WITH RECURSIVE inode_paths AS (
            SELECT
                id,
                parent_id,
                name AS path
            FROM inodes
            WHERE parent_id IS NULL

            UNION ALL

            SELECT
                child.id,
                child.parent_id,
                inode_paths.path || '/' || child.name AS path
            FROM inodes child
            INNER JOIN inode_paths
                ON child.parent_id = inode_paths.id
        )
        SELECT
            i.id AS inode_id,
            inode_paths.path AS path,
            fr.revision_id AS revision_id,
            fr.size AS size,
            fr.created_at AS created_at
        FROM inodes i
        INNER JOIN inode_paths
            ON inode_paths.id = i.id
        INNER JOIN file_revisions fr
            ON fr.inode_id = i.id
           AND fr.is_current = 1
        WHERE i.kind = 'FILE'
          AND i.deleted_at IS NULL
        ORDER BY inode_paths.path ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut base_paths = list_sync_policies(pool)
        .await?
        .into_iter()
        .map(|policy| policy.path_prefix)
        .collect::<Vec<_>>();
    if let Ok(watch_dir) = std::env::var("OMNIDRIVE_WATCH_DIR") {
        base_paths.push(watch_dir);
    }

    for record in &mut records {
        record.path = projection_relative_path(&record.path, &base_paths);
    }

    Ok(records)
}

#[allow(dead_code)]
pub async fn get_active_file_for_projection_by_inode(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Option<ProjectionFileRecord>, sqlx::Error> {
    let mut records = sqlx::query_as::<_, ProjectionFileRecord>(
        r#"
        WITH RECURSIVE inode_paths AS (
            SELECT
                id,
                parent_id,
                name AS path
            FROM inodes
            WHERE parent_id IS NULL

            UNION ALL

            SELECT
                child.id,
                child.parent_id,
                inode_paths.path || '/' || child.name AS path
            FROM inodes child
            INNER JOIN inode_paths
                ON child.parent_id = inode_paths.id
        )
        SELECT
            i.id AS inode_id,
            inode_paths.path AS path,
            fr.revision_id AS revision_id,
            fr.size AS size,
            fr.created_at AS created_at
        FROM inodes i
        INNER JOIN inode_paths
            ON inode_paths.id = i.id
        INNER JOIN file_revisions fr
            ON fr.inode_id = i.id
           AND fr.is_current = 1
        WHERE i.kind = 'FILE'
          AND i.id = ?
        LIMIT 1
        "#,
    )
    .bind(inode_id)
    .fetch_all(pool)
    .await?;

    let Some(mut record) = records.pop() else {
        return Ok(None);
    };

    let mut base_paths = list_sync_policies(pool)
        .await?
        .into_iter()
        .map(|policy| policy.path_prefix)
        .collect::<Vec<_>>();
    if let Ok(watch_dir) = std::env::var("OMNIDRIVE_WATCH_DIR") {
        base_paths.push(watch_dir);
    }
    record.path = projection_relative_path(&record.path, &base_paths);

    Ok(Some(record))
}

#[allow(dead_code)]
pub async fn list_unpinned_hydrated_files_for_eviction(
    pool: &SqlitePool,
) -> Result<Vec<SmartSyncEvictionRecord>, sqlx::Error> {
    let mut records = sqlx::query_as::<_, SmartSyncEvictionRecord>(
        r#"
        WITH RECURSIVE inode_paths AS (
            SELECT
                id,
                parent_id,
                name AS path
            FROM inodes
            WHERE parent_id IS NULL

            UNION ALL

            SELECT
                child.id,
                child.parent_id,
                inode_paths.path || '/' || child.name AS path
            FROM inodes child
            INNER JOIN inode_paths
                ON child.parent_id = inode_paths.id
        )
        SELECT
            s.inode_id AS inode_id,
            s.revision_id AS revision_id,
            inode_paths.path AS path
        FROM smart_sync_state s
        INNER JOIN inodes i
            ON i.id = s.inode_id
        INNER JOIN inode_paths
            ON inode_paths.id = i.id
        WHERE i.kind = 'FILE'
          AND i.deleted_at IS NULL
          AND s.pin_state = 0
          AND s.hydration_state = 1
        ORDER BY inode_paths.path ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut base_paths = list_sync_policies(pool)
        .await?
        .into_iter()
        .map(|policy| policy.path_prefix)
        .collect::<Vec<_>>();
    if let Ok(watch_dir) = std::env::var("OMNIDRIVE_WATCH_DIR") {
        base_paths.push(watch_dir);
    }

    for record in &mut records {
        record.path = projection_relative_path(&record.path, &base_paths);
    }

    Ok(records)
}

fn projection_relative_path(path: &str, base_paths: &[String]) -> String {
    let normalized = path.replace('\\', "/");
    let normalized = normalized.trim().trim_start_matches('/').to_string();
    if !normalized.contains(':') {
        return normalized;
    }

    let candidate = format!("/{}", normalized);
    let mut best_match_len = 0usize;
    let mut best_suffix = normalized.clone();

    for base in base_paths {
        let base_normalized = normalize_policy_path(base);
        if base_normalized.is_empty() {
            continue;
        }

        for prefix in [base_normalized.clone(), format!("/{}", base_normalized)] {
            if let Some(stripped) = candidate.strip_prefix(&prefix) {
                let stripped = stripped.trim_start_matches('/').trim_start_matches('\\');
                if !stripped.is_empty() && prefix.len() > best_match_len {
                    best_match_len = prefix.len();
                    best_suffix = stripped.replace('\\', "/");
                }
            }
        }
    }

    best_suffix
}

#[allow(dead_code)]
pub async fn get_inode_path(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Option<String>, sqlx::Error> {
    let mut names = Vec::new();
    let mut current = get_inode_by_id(pool, inode_id).await?;

    while let Some(inode) = current {
        names.push(inode.name);
        current = match inode.parent_id {
            Some(parent_id) => get_inode_by_id(pool, parent_id).await?,
            None => None,
        };
    }

    if names.is_empty() {
        return Ok(None);
    }

    names.reverse();
    Ok(Some(names.join("/")))
}

#[allow(dead_code)]
pub async fn list_active_files(pool: &SqlitePool) -> Result<Vec<FileInventoryRecord>, sqlx::Error> {
    sqlx::query_as::<_, FileInventoryRecord>(
        r#"
        WITH RECURSIVE inode_paths AS (
            SELECT
                id,
                parent_id,
                name AS path
            FROM inodes
            WHERE parent_id IS NULL

            UNION ALL

            SELECT
                child.id,
                child.parent_id,
                inode_paths.path || '/' || child.name AS path
            FROM inodes child
            INNER JOIN inode_paths
                ON child.parent_id = inode_paths.id
        )
        SELECT
            i.id AS inode_id,
            inode_paths.path AS path,
            COALESCE(fr.size, i.size) AS size,
            fr.revision_id AS current_revision_id,
            fr.created_at AS current_revision_created_at,
            ss.pin_state AS smart_sync_pin_state,
            ss.hydration_state AS smart_sync_hydration_state
        FROM inodes i
        INNER JOIN inode_paths
            ON inode_paths.id = i.id
        LEFT JOIN file_revisions fr
            ON fr.inode_id = i.id
           AND fr.is_current = 1
        LEFT JOIN smart_sync_state ss
            ON ss.inode_id = i.id
        WHERE i.kind = 'FILE'
          AND i.deleted_at IS NULL
        ORDER BY inode_paths.path ASC
        "#,
    )
    .fetch_all(pool)
    .await
}
