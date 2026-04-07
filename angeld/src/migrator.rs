#![allow(dead_code)]

use crate::db;
use crate::db::{PackStatus, StorageMode};
use crate::packer::{
    build_manifest_bytes_v2, build_shards, compute_pack_id, local_pack_path,
    storage_mode_scheme,
};
use crate::secure_fs::write_ephemeral_bytes;
use crate::vault::{VaultError, VaultKeyStore};
use omnidrive_core::crypto::{decrypt_chunk, encrypt_chunk_v2, KeyBytes};
use omnidrive_core::layout::ChunkRecordPrefix;
use secrecy::ExposeSecret;
use sqlx::SqlitePool;
use std::path::PathBuf;
use tokio::fs;
use tracing::{error, info, warn};

const MIGRATION_BATCH_SIZE: i64 = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationStats {
    pub migrated: u64,
    pub failed: u64,
    pub remaining: u64,
    pub finalized: bool,
}

#[derive(Clone)]
pub struct MigrationManager {
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
    spool_dir: PathBuf,
}

impl MigrationManager {
    pub fn new(pool: SqlitePool, vault_keys: VaultKeyStore, spool_dir: impl Into<PathBuf>) -> Self {
        Self {
            pool,
            vault_keys,
            spool_dir: spool_dir.into(),
        }
    }

    /// Run a single migration pass: fetch a batch of V1 packs, re-encrypt each to V2.
    /// Returns stats for this pass. Call in a loop until `remaining == 0`.
    pub async fn run_batch(&self) -> Result<MigrationStats, MigrationError> {
        let vault_key = self.vault_keys.require_key().await?;
        let v1_packs = db::get_v1_packs_for_migration(&self.pool, MIGRATION_BATCH_SIZE).await?;

        if v1_packs.is_empty() {
            let remaining = db::count_v1_packs(&self.pool).await?;
            if remaining == 0 {
                return self.try_finalize().await;
            }
            return Ok(MigrationStats {
                migrated: 0,
                failed: 0,
                remaining: remaining as u64,
                finalized: false,
            });
        }

        let total = v1_packs.len();
        let mut migrated = 0u64;
        let mut failed = 0u64;

        for (idx, v1_pack) in v1_packs.iter().enumerate() {
            info!(
                "[MIGRATOR] Migrating pack {}/{} (pack_id={}, inode_id={})",
                idx + 1,
                total,
                v1_pack.pack_id,
                v1_pack.inode_id
            );

            match self.migrate_single_pack(v1_pack, &vault_key).await {
                Ok(()) => {
                    migrated += 1;
                }
                Err(err) => {
                    error!(
                        "[MIGRATOR] Failed to migrate pack_id={}: {err}",
                        v1_pack.pack_id
                    );
                    failed += 1;
                }
            }
        }

        let remaining = db::count_v1_packs(&self.pool).await? as u64;
        info!("[MIGRATOR] Batch complete: migrated={migrated}, failed={failed}, remaining={remaining}");

        if remaining == 0 {
            db::finalize_vault_format_v2(&self.pool).await?;
            info!("[MIGRATOR] All V1 packs migrated — vault_format_version set to 2");
            return Ok(MigrationStats {
                migrated,
                failed,
                remaining: 0,
                finalized: true,
            });
        }

        Ok(MigrationStats {
            migrated,
            failed,
            remaining,
            finalized: false,
        })
    }

