#![allow(dead_code)]

use crate::db;
use crate::diagnostics::{self, WorkerKind, WorkerStatus};
use crate::uploader::ProviderConfig;
use aws_config::timeout::TimeoutConfig;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::warn;

pub struct ScrubberWorker {
    pool: SqlitePool,
    providers: HashMap<String, ScrubProvider>,
    poll_interval: Duration,
    provider_timeout: Duration,
    batch_size: i64,
    deep_verify_modulus: usize,
}

struct ScrubProvider {
    provider_name: &'static str,
    bucket: String,
    client: Client,
}

#[derive(Debug)]
pub enum ScrubberError {
    MissingProviderConfig,
    InvalidEnv(&'static str),
    Db(sqlx::Error),
    Timeout {
        provider: &'static str,
        duration: Duration,
    },
}

impl fmt::Display for ScrubberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProviderConfig => write!(f, "no scrub providers configured"),
            Self::InvalidEnv(key) => write!(f, "invalid environment variable {key}"),
            Self::Db(err) => write!(f, "sqlite error: {err}"),
            Self::Timeout { provider, duration } => {
                write!(f, "scrub operation to {provider} timed out after {:?}", duration)
            }
        }
    }
}

impl std::error::Error for ScrubberError {}

impl From<sqlx::Error> for ScrubberError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

impl ScrubberWorker {
    pub async fn from_env(pool: SqlitePool) -> Result<Self, ScrubberError> {
        let _ = dotenvy::dotenv();
        Self::from_onboarding_db(pool).await
    }

