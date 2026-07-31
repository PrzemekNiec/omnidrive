use sqlx::FromRow;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct MetadataBackupRecord {
    pub backup_id: String,
    pub created_at: i64,
    pub snapshot_version: i64,
    pub object_key: String,
    pub provider: String,
    pub encrypted_size: i64,
    pub status: String,
    pub last_error: Option<String>,
}

#[allow(dead_code)]
pub async fn record_metadata_backup_attempt(
    pool: &SqlitePool,
    backup_id: &str,
    created_at: i64,
    snapshot_version: i64,
    object_key: &str,
    provider: &str,
    encrypted_size: i64,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO metadata_backups (
            backup_id,
            created_at,
            snapshot_version,
            object_key,
            provider,
            encrypted_size,
            status,
            last_error
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
        ON CONFLICT(backup_id) DO UPDATE SET
            created_at = excluded.created_at,
            snapshot_version = excluded.snapshot_version,
            object_key = excluded.object_key,
            provider = excluded.provider,
            encrypted_size = excluded.encrypted_size,
            status = excluded.status,
            last_error = NULL
        "#,
    )
    .bind(backup_id)
    .bind(created_at)
    .bind(snapshot_version)
    .bind(object_key)
    .bind(provider)
    .bind(encrypted_size)
    .bind(status)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn update_metadata_backup_status(
    pool: &SqlitePool,
    backup_id: &str,
    status: &str,
    last_error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE metadata_backups
        SET status = ?, last_error = ?
        WHERE backup_id = ?
        "#,
    )
    .bind(status)
    .bind(last_error)
    .bind(backup_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_last_successful_metadata_backup_at(
    pool: &SqlitePool,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, Option<i64>>(
        r#"
        SELECT MAX(created_at)
        FROM metadata_backups
        WHERE status = 'COMPLETED'
        "#,
    )
    .fetch_one(pool)
    .await
}

#[allow(dead_code)]
pub async fn list_recent_metadata_backups(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<MetadataBackupRecord>, sqlx::Error> {
    sqlx::query_as::<_, MetadataBackupRecord>(
        r#"
        SELECT
            backup_id,
            created_at,
            snapshot_version,
            object_key,
            provider,
            encrypted_size,
            status,
            last_error
        FROM metadata_backups
        ORDER BY created_at DESC, backup_id DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}