    /// Migrate a single V1 pack to V2 in-place:
    /// 1. Read local pack file
    /// 2. Decrypt V1 → plaintext
    /// 3. Re-encrypt V2 with per-file DEK
    /// 4. Write new manifest + shards
    /// 5. Insert new pack record, update pack_locations, queue upload
    /// 6. Delete old pack record
    async fn migrate_single_pack(
        &self,
        v1_pack: &db::V1PackForMigration,
        vault_key: &KeyBytes,
    ) -> Result<(), MigrationError> {
        let storage_mode = StorageMode::from_str(&v1_pack.storage_mode);

        // ── Step 1: Read the V1 pack manifest from local spool ──
        let old_manifest_path = local_pack_path(&self.spool_dir, &v1_pack.pack_id);
        if !fs::try_exists(&old_manifest_path).await.unwrap_or(false) {
            return Err(MigrationError::PackFileNotFound(v1_pack.pack_id.clone()));
        }
        let pack_bytes = fs::read(&old_manifest_path).await?;

        // ── Step 2: Parse V1 prefix and decrypt ──
        if pack_bytes.len() < ChunkRecordPrefix::SIZE + ChunkRecordPrefix::GCM_TAG_SIZE {
            return Err(MigrationError::InvalidPack("pack too small"));
        }

        let chunk_id: [u8; 32] = pack_bytes[8..40]
            .try_into()
            .map_err(|_| MigrationError::InvalidPack("chunk_id"))?;

        let cipher_len = u64::from_be_bytes(
            pack_bytes[48..56]
                .try_into()
                .map_err(|_| MigrationError::InvalidPack("cipher_len"))?,
        ) as usize;

        let ciphertext_start = ChunkRecordPrefix::SIZE;
        let ciphertext_end = ciphertext_start + cipher_len;
        let tag_end = ciphertext_end + ChunkRecordPrefix::GCM_TAG_SIZE;

        if tag_end > pack_bytes.len() {
            return Err(MigrationError::InvalidPack("record extends beyond file"));
        }

        let ciphertext = &pack_bytes[ciphertext_start..ciphertext_end];
        let gcm_tag: [u8; 16] = pack_bytes[ciphertext_end..tag_end]
            .try_into()
            .map_err(|_| MigrationError::InvalidPack("gcm_tag"))?;

        let plaintext = decrypt_chunk(vault_key, &chunk_id, &[], ciphertext, &gcm_tag)?;

        // ── Step 3: Get/create DEK for the owning inode ──
        let (dek_id, dek_secret) = self
            .vault_keys
            .get_or_create_dek(&self.pool, v1_pack.inode_id)
            .await?;
        let dek: KeyBytes = *dek_secret.expose_secret();

        // ── Step 4: Re-encrypt with V2 ──
        let v2_encrypted = encrypt_chunk_v2(&dek, &plaintext, &[])?;

        let manifest_bytes = build_manifest_bytes_v2(
            v2_encrypted.chunk_id,
            v2_encrypted.nonce,
            &v2_encrypted.ciphertext,
            &v2_encrypted.gcm_tag,
            plaintext.len(),
            dek_id,
        )
        .map_err(|e| MigrationError::Packer(e.to_string()))?;

        let new_pack_id = compute_pack_id(storage_mode, &manifest_bytes);
        let manifest_path = local_pack_path(&self.spool_dir, &new_pack_id);

        // ── Step 5: Write new manifest + shards ──
        write_ephemeral_bytes(&manifest_path, &manifest_bytes)
            .await
            .map_err(|err| MigrationError::Io(std::io::Error::other(err.to_string())))?;

        let shards = build_shards(&self.spool_dir, &new_pack_id, &v2_encrypted.ciphertext, storage_mode)
            .await
            .map_err(|err| MigrationError::Packer(err.to_string()))?;

        let cipher_size =
            i64::try_from(v2_encrypted.ciphertext.len()).unwrap_or(v1_pack.cipher_size);
        let shard_size = if storage_mode == StorageMode::LocalOnly {
            0
        } else {
            shards.first().map(|s| s.size).unwrap_or(0)
        };
        let manifest_size =
            i64::try_from(manifest_bytes.len()).unwrap_or(0);

        // ── Step 6: DB transaction — insert new pack, update pointers, remove old ──
        db::create_pack(
            &self.pool,
            &new_pack_id,
            &v2_encrypted.chunk_id,
            &v1_pack.plaintext_hash.as_deref().unwrap_or(""),
            storage_mode,
            2,
            storage_mode_scheme(storage_mode),
            v1_pack.logical_size,
            cipher_size,
            shard_size,
            &v2_encrypted.nonce,
            &v2_encrypted.gcm_tag,
            if storage_mode == StorageMode::LocalOnly {
                PackStatus::Healthy
            } else {
                PackStatus::Uploading
            },
        )
        .await?;

        // Update pack_locations to point chunk_id → new pack
        db::link_chunk_to_pack(&self.pool, &v1_pack.chunk_id, &new_pack_id, 0, manifest_size)
            .await?;

        // Register new shards for upload
        for shard in &shards {
            db::register_pack_shard(
                &self.pool,
                &new_pack_id,
                shard.shard_index,
                shard.shard_role,
                shard.provider,
                &shard.object_key,
                shard.size,
                &shard.checksum,
                "PENDING",
            )
            .await?;
        }

        if storage_mode != StorageMode::LocalOnly {
            db::queue_pack_for_upload(&self.pool, &new_pack_id).await?;
        }

        // Mark old pack as unreadable (GC will clean it up)
        db::update_pack_status(&self.pool, &v1_pack.pack_id, PackStatus::Unreadable).await?;

        // Clean up old manifest file
        let _ = fs::remove_file(&old_manifest_path).await;

        info!(
            "[MIGRATOR] Pack migrated: {} → {} (inode={})",
            v1_pack.pack_id, new_pack_id, v1_pack.inode_id
        );

        Ok(())
    }

