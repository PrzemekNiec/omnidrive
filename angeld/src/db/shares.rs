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

// ── Shared Links (Epic 33) ───────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct SharedLinkRecord {
    pub share_id: String,
    pub inode_id: i64,
    pub revision_id: i64,
    pub file_name: String,
    pub file_size: i64,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub max_downloads: Option<i64>,
    pub download_count: i64,
    pub revoked: i64,
    pub password_hash: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct SharePasswordToken {
    pub token: String,
    pub share_id: String,
    pub created_at: i64,
    pub expires_at: i64,
}

pub fn is_shared_link_valid(link: &SharedLinkRecord) -> bool {
    if link.revoked != 0 {
        return false;
    }
    if let Some(expires_at) = link.expires_at {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        if now > expires_at {
            return false;
        }
    }
    if let Some(max) = link.max_downloads
        && link.download_count >= max
    {
        return false;
    }
    true
}

pub async fn create_shared_link(
    pool: &SqlitePool,
    share_id: &str,
    inode_id: i64,
    revision_id: i64,
    file_name: &str,
    file_size: i64,
    expires_at: Option<i64>,
    max_downloads: Option<i64>,
    password_hash: Option<&str>,
) -> Result<(), sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    sqlx::query(
        "INSERT INTO shared_links (share_id, inode_id, revision_id, file_name, file_size, \
         created_at, expires_at, max_downloads, password_hash) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(share_id)
    .bind(inode_id)
    .bind(revision_id)
    .bind(file_name)
    .bind(file_size)
    .bind(now)
    .bind(expires_at)
    .bind(max_downloads)
    .bind(password_hash)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_shared_link(
    pool: &SqlitePool,
    share_id: &str,
) -> Result<Option<SharedLinkRecord>, sqlx::Error> {
    sqlx::query_as::<_, SharedLinkRecord>(
        "SELECT share_id, inode_id, revision_id, file_name, file_size, created_at, \
         expires_at, max_downloads, download_count, revoked, password_hash FROM shared_links WHERE share_id = ?",
    )
    .bind(share_id)
    .fetch_optional(pool)
    .await
}

pub async fn list_shared_links(pool: &SqlitePool) -> Result<Vec<SharedLinkRecord>, sqlx::Error> {
    sqlx::query_as::<_, SharedLinkRecord>(
        "SELECT share_id, inode_id, revision_id, file_name, file_size, created_at, \
         expires_at, max_downloads, download_count, revoked, password_hash FROM shared_links \
         ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn list_shared_links_for_inode(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Vec<SharedLinkRecord>, sqlx::Error> {
    sqlx::query_as::<_, SharedLinkRecord>(
        "SELECT share_id, inode_id, revision_id, file_name, file_size, created_at, \
         expires_at, max_downloads, download_count, revoked, password_hash FROM shared_links \
         WHERE inode_id = ? ORDER BY created_at DESC",
    )
    .bind(inode_id)
    .fetch_all(pool)
    .await
}

pub async fn revoke_shared_link(pool: &SqlitePool, share_id: &str) -> Result<bool, sqlx::Error> {
    let result =
        sqlx::query("UPDATE shared_links SET revoked = 1 WHERE share_id = ? AND revoked = 0")
            .bind(share_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn increment_shared_link_download_count(
    pool: &SqlitePool,
    share_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE shared_links SET download_count = download_count + 1 WHERE share_id = ?")
        .bind(share_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_shared_link(pool: &SqlitePool, share_id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM shared_links WHERE share_id = ?")
        .bind(share_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ── Share Password Tokens ────────────────────────────────────────────

pub async fn create_share_password_token(
    pool: &SqlitePool,
    token: &str,
    share_id: &str,
    ttl_seconds: i64,
) -> Result<(), sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let expires_at = now + (ttl_seconds * 1000);
    sqlx::query(
        "INSERT INTO share_password_tokens (token, share_id, created_at, expires_at) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(token)
    .bind(share_id)
    .bind(now)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn validate_share_password_token(
    pool: &SqlitePool,
    token: &str,
    share_id: &str,
) -> Result<bool, sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let row = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM share_password_tokens \
         WHERE token = ? AND share_id = ? AND expires_at > ?",
    )
    .bind(token)
    .bind(share_id)
    .bind(now)
    .fetch_one(pool)
    .await?;
    Ok(row > 0)
}

pub async fn cleanup_expired_share_tokens(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let result = sqlx::query("DELETE FROM share_password_tokens WHERE expires_at <= ?")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
