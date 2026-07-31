// reserved for Epic 33.2 (ZK share download + DEK unwrap flow)
#![allow(dead_code)]

mod chunk;
mod pack;
mod prefetch;
mod provider;
mod read;
mod util;

pub use chunk::*;

use crate::cache::CacheError;
use crate::cache::CacheManager;
use crate::config::AppConfig;
use crate::peer::PeerClient;
use crate::vault::VaultError;
use crate::vault::VaultKeyStore;
use aws_sdk_s3::Client;
use omnidrive_core::crypto::CryptoError;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Downloader {
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
    download_spool_dir: PathBuf,
    cache: CacheManager,
    providers: Arc<RwLock<HashMap<String, DownloadProvider>>>,
    provider_timeout: Duration,
    app_config: AppConfig,
    prefetch_state: Arc<Mutex<HashMap<i64, i64>>>,
    peer_client: Arc<Mutex<Option<PeerClient>>>,
    // Per-pack mutex: ensures only one B2 download per pack_id at a time.
    // Concurrent FetchData callbacks for the same pack wait for the first
    // download to finish, then return the already-written spool file.
    pack_download_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

#[derive(Clone)]
struct DownloadProvider {
    provider_name: &'static str,
    bucket: String,
    client: Client,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoredPackSource {
    pub pack_id: String,
    pub providers: Vec<String>,
    pub local_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoreResult {
    pub inode_id: i64,
    pub output_path: PathBuf,
    pub bytes_written: u64,
    pub pack_sources: Vec<RestoredPackSource>,
}

#[derive(Debug)]
pub enum DownloaderError {
    MissingProviderConfig,
    InvalidEnv(&'static str),
    Io(std::io::Error),
    Db(sqlx::Error),
    Cache(CacheError),
    Crypto(CryptoError),
    Vault(VaultError),
    ErasureCoding(reed_solomon_erasure::Error),
    NumericOverflow(&'static str),
    NoChunksForInode(i64),
    PackMissing(String),
    NoPackShards(String),
    NoConfiguredProvider(String),
    ShardDownloadFailed {
        pack_id: String,
        errors: Vec<String>,
    },
    InvalidPackRecord(&'static str),
    CloudGuard(String),
    RuntimeConfig(String),
}

impl fmt::Display for DownloaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProviderConfig => write!(f, "no download providers configured"),
            Self::InvalidEnv(key) => write!(f, "invalid environment variable {key}"),
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::Db(err) => write!(f, "sqlite error: {err}"),
            Self::Cache(err) => write!(f, "cache error: {err}"),
            Self::Crypto(err) => write!(f, "crypto error: {err}"),
            Self::Vault(err) => write!(f, "vault error: {err}"),
            Self::ErasureCoding(err) => write!(f, "erasure coding error: {err}"),
            Self::NumericOverflow(ctx) => write!(f, "numeric overflow while handling {ctx}"),
            Self::NoChunksForInode(inode_id) => write!(f, "no chunks found for inode {inode_id}"),
            Self::PackMissing(pack_id) => write!(f, "pack {pack_id} is missing from SQLite"),
            Self::NoPackShards(pack_id) => write!(f, "no shards found for pack {pack_id}"),
            Self::NoConfiguredProvider(provider) => {
                write!(f, "provider {provider} is not configured for downloads")
            }
            Self::ShardDownloadFailed { pack_id, errors } => {
                write!(
                    f,
                    "failed to download enough shards for pack {pack_id}: {}",
                    errors.join(" | ")
                )
            }
            Self::InvalidPackRecord(reason) => write!(f, "invalid pack record: {reason}"),
            Self::CloudGuard(reason) => write!(f, "cloud guard blocked operation: {reason}"),
            Self::RuntimeConfig(reason) => {
                write!(f, "runtime provider configuration error: {reason}")
            }
        }
    }
}

impl std::error::Error for DownloaderError {}

impl From<std::io::Error> for DownloaderError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<sqlx::Error> for DownloaderError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

impl From<CacheError> for DownloaderError {
    fn from(value: CacheError) -> Self {
        Self::Cache(value)
    }
}

impl From<CryptoError> for DownloaderError {
    fn from(value: CryptoError) -> Self {
        Self::Crypto(value)
    }
}

impl From<VaultError> for DownloaderError {
    fn from(value: VaultError) -> Self {
        Self::Vault(value)
    }
}

impl From<reed_solomon_erasure::Error> for DownloaderError {
    fn from(value: reed_solomon_erasure::Error) -> Self {
        Self::ErasureCoding(value)
    }
}
