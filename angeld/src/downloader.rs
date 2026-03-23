#![allow(dead_code)]

use crate::cache::{CacheError, CacheManager};
use crate::db;
use crate::packer::{DATA_SHARDS, LOCAL_PACK_EXTENSION, PARITY_SHARDS, TOTAL_SHARDS};
use crate::uploader::ProviderConfig;
use crate::vault::{VaultError, VaultKeyStore};
use aws_config::timeout::TimeoutConfig;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use omnidrive_core::crypto::{ChunkId, CryptoError, GcmTag, KeyBytes, decrypt_chunk};
use omnidrive_core::layout::{CHUNK_RECORD_MAGIC, COMPRESSION_ALGO_NONE, ChunkRecordPrefix};
use reed_solomon_erasure::galois_8::ReedSolomon;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs::{self, File};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;
use zerocopy::AsBytes;
use zerocopy::byteorder::big_endian::U64;

#[derive(Clone)]
pub struct Downloader {
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
    download_spool_dir: PathBuf,
    cache: CacheManager,
    providers: HashMap<String, DownloadProvider>,
    provider_timeout: Duration,
    prefetch_state: Arc<Mutex<HashMap<i64, i64>>>,
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

impl Downloader {
    pub async fn from_env(
        pool: SqlitePool,
        vault_keys: VaultKeyStore,
    ) -> Result<Self, DownloaderError> {
        let _ = dotenvy::dotenv();

        let download_spool_dir =
            env_path("OMNIDRIVE_DOWNLOAD_SPOOL_DIR", ".omnidrive/download-spool");
        let provider_timeout = duration_from_env("OMNIDRIVE_DOWNLOAD_TIMEOUT_MS", 120_000);

        let mut configs = Vec::new();
        if let Ok(config) = ProviderConfig::from_r2_env() {
            configs.push(config);
        }
        if let Ok(config) = ProviderConfig::from_scaleway_env() {
            configs.push(config);
        }
        if let Ok(config) = ProviderConfig::from_b2_env() {
            configs.push(config);
        }

        Self::from_provider_configs(
            pool,
            vault_keys,
            download_spool_dir,
            provider_timeout,
            configs,
        )
        .await
    }

