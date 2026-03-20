#![allow(dead_code)]

use crate::db;
use aws_config::BehaviorVersion;
use aws_config::timeout::TimeoutConfig;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use sqlx::SqlitePool;
use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs;
use tokio::time::{sleep, timeout};

use crate::packer::LOCAL_PACK_EXTENSION;

pub struct Uploader {
    provider_name: &'static str,
    client: Client,
    bucket: String,
    force_path_style: bool,
}

pub struct UploadedPack {
    pub provider: &'static str,
    pub bucket: String,
    pub key: String,
    pub etag: Option<String>,
    pub version_id: Option<String>,
}

pub struct UploadWorker {
    pool: SqlitePool,
    uploaders: Vec<Uploader>,
    spool_dir: PathBuf,
    poll_interval: Duration,
    provider_timeout: Duration,
    connect_timeout: Duration,
    read_timeout: Duration,
    retry_base_delay: Duration,
    retry_max_delay: Duration,
}

enum JobProcessOutcome {
    Completed,
    PendingRetry {
        delay: Duration,
        failed_providers: Vec<String>,
    },
}

struct ProviderConfig {
    provider_name: &'static str,
    endpoint: String,
    region: String,
    bucket: String,
    access_key_id: String,
    secret_access_key: String,
    force_path_style: bool,
}

#[derive(Debug)]
pub enum UploaderError {
    MissingEnv(&'static str),
    InvalidEnv(&'static str),
    Io(std::io::Error),
    Db(sqlx::Error),
    PackMissing(PathBuf),
    Timeout {
        provider: &'static str,
        duration: Duration,
    },
    Upload {
        provider: &'static str,
        operation: &'static str,
        details: String,
    },
}

impl fmt::Display for UploaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnv(key) => write!(f, "missing required environment variable {key}"),
            Self::InvalidEnv(key) => write!(f, "invalid environment variable {key}"),
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::Db(err) => write!(f, "sqlite error: {err}"),
            Self::PackMissing(path) => write!(f, "pack file not found: {}", path.display()),
            Self::Timeout { provider, duration } => {
                write!(f, "upload to {provider} timed out after {:?}", duration)
            }
            Self::Upload {
                provider,
                operation,
                details,
            } => write!(f, "{provider} {operation} failed: {details}"),
        }
    }
}

impl std::error::Error for UploaderError {}

impl From<std::io::Error> for UploaderError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<sqlx::Error> for UploaderError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

impl Uploader {
    pub async fn from_r2_env() -> Result<Self, UploaderError> {
        let _ = dotenvy::dotenv();
        let config = ProviderConfig::from_r2_env()?;
        Self::from_provider_config(config).await
    }

    pub async fn all_from_env() -> Result<Vec<Self>, UploaderError> {
        let _ = dotenvy::dotenv();

        Ok(vec![
            Self::from_provider_config(ProviderConfig::from_r2_env()?).await?,
            Self::from_provider_config(ProviderConfig::from_scaleway_env()?).await?,
            Self::from_provider_config(ProviderConfig::from_b2_env()?).await?,
        ])
    }

    async fn from_provider_config(config: ProviderConfig) -> Result<Self, UploaderError> {
        let provider_name = config.provider_name;
        let operation_timeout = duration_from_env("OMNIDRIVE_UPLOAD_TIMEOUT_MS", 120_000);
        let operation_attempt_timeout =
            duration_from_env("OMNIDRIVE_UPLOAD_ATTEMPT_TIMEOUT_MS", 90_000);
        let connect_timeout = duration_from_env("OMNIDRIVE_UPLOAD_CONNECT_TIMEOUT_MS", 10_000);
        let read_timeout = duration_from_env("OMNIDRIVE_UPLOAD_READ_TIMEOUT_MS", 90_000);
        let timeout_config = TimeoutConfig::builder()
            .connect_timeout(connect_timeout)
            .read_timeout(read_timeout)
            .operation_attempt_timeout(operation_attempt_timeout)
            .operation_timeout(operation_timeout)
            .build();

        let shared_config = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(config.region.clone()))
            .timeout_config(timeout_config.clone())
            .load()
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

