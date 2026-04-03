#![allow(dead_code)]

use crate::db;
use crate::db::{PackStatus, ShardRole, StorageMode};
use crate::secure_fs::write_ephemeral_bytes;
use crate::vault::{VaultError, VaultKeyStore};
use omnidrive_core::crypto::{CryptoError, encrypt_chunk};
use omnidrive_core::layout::{CHUNK_RECORD_MAGIC, COMPRESSION_ALGO_NONE, ChunkRecordPrefix};
use reed_solomon_erasure::galois_8::ReedSolomon;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs::{self, File};
use tokio::io::AsyncReadExt;
use zerocopy::AsBytes;
use zerocopy::byteorder::big_endian::U64;

pub const DEFAULT_CHUNK_SIZE: usize = 4 * 1024 * 1024;
pub const LOCAL_PACK_EXTENSION: &str = "odpk";
pub const LOCAL_SHARD_EXTENSION: &str = "shard";
pub const DATA_SHARDS: usize = 2;
pub const PARITY_SHARDS: usize = 1;
pub const TOTAL_SHARDS: usize = DATA_SHARDS + PARITY_SHARDS;
pub const EC_SCHEME_RS_2_1: &str = "rs_2_1";
pub const EC_SCHEME_SINGLE_REPLICA: &str = "single_replica";
pub const EC_SCHEME_LOCAL_ONLY: &str = "local_only";
pub const SINGLE_REPLICA_PROVIDER: &str = "backblaze-b2";

const SHARD_PROVIDERS: [&str; TOTAL_SHARDS] = ["cloudflare-r2", "backblaze-b2", "scaleway"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackerConfig {
    pub chunk_size: usize,
    pub spool_dir: PathBuf,
}

impl PackerConfig {
    pub fn new(spool_dir: impl Into<PathBuf>) -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            spool_dir: spool_dir.into(),
        }
    }

    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }
}

