#![allow(dead_code)]

use crate::db;
use crate::db::PackStatus;
use crate::packer::{DATA_SHARDS, PARITY_SHARDS, TOTAL_SHARDS, local_shard_path};
use crate::uploader::ProviderConfig;
use aws_config::timeout::TimeoutConfig;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use reed_solomon_erasure::galois_8::ReedSolomon;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;
use tokio::time::{sleep, timeout};

pub struct RepairWorker {
    pool: SqlitePool,
    providers: HashMap<String, ProviderClient>,
    spool_dir: PathBuf,
    poll_interval: Duration,
    provider_timeout: Duration,
    retry_delay: Duration,
}

struct ProviderClient {
    provider_name: &'static str,
    bucket: String,
    client: Client,
}

#[derive(Debug)]
pub enum RepairError {
    MissingProviderConfig,
    InvalidEnv(&'static str),
    Io(std::io::Error),
    Db(sqlx::Error),
    ErasureCoding(reed_solomon_erasure::Error),
    Timeout {
        provider: &'static str,
        duration: Duration,
    },
    Provider {
        provider: &'static str,
        operation: &'static str,
        details: String,
    },
    MissingShardRecord(&'static str),
    InvalidShardLayout(String),
    NumericOverflow(&'static str),
}

impl fmt::Display for RepairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProviderConfig => write!(f, "no repair providers configured"),
            Self::InvalidEnv(key) => write!(f, "invalid environment variable {key}"),
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::Db(err) => write!(f, "sqlite error: {err}"),
            Self::ErasureCoding(err) => write!(f, "erasure coding error: {err}"),
            Self::Timeout { provider, duration } => {
                write!(
                    f,
                    "repair operation to {provider} timed out after {:?}",
                    duration
                )
            }
            Self::Provider {
                provider,
                operation,
                details,
            } => write!(f, "{provider} {operation} failed: {details}"),
            Self::MissingShardRecord(reason) => write!(f, "missing shard record: {reason}"),
            Self::InvalidShardLayout(pack_id) => {
                write!(f, "invalid shard layout for pack {pack_id}")
            }
            Self::NumericOverflow(ctx) => write!(f, "numeric overflow while handling {ctx}"),
        }
    }
}

impl std::error::Error for RepairError {}

impl From<std::io::Error> for RepairError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<sqlx::Error> for RepairError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

impl From<reed_solomon_erasure::Error> for RepairError {
    fn from(value: reed_solomon_erasure::Error) -> Self {
        Self::ErasureCoding(value)
    }
}

impl RepairWorker {
    pub async fn from_env(pool: SqlitePool) -> Result<Self, RepairError> {
        let _ = dotenvy::dotenv();

        let providers = provider_clients_from_env().await?;
        Ok(Self {
            pool,
            providers,
            spool_dir: env_path("OMNIDRIVE_SPOOL_DIR", ".omnidrive/spool"),
            poll_interval: duration_from_env("OMNIDRIVE_REPAIR_POLL_INTERVAL_MS", 5_000),
            provider_timeout: duration_from_env("OMNIDRIVE_REPAIR_TIMEOUT_MS", 120_000),
            retry_delay: duration_from_env("OMNIDRIVE_REPAIR_RETRY_DELAY_MS", 10_000),
        })
    }

    pub async fn run(self) -> Result<(), RepairError> {
        db::reset_in_progress_pack_shards(&self.pool).await?;

        loop {
            let Some(pack) = db::get_next_degraded_pack(&self.pool).await? else {
                sleep(self.poll_interval).await;
                continue;
            };

            if !db::pack_requires_healthy(&self.pool, &pack.pack_id).await? {
                sleep(self.poll_interval).await;
                continue;
            }

            match self.repair_pack(&pack).await {
                Ok(()) => {
                    println!("repair worker restored pack {} to healthy", pack.pack_id);
                }
                Err(err) => {
                    eprintln!("repair worker failed for pack {}: {}", pack.pack_id, err);
                    sleep(self.retry_delay).await;
                }
            }
        }
    }

