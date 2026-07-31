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
pub struct AuditLogRecord {
    pub id: i64,
    pub timestamp: i64,
    pub actor_user_id: Option<String>,
    pub actor_device_id: Option<String>,
    pub action: String,
    pub target_user_id: Option<String>,
    pub target_device_id: Option<String>,
    pub details: Option<String>,
    pub vault_id: String,
}

// ── Audit Logs ──

pub async fn insert_audit_log(
    pool: &SqlitePool,
    vault_id: &str,
    action: &str,
    actor_user_id: Option<&str>,
    actor_device_id: Option<&str>,
    target_user_id: Option<&str>,
    target_device_id: Option<&str>,
    details: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "INSERT INTO audit_logs (timestamp, actor_user_id, actor_device_id, action, \
         target_user_id, target_device_id, details, vault_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(now)
    .bind(actor_user_id)
    .bind(actor_device_id)
    .bind(action)
    .bind(target_user_id)
    .bind(target_device_id)
    .bind(details)
    .bind(vault_id)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn list_audit_logs(
    pool: &SqlitePool,
    vault_id: &str,
    limit: i64,
) -> Result<Vec<AuditLogRecord>, sqlx::Error> {
    sqlx::query_as::<_, AuditLogRecord>(
        "SELECT id, timestamp, actor_user_id, actor_device_id, action, \
         target_user_id, target_device_id, details, vault_id \
         FROM audit_logs WHERE vault_id = ? ORDER BY timestamp DESC LIMIT ?",
    )
    .bind(vault_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}
