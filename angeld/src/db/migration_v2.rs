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

// ── V1→V2 Migration queries ──────────────────────────────────────────────

/// A V1 pack together with the inode that references it (for migration).
#[allow(dead_code)]
#[derive(Clone, Debug, FromRow)]
pub struct V1PackForMigration {
    pub pack_id: String,
    pub chunk_id: Vec<u8>,
    pub plaintext_hash: Option<String>,
    pub storage_mode: String,
    pub logical_size: i64,
    pub cipher_size: i64,
    pub shard_size: i64,
    pub nonce: Vec<u8>,
    pub gcm_tag: Vec<u8>,
    pub ec_scheme: String,
    pub inode_id: i64,
}

/// Fetch a batch of V1 packs that need migration, each joined with an owning inode.
#[allow(dead_code)]
pub async fn get_v1_packs_for_migration(
    pool: &SqlitePool,
    batch_size: i64,
) -> Result<Vec<V1PackForMigration>, sqlx::Error> {
    sqlx::query_as::<_, V1PackForMigration>(
        r#"
        SELECT
            p.pack_id,
            p.chunk_id,
            p.plaintext_hash,
            p.storage_mode,
            p.logical_size,
            p.cipher_size,
            p.shard_size,
            p.nonce,
            p.gcm_tag,
            p.ec_scheme,
            fr.inode_id
        FROM packs p
        INNER JOIN pack_locations pl ON pl.pack_id = p.pack_id
        INNER JOIN chunk_refs cr     ON cr.chunk_id = pl.chunk_id
        INNER JOIN file_revisions fr ON fr.revision_id = cr.revision_id
        WHERE p.encryption_version = 1
          AND p.status IN ('COMPLETED_HEALTHY', 'COMPLETED_DEGRADED')
        GROUP BY p.pack_id
        ORDER BY p.pack_id ASC
        LIMIT ?
        "#,
    )
    .bind(batch_size)
    .fetch_all(pool)
    .await
}

/// Count total number of packs in the vault. Used by adaptive scrubber pacing.
#[allow(dead_code)]
pub async fn count_all_packs(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM packs")
        .fetch_one(pool)
        .await
}

/// Count how many active V1 packs remain in the vault (healthy or degraded).
#[allow(dead_code)]
pub async fn count_v1_packs(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM packs \
         WHERE encryption_version = 1 \
         AND status IN ('COMPLETED_HEALTHY', 'COMPLETED_DEGRADED')",
    )
    .fetch_one(pool)
    .await
}

/// Update a pack's encryption_version to 2 after successful re-encryption.
/// Also stores the new nonce and gcm_tag produced by the V2 encryption.
#[allow(dead_code)]
pub async fn mark_pack_migrated_v2(
    pool: &SqlitePool,
    pack_id: &str,
    new_nonce: &[u8],
    new_gcm_tag: &[u8],
    new_cipher_size: i64,
    new_shard_size: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE packs
        SET encryption_version = 2,
            nonce = ?,
            gcm_tag = ?,
            cipher_size = ?,
            shard_size = ?
        WHERE pack_id = ?
        "#,
    )
    .bind(new_nonce)
    .bind(new_gcm_tag)
    .bind(new_cipher_size)
    .bind(new_shard_size)
    .bind(pack_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Set vault_format_version = 2 when all V1 packs have been migrated.
#[allow(dead_code)]
pub async fn finalize_vault_format_v2(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE vault_state SET vault_format_version = 2 WHERE id = 1")
        .execute(pool)
        .await?;
    Ok(())
}
