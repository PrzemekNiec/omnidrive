#![allow(dead_code)]

use crate::db;
use crate::diagnostics::{self, WorkerKind, WorkerStatus};
use crate::packer::{LOCAL_PACK_EXTENSION, TOTAL_SHARDS, local_shard_path};
use crate::uploader::ProviderConfig;
use aws_config::timeout::TimeoutConfig;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;
use tokio::time::{sleep, timeout};
use tracing::{info, warn};

pub struct GcWorker {
    pool: SqlitePool,
    providers: HashMap<String, ProviderClient>,
    spool_dir: PathBuf,
    poll_interval: Duration,
    provider_timeout: Duration,
    retry_delay: Duration,
    batch_size: i64,
}

struct ProviderClient {
    provider_name: &'static str,
    bucket: String,
    client: Client,
}

#[derive(Debug)]
pub enum GcError {
    MissingProviderConfig,
    InvalidEnv(&'static str),
    Io(std::io::Error),
    Db(sqlx::Error),
    Timeout {
        provider: &'static str,
        duration: Duration,
    },
    Provider {
        provider: &'static str,
        operation: &'static str,
        details: String,
    },
}

impl fmt::Display for GcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProviderConfig => write!(f, "no GC providers configured"),
            Self::InvalidEnv(key) => write!(f, "invalid environment variable {key}"),
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::Db(err) => write!(f, "sqlite error: {err}"),
            Self::Timeout { provider, duration } => {
                write!(f, "delete to {provider} timed out after {:?}", duration)
            }
            Self::Provider {
                provider,
                operation,
                details,
            } => write!(f, "{provider} {operation} failed: {details}"),
        }
    }
}

impl std::error::Error for GcError {}

impl From<std::io::Error> for GcError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<sqlx::Error> for GcError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

impl GcWorker {
    pub async fn from_env(pool: SqlitePool) -> Result<Self, GcError> {
        let _ = dotenvy::dotenv();

        Ok(Self {
            pool,
            providers: provider_clients_from_env().await?,
            spool_dir: env_path("OMNIDRIVE_SPOOL_DIR", ".omnidrive/spool"),
            poll_interval: duration_from_env("OMNIDRIVE_GC_POLL_INTERVAL_MS", 10_000),
            provider_timeout: duration_from_env("OMNIDRIVE_GC_TIMEOUT_MS", 120_000),
            retry_delay: duration_from_env("OMNIDRIVE_GC_RETRY_DELAY_MS", 10_000),
            batch_size: env::var("OMNIDRIVE_GC_BATCH_SIZE")
                .ok()
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(25),
        })
    }

    pub async fn run(self) -> Result<(), GcError> {
        diagnostics::set_worker_status(WorkerKind::Gc, WorkerStatus::Idle);
        loop {
            let orphaned = db::get_orphaned_pack_ids(&self.pool, self.batch_size).await?;
            if orphaned.is_empty() {
                diagnostics::set_worker_status(WorkerKind::Gc, WorkerStatus::Idle);
                sleep(self.poll_interval).await;
                continue;
            }

            diagnostics::set_worker_status(WorkerKind::Gc, WorkerStatus::Active);
            for pack_id in orphaned {
                if let Err(err) = self.collect_pack(&pack_id).await {
                    warn!("gc failed for orphaned pack {}: {}", pack_id, err);
                    sleep(self.retry_delay).await;
                }
            }
        }
    }

    async fn collect_pack(&self, pack_id: &str) -> Result<(), GcError> {
        let shards = db::get_pack_shards(&self.pool, pack_id).await?;

        for shard in &shards {
            let Some(provider) = self.providers.get(&shard.provider) else {
                return Err(GcError::InvalidEnv("gc provider not configured"));
            };

            match timeout(
                self.provider_timeout,
                provider
                    .client
                    .delete_object()
                    .bucket(&provider.bucket)
                    .key(&shard.object_key)
                    .send(),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(err)) => {
                    let details = format_error_details(&err);
                    if !is_not_found_details(&details) {
                        return Err(GcError::Provider {
                            provider: provider.provider_name,
                            operation: "delete_object",
                            details,
                        });
                    }
                }
                Err(_) => {
                    return Err(GcError::Timeout {
                        provider: provider.provider_name,
                        duration: self.provider_timeout,
                    });
                }
            }
        }

        db::delete_pack_metadata(&self.pool, pack_id).await?;
        self.cleanup_local_files(pack_id).await?;
        info!("gc removed orphaned pack {}", pack_id);
        Ok(())
    }

    async fn cleanup_local_files(&self, pack_id: &str) -> Result<(), GcError> {
        let manifest_path = self
            .spool_dir
            .join(format!("{pack_id}.{LOCAL_PACK_EXTENSION}"));
        remove_file_if_exists(manifest_path).await?;

        for shard_index in 0..TOTAL_SHARDS {
            remove_file_if_exists(local_shard_path(&self.spool_dir, pack_id, shard_index)).await?;
        }

        Ok(())
    }
}

impl ProviderClient {
    async fn from_provider_config(config: ProviderConfig) -> Result<Self, GcError> {
        let provider_name = config.provider_name;
        let operation_timeout = duration_from_env("OMNIDRIVE_GC_TIMEOUT_MS", 120_000);
        let operation_attempt_timeout =
            duration_from_env("OMNIDRIVE_GC_ATTEMPT_TIMEOUT_MS", 90_000);
        let connect_timeout = duration_from_env("OMNIDRIVE_GC_CONNECT_TIMEOUT_MS", 10_000);
        let read_timeout = duration_from_env("OMNIDRIVE_GC_READ_TIMEOUT_MS", 90_000);
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

async fn provider_clients_from_env() -> Result<HashMap<String, ProviderClient>, GcError> {
    let mut clients = HashMap::new();
    for config in [
        ProviderConfig::from_r2_env(),
        ProviderConfig::from_scaleway_env(),
        ProviderConfig::from_b2_env(),
    ] {
        let config = config.map_err(|_| GcError::MissingProviderConfig)?;
        let client = ProviderClient::from_provider_config(config).await?;
        clients.insert(client.provider_name.to_string(), client);
    }

    if clients.is_empty() {
        return Err(GcError::MissingProviderConfig);
    }

    Ok(clients)
}

async fn remove_file_if_exists(path: PathBuf) -> Result<(), GcError> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(GcError::Io(err)),
    }
}

fn is_not_found_details(details: &str) -> bool {
    let lower = details.to_ascii_lowercase();
    lower.contains("404") || lower.contains("notfound") || lower.contains("nosuchkey")
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
