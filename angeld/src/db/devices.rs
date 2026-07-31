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
pub struct DeviceRecord {
    pub device_id: String,
    pub user_id: String,
    pub device_name: String,
    pub public_key: Vec<u8>,
    pub wrapped_vault_key: Option<Vec<u8>>,
    pub vault_key_generation: Option<i64>,
    pub revoked_at: Option<i64>,
    pub last_seen_at: Option<i64>,
    pub created_at: i64,
    pub enrolled_at: Option<i64>,
    pub kyber_public_key: Option<Vec<u8>>,
    pub wrapped_vault_key_kyber: Option<Vec<u8>>,
}

// ── Devices ──

pub async fn create_device(
    pool: &SqlitePool,
    device_id: &str,
    user_id: &str,
    device_name: &str,
    public_key: &[u8],
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "INSERT INTO devices (device_id, user_id, device_name, public_key, created_at, last_seen_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(device_id)
    .bind(user_id)
    .bind(device_name)
    .bind(public_key)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_device(
    pool: &SqlitePool,
    device_id: &str,
) -> Result<Option<DeviceRecord>, sqlx::Error> {
    sqlx::query_as::<_, DeviceRecord>(
        "SELECT device_id, user_id, device_name, public_key, wrapped_vault_key, \
         vault_key_generation, revoked_at, last_seen_at, created_at, enrolled_at, \
         kyber_public_key, wrapped_vault_key_kyber \
         FROM devices WHERE device_id = ?",
    )
    .bind(device_id)
    .fetch_optional(pool)
    .await
}

pub async fn set_device_safety_verified(
    pool: &SqlitePool,
    device_id: &str,
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query("UPDATE devices SET safety_numbers_verified_at = ? WHERE device_id = ?")
        .bind(now)
        .bind(device_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_device_safety_verified_at(
    pool: &SqlitePool,
    device_id: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let row: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT safety_numbers_verified_at FROM devices WHERE device_id = ?")
            .bind(device_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(ts,)| ts))
}

pub async fn list_devices_for_user(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<Vec<DeviceRecord>, sqlx::Error> {
    sqlx::query_as::<_, DeviceRecord>(
        "SELECT device_id, user_id, device_name, public_key, wrapped_vault_key, \
         vault_key_generation, revoked_at, last_seen_at, created_at, enrolled_at, \
         kyber_public_key, wrapped_vault_key_kyber \
         FROM devices WHERE user_id = ? ORDER BY created_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn set_device_wrapped_vault_key(
    pool: &SqlitePool,
    device_id: &str,
    wrapped_vault_key: &[u8],
    vault_key_generation: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devices SET wrapped_vault_key = ?, vault_key_generation = ? WHERE device_id = ?",
    )
    .bind(wrapped_vault_key)
    .bind(vault_key_generation)
    .bind(device_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn set_device_wrapped_vault_key_kyber(
    pool: &SqlitePool,
    device_id: &str,
    wrapped_vault_key_kyber: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devices SET wrapped_vault_key_kyber = ? WHERE device_id = ?")
        .bind(wrapped_vault_key_kyber)
        .bind(device_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Returns active devices for a user: non-revoked and with a wrapped vault key.
pub async fn get_active_devices_for_user(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<Vec<DeviceRecord>, sqlx::Error> {
    sqlx::query_as::<_, DeviceRecord>(
        "SELECT device_id, user_id, device_name, public_key, wrapped_vault_key, \
         vault_key_generation, revoked_at, last_seen_at, created_at, enrolled_at, \
         kyber_public_key, wrapped_vault_key_kyber \
         FROM devices WHERE user_id = ? AND revoked_at IS NULL AND wrapped_vault_key IS NOT NULL \
         ORDER BY created_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn revoke_device(pool: &SqlitePool, device_id: &str) -> Result<bool, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "UPDATE devices SET revoked_at = ?, wrapped_vault_key = NULL, \
         wrapped_vault_key_kyber = NULL, vault_key_generation = NULL \
         WHERE device_id = ? AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(device_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn set_device_public_key(
    pool: &SqlitePool,
    device_id: &str,
    public_key: &[u8],
) -> Result<bool, sqlx::Error> {
    let now = epoch_secs();
    let result =
        sqlx::query("UPDATE devices SET public_key = ?, enrolled_at = ? WHERE device_id = ?")
            .bind(public_key)
            .bind(now)
            .bind(device_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn touch_device_last_seen(pool: &SqlitePool, device_id: &str) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query("UPDATE devices SET last_seen_at = ? WHERE device_id = ?")
        .bind(now)
        .bind(device_id)
        .execute(pool)
        .await?;
    Ok(())
}