    pub async fn from_onboarding_db(pool: SqlitePool) -> Result<Self, ScrubberError> {
        let providers = provider_clients_from_onboarding_db(&pool).await?;
        Ok(Self {
            pool,
            providers,
            poll_interval: duration_from_env("OMNIDRIVE_SCRUB_POLL_INTERVAL_MS", 300_000),
            provider_timeout: duration_from_env("OMNIDRIVE_SCRUB_TIMEOUT_MS", 30_000),
            batch_size: env::var("OMNIDRIVE_SCRUB_BATCH_SIZE")
                .ok()
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(16),
            deep_verify_modulus: env::var("OMNIDRIVE_SCRUB_DEEP_MODULUS")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value > 0)
                .unwrap_or(20),
        })
    }

    pub async fn run(self) -> Result<(), ScrubberError> {
        diagnostics::set_worker_status(WorkerKind::Scrubber, WorkerStatus::Idle);
        loop {
            diagnostics::set_worker_status(WorkerKind::Scrubber, WorkerStatus::Active);
            let processed = self.run_one_batch().await?;
            if processed == 0 {
                diagnostics::set_worker_status(WorkerKind::Scrubber, WorkerStatus::Idle);
                sleep(self.poll_interval).await;
                continue;
            }

            diagnostics::set_worker_status(WorkerKind::Scrubber, WorkerStatus::Idle);
            sleep(self.poll_interval).await;
        }
    }

    pub async fn run_one_batch(&self) -> Result<usize, ScrubberError> {
        let batch = db::get_next_shards_for_scrub(&self.pool, self.batch_size).await?;
        if batch.is_empty() {
            return Ok(0);
        }

        let mut processed = 0usize;
        for (idx, shard) in batch.into_iter().enumerate() {
            processed += 1;
            let use_deep = self.should_deep_verify(&shard, idx);
            if let Err(err) = self.verify_shard(&shard, use_deep).await {
                warn!(
                    "scrubber verification error pack={} shard={} provider={}: {}",
                    shard.pack_id, shard.shard_index, shard.provider, err
                );
            }
        }

        Ok(processed)
    }

    async fn verify_shard(
        &self,
        shard: &db::ScrubShardRecord,
        use_deep: bool,
    ) -> Result<(), ScrubberError> {
        let Some(provider) = self.providers.get(&shard.provider) else {
            return Err(ScrubberError::InvalidEnv("scrub provider not configured"));
        };

        let head_result = timeout(
            self.provider_timeout,
            provider
                .client
                .head_object()
                .bucket(&provider.bucket)
                .key(&shard.object_key)
                .send(),
        )
        .await;

        match head_result {
            Ok(Ok(output)) => {
                let content_length = output.content_length().unwrap_or_default();
                if content_length != shard.size {
                    db::update_shard_verification_status(
                        &self.pool,
                        &shard.pack_id,
                        shard.shard_index,
                        "LIGHT",
                        "SIZE_MISMATCH",
                        Some(content_length),
                        true,
                        Some("provider content-length does not match recorded shard size"),
                    )
                    .await?;
                } else {
                    if use_deep {
                        self.verify_shard_integrity_deep(shard, provider).await?;
                    } else {
                        db::update_shard_verification_status(
                            &self.pool,
                            &shard.pack_id,
                            shard.shard_index,
                            "LIGHT",
                            "HEALTHY",
                            Some(content_length),
                            false,
                            None,
                        )
                        .await?;
                    }
                }
            }
            Ok(Err(err)) => {
                let details = format_error_details(&err);
                if is_missing_error(&details) {
                    db::update_shard_verification_status(
                        &self.pool,
                        &shard.pack_id,
                        shard.shard_index,
                        "LIGHT",
                        "MISSING",
                        None,
                        true,
                        Some(&details),
                    )
                    .await?;
                } else if is_transient_error(&details) {
                    warn!(
                        "scrubber transient error pack={} shard={} provider={}: {}",
                        shard.pack_id, shard.shard_index, provider.provider_name, details
                    );
                    return Ok(());
                } else {
                    warn!(
                        "scrubber non-integrity error pack={} shard={} provider={}: {}",
                        shard.pack_id, shard.shard_index, provider.provider_name, details
                    );
                    return Ok(());
                }
            }
            Err(_) => {
                warn!(
                    "scrubber timeout pack={} shard={} provider={}",
                    shard.pack_id, shard.shard_index, provider.provider_name
                );
                return Ok(());
            }
        }

        let summary = db::summarize_pack_shards(&self.pool, &shard.pack_id).await?;
        let pack = db::get_pack(&self.pool, &shard.pack_id).await?;
        let storage_mode = pack
            .as_ref()
            .map(|pack| db::StorageMode::from_str(&pack.storage_mode))
            .unwrap_or(db::StorageMode::Ec2_1);
        let pack_status = db::resolve_pack_status_for_mode(storage_mode, summary);
        db::update_pack_status(&self.pool, &shard.pack_id, pack_status).await?;
        Ok(())
    }

    async fn verify_shard_integrity_deep(
        &self,
        shard: &db::ScrubShardRecord,
        provider: &ScrubProvider,
    ) -> Result<(), ScrubberError> {
        let response = timeout(
            self.provider_timeout,
            provider
                .client
                .get_object()
                .bucket(&provider.bucket)
                .key(&shard.object_key)
                .send(),
        )
        .await;

        match response {
            Ok(Ok(output)) => {
                let body = match output.body.collect().await {
                    Ok(body) => body,
                    Err(err) => {
                        let details = format_error_details(&err);
                        if is_transient_error(&details) {
                            warn!(
                                "scrubber deep body transient error pack={} shard={} provider={}: {}",
                                shard.pack_id, shard.shard_index, provider.provider_name, details
                            );
                        } else {
                            warn!(
                                "scrubber deep body error pack={} shard={} provider={}: {}",
                                shard.pack_id, shard.shard_index, provider.provider_name, details
                            );
                        }
                        return Ok(());
                    }
                };
                let bytes = body.into_bytes();
                let verified_size = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
                let checksum = hex_sha256(&bytes);

                if checksum == shard.checksum {
                    db::update_shard_verification_status(
                        &self.pool,
                        &shard.pack_id,
                        shard.shard_index,
                        "DEEP",
                        "HEALTHY",
                        Some(verified_size),
                        false,
                        None,
                    )
                    .await?;
                } else {
                    db::update_shard_verification_status(
                        &self.pool,
                        &shard.pack_id,
                        shard.shard_index,
                        "DEEP",
                        "CORRUPTED",
                        Some(verified_size),
                        true,
                        Some("deep checksum mismatch"),
                    )
                    .await?;
                }
                Ok(())
            }
            Ok(Err(err)) => {
                let details = format_error_details(&err);
                if is_missing_error(&details) {
                    db::update_shard_verification_status(
                        &self.pool,
                        &shard.pack_id,
                        shard.shard_index,
                        "DEEP",
                        "MISSING",
                        None,
                        true,
                        Some(&details),
                    )
                    .await?;
                } else if is_transient_error(&details) {
                    warn!(
                        "scrubber deep transient error pack={} shard={} provider={}: {}",
                        shard.pack_id, shard.shard_index, provider.provider_name, details
                    );
                } else {
                    warn!(
                        "scrubber deep non-integrity error pack={} shard={} provider={}: {}",
                        shard.pack_id, shard.shard_index, provider.provider_name, details
                    );
                }
                Ok(())
            }
            Err(_) => {
                warn!(
                    "scrubber deep timeout pack={} shard={} provider={}",
                    shard.pack_id, shard.shard_index, provider.provider_name
                );
                Ok(())
            }
        }
    }

    fn should_deep_verify(&self, shard: &db::ScrubShardRecord, batch_index: usize) -> bool {
        batch_index % self.deep_verify_modulus == 0
            || usize::try_from(shard.id)
                .ok()
                .is_some_and(|id| id % self.deep_verify_modulus == 0)
    }
}