        let client = Client::from_conf(s3_config);

        Ok(Self {
            provider_name,
            client,
            bucket: config.bucket,
            force_path_style: config.force_path_style,
        })
    }

    pub async fn upload_pack(
        &self,
        pack_path: impl AsRef<Path>,
    ) -> Result<UploadedPack, UploaderError> {
        let pack_path = pack_path.as_ref();
        let file_name = pack_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(UploaderError::InvalidEnv("pack_path.file_name"))?;
        let key = format!("packs/{file_name}");
        self.upload_file(pack_path, &key, "application/octet-stream")
            .await
    }

    pub async fn upload_debug_file(
        &self,
        file_path: impl AsRef<Path>,
        key: &str,
    ) -> Result<UploadedPack, UploaderError> {
        self.upload_file(file_path.as_ref(), key, "text/plain")
            .await
    }

    async fn upload_file(
        &self,
        file_path: &Path,
        key: &str,
        content_type: &'static str,
    ) -> Result<UploadedPack, UploaderError> {
        let body = ByteStream::from_path(file_path)
            .await
            .map_err(|err| self.sdk_error("read_body", err))?;

        let response = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(body)
            .content_type(content_type)
            .send()
            .await
            .map_err(|err| self.sdk_error("put_object", err))?;

        Ok(UploadedPack {
            provider: self.provider_name,
            bucket: self.bucket.clone(),
            key: key.to_string(),
            etag: response.e_tag,
            version_id: response.version_id,
        })
    }
}

impl UploadWorker {
    pub async fn from_env(pool: SqlitePool) -> Result<Self, UploaderError> {
        let _ = dotenvy::dotenv();

        let uploaders = Uploader::all_from_env().await?;

        Ok(Self {
            pool,
            uploaders,
            spool_dir: env_path("OMNIDRIVE_SPOOL_DIR", ".omnidrive/spool"),
            poll_interval: duration_from_env("OMNIDRIVE_UPLOAD_POLL_INTERVAL_MS", 1_000),
            provider_timeout: duration_from_env("OMNIDRIVE_UPLOAD_TIMEOUT_MS", 120_000),
            connect_timeout: duration_from_env("OMNIDRIVE_UPLOAD_CONNECT_TIMEOUT_MS", 10_000),
            read_timeout: duration_from_env("OMNIDRIVE_UPLOAD_READ_TIMEOUT_MS", 90_000),
            retry_base_delay: duration_from_env("OMNIDRIVE_UPLOAD_RETRY_BASE_MS", 2_000),
            retry_max_delay: duration_from_env("OMNIDRIVE_UPLOAD_RETRY_MAX_MS", 60_000),
        })
    }

    pub async fn run(self) -> Result<(), UploaderError> {
        db::reset_in_progress_upload_jobs(&self.pool).await?;
        db::reset_in_progress_upload_targets(&self.pool).await?;

        loop {
            let Some(job) = db::get_next_upload_job(&self.pool).await? else {
                sleep(self.poll_interval).await;
                continue;
            };

            match self.process_job(&job).await {
                Ok(JobProcessOutcome::Completed) => {
                    db::mark_upload_job_completed(&self.pool, job.id).await?;
                }
                Ok(JobProcessOutcome::PendingRetry {
                    delay,
                    failed_providers,
                }) => {
                    let attempts = db::requeue_upload_job(&self.pool, job.id).await?;
                    eprintln!(
                        "upload job {} remains pending for [{}]; retry after {:?} (job_attempts={})",
                        job.pack_id,
                        failed_providers.join(", "),
                        delay,
                        attempts
                    );
                    sleep(delay).await;
                }
                Err(err) if err.is_retryable() => {
                    let attempts = db::requeue_upload_job(&self.pool, job.id).await?;
                    let delay = self.retry_delay(attempts);
                    eprintln!("upload job {} will retry after {:?}: {}", job.pack_id, delay, err);
                    sleep(delay).await;
                }
                Err(err) => {
                    db::mark_upload_job_failed(&self.pool, job.id).await?;
                    eprintln!("upload job {} failed permanently: {}", job.pack_id, err);
                }
            }
        }
    }

