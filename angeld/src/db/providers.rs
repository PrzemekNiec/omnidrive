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
pub struct ProviderConfigRecord {
    pub provider_name: String,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub force_path_style: i64,
    pub enabled: i64,
    pub draft_source: Option<String>,
    pub last_test_status: Option<String>,
    pub last_test_error: Option<String>,
    pub last_test_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ProviderSecretRecord {
    pub provider_name: String,
    pub access_key_id_ciphertext: Vec<u8>,
    pub secret_access_key_ciphertext: Vec<u8>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[allow(dead_code)]
pub async fn get_provider_config(
    pool: &SqlitePool,
    provider_name: &str,
) -> Result<Option<ProviderConfigRecord>, sqlx::Error> {
    sqlx::query_as::<_, ProviderConfigRecord>(
        r#"
        SELECT
            provider_name,
            endpoint,
            region,
            bucket,
            force_path_style,
            enabled,
            draft_source,
            last_test_status,
            last_test_error,
            last_test_at,
            created_at,
            updated_at
        FROM provider_configs
        WHERE provider_name = ?
        "#,
    )
    .bind(provider_name)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn list_provider_configs(
    pool: &SqlitePool,
) -> Result<Vec<ProviderConfigRecord>, sqlx::Error> {
    sqlx::query_as::<_, ProviderConfigRecord>(
        r#"
        SELECT
            provider_name,
            endpoint,
            region,
            bucket,
            force_path_style,
            enabled,
            draft_source,
            last_test_status,
            last_test_error,
            last_test_at,
            created_at,
            updated_at
        FROM provider_configs
        ORDER BY provider_name ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn upsert_provider_config(
    pool: &SqlitePool,
    provider_name: &str,
    endpoint: &str,
    region: &str,
    bucket: &str,
    force_path_style: bool,
    enabled: bool,
    draft_source: Option<&str>,
    last_test_status: Option<&str>,
    last_test_error: Option<&str>,
    last_test_at: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO provider_configs (
            provider_name,
            endpoint,
            region,
            bucket,
            force_path_style,
            enabled,
            draft_source,
            last_test_status,
            last_test_error,
            last_test_at,
            created_at,
            updated_at
        )
        VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        )
        ON CONFLICT(provider_name) DO UPDATE SET
            endpoint = excluded.endpoint,
            region = excluded.region,
            bucket = excluded.bucket,
            force_path_style = excluded.force_path_style,
            enabled = excluded.enabled,
            draft_source = excluded.draft_source,
            last_test_status = excluded.last_test_status,
            last_test_error = excluded.last_test_error,
            last_test_at = excluded.last_test_at,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(provider_name)
    .bind(endpoint)
    .bind(region)
    .bind(bucket)
    .bind(i64::from(force_path_style))
    .bind(i64::from(enabled))
    .bind(draft_source)
    .bind(last_test_status)
    .bind(last_test_error)
    .bind(last_test_at)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn delete_provider_config(
    pool: &SqlitePool,
    provider_name: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM provider_configs WHERE provider_name = ?")
        .bind(provider_name)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[allow(dead_code)]
pub async fn get_provider_secret(
    pool: &SqlitePool,
    provider_name: &str,
) -> Result<Option<ProviderSecretRecord>, sqlx::Error> {
    sqlx::query_as::<_, ProviderSecretRecord>(
        r#"
        SELECT
            provider_name,
            access_key_id_ciphertext,
            secret_access_key_ciphertext,
            created_at,
            updated_at
        FROM provider_secrets
        WHERE provider_name = ?
        "#,
    )
    .bind(provider_name)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn upsert_provider_secret(
    pool: &SqlitePool,
    provider_name: &str,
    access_key_id_ciphertext: &[u8],
    secret_access_key_ciphertext: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO provider_secrets (
            provider_name,
            access_key_id_ciphertext,
            secret_access_key_ciphertext,
            created_at,
            updated_at
        )
        VALUES (
            ?, ?, ?,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        )
        ON CONFLICT(provider_name) DO UPDATE SET
            access_key_id_ciphertext = excluded.access_key_id_ciphertext,
            secret_access_key_ciphertext = excluded.secret_access_key_ciphertext,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(provider_name)
    .bind(access_key_id_ciphertext)
    .bind(secret_access_key_ciphertext)
    .execute(pool)
    .await?;

    Ok(())
}