#[derive(Clone)]
pub struct Packer {
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
    config: PackerConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackResult {
    pub source_path: PathBuf,
    pub revision_id: Option<i64>,
    pub pack_id: Option<String>,
    pub pack_ids: Vec<String>,
    pub pack_path: Option<PathBuf>,
    pub pack_paths: Vec<PathBuf>,
    pub chunk_count: usize,
    pub logical_size: u64,
    pub encrypted_size: u64,
    pub created_at_ms: u64,
    pub conflict_copy_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreparedPack {
    pack_id: String,
    chunk_id: [u8; 32],
    plaintext_hash: String,
    storage_mode: StorageMode,
    file_offset: i64,
    plain_size: i64,
    cipher_size: i64,
    shard_size: i64,
    manifest_path: PathBuf,
    manifest_size: i64,
    nonce: [u8; 12],
    gcm_tag: [u8; 16],
    shards: Vec<PreparedShard>,
    is_deduplicated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedShard {
    pub(crate) shard_index: i64,
    pub(crate) shard_role: ShardRole,
    pub(crate) provider: &'static str,
    pub(crate) object_key: String,
    pub(crate) local_path: PathBuf,
    pub(crate) size: i64,
    pub(crate) checksum: String,
}

#[derive(Debug)]
pub enum PackerError {
    InvalidChunkSize(usize),
    Io(std::io::Error),
    Db(sqlx::Error),
    Crypto(CryptoError),
    Vault(VaultError),
    ErasureCoding(reed_solomon_erasure::Error),
    NumericOverflow(&'static str),
    Clock(std::time::SystemTimeError),
    InvalidStoredPack(&'static str),
}

impl fmt::Display for PackerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChunkSize(size) => {
                write!(f, "invalid chunk size {size}, expected a non-zero value")
            }
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::Db(err) => write!(f, "sqlite error: {err}"),
            Self::Crypto(err) => write!(f, "crypto error: {err}"),
            Self::Vault(err) => write!(f, "vault error: {err}"),
            Self::ErasureCoding(err) => write!(f, "erasure coding error: {err}"),
            Self::NumericOverflow(ctx) => write!(f, "numeric overflow while handling {ctx}"),
            Self::Clock(err) => write!(f, "system clock error: {err}"),
            Self::InvalidStoredPack(ctx) => write!(f, "invalid stored pack metadata: {ctx}"),
        }
    }
}

impl std::error::Error for PackerError {}

impl From<std::io::Error> for PackerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<sqlx::Error> for PackerError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

impl From<CryptoError> for PackerError {
    fn from(value: CryptoError) -> Self {
        Self::Crypto(value)
    }
}

impl From<VaultError> for PackerError {
    fn from(value: VaultError) -> Self {
        Self::Vault(value)
    }
}

impl From<reed_solomon_erasure::Error> for PackerError {
    fn from(value: reed_solomon_erasure::Error) -> Self {
        Self::ErasureCoding(value)
    }
}

impl From<std::time::SystemTimeError> for PackerError {
    fn from(value: std::time::SystemTimeError) -> Self {
        Self::Clock(value)
    }
}

impl Packer {
    pub fn new(
        pool: SqlitePool,
        vault_keys: VaultKeyStore,
        config: PackerConfig,
    ) -> Result<Self, PackerError> {
        if config.chunk_size == 0 {
            return Err(PackerError::InvalidChunkSize(config.chunk_size));
        }

        Ok(Self {
            pool,
            vault_keys,
            config,
        })
    }

    pub async fn pack_file(
        &self,
        inode_id: i64,
        source_path: impl AsRef<Path>,
    ) -> Result<PackResult, PackerError> {
        self.pack_file_with_expected_parent(inode_id, source_path, None)
            .await
    }

    pub async fn pack_file_with_expected_parent(
        &self,
        inode_id: i64,
        source_path: impl AsRef<Path>,
        expected_parent_revision_id: Option<i64>,
    ) -> Result<PackResult, PackerError> {
        let source_path = source_path.as_ref().to_path_buf();
        fs::create_dir_all(&self.config.spool_dir).await?;

        let created_at_ms = unix_timestamp_ms()?;
        let vault_key = self.vault_keys.require_key().await?;
        let storage_mode = db::get_storage_mode_for_inode(&self.pool, inode_id).await?;
        let mut file = File::open(&source_path).await?;
        let mut read_buffer = vec![0u8; self.config.chunk_size];
        let mut prepared_packs = Vec::new();
        let mut logical_size = 0u64;
        let mut encrypted_size = 0u64;
        let mut file_offset = 0i64;

        loop {
            let bytes_read = read_next_chunk(&mut file, &mut read_buffer).await?;
            if bytes_read == 0 {
                break;
            }

            let plaintext = &read_buffer[..bytes_read];
            let plaintext_hash = hex_sha256(plaintext);
            let plain_size = to_i64(bytes_read, "chunk size")?;

            if let Some(existing_pack) =
                db::find_pack_by_plaintext_hash(&self.pool, &plaintext_hash, storage_mode).await?
            {
                let manifest_size = manifest_size_from_pack(&existing_pack)?;

                prepared_packs.push(PreparedPack {
                    pack_id: existing_pack.pack_id.clone(),
                    chunk_id: vec_to_array_32(&existing_pack.chunk_id, "chunk_id")?,
                    plaintext_hash,
                    storage_mode,
                    file_offset,
                    plain_size,
                    cipher_size: existing_pack.cipher_size,
                    shard_size: existing_pack.shard_size,
                    manifest_path: local_pack_path(&self.config.spool_dir, &existing_pack.pack_id),
                    manifest_size,
                    nonce: vec_to_array_12(&existing_pack.nonce, "nonce")?,
                    gcm_tag: vec_to_array_16(&existing_pack.gcm_tag, "gcm_tag")?,
                    shards: Vec::new(),
                    is_deduplicated: true,
                });

                logical_size += bytes_read as u64;
                encrypted_size += u64::try_from(manifest_size)
                    .map_err(|_| PackerError::NumericOverflow("manifest size total"))?;
                file_offset = checked_add_i64(file_offset, plain_size, "file offset")?;
                continue;
            }

            let encrypted = encrypt_chunk(&vault_key, plaintext, &[])?;
            let manifest_bytes = build_manifest_bytes(
                encrypted.chunk_id,
                encrypted.nonce,
                &encrypted.ciphertext,
                &encrypted.gcm_tag,
                bytes_read,
            )?;
            let pack_id = compute_pack_id(storage_mode, &manifest_bytes);
            let manifest_path = local_pack_path(&self.config.spool_dir, &pack_id);
            write_ephemeral_bytes(&manifest_path, &manifest_bytes)
                .await
                .map_err(|err| PackerError::Io(std::io::Error::other(err.to_string())))?;

            let shards = build_shards(
                &self.config.spool_dir,
                &pack_id,
                &encrypted.ciphertext,
                storage_mode,
            )
            .await?;

            let cipher_size = to_i64(encrypted.ciphertext.len(), "cipher size")?;
            let shard_size = if storage_mode == StorageMode::LocalOnly {
                0
            } else {
                shards
                    .first()
                    .map(|shard| shard.size)
                    .ok_or(PackerError::NumericOverflow("missing shard size"))?
            };
            let manifest_size = to_i64(manifest_bytes.len(), "manifest size")?;

            prepared_packs.push(PreparedPack {
                pack_id,
                chunk_id: encrypted.chunk_id,
                plaintext_hash,
                storage_mode,
                file_offset,
                plain_size,
                cipher_size,
                shard_size,
                manifest_path,
                manifest_size,
                nonce: encrypted.nonce,
                gcm_tag: encrypted.gcm_tag,
                shards,
                is_deduplicated: false,
            });

            logical_size += bytes_read as u64;
            encrypted_size += u64::try_from(manifest_size)
                .map_err(|_| PackerError::NumericOverflow("manifest size total"))?;
            file_offset = checked_add_i64(file_offset, plain_size, "file offset")?;
        }

        let local_device = db::get_local_device_identity(&self.pool).await?;
        let local_device_id = local_device.as_ref().map(|device| device.device_id.as_str());
        let local_device_name = local_device
            .as_ref()
            .map(|device| device.device_name.as_str())
            .unwrap_or("Unknown Device");
        let current_revision = db::get_current_file_revision(&self.pool, inode_id).await?;
        let mut conflict_copy_name = None;
        let parent_revision_id = if let Some(expected_parent_revision_id) = expected_parent_revision_id {
            match current_revision.as_ref() {
                Some(current) => {
                    let lineage = db::classify_revision_lineage(
                        &self.pool,
                        expected_parent_revision_id,
                        current.revision_id,
                    )
                    .await?;
                    match lineage {
                        db::RevisionLineageRelation::Same => Some(expected_parent_revision_id),
                        db::RevisionLineageRelation::CandidateDescendsFromCurrent => {
                            Some(expected_parent_revision_id)
                        }
                        db::RevisionLineageRelation::CurrentDescendsFromCandidate
                        | db::RevisionLineageRelation::Parallel => {
                            let reason = match lineage {
                                db::RevisionLineageRelation::CurrentDescendsFromCandidate => {
                                    "stale_local_base"
                                }
                                db::RevisionLineageRelation::Parallel => "parallel_local_edit",
                                _ => unreachable!(),
                            };
                            let (_conflict_inode_id, _conflict_revision_id, materialized_name, _conflict_id) =
                                db::materialize_conflict_copy_from_revision(
                                    &self.pool,
                                    current.revision_id,
                                    local_device_id,
                                    local_device_name,
                                    reason,
                                )
                                .await?;
                            conflict_copy_name = Some(materialized_name);
                            Some(expected_parent_revision_id)
                        }
                    }
                }
                None => Some(expected_parent_revision_id),
            }
        } else {
            current_revision.as_ref().map(|revision| revision.revision_id)
        };
        let revision_id = db::create_file_revision(
            &self.pool,
            inode_id,
            i64::try_from(logical_size)
                .map_err(|_| PackerError::NumericOverflow("logical size"))?,
            None,
            local_device_id,
            parent_revision_id,
            "local_write",
            None,
        )
        .await?;

        if prepared_packs.is_empty() {
            return Ok(PackResult {
                source_path,
                revision_id: Some(revision_id),
                pack_id: None,
                pack_ids: Vec::new(),
                pack_path: None,
                pack_paths: Vec::new(),
                chunk_count: 0,
                logical_size: 0,
                encrypted_size: 0,
                created_at_ms,
                conflict_copy_name,
            });
        }

        let mut pack_ids = Vec::with_capacity(prepared_packs.len());
        let mut pack_paths = Vec::with_capacity(prepared_packs.len());

        for pack in &prepared_packs {
            db::register_chunk(
                &self.pool,
                revision_id,
                &pack.chunk_id,
                pack.file_offset,
                pack.plain_size,
            )
            .await?;

            db::link_chunk_to_pack(
                &self.pool,
                &pack.chunk_id,
                &pack.pack_id,
                0,
                pack.manifest_size,
            )
            .await?;

            pack_ids.push(pack.pack_id.clone());
            pack_paths.push(pack.manifest_path.clone());

            if pack.is_deduplicated {
                continue;
            }

            db::create_pack(
                &self.pool,
                &pack.pack_id,
                &pack.chunk_id,
                &pack.plaintext_hash,
                pack.storage_mode,
                1,
                storage_mode_scheme(pack.storage_mode),
                pack.plain_size,
                pack.cipher_size,
                pack.shard_size,
                &pack.nonce,
                &pack.gcm_tag,
                if pack.storage_mode == StorageMode::LocalOnly {
                    PackStatus::Healthy
                } else {
                    PackStatus::Uploading
                },
            )
            .await?;

            for shard in &pack.shards {
                db::register_pack_shard(
                    &self.pool,
                    &pack.pack_id,
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

            if pack.storage_mode != StorageMode::LocalOnly {
                db::queue_pack_for_upload(&self.pool, &pack.pack_id).await?;
            }
        }

        Ok(PackResult {
            source_path,
            revision_id: Some(revision_id),
            pack_id: pack_ids.first().cloned(),
            pack_ids,
            pack_path: pack_paths.first().cloned(),
            pack_paths,
            chunk_count: prepared_packs.len(),
            logical_size,
            encrypted_size,
            created_at_ms,
            conflict_copy_name,
        })
    }
}

pub(crate) fn local_pack_path(spool_dir: &Path, pack_id: &str) -> PathBuf {
    spool_dir.join(format!("{pack_id}.{LOCAL_PACK_EXTENSION}"))
}

pub fn local_shard_path(spool_dir: &Path, pack_id: &str, shard_index: usize) -> PathBuf {
    spool_dir.join(format!("{pack_id}.{LOCAL_SHARD_EXTENSION}{shard_index}"))
}

pub(crate) async fn build_shards(
    spool_dir: &Path,
    pack_id: &str,
    ciphertext: &[u8],
    storage_mode: StorageMode,
) -> Result<Vec<PreparedShard>, PackerError> {
    let shard_bytes = match storage_mode {
        StorageMode::Ec2_1 => split_ciphertext_into_shards(ciphertext)?,
        StorageMode::SingleReplica => vec![ciphertext.to_vec()],
        StorageMode::LocalOnly => Vec::new(),
    };
    let mut prepared = Vec::with_capacity(TOTAL_SHARDS);

    for (index, bytes) in shard_bytes.into_iter().enumerate() {
        let shard_role = if storage_mode == StorageMode::Ec2_1 && index >= DATA_SHARDS {
            ShardRole::Parity
        } else {
            ShardRole::Data
        };
        let provider = match storage_mode {
            StorageMode::Ec2_1 => SHARD_PROVIDERS[index],
            StorageMode::SingleReplica => SINGLE_REPLICA_PROVIDER,
            StorageMode::LocalOnly => unreachable!("local-only packs do not create shards"),
        };
        let local_path = local_shard_path(spool_dir, pack_id, index);
        write_ephemeral_bytes(&local_path, &bytes)
            .await
            .map_err(|err| PackerError::Io(std::io::Error::other(err.to_string())))?;

        prepared.push(PreparedShard {
            shard_index: i64::try_from(index)
                .map_err(|_| PackerError::NumericOverflow("shard index"))?,
            shard_role,
            provider,
            object_key: format!("packs/{pack_id}/shards/{index}.{LOCAL_SHARD_EXTENSION}"),
            local_path,
            size: to_i64(bytes.len(), "shard size")?,
            checksum: hex_sha256(&bytes),
        });
    }

    Ok(prepared)
}

pub(crate) fn split_ciphertext_into_shards(ciphertext: &[u8]) -> Result<Vec<Vec<u8>>, PackerError> {
    let reed_solomon = ReedSolomon::new(DATA_SHARDS, PARITY_SHARDS)?;
    let shard_len = ciphertext.len().div_ceil(DATA_SHARDS).max(1);
    let mut shards = vec![vec![0u8; shard_len]; TOTAL_SHARDS];

    for (offset, byte) in ciphertext.iter().copied().enumerate() {
        let shard_index = offset / shard_len;
        let position = offset % shard_len;
        shards[shard_index][position] = byte;
    }

    reed_solomon.encode(&mut shards)?;
    Ok(shards)
}

pub(crate) fn build_manifest_bytes(
    chunk_id: [u8; 32],
    nonce: [u8; 12],
    ciphertext: &[u8],
    gcm_tag: &[u8; 16],
    plaintext_len: usize,
) -> Result<Vec<u8>, PackerError> {
    let prefix = ChunkRecordPrefix {
        record_magic: CHUNK_RECORD_MAGIC,
        record_version: 1,
        flags: 0,
        compression_algo: COMPRESSION_ALGO_NONE,
        reserved_0: 0,
        chunk_id,
        plain_len: U64::new(plaintext_len as u64),
        cipher_len: U64::new(ciphertext.len() as u64),
        nonce,
        reserved_1: [0u8; 12],
    };

    let mut bytes = Vec::with_capacity(ChunkRecordPrefix::SIZE + ciphertext.len() + gcm_tag.len());
    bytes.extend_from_slice(prefix.as_bytes());
    bytes.extend_from_slice(ciphertext);
    bytes.extend_from_slice(gcm_tag);
    Ok(bytes)
}

pub(crate) fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

pub(crate) fn compute_pack_id(storage_mode: StorageMode, manifest_bytes: &[u8]) -> String {
    let mut bytes = Vec::with_capacity(storage_mode.as_str().len() + 1 + manifest_bytes.len());
    bytes.extend_from_slice(storage_mode.as_str().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(manifest_bytes);
    hex_sha256(&bytes)
}

fn unix_timestamp_ms() -> Result<u64, PackerError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64)
}

fn to_i64(value: usize, context: &'static str) -> Result<i64, PackerError> {
    i64::try_from(value).map_err(|_| PackerError::NumericOverflow(context))
}

fn checked_add_i64(lhs: i64, rhs: i64, context: &'static str) -> Result<i64, PackerError> {
    lhs.checked_add(rhs)
        .ok_or(PackerError::NumericOverflow(context))
}

fn manifest_size_from_pack(pack: &db::PackRecord) -> Result<i64, PackerError> {
    let cipher_size = usize::try_from(pack.cipher_size)
        .map_err(|_| PackerError::NumericOverflow("stored cipher_size"))?;
    let gcm_tag_len = pack.gcm_tag.len();
    let total = ChunkRecordPrefix::SIZE
        .checked_add(cipher_size)
        .and_then(|value| value.checked_add(gcm_tag_len))
        .ok_or(PackerError::NumericOverflow("stored manifest size"))?;
    to_i64(total, "stored manifest size")
}

pub(crate) fn storage_mode_scheme(storage_mode: StorageMode) -> &'static str {
    match storage_mode {
        StorageMode::Ec2_1 => EC_SCHEME_RS_2_1,
        StorageMode::SingleReplica => EC_SCHEME_SINGLE_REPLICA,
        StorageMode::LocalOnly => EC_SCHEME_LOCAL_ONLY,
    }
}

fn vec_to_array_32(bytes: &[u8], field: &'static str) -> Result<[u8; 32], PackerError> {
    <[u8; 32]>::try_from(bytes).map_err(|_| PackerError::InvalidStoredPack(field))
}

fn vec_to_array_16(bytes: &[u8], field: &'static str) -> Result<[u8; 16], PackerError> {
    <[u8; 16]>::try_from(bytes).map_err(|_| PackerError::InvalidStoredPack(field))
}

fn vec_to_array_12(bytes: &[u8], field: &'static str) -> Result<[u8; 12], PackerError> {
    <[u8; 12]>::try_from(bytes).map_err(|_| PackerError::InvalidStoredPack(field))
}

async fn read_next_chunk(file: &mut File, buffer: &mut [u8]) -> Result<usize, std::io::Error> {
    let mut filled = 0usize;

    while filled < buffer.len() {
        let bytes_read = file.read(&mut buffer[filled..]).await?;
        if bytes_read == 0 {
            break;
        }

        filled += bytes_read;
    }

    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::vault::VaultKeyStore;
    use std::env;

    #[tokio::test]
    async fn splits_into_default_4mb_chunks() -> Result<(), Box<dyn std::error::Error>> {
        let test_root = env::temp_dir().join(format!(
            "omnidrive-packer-test-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let spool_dir = test_root.join("spool");
        let source_path = test_root.join("source.bin");
        let payload_len = (DEFAULT_CHUNK_SIZE * 2) + 123;
        let payload = vec![0xAB; payload_len];

        fs::create_dir_all(&spool_dir).await?;
        fs::write(&source_path, &payload).await?;

        let pool = db::init_db("sqlite::memory:").await?;
        let inode_id = db::create_inode(
            &pool,
            None,
            "source.bin",
            "FILE",
            i64::try_from(payload_len)?,
        )
        .await?;

        let vault_keys = VaultKeyStore::new();
        vault_keys.set_key_for_tests([0x11; 32]).await;
        let packer = Packer::new(pool.clone(), vault_keys, PackerConfig::new(&spool_dir))?;
        let result = packer.pack_file(inode_id, &source_path).await?;
        let chunks = db::get_file_chunks(&pool, inode_id).await?;
        let sizes: Vec<i64> = chunks.iter().map(|chunk| chunk.size).collect();

        assert_eq!(result.chunk_count, 3);
        assert_eq!(result.pack_ids.len(), 3);
        assert_eq!(result.logical_size, payload_len as u64);
        assert_eq!(
            sizes,
            vec![DEFAULT_CHUNK_SIZE as i64, DEFAULT_CHUNK_SIZE as i64, 123]
        );

        let _ = fs::remove_dir_all(&test_root).await;
        Ok(())
    }

    #[tokio::test]
    async fn rejects_packing_when_vault_is_locked() -> Result<(), Box<dyn std::error::Error>> {
        let test_root = env::temp_dir().join(format!(
            "omnidrive-packer-locked-test-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let spool_dir = test_root.join("spool");
        let source_path = test_root.join("source.bin");
        let payload = vec![0xCD; 1024];

        fs::create_dir_all(&spool_dir).await?;
        fs::write(&source_path, &payload).await?;

        let pool = db::init_db("sqlite::memory:").await?;
        let inode_id = db::create_inode(
            &pool,
            None,
            "source.bin",
            "FILE",
            i64::try_from(payload.len())?,
        )
        .await?;

        let vault_keys = VaultKeyStore::new();
        let packer = Packer::new(pool, vault_keys, PackerConfig::new(&spool_dir))?;
        let err = packer.pack_file(inode_id, &source_path).await.unwrap_err();

        assert!(matches!(
            err,
            PackerError::Vault(crate::vault::VaultError::Locked)
        ));

        let _ = fs::remove_dir_all(&test_root).await;
        Ok(())
    }
}