pub async fn run_scrub_batch_now(pool: SqlitePool) -> Result<usize, ScrubberError> {
    let worker = ScrubberWorker::from_onboarding_db(pool).await?;
    worker.run_one_batch().await
}

async fn provider_clients_from_env() -> Result<HashMap<String, ScrubProvider>, ScrubberError> {
    provider_clients_from_configs(
        [
            ProviderConfig::from_r2_env(),
            ProviderConfig::from_scaleway_env(),
            ProviderConfig::from_b2_env(),
        ]
        .into_iter()
        .filter_map(Result::ok)
        .collect(),
    )
    .await
}

async fn provider_clients_from_onboarding_db(
    pool: &SqlitePool,
) -> Result<HashMap<String, ScrubProvider>, ScrubberError> {
    let configs = crate::onboarding::get_active_provider_configs(pool)
        .await
        .map_err(|_| ScrubberError::MissingProviderConfig)?;
    provider_clients_from_configs(configs).await
}

async fn provider_clients_from_configs(
    configs: Vec<ProviderConfig>,
) -> Result<HashMap<String, ScrubProvider>, ScrubberError> {
    let mut clients = HashMap::new();
    for config in configs {
        let provider = ScrubProvider::from_provider_config(config).await?;
        clients.insert(provider.provider_name.to_string(), provider);
    }
    if clients.is_empty() {
        return Err(ScrubberError::MissingProviderConfig);
    }
    Ok(clients)
}

impl ScrubProvider {
    async fn from_provider_config(config: ProviderConfig) -> Result<Self, ScrubberError> {
        let provider_name = config.provider_name;
        let operation_timeout = duration_from_env("OMNIDRIVE_SCRUB_TIMEOUT_MS", 30_000);
        let operation_attempt_timeout =
            duration_from_env("OMNIDRIVE_SCRUB_ATTEMPT_TIMEOUT_MS", 25_000);
        let connect_timeout = duration_from_env("OMNIDRIVE_SCRUB_CONNECT_TIMEOUT_MS", 8_000);
        let read_timeout = duration_from_env("OMNIDRIVE_SCRUB_READ_TIMEOUT_MS", 25_000);
        let timeout_config = TimeoutConfig::builder()
            .connect_timeout(connect_timeout)
            .read_timeout(read_timeout)
            .operation_attempt_timeout(operation_attempt_timeout)
            .operation_timeout(operation_timeout)
            .build();

        let shared_config = crate::aws_http::load_shared_config(
            Region::new(config.region.clone()),
            timeout_config.clone(),
            config.endpoint.starts_with("http://"),
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
            .region(Region::new(config.region.clone()))
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

fn duration_from_env(key: &str, default_ms: u64) -> Duration {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(default_ms))
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

fn is_missing_error(details: &str) -> bool {
    let lower = details.to_ascii_lowercase();
    lower.contains("404")
        || lower.contains("410")
        || lower.contains("notfound")
        || lower.contains("no such key")
        || lower.contains("nosuchkey")
        || lower.contains("resource not found")
}

fn is_transient_error(details: &str) -> bool {
    let lower = details.to_ascii_lowercase();
    lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("dns")
        || lower.contains("tls")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("503")
        || lower.contains("502")
        || lower.contains("500")
        || lower.contains("service unavailable")
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}
