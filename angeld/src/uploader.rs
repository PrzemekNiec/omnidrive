#![allow(dead_code)]

use crate::config::AppConfig;
use crate::db;
use crate::db::PackStatus;
use crate::packer::local_shard_path;
use async_stream::stream;
use aws_config::timeout::TimeoutConfig;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::primitives::{ByteStream, SdkBody};
use bytes::Bytes;
use http_body::Frame;
use http_body_util::StreamBody;
use sqlx::SqlitePool;
use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::{self, File};
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep, timeout};

pub const KNOWN_PROVIDERS: [&str; 3] = ["cloudflare-r2", "backblaze-b2", "scaleway"];

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
    app_config: AppConfig,
    rate_limiter: Arc<UploadRateLimiter>,
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
        failed_shards: Vec<String>,
    },
    Failed,
}

struct UploadRateLimiter {
    max_bytes_per_sec: u64,
    state: Mutex<UploadRateLimiterState>,
}

struct UploadRateLimiterState {
    available_bytes: f64,
    last_refill: Instant,
}

#[derive(Clone, Debug)]
pub(crate) struct ProviderConfig {
    pub(crate) provider_name: &'static str,
    pub(crate) endpoint: String,
    pub(crate) region: String,
    pub(crate) bucket: String,
    pub(crate) access_key_id: String,
    pub(crate) secret_access_key: String,
    pub(crate) force_path_style: bool,
}

