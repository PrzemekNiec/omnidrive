use sqlx::FromRow;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct SyncPolicyRecord {
    pub policy_id: i64,
    pub path_prefix: String,
    pub require_healthy: i64,
    pub enable_versioning: i64,
    pub policy_type: String,
}

#[allow(dead_code)]
pub async fn upsert_sync_policy(
    pool: &SqlitePool,
    path_prefix: &str,
    require_healthy: bool,
    enable_versioning: bool,
) -> Result<i64, sqlx::Error> {
    let policy_type = if require_healthy {
        "PARANOIA"
    } else {
        "STANDARD"
    };
    sqlx::query(
        r#"
        INSERT INTO sync_policies (path_prefix, require_healthy, enable_versioning, policy_type)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(path_prefix) DO UPDATE SET
            require_healthy = excluded.require_healthy,
            enable_versioning = excluded.enable_versioning,
            policy_type = excluded.policy_type
        "#,
    )
    .bind(path_prefix)
    .bind(if require_healthy { 1 } else { 0 })
    .bind(if enable_versioning { 1 } else { 0 })
    .bind(policy_type)
    .execute(pool)
    .await?;

    let policy_id = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT policy_id
        FROM sync_policies
        WHERE path_prefix = ?
        LIMIT 1
        "#,
    )
    .bind(path_prefix)
    .fetch_one(pool)
    .await?;

    Ok(policy_id)
}

#[allow(dead_code)]
pub async fn list_sync_policies(pool: &SqlitePool) -> Result<Vec<SyncPolicyRecord>, sqlx::Error> {
    sqlx::query_as::<_, SyncPolicyRecord>(
        r#"
        SELECT
            policy_id,
            path_prefix,
            require_healthy,
            enable_versioning,
            COALESCE(policy_type, 'PARANOIA') AS policy_type
        FROM sync_policies
        ORDER BY LENGTH(path_prefix) DESC, policy_id ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn set_sync_policy_type_for_path(
    pool: &SqlitePool,
    path_prefix: &str,
    policy_type: &str,
) -> Result<i64, sqlx::Error> {
    let (require_healthy, enable_versioning) = match policy_type {
        "PARANOIA" => (1_i64, 1_i64),
        "STANDARD" => (0_i64, 1_i64),
        "LOCAL" => (0_i64, 1_i64),
        _ => (1_i64, 1_i64),
    };

    sqlx::query(
        r#"
        INSERT INTO sync_policies (path_prefix, require_healthy, enable_versioning, policy_type)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(path_prefix) DO UPDATE SET
            require_healthy = excluded.require_healthy,
            enable_versioning = excluded.enable_versioning,
            policy_type = excluded.policy_type
        "#,
    )
    .bind(path_prefix)
    .bind(require_healthy)
    .bind(enable_versioning)
    .bind(policy_type)
    .execute(pool)
    .await?;

    let policy_id = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT policy_id
        FROM sync_policies
        WHERE path_prefix = ?
        LIMIT 1
        "#,
    )
    .bind(path_prefix)
    .fetch_one(pool)
    .await?;

    Ok(policy_id)
}

#[allow(dead_code)]
pub async fn find_sync_policy_for_path(
    pool: &SqlitePool,
    path: &str,
) -> Result<Option<SyncPolicyRecord>, sqlx::Error> {
    let normalized_path = normalize_policy_path(path);
    let policies = list_sync_policies(pool).await?;

    Ok(policies
        .into_iter()
        .filter(|policy| path_matches_policy(&normalized_path, &policy.path_prefix))
        .max_by_key(|policy| policy.path_prefix.len()))
}

pub(super) fn normalize_policy_path(path: &str) -> String {
    let replaced = path.replace('\\', "/");
    let mut normalized = replaced.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        normalized.push('/');
    }
    normalized
}

fn path_matches_policy(path: &str, prefix: &str) -> bool {
    let path = normalize_policy_path(path);
    let prefix = normalize_policy_path(prefix);

    if prefix == "/" {
        return true;
    }

    if path == prefix {
        return true;
    }

    path.strip_prefix(&prefix)
        .is_some_and(|suffix| suffix.starts_with('/'))
}