    pub(crate) async fn from_provider_configs(
        pool: SqlitePool,
        vault_keys: VaultKeyStore,
        download_spool_dir: impl Into<PathBuf>,
        provider_timeout: Duration,
        configs: Vec<ProviderConfig>,
    ) -> Result<Self, DownloaderError> {
        if configs.is_empty() {
            return Err(DownloaderError::MissingProviderConfig);
        }

        let download_spool_dir = download_spool_dir.into();
        fs::create_dir_all(&download_spool_dir).await?;
        let cache = CacheManager::from_env(pool.clone()).await?;

        let mut providers = HashMap::new();
        for config in configs {
            let provider = DownloadProvider::from_provider_config(config).await?;
            providers.insert(provider.provider_name.to_string(), provider);
        }

        Ok(Self {
            pool,
            vault_keys,
            download_spool_dir,
            cache,
            providers,
            provider_timeout,
            prefetch_state: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn restore_file(
        &self,
        inode_id: i64,
        output_path: impl AsRef<Path>,
    ) -> Result<RestoreResult, DownloaderError> {
        let output_path = output_path.as_ref().to_path_buf();
        let vault_key = self.vault_keys.require_key().await?;
        let chunk_locations = db::get_file_chunk_locations(&self.pool, inode_id).await?;
        if chunk_locations.is_empty() {
            return Err(DownloaderError::NoChunksForInode(inode_id));
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let mut output = File::create(&output_path).await?;
        let mut current_offset = 0u64;
        let mut downloaded_packs = HashMap::<String, RestoredPackSource>::new();

        for chunk in chunk_locations {
            let source = if let Some(existing) = downloaded_packs.get(&chunk.pack_id) {
                existing.clone()
            } else {
                let downloaded = self.download_pack(&chunk.pack_id).await?;
                downloaded_packs.insert(chunk.pack_id.clone(), downloaded.clone());
                downloaded
            };

            let pack_bytes = fs::read(&source.local_path).await?;
            let plaintext = decrypt_chunk_record(&pack_bytes, &chunk, &vault_key)?;

            let desired_offset = to_u64(chunk.file_offset, "file offset")?;
            if current_offset != desired_offset {
                output
                    .seek(std::io::SeekFrom::Start(desired_offset))
                    .await?;
                current_offset = desired_offset;
            }

            output.write_all(&plaintext).await?;
            current_offset = current_offset
                .checked_add(plaintext.len() as u64)
                .ok_or(DownloaderError::NumericOverflow("bytes written"))?;
        }

        output.flush().await?;

        Ok(RestoreResult {
            inode_id,
            output_path,
            bytes_written: current_offset,
            pack_sources: downloaded_packs.into_values().collect(),
        })
    }

    pub async fn read_range(
        &self,
        inode_id: i64,
        revision_id: i64,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, DownloaderError> {
        let revision = db::get_file_revision(&self.pool, inode_id, revision_id)
            .await?
            .ok_or(DownloaderError::NoChunksForInode(inode_id))?;

        if length == 0 {
            return Ok(Vec::new());
        }

        let end_offset = offset
            .checked_add(length)
            .ok_or(DownloaderError::NumericOverflow("range end"))?;
        let start_i64 = i64::try_from(offset)
            .map_err(|_| DownloaderError::NumericOverflow("range start"))?;
        let end_i64 = i64::try_from(end_offset)
            .map_err(|_| DownloaderError::NumericOverflow("range end"))?;

        let chunk_locations = db::get_revision_chunk_locations_in_range(
            &self.pool,
            inode_id,
            revision_id,
            start_i64,
            end_i64,
        )
        .await?;
        if chunk_locations.is_empty() {
            return Err(DownloaderError::NoChunksForInode(inode_id));
        }

        let inode_path = db::get_inode_path(&self.pool, inode_id)
            .await?
            .unwrap_or_else(|| format!("inode/{inode_id}"));
        let vault_key = self.vault_keys.require_key().await?;
        let mut downloaded_packs = HashMap::<String, RestoredPackSource>::new();
        let mut result = Vec::with_capacity(
            usize::try_from(length).map_err(|_| DownloaderError::NumericOverflow("range length"))?,
        );
        let first_chunk_index = chunk_locations.first().map(|chunk| chunk.chunk_index);
        let last_chunk_index = chunk_locations.last().map(|chunk| chunk.chunk_index);

        for chunk in chunk_locations {
            let plaintext = self
                .load_plaintext_chunk(
                    inode_id,
                    revision_id,
                    &inode_path,
                    &vault_key,
                    &mut downloaded_packs,
                    &chunk,
                    false,
                )
                .await?;

            let chunk_start = to_u64(chunk.file_offset, "file offset")?;
            let chunk_end = chunk_start
                .checked_add(plaintext.len() as u64)
                .ok_or(DownloaderError::NumericOverflow("chunk end"))?;
            let slice_start = offset.max(chunk_start);
            let slice_end = end_offset.min(chunk_end);

            if slice_start >= slice_end {
                continue;
            }

            let local_start = usize::try_from(slice_start - chunk_start)
                .map_err(|_| DownloaderError::NumericOverflow("slice start"))?;
            let local_end = usize::try_from(slice_end - chunk_start)
                .map_err(|_| DownloaderError::NumericOverflow("slice end"))?;
            result.extend_from_slice(&plaintext[local_start..local_end]);

            if result.len()
                >= usize::try_from(length)
                    .map_err(|_| DownloaderError::NumericOverflow("range length"))?
            {
                break;
            }
        }

        let target_len =
            usize::try_from(length).map_err(|_| DownloaderError::NumericOverflow("range length"))?;
        if result.len() > target_len {
            result.truncate(target_len);
        }

        self.maybe_schedule_prefetch(
            inode_id,
            revision_id,
            revision.size,
            &inode_path,
            first_chunk_index,
            last_chunk_index,
        )
        .await;

        Ok(result)
    }

    async fn load_plaintext_chunk(
        &self,
        inode_id: i64,
        revision_id: i64,
        inode_path: &str,
        vault_key: &KeyBytes,
        downloaded_packs: &mut HashMap<String, RestoredPackSource>,
        chunk: &db::FileChunkLocation,
        is_prefetched: bool,
    ) -> Result<Vec<u8>, DownloaderError> {
        let cache_key = CacheManager::cache_key(revision_id, chunk.chunk_index);
        if let Some(bytes) = self.cache.get_chunk(&cache_key).await? {
            return Ok(bytes);
        }

        let source = if let Some(existing) = downloaded_packs.get(&chunk.pack_id) {
            existing.clone()
        } else {
            let downloaded = self.download_pack(&chunk.pack_id).await?;
            downloaded_packs.insert(chunk.pack_id.clone(), downloaded.clone());
            downloaded
        };

        let pack_bytes = fs::read(&source.local_path).await?;
        let plaintext = decrypt_chunk_record(&pack_bytes, chunk, vault_key)?;
        self.cache
            .put_chunk(
                inode_id,
                revision_id,
                chunk.chunk_index,
                &chunk.pack_id,
                inode_path,
                &plaintext,
                is_prefetched,
            )
            .await?;
        Ok(plaintext)
    }

    async fn maybe_schedule_prefetch(
        &self,
        inode_id: i64,
        revision_id: i64,
        revision_size: i64,
        inode_path: &str,
        first_chunk_index: Option<i64>,
        last_chunk_index: Option<i64>,
    ) {
        let Some(first_chunk_index) = first_chunk_index else {
            return;
        };
        let Some(last_chunk_index) = last_chunk_index else {
            return;
        };

        let previous_chunk_index = {
            let mut state = self.prefetch_state.lock().await;
            let previous = state.insert(revision_id, last_chunk_index);
            previous
        };

        let mut targets = Vec::new();
        if previous_chunk_index.is_some_and(|prev| prev + 1 == first_chunk_index) {
            targets.push(last_chunk_index + 1);
            targets.push(last_chunk_index + 2);
        }

        let small_file_threshold = 8_i64 * 1024 * 1024;
        if revision_size > 0 && revision_size <= small_file_threshold && first_chunk_index == 0 {
            let total_chunks = ((revision_size - 1) / crate::packer::DEFAULT_CHUNK_SIZE as i64) + 1;
            for chunk_index in (last_chunk_index + 1)..total_chunks {
                targets.push(chunk_index);
            }
        }

        targets.sort_unstable();
        targets.dedup();
        if targets.is_empty() {
            return;
        }

        let downloader = self.clone();
        let inode_path = inode_path.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(75)).await;
            let _ = downloader
                .prefetch_chunks(inode_id, revision_id, &inode_path, targets)
                .await;
        });
    }

    async fn prefetch_chunks(
        &self,
        inode_id: i64,
        revision_id: i64,
        inode_path: &str,
        chunk_indexes: Vec<i64>,
    ) -> Result<(), DownloaderError> {
        if chunk_indexes.is_empty() {
            return Ok(());
        }

        let revision = db::get_file_revision(&self.pool, inode_id, revision_id)
            .await?
            .ok_or(DownloaderError::NoChunksForInode(inode_id))?;
        let chunk_locations = db::get_revision_chunk_locations_in_range(
            &self.pool,
            inode_id,
            revision_id,
            0,
            revision.size,
        )
        .await?;
        if chunk_locations.is_empty() {
            return Ok(());
        }

        let vault_key = self.vault_keys.require_key().await?;
        let mut downloaded_packs = HashMap::<String, RestoredPackSource>::new();
        for chunk in chunk_locations
            .into_iter()
            .filter(|chunk| chunk_indexes.contains(&chunk.chunk_index))
        {
            let _ = self
                .load_plaintext_chunk(
                    inode_id,
                    revision_id,
                    inode_path,
                    &vault_key,
                    &mut downloaded_packs,
                    &chunk,
                    true,
                )
                .await?;
        }

        Ok(())
    }

    async fn download_pack(&self, pack_id: &str) -> Result<RestoredPackSource, DownloaderError> {
        let pack = db::get_pack(&self.pool, pack_id)
            .await?
            .ok_or_else(|| DownloaderError::PackMissing(pack_id.to_string()))?;
        let shards = db::get_pack_shards(&self.pool, pack_id).await?;
        if shards.is_empty() {
            return Err(DownloaderError::NoPackShards(pack_id.to_string()));
        }

        let mut candidates = Vec::new();
        for shard in shards {
            let Some(provider) = self.providers.get(&shard.provider) else {
                continue;
            };

            let latency = self.probe_latency(provider, &shard.object_key).await.ok();
            candidates.push((provider, shard, latency));
        }

        if candidates.is_empty() {
            return Err(DownloaderError::ShardDownloadFailed {
                pack_id: pack_id.to_string(),
                errors: vec![format!(
                    "no configured providers available for pack {pack_id}"
                )],
            });
        }

        candidates.sort_by_key(|(_, shard, latency)| {
            (
                latency.unwrap_or(Duration::MAX),
                if shard.status == "COMPLETED" { 0 } else { 1 },
            )
        });

        let mut shard_bytes: Vec<Option<Vec<u8>>> = vec![None; TOTAL_SHARDS];
        let mut downloaded_from = Vec::new();
        let mut errors = Vec::new();

        for (provider, shard, _) in candidates {
            let shard_index = usize::try_from(shard.shard_index)
                .map_err(|_| DownloaderError::NumericOverflow("shard index"))?;
            if shard_index >= TOTAL_SHARDS || shard_bytes[shard_index].is_some() {
                continue;
            }

            match self
                .download_shard(pack_id, provider, &shard.object_key, shard_index)
                .await
            {
                Ok(local_path) => {
                    let bytes = fs::read(&local_path).await?;
                    shard_bytes[shard_index] = Some(bytes);
                    downloaded_from.push(provider.provider_name.to_string());
                    if shard_bytes.iter().flatten().count() >= DATA_SHARDS {
                        break;
                    }
                }
                Err(err) => {
                    errors.push(format!(
                        "{} shard {}: {}",
                        shard.provider, shard.shard_index, err
                    ));
                }
            }
        }

        if shard_bytes.iter().flatten().count() < DATA_SHARDS {
            return Err(DownloaderError::ShardDownloadFailed {
                pack_id: pack_id.to_string(),
                errors,
            });
        }

        let ciphertext = reconstruct_ciphertext(&pack, &mut shard_bytes)?;
        let manifest_bytes = build_manifest_bytes(&pack, &ciphertext)?;
        let local_path = self
            .download_spool_dir
            .join(format!("{pack_id}.{LOCAL_PACK_EXTENSION}"));
        fs::write(&local_path, &manifest_bytes).await?;

        Ok(RestoredPackSource {
            pack_id: pack_id.to_string(),
            providers: downloaded_from,
            local_path,
        })
    }

    async fn probe_latency(
        &self,
        provider: &DownloadProvider,
        object_key: &str,
    ) -> Result<Duration, DownloaderError> {
        let start = Instant::now();
        tokio::time::timeout(
            self.provider_timeout,
            provider
                .client
                .head_object()
                .bucket(&provider.bucket)
                .key(object_key)
                .send(),
        )
        .await
        .map_err(|_| DownloaderError::InvalidPackRecord("provider probe timed out"))?
        .map_err(|_| DownloaderError::InvalidPackRecord("provider probe failed"))?;
        Ok(start.elapsed())
    }

    async fn download_shard(
        &self,
        pack_id: &str,
        provider: &DownloadProvider,
        object_key: &str,
        shard_index: usize,
    ) -> Result<PathBuf, String> {
        let response = tokio::time::timeout(
            self.provider_timeout,
            provider
                .client
                .get_object()
                .bucket(&provider.bucket)
                .key(object_key)
                .send(),
        )
        .await
        .map_err(|_| format!("{} download timed out", provider.provider_name))?
        .map_err(|err| {
            format!(
                "{} get_object failed: {}",
                provider.provider_name,
                format_error_details(&err)
            )
        })?;

        let body = response.body.collect().await.map_err(|err| {
            format!(
                "{} body read failed: {}",
                provider.provider_name,
                format_error_details(&err)
            )
        })?;

        let local_path = self
            .download_spool_dir
            .join(format!("{pack_id}.download-shard{shard_index}"));
        fs::write(&local_path, body.into_bytes())
            .await
            .map_err(|err| err.to_string())?;

        Ok(local_path)
    }
}

impl DownloadProvider {
    async fn from_provider_config(config: ProviderConfig) -> Result<Self, DownloaderError> {
        let provider_name = config.provider_name;
        let operation_timeout = duration_from_env("OMNIDRIVE_DOWNLOAD_TIMEOUT_MS", 120_000);
        let operation_attempt_timeout =
            duration_from_env("OMNIDRIVE_DOWNLOAD_ATTEMPT_TIMEOUT_MS", 90_000);
        let connect_timeout = duration_from_env("OMNIDRIVE_DOWNLOAD_CONNECT_TIMEOUT_MS", 10_000);
        let read_timeout = duration_from_env("OMNIDRIVE_DOWNLOAD_READ_TIMEOUT_MS", 90_000);
        let timeout_config = TimeoutConfig::builder()
            .connect_timeout(connect_timeout)
            .read_timeout(read_timeout)
            .operation_attempt_timeout(operation_attempt_timeout)
            .operation_timeout(operation_timeout)
            .build();

        let shared_config = crate::aws_http::load_shared_config(
            Region::new(config.region.clone()),
            timeout_config.clone(),
        )
        .await;

        let s3_config = aws_sdk_s3::config::Builder::from(&shared_config)
            .credentials_provider(Credentials::new(
                config.access_key_id,
                config.secret_access_key,
                None,
                None,
                provider_name,
            ))
            .endpoint_url(config.endpoint)
            .region(Region::new(config.region))
            .timeout_config(timeout_config)
            .force_path_style(config.force_path_style)
            .build();

        Ok(Self {
            provider_name,
            bucket: config.bucket,
            client: Client::from_conf(s3_config),
        })
    }
}

fn reconstruct_ciphertext(
    pack: &db::PackRecord,
    shards: &mut [Option<Vec<u8>>],
) -> Result<Vec<u8>, DownloaderError> {
    if shards.len() != TOTAL_SHARDS {
        return Err(DownloaderError::InvalidPackRecord("shard count mismatch"));
    }

    let shard_len = to_usize(pack.shard_size, "shard size")?;
    for shard in shards.iter_mut() {
        if let Some(bytes) = shard.as_mut() {
            if bytes.len() != shard_len {
                return Err(DownloaderError::InvalidPackRecord("shard size mismatch"));
            }
        }
    }

    let reed_solomon = ReedSolomon::new(DATA_SHARDS, PARITY_SHARDS)?;
    reed_solomon.reconstruct(shards)?;

    let mut ciphertext = Vec::with_capacity(to_usize(pack.cipher_size, "cipher size")?);
    for shard in shards.iter().take(DATA_SHARDS) {
        let bytes = shard.as_ref().ok_or(DownloaderError::InvalidPackRecord(
            "missing data shard after reconstruct",
        ))?;
        ciphertext.extend_from_slice(bytes);
    }

    let cipher_size = to_usize(pack.cipher_size, "cipher size")?;
    if ciphertext.len() < cipher_size {
        return Err(DownloaderError::InvalidPackRecord(
            "reconstructed ciphertext shorter than expected",
        ));
    }
    ciphertext.truncate(cipher_size);
    Ok(ciphertext)
}

fn build_manifest_bytes(
    pack: &db::PackRecord,
    ciphertext: &[u8],
) -> Result<Vec<u8>, DownloaderError> {
    let chunk_id = vec_to_chunk_id(&pack.chunk_id)?;
    let nonce = vec_to_nonce(&pack.nonce)?;
    let gcm_tag = vec_to_gcm_tag(&pack.gcm_tag)?;
    let plain_len = u64::try_from(pack.logical_size)
        .map_err(|_| DownloaderError::NumericOverflow("logical size"))?;

    let prefix = ChunkRecordPrefix {
        record_magic: CHUNK_RECORD_MAGIC,
        record_version: u8::try_from(pack.encryption_version)
            .map_err(|_| DownloaderError::NumericOverflow("encryption version"))?,
        flags: 0,
        compression_algo: COMPRESSION_ALGO_NONE,
        reserved_0: 0,
        chunk_id,
        plain_len: U64::new(plain_len),
        cipher_len: U64::new(ciphertext.len() as u64),
        nonce,
        reserved_1: [0u8; 12],
    };

    let mut bytes = Vec::with_capacity(ChunkRecordPrefix::SIZE + ciphertext.len() + gcm_tag.len());
    bytes.extend_from_slice(prefix.as_bytes());
    bytes.extend_from_slice(ciphertext);
    bytes.extend_from_slice(&gcm_tag);
    Ok(bytes)
}

fn decrypt_chunk_record(
    pack_bytes: &[u8],
    chunk: &db::FileChunkLocation,
    vault_key: &KeyBytes,
) -> Result<Vec<u8>, DownloaderError> {
    let pack_offset = to_usize(chunk.pack_offset, "pack offset")?;
    let encrypted_size = to_usize(chunk.encrypted_size, "encrypted size")?;
    let record_end = pack_offset
        .checked_add(encrypted_size)
        .ok_or(DownloaderError::NumericOverflow("record end"))?;

    if record_end > pack_bytes.len() || encrypted_size < ChunkRecordPrefix::SIZE {
        return Err(DownloaderError::InvalidPackRecord("record bounds"));
    }

    let record = &pack_bytes[pack_offset..record_end];
    if record[..4] != CHUNK_RECORD_MAGIC {
        return Err(DownloaderError::InvalidPackRecord("chunk magic"));
    }

    let expected_chunk_id = vec_to_chunk_id(&chunk.chunk_id)?;
    let actual_chunk_id = vec_to_chunk_id(&record[8..40])?;
    if actual_chunk_id != expected_chunk_id {
        return Err(DownloaderError::InvalidPackRecord("chunk_id mismatch"));
    }

    let plain_len = u64::from_be_bytes(
        record[40..48]
            .try_into()
            .map_err(|_| DownloaderError::InvalidPackRecord("plain_len"))?,
    );
    let cipher_len = u64::from_be_bytes(
        record[48..56]
            .try_into()
            .map_err(|_| DownloaderError::InvalidPackRecord("cipher_len"))?,
    );
    let cipher_len_usize = usize::try_from(cipher_len)
        .map_err(|_| DownloaderError::NumericOverflow("cipher length"))?;
    let expected_record_size = ChunkRecordPrefix::SIZE
        .checked_add(cipher_len_usize)
        .and_then(|value| value.checked_add(ChunkRecordPrefix::GCM_TAG_SIZE))
        .ok_or(DownloaderError::NumericOverflow("record size"))?;
    if expected_record_size != encrypted_size {
        return Err(DownloaderError::InvalidPackRecord(
            "encrypted size mismatch",
        ));
    }

    let ciphertext_start = ChunkRecordPrefix::SIZE;
    let ciphertext_end = ciphertext_start + cipher_len_usize;
    let tag_end = ciphertext_end + ChunkRecordPrefix::GCM_TAG_SIZE;
    let ciphertext = &record[ciphertext_start..ciphertext_end];
    let gcm_tag: GcmTag = record[ciphertext_end..tag_end]
        .try_into()
        .map_err(|_| DownloaderError::InvalidPackRecord("gcm tag"))?;

    let plaintext = decrypt_chunk(vault_key, &expected_chunk_id, &[], ciphertext, &gcm_tag)?;
    if plaintext.len() as i64 != chunk.size || plaintext.len() as u64 != plain_len {
        return Err(DownloaderError::InvalidPackRecord("plain size mismatch"));
    }

    Ok(plaintext)
}

fn vec_to_chunk_id(bytes: &[u8]) -> Result<ChunkId, DownloaderError> {
    bytes
        .try_into()
        .map_err(|_| DownloaderError::InvalidPackRecord("chunk id length"))
}

fn vec_to_nonce(bytes: &[u8]) -> Result<[u8; 12], DownloaderError> {
    bytes
        .try_into()
        .map_err(|_| DownloaderError::InvalidPackRecord("nonce length"))
}

fn vec_to_gcm_tag(bytes: &[u8]) -> Result<GcmTag, DownloaderError> {
    bytes
        .try_into()
        .map_err(|_| DownloaderError::InvalidPackRecord("gcm tag length"))
}

fn env_path(key: &str, default: &str) -> PathBuf {
    env::var(key)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(default))
}

fn duration_from_env(key: &str, default_ms: u64) -> Duration {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(default_ms))
}

fn to_usize(value: i64, context: &'static str) -> Result<usize, DownloaderError> {
    usize::try_from(value).map_err(|_| DownloaderError::NumericOverflow(context))
}

fn to_u64(value: i64, context: &'static str) -> Result<u64, DownloaderError> {
    u64::try_from(value).map_err(|_| DownloaderError::NumericOverflow(context))
}

fn format_error_details(err: &(impl std::error::Error + fmt::Debug)) -> String {
    let mut details = vec![format!("display={err}"), format!("debug={err:?}")];
    let mut current = err.source();
    let mut depth = 0usize;
    while let Some(source) = current {
        depth += 1;
        details.push(format!("source[{depth}]={source}"));
        current = source.source();
    }
    details.join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packer::{DEFAULT_CHUNK_SIZE, Packer, PackerConfig};
    use crate::vault::VaultKeyStore;
    use axum::Router;
    use axum::body::Bytes;
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::put;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    #[derive(Clone)]
    struct MockS3State {
        objects: Arc<Mutex<HashMap<(String, String), Vec<u8>>>>,
        head_delay_by_bucket: Arc<HashMap<String, Duration>>,
    }

    #[tokio::test]
    async fn roundtrip_pack_upload_download_restore_file() -> Result<(), Box<dyn std::error::Error>>
    {
        let test_root = env::temp_dir().join(format!(
            "omnidrive-downloader-test-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let upload_spool_dir = test_root.join("upload-spool");
        let download_spool_dir = test_root.join("download-spool");
        let source_path = test_root.join("source.bin");
        let restored_path = test_root.join("restored.bin");
        let payload = vec![0x5Au8; DEFAULT_CHUNK_SIZE + 777];
        let vault_key = [0x33; 32];

        fs::create_dir_all(&upload_spool_dir).await?;
        fs::create_dir_all(&download_spool_dir).await?;
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
        vault_keys.set_key_for_tests(vault_key).await;
        let packer = Packer::new(
            pool.clone(),
            vault_keys.clone(),
            PackerConfig::new(&upload_spool_dir),
        )?;
        let pack_result = packer.pack_file(inode_id, &source_path).await?;

        let state = MockS3State {
            objects: Arc::new(Mutex::new(HashMap::new())),
            head_delay_by_bucket: Arc::new(HashMap::from([
                ("bucket-r2".to_string(), Duration::from_millis(30)),
                ("bucket-scaleway".to_string(), Duration::from_millis(200)),
                ("bucket-b2".to_string(), Duration::from_millis(20)),
            ])),
        };
        let app = Router::new()
            .route(
                "/{*path}",
                put(mock_put_object)
                    .get(mock_get_object)
                    .head(mock_head_object),
            )
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let configs = vec![
            ProviderConfig {
                provider_name: "cloudflare-r2",
                endpoint: format!("http://{addr}"),
                region: "auto".to_string(),
                bucket: "bucket-r2".to_string(),
                access_key_id: "test".to_string(),
                secret_access_key: "test".to_string(),
                force_path_style: true,
            },
            ProviderConfig {
                provider_name: "scaleway",
                endpoint: format!("http://{addr}"),
                region: "pl-waw".to_string(),
                bucket: "bucket-scaleway".to_string(),
                access_key_id: "test".to_string(),
                secret_access_key: "test".to_string(),
                force_path_style: true,
            },
            ProviderConfig {
                provider_name: "backblaze-b2",
                endpoint: format!("http://{addr}"),
                region: "eu-central-003".to_string(),
                bucket: "bucket-b2".to_string(),
                access_key_id: "test".to_string(),
                secret_access_key: "test".to_string(),
                force_path_style: true,
            },
        ];

        for pack_id in &pack_result.pack_ids {
            let shards = db::get_pack_shards(&pool, pack_id).await?;
            for shard in shards {
                if shard.provider == "scaleway" {
                    continue;
                }

                let local_path =
                    download_spool_dir.join(format!("seed-{}-{}.bin", pack_id, shard.shard_index));
                let upload_path =
                    upload_spool_dir.join(format!("{pack_id}.download-shard{}", shard.shard_index));
                let packer_shard_path =
                    upload_spool_dir.join(format!("{pack_id}.shard{}", shard.shard_index));
                let bytes = fs::read(&packer_shard_path).await?;
                fs::write(&local_path, &bytes).await?;
                state.objects.lock().await.insert(
                    (
                        provider_bucket(&shard.provider).to_string(),
                        shard.object_key.clone(),
                    ),
                    bytes,
                );
                let _ = fs::remove_file(&upload_path).await;
            }
        }

        let downloader = Downloader::from_provider_configs(
            pool.clone(),
            vault_keys,
            &download_spool_dir,
            Duration::from_secs(30),
            configs,
        )
        .await?;
        let restored = downloader.restore_file(inode_id, &restored_path).await?;
        let restored_bytes = fs::read(&restored_path).await?;

        assert_eq!(restored_bytes, payload);
        assert_eq!(restored.bytes_written, payload.len() as u64);
        assert_eq!(restored.pack_sources.len(), pack_result.pack_ids.len());
        assert!(
            restored
                .pack_sources
                .iter()
                .all(|source| source.providers.len() >= 2)
        );

        let current_revision = db::get_current_file_revision(&pool, inode_id)
            .await?
            .expect("current revision");
        let range_offset = (DEFAULT_CHUNK_SIZE as u64) - 123;
        let range_length = 512u64;
        let range_bytes = downloader
            .read_range(
                inode_id,
                current_revision.revision_id,
                range_offset,
                range_length,
            )
            .await?;
        assert_eq!(
            range_bytes,
            payload[range_offset as usize..(range_offset + range_length) as usize]
        );

        server.abort();
        let _ = fs::remove_dir_all(&test_root).await;
        Ok(())
    }

    async fn mock_put_object(
        State(state): State<MockS3State>,
        Path(path): Path<String>,
        body: Bytes,
    ) -> impl IntoResponse {
        let (bucket, key) = split_bucket_and_key(&path);
        state
            .objects
            .lock()
            .await
            .insert((bucket.to_string(), key.to_string()), body.to_vec());
        StatusCode::OK
    }

    async fn mock_head_object(
        State(state): State<MockS3State>,
        Path(path): Path<String>,
    ) -> impl IntoResponse {
        let (bucket, key) = split_bucket_and_key(&path);
        if let Some(delay) = state.head_delay_by_bucket.get(bucket) {
            tokio::time::sleep(*delay).await;
        }

        let objects = state.objects.lock().await;
        if let Some(bytes) = objects.get(&(bucket.to_string(), key.to_string())) {
            (
                StatusCode::OK,
                [("content-length", bytes.len().to_string())],
            )
                .into_response()
        } else {
            StatusCode::NOT_FOUND.into_response()
        }
    }

    async fn mock_get_object(
        State(state): State<MockS3State>,
        Path(path): Path<String>,
    ) -> impl IntoResponse {
        let (bucket, key) = split_bucket_and_key(&path);
        let objects = state.objects.lock().await;
        if let Some(bytes) = objects.get(&(bucket.to_string(), key.to_string())) {
            (StatusCode::OK, bytes.clone()).into_response()
        } else {
            StatusCode::NOT_FOUND.into_response()
        }
    }

    fn split_bucket_and_key(path: &str) -> (&str, &str) {
        let trimmed = path.trim_start_matches('/');
        let mut segments = trimmed.splitn(2, '/');
        let bucket = segments.next().unwrap_or_default();
        let key = segments.next().unwrap_or_default();
        (bucket, key)
    }

    fn provider_bucket(provider: &str) -> &'static str {
        match provider {
            "cloudflare-r2" => "bucket-r2",
            "scaleway" => "bucket-scaleway",
            "backblaze-b2" => "bucket-b2",
            _ => "bucket-unknown",
        }
    }
}
