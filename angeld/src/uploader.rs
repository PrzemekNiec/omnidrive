// reserved for Epic 32.5 / Epic 33 (DEK-envelope upload to B2/R2)
#![allow(dead_code)]

use crate::cloud_guard::{self, GuardOperation};
use crate::config::AppConfig;
use crate::db;
use crate::db::{PackStatus, StorageMode};
use crate::diagnostics::{self, WorkerKind, WorkerStatus};
use crate::onboarding;
use crate::packer::local_shard_path;
use crate::packer::{LOCAL_PACK_EXTENSION, TOTAL_SHARDS};
use crate::secure_fs::secure_delete;
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
use tokio::sync::{Mutex, watch};
use tokio::time::{Instant, sleep, timeout};
use tracing::{error, info, warn};

pub const KNOWN_PROVIDERS: [&str; 3] = ["cloudflare-r2", "backblaze-b2", "scaleway"];

pub struct Uploader {
    provider_name: &'static str,
    client: Client,
    bucket: String,
    force_path_style: bool,
    buffered_uploads: bool,
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
    provider_reload_rx: Option<watch::Receiver<u64>>,
    app_config: AppConfig,
    rate_limiter: Arc<UploadRateLimiter>,
    spool_dir: PathBuf,
    poll_interval: Duration,
    provider_timeout: Duration,
    connect_timeout: Duration,
    read_timeout: Duration,
    retry_base_delay: Duration,
    retry_max_delay: Duration,
    test_process_delay: Duration,
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
    CloudGuard(String),
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
            Self::CloudGuard(details) => write!(f, "cloud guard blocked operation: {details}"),
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
            client: Client::from_conf(s3_config),
            bucket: config.bucket,
            force_path_style: config.force_path_style,
            buffered_uploads: bool_from_env("OMNIDRIVE_UPLOAD_BUFFERED", false),
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