    async fn process_job(&self, job: &db::UploadJob) -> Result<JobProcessOutcome, UploaderError> {
        let pack_path = self
            .spool_dir
            .join(format!("{}.{}", job.pack_id, LOCAL_PACK_EXTENSION));

        if !fs::try_exists(&pack_path).await? {
            return Err(UploaderError::PackMissing(pack_path));
        }

        let provider_names: Vec<&str> = self
            .uploaders
            .iter()
            .map(|uploader| uploader.provider_name())
            .collect();
        db::ensure_upload_targets(&self.pool, job.id, &provider_names).await?;

        let pending_targets = db::get_incomplete_upload_targets(&self.pool, job.id).await?;
        if pending_targets.is_empty() {
            return Ok(JobProcessOutcome::Completed);
        }

        let mut failed_providers = Vec::new();
        let mut max_target_attempts = 0i64;

        for target in pending_targets {
            let uploader = self
                .uploaders
                .iter()
                .find(|uploader| uploader.provider_name() == target.provider.as_str())
                .ok_or(UploaderError::InvalidEnv("upload_target.provider"))?;

            db::mark_upload_target_in_progress(&self.pool, job.id, &target.provider).await?;
            println!(
                "upload start pack={} provider={} force_path_style={} worker_timeout={:?} connect_timeout={:?} read_timeout={:?}",
                job.pack_id,
                uploader.provider_name(),
                uploader.force_path_style(),
                self.provider_timeout,
                self.connect_timeout,
                self.read_timeout
            );

            let upload_result = timeout(self.provider_timeout, uploader.upload_pack(&pack_path)).await;
            match upload_result {
                Ok(Ok(uploaded)) => {
                    db::mark_upload_target_completed(
                        &self.pool,
                        job.id,
                        &target.provider,
                        &uploaded.bucket,
                        &uploaded.key,
                        uploaded.etag.as_deref(),
                        uploaded.version_id.as_deref(),
                    )
                    .await?;
                    println!(
                        "uploaded pack {} to {} at {}/{}",
                        job.pack_id, uploaded.provider, uploaded.bucket, uploaded.key
                    );
                }
                Ok(Err(err)) if err.is_retryable() => {
                    let attempts = db::requeue_upload_target(
                        &self.pool,
                        job.id,
                        &target.provider,
                        &err.to_string(),
                    )
                    .await?;
                    max_target_attempts = max_target_attempts.max(attempts);
                    failed_providers.push(format!("{}: {}", target.provider, err));
                }
                Err(_) => {
                    let timeout_error = UploaderError::Timeout {
                        provider: uploader.provider_name(),
                        duration: self.provider_timeout,
                    };
                    let attempts = db::requeue_upload_target(
                        &self.pool,
                        job.id,
                        &target.provider,
                        &timeout_error.to_string(),
                    )
                    .await?;
                    max_target_attempts = max_target_attempts.max(attempts);
                    failed_providers.push(format!("{}: {}", target.provider, timeout_error));
                }
                Ok(Err(err)) => {
                    db::mark_upload_target_failed(
                        &self.pool,
                        job.id,
                        &target.provider,
                        &err.to_string(),
                    )
                    .await?;
                    return Err(err);
                }
            }
        }

        if db::has_incomplete_upload_targets(&self.pool, job.id).await? {
            let delay = self.retry_delay(max_target_attempts.max(1));
            return Ok(JobProcessOutcome::PendingRetry {
                delay,
                failed_providers,
            });
        }

        Ok(JobProcessOutcome::Completed)
    }