    /// When no V1 packs remain, finalize the vault by setting vault_format_version = 2.
    async fn try_finalize(&self) -> Result<MigrationStats, MigrationError> {
        let remaining = db::count_v1_packs(&self.pool).await? as u64;
        if remaining > 0 {
            return Ok(MigrationStats {
                migrated: 0,
                failed: 0,
                remaining,
                finalized: false,
            });
        }

        db::finalize_vault_format_v2(&self.pool).await?;
        info!("[MIGRATOR] All V1 packs migrated — vault_format_version set to 2");

        Ok(MigrationStats {
            migrated: 0,
            failed: 0,
            remaining: 0,
            finalized: true,
        })
    }

    /// Run migration to completion: loop batches until no V1 packs remain.
    /// Suitable for calling from an async background task.
    pub async fn run_to_completion(&self) -> Result<MigrationStats, MigrationError> {
        let mut total_migrated = 0u64;
        let mut total_failed = 0u64;

        loop {
            let stats = self.run_batch().await?;
            total_migrated += stats.migrated;
            total_failed += stats.failed;

            if stats.remaining == 0 {
                return Ok(MigrationStats {
                    migrated: total_migrated,
                    failed: total_failed,
                    remaining: 0,
                    finalized: stats.finalized,
                });
            }

            // If an entire batch failed with zero progress, stop to avoid an infinite loop
            if stats.migrated == 0 && stats.failed > 0 {
                warn!(
                    "[MIGRATOR] Stopping: batch made no progress ({} failures, {} remaining)",
                    stats.failed, stats.remaining
                );
                return Ok(MigrationStats {
                    migrated: total_migrated,
                    failed: total_failed,
                    remaining: stats.remaining,
                    finalized: false,
                });
            }

            // Small yield between batches to avoid starving other tasks
            tokio::task::yield_now().await;
        }
    }
}

#[derive(Debug)]
pub enum MigrationError {
    Vault(VaultError),
    Db(sqlx::Error),
    Crypto(omnidrive_core::crypto::CryptoError),
    Io(std::io::Error),
    PackFileNotFound(String),
    InvalidPack(&'static str),
    Packer(String),
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vault(err) => write!(f, "vault: {err}"),
            Self::Db(err) => write!(f, "db: {err}"),
            Self::Crypto(err) => write!(f, "crypto: {err}"),
            Self::Io(err) => write!(f, "io: {err}"),
            Self::PackFileNotFound(pack_id) => {
                write!(f, "local pack file not found for pack_id={pack_id}")
            }
            Self::InvalidPack(reason) => write!(f, "invalid pack: {reason}"),
            Self::Packer(msg) => write!(f, "packer: {msg}"),
        }
    }
}

impl std::error::Error for MigrationError {}

impl From<VaultError> for MigrationError {
    fn from(value: VaultError) -> Self {
        Self::Vault(value)
    }
}

impl From<sqlx::Error> for MigrationError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

impl From<omnidrive_core::crypto::CryptoError> for MigrationError {
    fn from(value: omnidrive_core::crypto::CryptoError) -> Self {
        Self::Crypto(value)
    }
}

