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

// ── Recovery Keys (Epic 34.6a) ──

#[derive(Debug, Clone, FromRow)]
pub struct RecoveryKeyRecord {
    pub id: i64,
    pub vault_id: String,
    pub wrapped_vault_key: Vec<u8>,
    pub vk_generation: i64,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub revoked_at: Option<i64>,
}

pub async fn insert_recovery_key(
    pool: &SqlitePool,
    vault_id: &str,
    wrapped_vault_key: &[u8],
    vk_generation: i64,
    created_by: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "INSERT INTO vault_recovery_keys \
         (vault_id, wrapped_vault_key, vk_generation, created_at, created_by) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(vault_id)
    .bind(wrapped_vault_key)
    .bind(vk_generation)
    .bind(now)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn list_active_recovery_keys(
    pool: &SqlitePool,
    vault_id: &str,
) -> Result<Vec<RecoveryKeyRecord>, sqlx::Error> {
    sqlx::query_as::<_, RecoveryKeyRecord>(
        "SELECT id, vault_id, wrapped_vault_key, vk_generation, created_at, created_by, revoked_at \
         FROM vault_recovery_keys \
         WHERE vault_id = ? AND revoked_at IS NULL \
         ORDER BY created_at DESC",
    )
    .bind(vault_id)
    .fetch_all(pool)
    .await
}

pub async fn revoke_all_recovery_keys(
    pool: &SqlitePool,
    vault_id: &str,
) -> Result<u64, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "UPDATE vault_recovery_keys SET revoked_at = ? \
         WHERE vault_id = ? AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(vault_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