    fn retry_delay(&self, attempts: i64) -> Duration {
        let exponent = attempts.saturating_sub(1).clamp(0, 10) as u32;
        let multiplier = 2u32.saturating_pow(exponent);
        let delay = self
            .retry_base_delay
            .checked_mul(multiplier)
            .unwrap_or(self.retry_max_delay);

        delay.min(self.retry_max_delay)
    }
}

impl ProviderConfig {
    fn from_r2_env() -> Result<Self, UploaderError> {
        Ok(Self {
            provider_name: "cloudflare-r2",
            endpoint: required_env("OMNIDRIVE_R2_ENDPOINT")?,
            region: env::var("OMNIDRIVE_R2_REGION").unwrap_or_else(|_| "auto".to_string()),
            bucket: required_env("OMNIDRIVE_R2_BUCKET")?,
            access_key_id: required_env("OMNIDRIVE_R2_ACCESS_KEY_ID")?,
            secret_access_key: required_env("OMNIDRIVE_R2_SECRET_ACCESS_KEY")?,
            force_path_style: bool_from_env("OMNIDRIVE_R2_FORCE_PATH_STYLE", false),
        })
    }

    fn from_scaleway_env() -> Result<Self, UploaderError> {
        Ok(Self {
            provider_name: "scaleway",
            endpoint: required_env("OMNIDRIVE_SCALEWAY_ENDPOINT")?,
            region: env::var("OMNIDRIVE_SCALEWAY_REGION").unwrap_or_else(|_| "pl-waw".to_string()),
            bucket: required_env("OMNIDRIVE_SCALEWAY_BUCKET")?,
            access_key_id: required_env("OMNIDRIVE_SCALEWAY_ACCESS_KEY_ID")?,
            secret_access_key: required_env("OMNIDRIVE_SCALEWAY_SECRET_ACCESS_KEY")?,
            force_path_style: bool_from_env("OMNIDRIVE_SCALEWAY_FORCE_PATH_STYLE", false),
        })
    }

    fn from_b2_env() -> Result<Self, UploaderError> {
        Ok(Self {
            provider_name: "backblaze-b2",
            endpoint: required_env("OMNIDRIVE_B2_ENDPOINT")?,
            region: env::var("OMNIDRIVE_B2_REGION")
                .unwrap_or_else(|_| "eu-central-003".to_string()),
            bucket: required_env("OMNIDRIVE_B2_BUCKET")?,
            access_key_id: required_env("OMNIDRIVE_B2_ACCESS_KEY_ID")?,
            secret_access_key: required_env("OMNIDRIVE_B2_SECRET_ACCESS_KEY")?,
            force_path_style: bool_from_env("OMNIDRIVE_B2_FORCE_PATH_STYLE", false),
        })
    }
}

impl UploaderError {
    fn is_retryable(&self) -> bool {
        matches!(self, Self::Upload { .. } | Self::Timeout { .. })
    }
}

impl Uploader {
    pub fn provider_name(&self) -> &'static str {
        self.provider_name
    }

    pub fn force_path_style(&self) -> bool {
        self.force_path_style
    }

    fn sdk_error(
        &self,
        operation: &'static str,
        err: impl std::error::Error + fmt::Debug,
    ) -> UploaderError {
        UploaderError::Upload {
            provider: self.provider_name,
            operation,
            details: format_error_details(&err),
        }
    }
}

fn required_env(key: &'static str) -> Result<String, UploaderError> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(UploaderError::MissingEnv(key)),
    }
}

fn duration_from_env(key: &str, default_ms: u64) -> Duration {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(default_ms))
}

fn env_path(key: &str, default: &str) -> PathBuf {
    env::var(key)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(default))
}

fn bool_from_env(key: &str, default: bool) -> bool {
    env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
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