    pub async fn upload_system_file(
        &self,
        file_path: impl AsRef<Path>,
        key: &str,
    ) -> Result<UploadedPack, UploaderError> {
        self.upload_file(file_path.as_ref(), key, "application/octet-stream", None)
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
        let body = if self.buffered_uploads {
            ByteStream::from(fs::read(file_path).await?)
        } else {
            throttled_byte_stream(file_path.to_path_buf(), rate_limiter)
        };

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
    pub async fn from_onboarding_db(
        pool: SqlitePool,
        provider_reload_rx: Option<watch::Receiver<u64>>,
    ) -> Result<Self, UploaderError> {
        let app_config = AppConfig::from_env();
        let mut worker = Self {
            pool,
            uploaders: Vec::new(),
            provider_reload_rx,
            rate_limiter: Arc::new(UploadRateLimiter::new(app_config.clone())),
            app_config,
            spool_dir: env_path("OMNIDRIVE_SPOOL_DIR", ".omnidrive/spool"),
            poll_interval: duration_from_env("OMNIDRIVE_UPLOAD_POLL_INTERVAL_MS", 1_000),
            provider_timeout: duration_from_env("OMNIDRIVE_UPLOAD_TIMEOUT_MS", 120_000),
            connect_timeout: duration_from_env("OMNIDRIVE_UPLOAD_CONNECT_TIMEOUT_MS", 10_000),
            read_timeout: duration_from_env("OMNIDRIVE_UPLOAD_READ_TIMEOUT_MS", 90_000),
            retry_base_delay: duration_from_env("OMNIDRIVE_UPLOAD_RETRY_BASE_MS", 2_000),
            retry_max_delay: duration_from_env("OMNIDRIVE_UPLOAD_RETRY_MAX_MS", 60_000),
            test_process_delay: duration_from_env("OMNIDRIVE_UPLOAD_TEST_PROCESS_DELAY_MS", 0),
        };
        worker.reload_uploaders_from_db().await?;
        Ok(worker)
    }

    pub async fn from_env(pool: SqlitePool) -> Result<Self, UploaderError> {
        let _ = dotenvy::dotenv();

        let app_config = AppConfig::from_env();
        let uploaders = match Uploader::all_from_env().await {
            Ok(uploaders) => uploaders,
            Err(err) if bool_from_env("OMNIDRIVE_ALLOW_EMPTY_UPLOADERS", false) => {
                warn!(
                    "starting uploader without configured remote providers: {}",
                    err
                );
                Vec::new()
            }
            Err(err) => return Err(err),
        };

        Ok(Self {
            pool,
            uploaders,
            provider_reload_rx: None,
            rate_limiter: Arc::new(UploadRateLimiter::new(app_config.clone())),
            app_config,
            spool_dir: env_path("OMNIDRIVE_SPOOL_DIR", ".omnidrive/spool"),
            poll_interval: duration_from_env("OMNIDRIVE_UPLOAD_POLL_INTERVAL_MS", 1_000),
            provider_timeout: duration_from_env("OMNIDRIVE_UPLOAD_TIMEOUT_MS", 120_000),
            connect_timeout: duration_from_env("OMNIDRIVE_UPLOAD_CONNECT_TIMEOUT_MS", 10_000),
            read_timeout: duration_from_env("OMNIDRIVE_UPLOAD_READ_TIMEOUT_MS", 90_000),
            retry_base_delay: duration_from_env("OMNIDRIVE_UPLOAD_RETRY_BASE_MS", 2_000),
            retry_max_delay: duration_from_env("OMNIDRIVE_UPLOAD_RETRY_MAX_MS", 60_000),
            test_process_delay: duration_from_env("OMNIDRIVE_UPLOAD_TEST_PROCESS_DELAY_MS", 0),
        })
    }

    pub async fn run(mut self) -> Result<(), UploaderError> {
        db::reset_in_progress_upload_jobs(&self.pool).await?;
        db::reset_in_progress_upload_targets(&self.pool).await?;
        db::reset_in_progress_pack_shards(&self.pool).await?;
        diagnostics::set_worker_status(WorkerKind::Uploader, WorkerStatus::Idle);

        loop {
            self.maybe_process_runtime_reload_signal().await;

            let Some(job) = db::get_next_upload_job(&self.pool).await? else {
                diagnostics::set_worker_status(WorkerKind::Uploader, WorkerStatus::Idle);
                sleep(self.poll_interval).await;
                continue;
            };
            diagnostics::set_worker_status(WorkerKind::Uploader, WorkerStatus::Active);

            match self.process_job(&job).await? {
                JobProcessOutcome::Completed => {
                    db::mark_upload_job_completed(&self.pool, job.id).await?;
                    diagnostics::clear_upload_error();
                }
                JobProcessOutcome::PendingRetry {
                    delay,
                    failed_shards,
                } => {
                    let attempts = db::requeue_upload_job(&self.pool, job.id).await?;
                    let message = format!(
                        "upload job {} remains pending for [{}]; retry after {:?} (job_attempts={})",
                        job.pack_id,
                        failed_shards.join(", "),
                        delay,
                        attempts
                    );
                    diagnostics::record_upload_error(message.clone());
                    warn!("{message}");
                    diagnostics::set_worker_status(WorkerKind::Uploader, WorkerStatus::Idle);
                    sleep(delay).await;
                }
                JobProcessOutcome::Failed => {
                    db::mark_upload_job_failed(&self.pool, job.id).await?;
                    let message = format!("upload job {} became unreadable", job.pack_id);
                    diagnostics::record_upload_error(message.clone());
                    error!("{message}");
                }
            }
        }
    }

    async fn maybe_process_runtime_reload_signal(&mut self) {
        let mut reload_requested = false;
        {
            let Some(reload_rx) = self.provider_reload_rx.as_mut() else {
                return;
            };

            loop {
                let changed = match reload_rx.has_changed() {
                    Ok(value) => value,
                    Err(_) => return,
                };
                if !changed {
                    break;
                }

                let _ = reload_rx.borrow_and_update();
                reload_requested = true;
            }
        }

        if reload_requested {
            match self.reload_uploaders_from_db().await {
                Ok(provider_names) => {
                    info!(
                        "[RUNTIME] Signal received: Reloading providers... OK ({})",
                        if provider_names.is_empty() {
                            "none".to_string()
                        } else {
                            provider_names.join(", ")
                        }
                    );
                }
                Err(err) => {
                    warn!("[RUNTIME] Signal received: Reloading providers... FAILED: {err}");
                }
            }
        }
    }

    async fn reload_uploaders_from_db(&mut self) -> Result<Vec<String>, UploaderError> {
        let configs = onboarding::get_active_provider_configs(&self.pool)
            .await
            .map_err(|err| UploaderError::CloudGuard(err.to_string()))?;

        let provider_names: Vec<String> = configs
            .iter()
            .map(|config| config.provider_name.to_string())
            .collect();

        let mut uploaders = Vec::with_capacity(configs.len());
        for config in configs {
            uploaders.push(Uploader::from_provider_config(config).await?);
        }
        self.uploaders = uploaders;

        if provider_names.is_empty() {
            warn!("uploader is running with no active remote providers loaded from DB");
        } else {
            info!("active providers loaded from DB for uploader: [{}]", provider_names.join(", "));
        }

        Ok(provider_names)
    }

    async fn process_job(&self, job: &db::UploadJob) -> Result<JobProcessOutcome, UploaderError> {
        if !self.test_process_delay.is_zero() {
            sleep(self.test_process_delay).await;
        }

        if self.app_config.dry_run_active {
            let message = format!(
                "[DRY-RUN] Would process cloud upload job for pack {} without performing S3 PUT/POST/DELETE operations.",
                job.pack_id
            );
            warn!("{message}");
            diagnostics::record_upload_error(message.clone());
            return Ok(JobProcessOutcome::PendingRetry {
                delay: self.retry_max_delay,
                failed_shards: vec![message],
            });
        }

        if cloud_guard::is_cloud_suspended(&self.pool)
            .await
            .map_err(|err| UploaderError::CloudGuard(err.to_string()))?
        {
            let reason = db::get_system_config_value(
                &self.pool,
                cloud_guard::SYSTEM_CONFIG_CLOUD_SUSPEND_REASON,
            )
            .await?
            .unwrap_or_else(|| "cloud operations suspended by circuit breaker".to_string());
            warn!("{reason}");
            diagnostics::record_upload_error(reason.clone());
            return Ok(JobProcessOutcome::PendingRetry {
                delay: self.retry_max_delay,
                failed_shards: vec![reason],
            });
        }

        let pack = db::get_pack(&self.pool, &job.pack_id)
            .await?
            .ok_or(UploaderError::InvalidEnv("upload_jobs.pack_id"))?;
        let storage_mode = StorageMode::from_str(&pack.storage_mode);
        let existing_shards = db::get_pack_shards(&self.pool, &job.pack_id).await?;
        let provider_names: Vec<&str> = existing_shards
            .iter()
            .map(|shard| shard.provider.as_str())
            .collect();
        db::ensure_upload_targets(&self.pool, job.id, &provider_names).await?;

        let pending_shards = db::get_incomplete_pack_shards(&self.pool, &job.pack_id).await?;
        if pending_shards.is_empty() {
            let summary = db::summarize_pack_shards(&self.pool, &job.pack_id).await?;
            let status = db::resolve_pack_status_for_mode(storage_mode, summary);
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
            let Some(uploader) = self
                .uploaders
                .iter()
                .find(|uploader| uploader.provider_name() == shard.provider.as_str())
            else {
                let message = format!(
                    "provider {} is not active in runtime configuration; waiting for hot-reload",
                    shard.provider
                );
                let attempts = db::requeue_pack_shard(
                    &self.pool,
                    &job.pack_id,
                    shard.shard_index,
                    &message,
                )
                .await?;
                db::requeue_upload_target(&self.pool, job.id, &shard.provider, &message).await?;
                max_attempts = max_attempts.max(attempts);
                failed_shards.push(format!(
                    "{} shard {}: {}",
                    shard.provider, shard.shard_index, message
                ));
                continue;
            };

            let current_usage =
                db::get_physical_usage_for_provider(&self.pool, &shard.provider).await?;
            let shard_size = u64::try_from(shard.size)
                .map_err(|_| UploaderError::InvalidEnv("pack_shards.size"))?;
            if let Err(message) = cloud_guard::enforce_single_upload_size_limit(shard_size) {
                diagnostics::record_upload_error(message.clone());
                warn!("{message}");
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
            let projected_usage = current_usage.saturating_add(shard_size);
            if projected_usage > self.app_config.max_physical_bytes_per_provider {
                let message = format!(
                    "quota exceeded for provider {}: projected={} limit={}",
                    shard.provider,
                    projected_usage,
                    self.app_config.max_physical_bytes_per_provider
                );
                diagnostics::record_upload_error(message.clone());
                warn!("{message}");
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

            match cloud_guard::current_decision(&self.pool, GuardOperation::Write { count: 1 })
                .await
                .map_err(|err| UploaderError::CloudGuard(err.to_string()))?
            {
                cloud_guard::GuardDecision::Allowed => {}
                cloud_guard::GuardDecision::DryRun { message } => {
                    warn!("{message}");
                    diagnostics::record_upload_error(message.clone());
                    return Ok(JobProcessOutcome::PendingRetry {
                        delay: self.retry_max_delay,
                        failed_shards: vec![message],
                    });
                }
                cloud_guard::GuardDecision::Suspended { reason }
                | cloud_guard::GuardDecision::QuotaExceeded { reason } => {
                    warn!("{reason}");
                    diagnostics::record_upload_error(reason.clone());
                    return Ok(JobProcessOutcome::PendingRetry {
                        delay: self.retry_max_delay,
                        failed_shards: vec![reason],
                    });
                }
            }

            let shard_path = local_shard_path(
                &self.spool_dir,
                &job.pack_id,
                usize::try_from(shard.shard_index)
                    .map_err(|_| UploaderError::InvalidEnv("pack_shards.shard_index"))?,
            );

            db::mark_pack_shard_in_progress(&self.pool, &job.pack_id, shard.shard_index).await?;
            db::mark_upload_target_in_progress(&self.pool, job.id, &shard.provider).await?;

            info!(
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
                    info!(
                        "uploaded pack={} shard={} provider={} key={}",
                        job.pack_id, shard.shard_index, uploaded.provider, uploaded.key
                    );
                    // G.2: record upload traffic for stats chart
                    let _ = db::record_traffic(&self.pool, shard.size, 0).await;
                }
                Ok(Err(err)) => {
                    if err.is_retryable() {
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
                    } else {
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
            }
        }

        let summary = db::summarize_pack_shards(&self.pool, &job.pack_id).await?;
        let status = db::resolve_pack_status_for_mode(storage_mode, summary);
        db::update_pack_status(&self.pool, &job.pack_id, status).await?;

        if matches!(status, PackStatus::Healthy | PackStatus::Degraded)
            && storage_mode != StorageMode::LocalOnly
        {
            self.cleanup_remote_backed_pack_spool(&job.pack_id).await;
        }

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

    async fn cleanup_remote_backed_pack_spool(&self, pack_id: &str) {
        let manifest_path = self
            .spool_dir
            .join(format!("{pack_id}.{LOCAL_PACK_EXTENSION}"));
        if let Err(err) = secure_delete(&manifest_path).await {
            warn!(
                "spool secure delete failed for manifest {}: {}",
                manifest_path.display(),
                err
            );
        }

        for shard_index in 0..TOTAL_SHARDS {
            let shard_path = local_shard_path(&self.spool_dir, pack_id, shard_index);
            if let Err(err) = secure_delete(&shard_path).await {
                warn!(
                    "spool secure delete failed for shard {}: {}",
                    shard_path.display(),
                    err
                );
            }
        }
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
        err: impl std::error::Error,
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

fn format_error_details(err: &impl std::error::Error) -> String {
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