impl From<std::io::Error> for MigrationError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::packer::{build_manifest_bytes, compute_pack_id, hex_sha256, local_pack_path};
    use crate::vault::VaultKeyStore;
    use omnidrive_core::crypto::encrypt_chunk;
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Full integration test: inject a V1 pack, run migrator, verify V2 readback.
    #[tokio::test]
    async fn migrate_v1_pack_to_v2() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let test_root = env::temp_dir().join(format!(
            "omnidrive-migrator-test-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let spool_dir = test_root.join("spool");
        fs::create_dir_all(&spool_dir).await.unwrap();

        // ── 1. Bootstrap vault and unlock ──
        let vault_keys = VaultKeyStore::new();
        vault_keys.unlock(&pool, "test-passphrase").await.unwrap();

        let vault_key = vault_keys.require_key().await.unwrap();

        // ── 2. Create an inode + file revision ──
        let plaintext = b"Hello, this is V1 encrypted data for migration test!";
        let inode_id = db::create_inode(&pool, None, "test.txt", "FILE", plaintext.len() as i64)
            .await
            .unwrap();
        let revision_id = db::create_file_revision(
            &pool, inode_id, plaintext.len() as i64, None, None, None, "local_write", None,
        )
        .await
        .unwrap();

        // ── 3. Encrypt a chunk with V1 and build manifest ──
        let v1_encrypted = encrypt_chunk(&vault_key, plaintext, &[]).unwrap();
        let manifest_bytes = build_manifest_bytes(
            v1_encrypted.chunk_id,
            v1_encrypted.nonce,
            &v1_encrypted.ciphertext,
            &v1_encrypted.gcm_tag,
            plaintext.len(),
        )
        .unwrap();

        let storage_mode = StorageMode::LocalOnly;
        let pack_id = compute_pack_id(storage_mode, &manifest_bytes);
        let manifest_path = local_pack_path(&spool_dir, &pack_id);
        fs::write(&manifest_path, &manifest_bytes).await.unwrap();

        // ── 4. Register V1 pack in DB ──
        db::create_pack(
            &pool,
            &pack_id,
            &v1_encrypted.chunk_id,
            &hex_sha256(plaintext),
            storage_mode,
            1, // V1
            "local_only",
            plaintext.len() as i64,
            v1_encrypted.ciphertext.len() as i64,
            0,
            &v1_encrypted.nonce,
            &v1_encrypted.gcm_tag,
            PackStatus::Healthy,
        )
        .await
        .unwrap();

        let manifest_size = manifest_bytes.len() as i64;
        db::register_chunk(&pool, revision_id, &v1_encrypted.chunk_id, 0, plaintext.len() as i64)
            .await
            .unwrap();
        db::link_chunk_to_pack(&pool, &v1_encrypted.chunk_id, &pack_id, 0, manifest_size)
            .await
            .unwrap();

        // ── 5. Verify V1 state ──
        let v1_count = db::count_v1_packs(&pool).await.unwrap();
        assert_eq!(v1_count, 1, "should have 1 V1 pack before migration");

        // ── 6. Run migrator ──
        let migrator = MigrationManager::new(pool.clone(), vault_keys.clone(), &spool_dir);
        let stats = migrator.run_to_completion().await.unwrap();

        assert_eq!(stats.migrated, 1, "should migrate 1 pack");
        assert_eq!(stats.failed, 0, "should have 0 failures");
        assert_eq!(stats.remaining, 0, "should have 0 remaining");
        assert!(stats.finalized, "should finalize vault format");

        // ── 7. Verify V2 state ──
        let v1_count = db::count_v1_packs(&pool).await.unwrap();
        assert_eq!(v1_count, 0, "should have 0 V1 packs after migration");

        // Verify the vault_format_version is 2
        let vault_params = db::get_vault_params(&pool).await.unwrap().unwrap();
        assert_eq!(
            vault_params.vault_format_version,
            Some(2),
            "vault_format_version should be 2"
        );

        // ── 8. Verify V2 pack can be decrypted ──
        // Find the new pack (the one with encryption_version=2)
        let chunks = db::get_file_chunks(&pool, inode_id).await.unwrap();
        assert_eq!(chunks.len(), 1, "should have 1 chunk");

        let locations = db::get_file_chunk_locations(&pool, inode_id).await.unwrap();
        assert_eq!(locations.len(), 1, "should have 1 chunk location");
        let loc = &locations[0];

        // Read the new pack from spool
        let new_pack_path = local_pack_path(&spool_dir, &loc.pack_id);
        assert!(
            fs::try_exists(&new_pack_path).await.unwrap(),
            "new V2 pack file should exist"
        );

        let v2_pack_bytes = fs::read(&new_pack_path).await.unwrap();

        // Verify record_version byte is 2
        assert_eq!(v2_pack_bytes[4], 2, "record_version should be 2");

        // Decrypt with V2 DEK and verify plaintext
        let (_, dek_secret) = vault_keys.get_or_create_dek(&pool, inode_id).await.unwrap();
        let dek: KeyBytes = *dek_secret.expose_secret();

        let cipher_len = u64::from_be_bytes(v2_pack_bytes[48..56].try_into().unwrap()) as usize;
        let ct_start = ChunkRecordPrefix::SIZE;
        let ct_end = ct_start + cipher_len;
        let tag_end = ct_end + ChunkRecordPrefix::GCM_TAG_SIZE;

        let nonce: [u8; 12] = v2_pack_bytes[56..68].try_into().unwrap();
        let ciphertext = &v2_pack_bytes[ct_start..ct_end];
        let gcm_tag: [u8; 16] = v2_pack_bytes[ct_end..tag_end].try_into().unwrap();

        use omnidrive_core::crypto::decrypt_chunk_v2;
        let decrypted = decrypt_chunk_v2(&dek, &nonce, &[], ciphertext, &gcm_tag).unwrap();
        assert_eq!(decrypted, plaintext, "decrypted plaintext should match original");

        // Old pack file should be deleted
        let old_exists = fs::try_exists(&manifest_path).await.unwrap_or(true);
        assert!(!old_exists, "old V1 pack file should be deleted");
    }
}