#[derive(Debug)]
pub enum UploaderError {
    MissingEnv(&'static str),
    InvalidEnv(&'static str),
    Io(std::io::Error),
    Db(sqlx::Error),
    LocalObjectMissing(PathBuf),
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
            Self::LocalObjectMissing(path) => {
                write!(f, "local upload object not found: {}", path.display())
            }
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

    pub(crate) async fn from_provider_config(
        config: ProviderConfig,
    ) -> Result<Self, UploaderError> {
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
            .region(Region::new(config.region.clone()))
            .timeout_config(timeout_config)
            .force_path_style(config.force_path_style)
            .build();

        Ok(Self {
            provider_name,
            client: Client::from_conf(s3_config),
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
        self.upload_file(pack_path, &key, "application/octet-stream", None)
            .await
    }

    pub async fn upload_debug_file(
        &self,
        file_path: impl AsRef<Path>,
        key: &str,
    ) -> Result<UploadedPack, UploaderError> {
        self.upload_file(file_path.as_ref(), key, "text/plain", None)
            .await
    }

    async fn upload_shard(
        &self,
        shard_path: impl AsRef<Path>,
        object_key: &str,
        rate_limiter: Option<Arc<UploadRateLimiter>>,
    ) -> Result<UploadedPack, UploaderError> {
        self.upload_file(
            shard_path.as_ref(),
            object_key,
            "application/octet-stream",
            rate_limiter,
        )
        .await
    }

    async fn upload_file(
        &self,
        file_path: &Path,
        key: &str,
        content_type: &'static str,
        rate_limiter: Option<Arc<UploadRateLimiter>>,
    ) -> Result<UploadedPack, UploaderError> {
        if !fs::try_exists(file_path).await? {
            return Err(UploaderError::LocalObjectMissing(file_path.to_path_buf()));
        }

        let file_size = fs::metadata(file_path).await?.len();
        let body = throttled_byte_stream(file_path.to_path_buf(), rate_limiter);

        let response = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(body)
            .content_length(
                i64::try_from(file_size).map_err(|_| UploaderError::InvalidEnv("file_size"))?,
            )
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

        let app_config = AppConfig::from_env();
        let uploaders = Uploader::all_from_env().await?;

        Ok(Self {
            pool,
            uploaders,
            rate_limiter: Arc::new(UploadRateLimiter::new(app_config.clone())),
            app_config,
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
        db::reset_in_progress_pack_shards(&self.pool).await?;

        loop {
            let Some(job) = db::get_next_upload_job(&self.pool).await? else {
                sleep(self.poll_interval).await;
                continue;
            };

            match self.process_job(&job).await? {
                JobProcessOutcome::Completed => {
                    db::mark_upload_job_completed(&self.pool, job.id).await?;
                }
                JobProcessOutcome::PendingRetry {
                    delay,
                    failed_shards,
                } => {
                    let attempts = db::requeue_upload_job(&self.pool, job.id).await?;
                    eprintln!(
                        "upload job {} remains pending for [{}]; retry after {:?} (job_attempts={})",
                        job.pack_id,
                        failed_shards.join(", "),
                        delay,
                        attempts
                    );
                    sleep(delay).await;
                }
                JobProcessOutcome::Failed => {
                    db::mark_upload_job_failed(&self.pool, job.id).await?;
                    eprintln!("upload job {} became unreadable", job.pack_id);
                }
            }
        }
    }

    async fn process_job(&self, job: &db::UploadJob) -> Result<JobProcessOutcome, UploaderError> {
        let provider_names: Vec<&str> = self
            .uploaders
            .iter()
            .map(|uploader| uploader.provider_name())
            .collect();
        db::ensure_upload_targets(&self.pool, job.id, &provider_names).await?;

        let pending_shards = db::get_incomplete_pack_shards(&self.pool, &job.pack_id).await?;
        if pending_shards.is_empty() {
            let summary = db::summarize_pack_shards(&self.pool, &job.pack_id).await?;
            let status = db::resolve_pack_status(summary);
            db::update_pack_status(&self.pool, &job.pack_id, status).await?;
            return Ok(match status {
                PackStatus::Healthy | PackStatus::Degraded => JobProcessOutcome::Completed,
                PackStatus::Uploading => JobProcessOutcome::PendingRetry {
                    delay: self.retry_delay(1),
                    failed_shards: vec!["waiting for shard retry".to_string()],
                },
                PackStatus::Unreadable => JobProcessOutcome::Failed,
            });
        }

        let mut failed_shards = Vec::new();
        let mut max_attempts = 0i64;

        for shard in pending_shards {
            let uploader = self
                .uploaders
                .iter()
                .find(|uploader| uploader.provider_name() == shard.provider.as_str())
                .ok_or(UploaderError::InvalidEnv("pack_shards.provider"))?;

            let current_usage =
                db::get_physical_usage_for_provider(&self.pool, &shard.provider).await?;
            let shard_size = u64::try_from(shard.size)
                .map_err(|_| UploaderError::InvalidEnv("pack_shards.size"))?;
            let projected_usage = current_usage.saturating_add(shard_size);
            if projected_usage > self.app_config.max_physical_bytes_per_provider {
                let message = format!(
                    "quota exceeded for provider {}: projected={} limit={}",
                    shard.provider,
                    projected_usage,
                    self.app_config.max_physical_bytes_per_provider
                );
                eprintln!("{message}");
                db::mark_pack_shard_failed(&self.pool, &job.pack_id, shard.shard_index, &message)
                    .await?;
                db::mark_upload_target_failed(&self.pool, job.id, &shard.provider, &message)
                    .await?;
                failed_shards.push(format!(
                    "{} shard {}: {}",
                    shard.provider, shard.shard_index, message
                ));
                continue;
            }

            let shard_path = local_shard_path(
                &self.spool_dir,
                &job.pack_id,
                usize::try_from(shard.shard_index)
                    .map_err(|_| UploaderError::InvalidEnv("pack_shards.shard_index"))?,
            );

            db::mark_pack_shard_in_progress(&self.pool, &job.pack_id, shard.shard_index).await?;
            db::mark_upload_target_in_progress(&self.pool, job.id, &shard.provider).await?;

            println!(
                "upload start pack={} shard={} provider={} force_path_style={} worker_timeout={:?} connect_timeout={:?} read_timeout={:?}",
                job.pack_id,
                shard.shard_index,
                uploader.provider_name(),
                uploader.force_path_style(),
                self.provider_timeout,
                self.connect_timeout,
                self.read_timeout
            );

            if !fs::try_exists(&shard_path).await? {
                let message = format!("local shard missing: {}", shard_path.display());
                db::mark_pack_shard_failed(&self.pool, &job.pack_id, shard.shard_index, &message)
                    .await?;
                db::mark_upload_target_failed(&self.pool, job.id, &shard.provider, &message)
                    .await?;
                failed_shards.push(format!(
                    "{} shard {}: {}",
                    shard.provider, shard.shard_index, message
                ));
                continue;
            }

            match timeout(
                self.provider_timeout,
                uploader.upload_shard(
                    &shard_path,
                    &shard.object_key,
                    Some(self.rate_limiter.clone()),
                ),
            )
            .await
            {
                Ok(Ok(uploaded)) => {
                    db::mark_pack_shard_completed(&self.pool, &job.pack_id, shard.shard_index)
                        .await?;
                    db::mark_upload_target_completed(
                        &self.pool,
                        job.id,
                        &shard.provider,
                        &uploaded.bucket,
                        &uploaded.key,
                        uploaded.etag.as_deref(),
                        uploaded.version_id.as_deref(),
                    )
                    .await?;
                    println!(
                        "uploaded pack={} shard={} provider={} key={}",
                        job.pack_id, shard.shard_index, uploaded.provider, uploaded.key
                    );
                }
                Ok(Err(err)) if err.is_retryable() => {
                    let attempts = db::requeue_pack_shard(
                        &self.pool,
                        &job.pack_id,
                        shard.shard_index,
                        &err.to_string(),
                    )
                    .await?;
                    db::requeue_upload_target(
                        &self.pool,
                        job.id,
                        &shard.provider,
                        &err.to_string(),
                    )
                    .await?;
                    max_attempts = max_attempts.max(attempts);
                    failed_shards.push(format!(
                        "{} shard {}: {}",
                        shard.provider, shard.shard_index, err
                    ));
                }
                Err(_) => {
                    let timeout_error = UploaderError::Timeout {
                        provider: uploader.provider_name(),
                        duration: self.provider_timeout,
                    };
                    let attempts = db::requeue_pack_shard(
                        &self.pool,
                        &job.pack_id,
                        shard.shard_index,
                        &timeout_error.to_string(),
                    )
                    .await?;
                    db::requeue_upload_target(
                        &self.pool,
                        job.id,
                        &shard.provider,
                        &timeout_error.to_string(),
                    )
                    .await?;
                    max_attempts = max_attempts.max(attempts);
                    failed_shards.push(format!(
                        "{} shard {}: {}",
                        shard.provider, shard.shard_index, timeout_error
                    ));
                }
                Ok(Err(err)) => {
                    db::mark_pack_shard_failed(
                        &self.pool,
                        &job.pack_id,
                        shard.shard_index,
                        &err.to_string(),
                    )
                    .await?;
                    db::mark_upload_target_failed(
                        &self.pool,
                        job.id,
                        &shard.provider,
                        &err.to_string(),
                    )
                    .await?;
                    failed_shards.push(format!(
                        "{} shard {}: {}",
                        shard.provider, shard.shard_index, err
                    ));
                }
            }
        }

        let summary = db::summarize_pack_shards(&self.pool, &job.pack_id).await?;
        let status = db::resolve_pack_status(summary);
        db::update_pack_status(&self.pool, &job.pack_id, status).await?;

        Ok(match status {
            PackStatus::Healthy | PackStatus::Degraded => JobProcessOutcome::Completed,
            PackStatus::Uploading => JobProcessOutcome::PendingRetry {
                delay: self.retry_delay(max_attempts.max(1)),
                failed_shards,
            },
            PackStatus::Unreadable => JobProcessOutcome::Failed,
        })
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

impl UploadRateLimiter {
    fn new(app_config: AppConfig) -> Self {
        let initial_tokens = app_config.max_upload_bytes_per_sec as f64;
        Self {
            max_bytes_per_sec: app_config.max_upload_bytes_per_sec,
            state: Mutex::new(UploadRateLimiterState {
                available_bytes: initial_tokens,
                last_refill: Instant::now(),
            }),
        }
    }

    async fn acquire(&self, bytes: usize) {
        if self.max_bytes_per_sec == 0 || bytes == 0 {
            return;
        }

        let requested = bytes as f64;
        loop {
            let mut state = self.state.lock().await;
            let now = Instant::now();
            let elapsed = now.duration_since(state.last_refill).as_secs_f64();
            if elapsed > 0.0 {
                state.available_bytes = (state.available_bytes
                    + elapsed * self.max_bytes_per_sec as f64)
                    .min(self.max_bytes_per_sec as f64);
                state.last_refill = now;
            }

            if state.available_bytes >= requested {
                state.available_bytes -= requested;
                return;
            }

            let missing = requested - state.available_bytes;
            drop(state);
            sleep(Duration::from_secs_f64(
                (missing / self.max_bytes_per_sec as f64).max(0.001),
            ))
            .await;
        }
    }
}

fn throttled_byte_stream(
    file_path: PathBuf,
    rate_limiter: Option<Arc<UploadRateLimiter>>,
) -> ByteStream {
    let body = SdkBody::retryable(move || {
        let path = file_path.clone();
        let limiter = rate_limiter.clone();
        let stream = stream! {
            let mut file = match File::open(&path).await {
                Ok(file) => file,
                Err(err) => {
                    yield Err(err);
                    return;
                }
            };
            let mut buffer = vec![0u8; 64 * 1024];

            loop {
                let bytes_read = match file.read(&mut buffer).await {
                    Ok(bytes_read) => bytes_read,
                    Err(err) => {
                        yield Err(err);
                        return;
                    }
                };
                if bytes_read == 0 {
                    break;
                }

                if let Some(limiter) = &limiter {
                    limiter.acquire(bytes_read).await;
                }

                yield Ok(Frame::data(Bytes::copy_from_slice(&buffer[..bytes_read])));
            }
        };

        SdkBody::from_body_1_x(StreamBody::new(stream))
    });

    ByteStream::new(body)
}

impl ProviderConfig {
    pub(crate) fn from_r2_env() -> Result<Self, UploaderError> {
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

    pub(crate) fn from_scaleway_env() -> Result<Self, UploaderError> {
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

    pub(crate) fn from_b2_env() -> Result<Self, UploaderError> {
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
