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
pub struct LocalDeviceIdentityRecord {
    pub device_id: String,
    pub device_name: String,
    pub peer_token: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub encrypted_private_key: Option<Vec<u8>>,
    pub public_key: Option<Vec<u8>>,
    pub encrypted_kyber_private_key: Option<Vec<u8>>,
    pub kyber_public_key: Option<Vec<u8>>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct TrustedPeerRecord {
    pub peer_id: String,
    pub device_name: String,
    pub vault_id: String,
    pub peer_api_base: String,
    pub trusted: i64,
    pub last_seen_at: i64,
    pub last_handshake_at: Option<i64>,
    pub last_error: Option<String>,
}

#[allow(dead_code)]
pub async fn get_local_device_identity(
    pool: &SqlitePool,
) -> Result<Option<LocalDeviceIdentityRecord>, sqlx::Error> {
    sqlx::query_as::<_, LocalDeviceIdentityRecord>(
        r#"
        SELECT device_id, device_name, peer_token, created_at, updated_at,
               encrypted_private_key, public_key,
               encrypted_kyber_private_key, kyber_public_key
        FROM local_device_identity
        WHERE id = 1
        "#,
    )
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn upsert_local_device_identity(
    pool: &SqlitePool,
    device_id: &str,
    device_name: &str,
    peer_token: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO local_device_identity (
            id,
            device_id,
            device_name,
            peer_token,
            created_at,
            updated_at
        )
        VALUES (
            1,
            ?,
            ?,
            ?,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        )
        ON CONFLICT(id) DO UPDATE SET
            device_id = excluded.device_id,
            device_name = excluded.device_name,
            peer_token = excluded.peer_token,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(device_id)
    .bind(device_name)
    .bind(peer_token)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn update_local_device_name(
    pool: &SqlitePool,
    device_name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE local_device_identity
        SET device_name = ?,
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        WHERE id = 1
        "#,
    )
    .bind(device_name)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn store_device_keypair(
    pool: &SqlitePool,
    encrypted_private_key: &[u8],
    public_key: &[u8],
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "UPDATE local_device_identity \
         SET encrypted_private_key = ?, public_key = ?, updated_at = ? \
         WHERE id = 1",
    )
    .bind(encrypted_private_key)
    .bind(public_key)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn store_kyber_keypair(
    pool: &SqlitePool,
    encrypted_kyber_private_key: &[u8],
    kyber_public_key: &[u8],
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "UPDATE local_device_identity \
         SET encrypted_kyber_private_key = ?, kyber_public_key = ?, updated_at = ? \
         WHERE id = 1",
    )
    .bind(encrypted_kyber_private_key)
    .bind(kyber_public_key)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_device_kyber_public_key(
    pool: &SqlitePool,
    device_id: &str,
    kyber_public_key: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devices SET kyber_public_key = ? WHERE device_id = ?")
        .bind(kyber_public_key)
        .bind(device_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn upsert_trusted_peer(
    pool: &SqlitePool,
    peer_id: &str,
    device_name: &str,
    vault_id: &str,
    peer_api_base: &str,
    last_error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO trusted_peers (
            peer_id,
            device_name,
            vault_id,
            peer_api_base,
            trusted,
            last_seen_at,
            last_handshake_at,
            last_error
        )
        VALUES (
            ?,
            ?,
            ?,
            ?,
            1,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            ?
        )
        ON CONFLICT(peer_id) DO UPDATE SET
            device_name = excluded.device_name,
            vault_id = excluded.vault_id,
            peer_api_base = excluded.peer_api_base,
            trusted = 1,
            last_seen_at = excluded.last_seen_at,
            last_handshake_at = excluded.last_handshake_at,
            last_error = excluded.last_error
        "#,
    )
    .bind(peer_id)
    .bind(device_name)
    .bind(vault_id)
    .bind(peer_api_base)
    .bind(last_error)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn note_peer_seen(
    pool: &SqlitePool,
    peer_id: &str,
    device_name: &str,
    vault_id: &str,
    peer_api_base: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO trusted_peers (
            peer_id,
            device_name,
            vault_id,
            peer_api_base,
            trusted,
            last_seen_at,
            last_error
        )
        VALUES (
            ?,
            ?,
            ?,
            ?,
            1,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            NULL
        )
        ON CONFLICT(peer_id) DO UPDATE SET
            device_name = excluded.device_name,
            vault_id = excluded.vault_id,
            peer_api_base = excluded.peer_api_base,
            trusted = 1,
            last_seen_at = excluded.last_seen_at
        "#,
    )
    .bind(peer_id)
    .bind(device_name)
    .bind(vault_id)
    .bind(peer_api_base)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn update_peer_error(
    pool: &SqlitePool,
    peer_id: &str,
    last_error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE trusted_peers
        SET last_error = ?,
            last_seen_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        WHERE peer_id = ?
        "#,
    )
    .bind(last_error)
    .bind(peer_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn list_trusted_peers(pool: &SqlitePool) -> Result<Vec<TrustedPeerRecord>, sqlx::Error> {
    sqlx::query_as::<_, TrustedPeerRecord>(
        r#"
        SELECT
            peer_id,
            device_name,
            vault_id,
            peer_api_base,
            trusted,
            last_seen_at,
            last_handshake_at,
            last_error
        FROM trusted_peers
        WHERE trusted = 1
        ORDER BY last_seen_at DESC, device_name ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_trusted_peer_by_id(
    pool: &SqlitePool,
    peer_id: &str,
) -> Result<Option<TrustedPeerRecord>, sqlx::Error> {
    sqlx::query_as::<_, TrustedPeerRecord>(
        r#"
        SELECT
            peer_id,
            device_name,
            vault_id,
            peer_api_base,
            trusted,
            last_seen_at,
            last_handshake_at,
            last_error
        FROM trusted_peers
        WHERE peer_id = ?
        LIMIT 1
        "#,
    )
    .bind(peer_id)
    .fetch_optional(pool)
    .await
}
