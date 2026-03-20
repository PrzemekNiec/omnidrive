#![allow(dead_code)]

use crate::db;
use omnidrive_core::crypto::{CryptoError, KeyBytes, encrypt_chunk};
use omnidrive_core::layout::{CHUNK_RECORD_MAGIC, COMPRESSION_ALGO_NONE, ChunkRecordPrefix};
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
    vault_key: KeyBytes,
    config: PackerConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackResult {
    pub source_path: PathBuf,
    pub pack_id: Option<String>,
    pub pack_path: Option<PathBuf>,
    pub chunk_count: usize,
    pub logical_size: u64,
    pub encrypted_size: u64,
    pub created_at_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreparedChunk {
    chunk_id: [u8; 32],
    file_offset: i64,
    pack_offset: i64,
    plain_size: i64,
    encrypted_size: i64,
}

#[derive(Debug)]
pub enum PackerError {
    InvalidChunkSize(usize),
    Io(std::io::Error),
    Db(sqlx::Error),
    Crypto(CryptoError),
    NumericOverflow(&'static str),
    Clock(std::time::SystemTimeError),
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
            Self::NumericOverflow(ctx) => write!(f, "numeric overflow while handling {ctx}"),
            Self::Clock(err) => write!(f, "system clock error: {err}"),
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

impl From<std::time::SystemTimeError> for PackerError {
    fn from(value: std::time::SystemTimeError) -> Self {
        Self::Clock(value)
    }
}

impl Packer {
    pub fn new(
        pool: SqlitePool,
        vault_key: KeyBytes,
        config: PackerConfig,
    ) -> Result<Self, PackerError> {
        if config.chunk_size == 0 {
            return Err(PackerError::InvalidChunkSize(config.chunk_size));
        }

        Ok(Self {
            pool,
            vault_key,
            config,
        })
    }

    pub async fn pack_file(
        &self,
        inode_id: i64,
        source_path: impl AsRef<Path>,
    ) -> Result<PackResult, PackerError> {
        let source_path = source_path.as_ref().to_path_buf();
        fs::create_dir_all(&self.config.spool_dir).await?;

        let created_at_ms = unix_timestamp_ms()?;
        let mut file = File::open(&source_path).await?;
        let mut read_buffer = vec![0u8; self.config.chunk_size];
        let mut pack_bytes = Vec::new();
        let mut prepared_chunks = Vec::new();
        let mut logical_size = 0u64;
        let mut file_offset = 0i64;

        loop {
            let bytes_read = read_next_chunk(&mut file, &mut read_buffer).await?;
            if bytes_read == 0 {
                break;
            }

            let plaintext = &read_buffer[..bytes_read];
            let encrypted = encrypt_chunk(&self.vault_key, plaintext, &[])?;
            let pack_offset = to_i64(pack_bytes.len(), "pack offset")?;
            let record_prefix = ChunkRecordPrefix {
                record_magic: CHUNK_RECORD_MAGIC,
                record_version: 1,
                flags: 0,
                compression_algo: COMPRESSION_ALGO_NONE,
                reserved_0: 0,
                chunk_id: encrypted.chunk_id,
                plain_len: U64::new(bytes_read as u64),
                cipher_len: U64::new(encrypted.ciphertext.len() as u64),
                nonce: encrypted.nonce,
                reserved_1: [0u8; 12],
            };

            pack_bytes.extend_from_slice(record_prefix.as_bytes());
            pack_bytes.extend_from_slice(&encrypted.ciphertext);
            pack_bytes.extend_from_slice(&encrypted.gcm_tag);

            let plain_size = to_i64(bytes_read, "chunk size")?;
            let encrypted_size = to_i64(
                ChunkRecordPrefix::SIZE + encrypted.ciphertext.len() + encrypted.gcm_tag.len(),
                "encrypted chunk size",
            )?;

            prepared_chunks.push(PreparedChunk {
                chunk_id: encrypted.chunk_id,
                file_offset,
                pack_offset,
                plain_size,
                encrypted_size,
            });

            logical_size += bytes_read as u64;
            file_offset = checked_add_i64(file_offset, plain_size, "file offset")?;
        }

        db::delete_file_chunks(&self.pool, inode_id).await?;

        if prepared_chunks.is_empty() {
            return Ok(PackResult {
                source_path,
                pack_id: None,
                pack_path: None,
                chunk_count: 0,
                logical_size: 0,
                encrypted_size: 0,
                created_at_ms,
            });
        }

        let pack_id = hex_sha256(&pack_bytes);
        let pack_path = self
            .config
            .spool_dir
            .join(format!("{pack_id}.{LOCAL_PACK_EXTENSION}"));

        fs::write(&pack_path, &pack_bytes).await?;

        for chunk in &prepared_chunks {
            db::register_chunk(
                &self.pool,
                inode_id,
                &chunk.chunk_id,
                chunk.file_offset,
                chunk.plain_size,
            )
            .await?;

            db::link_chunk_to_pack(
                &self.pool,
                &chunk.chunk_id,
                &pack_id,
                chunk.pack_offset,
                chunk.encrypted_size,
            )
            .await?;
        }

        db::queue_pack_for_upload(&self.pool, &pack_id).await?;

        Ok(PackResult {
            source_path,
            pack_id: Some(pack_id),
            pack_path: Some(pack_path),
            chunk_count: prepared_chunks.len(),
            logical_size,
            encrypted_size: pack_bytes.len() as u64,
            created_at_ms,
        })
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
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
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

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

        let packer = Packer::new(pool.clone(), [0x11; 32], PackerConfig::new(&spool_dir))?;
        let result = packer.pack_file(inode_id, &source_path).await?;
        let chunks = db::get_file_chunks(&pool, inode_id).await?;
        let sizes: Vec<i64> = chunks.iter().map(|chunk| chunk.size).collect();

        assert_eq!(result.chunk_count, 3);
        assert_eq!(result.logical_size, payload_len as u64);
        assert_eq!(
            sizes,
            vec![DEFAULT_CHUNK_SIZE as i64, DEFAULT_CHUNK_SIZE as i64, 123]
        );

        let _ = fs::remove_dir_all(&test_root).await;
        Ok(())
    }
}