    async fn repair_pack(&self, pack: &db::PackRecord) -> Result<(), RepairError> {
        let shards = db::get_pack_shards(&self.pool, &pack.pack_id).await?;
        if shards.len() != TOTAL_SHARDS {
            return Err(RepairError::InvalidShardLayout(pack.pack_id.clone()));
        }

        let summary = db::summarize_pack_shards(&self.pool, &pack.pack_id).await?;
        let status = db::resolve_pack_status(summary);
        match status {
            PackStatus::Healthy => return Ok(()),
            PackStatus::Unreadable => {
                db::update_pack_status(&self.pool, &pack.pack_id, PackStatus::Unreadable).await?;
                return Ok(());
            }
            PackStatus::Uploading | PackStatus::Degraded => {}
        }

        let mut completed = Vec::new();
        let mut missing = None;
        for shard in shards {
            if shard.status == "COMPLETED" {
                completed.push(shard);
            } else if missing.is_none() {
                missing = Some(shard);
            }
        }

        if completed.len() < DATA_SHARDS {
            db::update_pack_status(&self.pool, &pack.pack_id, PackStatus::Unreadable).await?;
            return Ok(());
        }

        let missing = missing.ok_or(RepairError::MissingShardRecord("no missing shard"))?;
        let missing_index = usize::try_from(missing.shard_index)
            .map_err(|_| RepairError::NumericOverflow("missing shard index"))?;
        let shard_len = usize::try_from(pack.shard_size)
            .map_err(|_| RepairError::NumericOverflow("pack shard size"))?;
        let mut shard_set: Vec<Option<Vec<u8>>> = vec![None; TOTAL_SHARDS];

        for shard in &completed {
            let provider = self
                .providers
                .get(&shard.provider)
                .ok_or_else(|| RepairError::InvalidEnv("repair provider not configured"))?;
            let bytes = self
                .download_shard(
                    provider,
                    &shard.object_key,
                    &pack.pack_id,
                    shard.shard_index,
                )
                .await?;
            if bytes.len() != shard_len {
                return Err(RepairError::InvalidShardLayout(pack.pack_id.clone()));
            }
            let shard_index = usize::try_from(shard.shard_index)
                .map_err(|_| RepairError::NumericOverflow("completed shard index"))?;
            shard_set[shard_index] = Some(bytes);
        }

        let reed_solomon = ReedSolomon::new(DATA_SHARDS, PARITY_SHARDS)?;
        reed_solomon.reconstruct(&mut shard_set)?;

        let reconstructed =
            shard_set[missing_index]
                .clone()
                .ok_or(RepairError::MissingShardRecord(
                    "reconstructed shard missing",
                ))?;
        let provider = self
            .providers
            .get(&missing.provider)
            .ok_or_else(|| RepairError::InvalidEnv("missing shard provider not configured"))?;

        db::mark_pack_shard_in_progress(&self.pool, &pack.pack_id, missing.shard_index).await?;
        if let Some(job) = db::get_upload_job_by_pack_id(&self.pool, &pack.pack_id).await? {
            db::mark_upload_target_in_progress(&self.pool, job.id, &missing.provider).await?;
        }

        let upload_result = timeout(
            self.provider_timeout,
            self.upload_shard(
                provider,
                &missing.object_key,
                &reconstructed,
                &pack.pack_id,
                missing_index,
            ),
        )
        .await;

        match upload_result {
            Ok(Ok((etag, version_id))) => {
                db::mark_pack_shard_completed(&self.pool, &pack.pack_id, missing.shard_index)
                    .await?;
                db::update_pack_status(&self.pool, &pack.pack_id, PackStatus::Healthy).await?;

                if let Some(job) = db::get_upload_job_by_pack_id(&self.pool, &pack.pack_id).await? {
                    db::mark_upload_target_completed(
                        &self.pool,
                        job.id,
                        &missing.provider,
                        &provider.bucket,
                        &missing.object_key,
                        etag.as_deref(),
                        version_id.as_deref(),
                    )
                    .await?;
                    db::mark_upload_job_completed(&self.pool, job.id).await?;
                }
            }
            Ok(Err(err)) => {
                db::requeue_pack_shard(
                    &self.pool,
                    &pack.pack_id,
                    missing.shard_index,
                    &err.to_string(),
                )
                .await?;
                if let Some(job) = db::get_upload_job_by_pack_id(&self.pool, &pack.pack_id).await? {
                    db::requeue_upload_target(
                        &self.pool,
                        job.id,
                        &missing.provider,
                        &err.to_string(),
                    )
                    .await?;
                }
                return Err(err);
            }
            Err(_) => {
                let err = RepairError::Timeout {
                    provider: provider.provider_name,
                    duration: self.provider_timeout,
                };
                db::requeue_pack_shard(
                    &self.pool,
                    &pack.pack_id,
                    missing.shard_index,
                    &err.to_string(),
                )
                .await?;
                if let Some(job) = db::get_upload_job_by_pack_id(&self.pool, &pack.pack_id).await? {
                    db::requeue_upload_target(
                        &self.pool,
                        job.id,
                        &missing.provider,
                        &err.to_string(),
                    )
                    .await?;
                }
                return Err(err);
            }
        }

        Ok(())
    }

    async fn download_shard(
        &self,
        provider: &ProviderClient,
        object_key: &str,
        pack_id: &str,
        shard_index: i64,
    ) -> Result<Vec<u8>, RepairError> {
        let response = timeout(
            self.provider_timeout,
            provider
                .client
                .get_object()
                .bucket(&provider.bucket)
                .key(object_key)
                .send(),
        )
        .await
        .map_err(|_| RepairError::Timeout {
            provider: provider.provider_name,
            duration: self.provider_timeout,
        })?
        .map_err(|err| provider_error(provider.provider_name, "get_object", err))?;

        let body = response
            .body
            .collect()
            .await
            .map_err(|err| provider_error(provider.provider_name, "read_body", err))?;
        let bytes = body.into_bytes().to_vec();

        let shard_path = local_shard_path(
            &self.spool_dir,
            pack_id,
            usize::try_from(shard_index)
                .map_err(|_| RepairError::NumericOverflow("repair shard index"))?,
        );
        fs::write(shard_path, &bytes).await?;

        Ok(bytes)
    }

    async fn upload_shard(
        &self,
        provider: &ProviderClient,
        object_key: &str,
        bytes: &[u8],
        pack_id: &str,
        shard_index: usize,
    ) -> Result<(Option<String>, Option<String>), RepairError> {
        let shard_path = local_shard_path(&self.spool_dir, pack_id, shard_index);
        fs::write(&shard_path, bytes).await?;

        let body = ByteStream::from_path(&shard_path)
            .await
            .map_err(|err| provider_error(provider.provider_name, "read_body", err))?;
        let response = provider
            .client
            .put_object()
            .bucket(&provider.bucket)
            .key(object_key)
            .body(body)
            .content_type("application/octet-stream")
            .send()
            .await
            .map_err(|err| provider_error(provider.provider_name, "put_object", err))?;

        Ok((response.e_tag, response.version_id))
    }
}

async fn provider_clients_from_env() -> Result<HashMap<String, ProviderClient>, RepairError> {
    let mut clients = HashMap::new();
    for config in [
        ProviderConfig::from_r2_env(),
        ProviderConfig::from_scaleway_env(),
        ProviderConfig::from_b2_env(),
    ] {
        let config = config.map_err(|_| RepairError::MissingProviderConfig)?;
        let client = ProviderClient::from_provider_config(config).await?;
        clients.insert(client.provider_name.to_string(), client);
    }

    if clients.is_empty() {
        return Err(RepairError::MissingProviderConfig);
    }

    Ok(clients)
}

impl ProviderClient {
    async fn from_provider_config(config: ProviderConfig) -> Result<Self, RepairError> {
        let provider_name = config.provider_name;
        let operation_timeout = duration_from_env("OMNIDRIVE_REPAIR_TIMEOUT_MS", 120_000);
        let operation_attempt_timeout =
            duration_from_env("OMNIDRIVE_REPAIR_ATTEMPT_TIMEOUT_MS", 90_000);
        let connect_timeout = duration_from_env("OMNIDRIVE_REPAIR_CONNECT_TIMEOUT_MS", 10_000);
        let read_timeout = duration_from_env("OMNIDRIVE_REPAIR_READ_TIMEOUT_MS", 90_000);
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

fn provider_error(
    provider: &'static str,
    operation: &'static str,
    err: impl std::error::Error + fmt::Debug,
) -> RepairError {
    RepairError::Provider {
        provider,
        operation,
        details: format_error_details(&err),
    }
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
