#![allow(dead_code)]

use crate::db;
use crate::db::{PackStatus, StorageMode};
use crate::diagnostics::{self, WorkerKind, WorkerStatus};
use crate::packer::{
    DATA_SHARDS, PARITY_SHARDS, TOTAL_SHARDS, build_manifest_bytes, build_shards, compute_pack_id,
    local_pack_path, local_shard_path, storage_mode_scheme,
};
use crate::secure_fs::write_ephemeral_bytes;
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
use tracing::{error, info, warn};

pub struct RepairWorker {
    pool: SqlitePool,
    providers: HashMap<String, ProviderClient>,
    spool_dir: PathBuf,
    poll_interval: Duration,
    provider_timeout: Duration,
    retry_delay: Duration,
}

#[derive(Debug, Clone, Copy)]
enum RepairBatchMode {
    RepairOnly,
    ReconcileOnly,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RepairBatchReport {
    pub processed_packs: usize,
    pub repaired_packs: usize,
    pub reconciled_packs: usize,
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
    Packer(String),
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
            Self::Packer(message) => write!(f, "packer error: {message}"),
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
        diagnostics::set_worker_status(WorkerKind::Repair, WorkerStatus::Idle);

        loop {
            if let Some(pack) = db::get_next_pack_requiring_reconciliation(&self.pool).await? {
                diagnostics::set_worker_status(WorkerKind::Repair, WorkerStatus::Active);
                let desired_mode =
                    db::get_desired_storage_mode_for_pack(&self.pool, &pack.pack_id).await?;
                match self.reconcile_pack_mode(&pack).await {
                    Ok(()) => {
                        info!(
                            "repair worker reconciled pack {} to mode {}",
                            pack.pack_id,
                            desired_mode.as_str()
                        );
                    }
                    Err(err) => {
                        warn!(
                            "repair worker reconciliation failed for pack {}: {}",
                            pack.pack_id, err
                        );
                        diagnostics::set_worker_status(WorkerKind::Repair, WorkerStatus::Idle);
                        sleep(self.retry_delay).await;
                    }
                }
                continue;
            }

            if let Some(pack) = db::get_next_degraded_pack(&self.pool).await? {
                if !db::pack_requires_healthy(&self.pool, &pack.pack_id).await? {
                    diagnostics::set_worker_status(WorkerKind::Repair, WorkerStatus::Idle);
                    sleep(self.poll_interval).await;
                    continue;
                }

                diagnostics::set_worker_status(WorkerKind::Repair, WorkerStatus::Active);
                match self.repair_pack(&pack).await {
                    Ok(()) => {
                        info!("repair worker restored pack {} to healthy", pack.pack_id);
                    }
                    Err(err) => {
                        error!("repair worker failed for pack {}: {}", pack.pack_id, err);
                        diagnostics::set_worker_status(WorkerKind::Repair, WorkerStatus::Idle);
                        sleep(self.retry_delay).await;
                    }
                }
            } else {
                diagnostics::set_worker_status(WorkerKind::Repair, WorkerStatus::Idle);
                sleep(self.poll_interval).await;
            }
        }
    }

    pub async fn run_repair_batch_now(pool: SqlitePool) -> Result<RepairBatchReport, RepairError> {
        let worker = Self::from_env(pool).await?;
        worker.run_batch_now(RepairBatchMode::RepairOnly).await
    }

    pub async fn run_reconcile_batch_now(
        pool: SqlitePool,
    ) -> Result<RepairBatchReport, RepairError> {
        let worker = Self::from_env(pool).await?;
        worker.run_batch_now(RepairBatchMode::ReconcileOnly).await
    }

    async fn run_batch_now(
        self,
        mode: RepairBatchMode,
    ) -> Result<RepairBatchReport, RepairError> {
        let mut report = RepairBatchReport {
            processed_packs: 0,
            repaired_packs: 0,
            reconciled_packs: 0,
        };

        diagnostics::set_worker_status(WorkerKind::Repair, WorkerStatus::Active);

        loop {
            match mode {
                RepairBatchMode::ReconcileOnly => {
                    let Some(pack) = db::get_next_pack_requiring_reconciliation(&self.pool).await?
                    else {
                        break;
                    };
                    self.reconcile_pack_mode(&pack).await?;
                    report.processed_packs += 1;
                    report.reconciled_packs += 1;
                }
                RepairBatchMode::RepairOnly => {
                    let Some(pack) = db::get_next_degraded_pack(&self.pool).await? else {
                        break;
                    };

                    if !db::pack_requires_healthy(&self.pool, &pack.pack_id).await? {
                        continue;
                    }

                    self.repair_pack(&pack).await?;
                    report.processed_packs += 1;
                    report.repaired_packs += 1;
                }
            }
        }

        diagnostics::set_worker_status(WorkerKind::Repair, WorkerStatus::Idle);
        Ok(report)
    }

    async fn repair_pack(&self, pack: &db::PackRecord) -> Result<(), RepairError> {
        if db::StorageMode::from_str(&pack.storage_mode) != StorageMode::Ec2_1 {
            return Ok(());
        }
        info!(
            "repair degraded pack start: pack={} mode={} status={}",
            pack.pack_id,
            pack.storage_mode,
            pack.status
        );
        let shards = db::get_pack_shards(&self.pool, &pack.pack_id).await?;
        if shards.len() != TOTAL_SHARDS {
            return Err(RepairError::InvalidShardLayout(pack.pack_id.clone()));
        }

        let summary = db::summarize_pack_shards(&self.pool, &pack.pack_id).await?;
        let status = db::resolve_pack_status_for_mode(StorageMode::Ec2_1, summary);
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
        info!(
            "repair degraded pack reconstructing shard: pack={} shard={} provider={}",
            pack.pack_id,
            missing.shard_index,
            missing.provider
        );
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
                info!(
                    "repair degraded pack complete: pack={} shard={} provider={}",
                    pack.pack_id,
                    missing.shard_index,
                    missing.provider
                );

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

    async fn reconcile_pack_mode(&self, pack: &db::PackRecord) -> Result<(), RepairError> {
        let desired_mode = db::get_desired_storage_mode_for_pack(&self.pool, &pack.pack_id).await?;
        let current_mode = StorageMode::from_str(&pack.storage_mode);
        if current_mode == desired_mode {
            return Ok(());
        }

        info!(
            "repair reconcile start: old_pack={} current_mode={} desired_mode={}",
            pack.pack_id,
            current_mode.as_str(),
            desired_mode.as_str()
        );

        let ciphertext = self.load_ciphertext_for_pack(pack).await?;
        let chunk_id = vec_to_array_32(&pack.chunk_id, "chunk_id")?;
        let nonce = vec_to_array_12(&pack.nonce, "nonce")?;
        let gcm_tag = vec_to_array_16(&pack.gcm_tag, "gcm_tag")?;
        let logical_size = usize::try_from(pack.logical_size)
            .map_err(|_| RepairError::NumericOverflow("logical size"))?;
        let manifest_bytes = build_manifest_bytes(chunk_id, nonce, &ciphertext, &gcm_tag, logical_size)
            .map_err(|err| RepairError::Packer(err.to_string()))?;
        let new_pack_id = compute_pack_id(desired_mode, &manifest_bytes);
        let manifest_path = local_pack_path(&self.spool_dir, &new_pack_id);
        write_ephemeral_bytes(&manifest_path, &manifest_bytes)
            .await
            .map_err(|err| RepairError::Io(std::io::Error::other(err.to_string())))?;

        let prepared_shards = build_shards(&self.spool_dir, &new_pack_id, &ciphertext, desired_mode)
            .await
            .map_err(|err| RepairError::Packer(err.to_string()))?;
        let shard_size = prepared_shards.first().map(|shard| shard.size).unwrap_or(0);
        let manifest_size = i64::try_from(manifest_bytes.len())
            .map_err(|_| RepairError::NumericOverflow("manifest size"))?;

        db::create_pack(
            &self.pool,
            &new_pack_id,
            &pack.chunk_id,
            pack.plaintext_hash.as_deref().unwrap_or(""),
            desired_mode,
            1,
            storage_mode_scheme(desired_mode),
            pack.logical_size,
            pack.cipher_size,
            shard_size,
            &pack.nonce,
            &pack.gcm_tag,
            if desired_mode == StorageMode::LocalOnly {
                PackStatus::Healthy
            } else {
                PackStatus::Uploading
            },
        )
        .await?;

        if desired_mode != StorageMode::LocalOnly && db::get_pack_shards(&self.pool, &new_pack_id).await?.is_empty() {
            for shard in &prepared_shards {
                db::register_pack_shard(
                    &self.pool,
                    &new_pack_id,
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
        }

        if desired_mode == StorageMode::LocalOnly {
            db::update_pack_status(&self.pool, &new_pack_id, PackStatus::Healthy).await?;
            info!(
                "repair reconcile SWAP start: old_pack={} new_pack={} current_mode={} desired_mode={}",
                pack.pack_id,
                new_pack_id,
                current_mode.as_str(),
                desired_mode.as_str()
            );
            db::link_chunk_to_pack(&self.pool, &pack.chunk_id, &new_pack_id, 0, manifest_size).await?;
            info!(
                "repair reconcile SWAP complete: old_pack={} new_pack={} current_mode={} desired_mode={}",
                pack.pack_id,
                new_pack_id,
                current_mode.as_str(),
                desired_mode.as_str()
            );
            return Ok(());
        }

        let pending_shards = db::get_incomplete_pack_shards(&self.pool, &new_pack_id).await?;
        for shard in pending_shards {
            let provider = self
                .providers
                .get(&shard.provider)
                .ok_or_else(|| RepairError::InvalidEnv("reconcile provider not configured"))?;
            let local_shard = local_shard_path(
                &self.spool_dir,
                &new_pack_id,
                usize::try_from(shard.shard_index)
                    .map_err(|_| RepairError::NumericOverflow("reconcile shard index"))?,
            );
            let bytes = fs::read(&local_shard).await?;
            db::mark_pack_shard_in_progress(&self.pool, &new_pack_id, shard.shard_index).await?;
            match timeout(
                self.provider_timeout,
                self.upload_shard(
                    provider,
                    &shard.object_key,
                    &bytes,
                    &new_pack_id,
                    usize::try_from(shard.shard_index)
                        .map_err(|_| RepairError::NumericOverflow("reconcile shard index"))?,
                ),
            )
            .await
            {
                Ok(Ok(_)) => {
                    db::mark_pack_shard_completed(&self.pool, &new_pack_id, shard.shard_index).await?;
                }
                Ok(Err(err)) => {
                    db::requeue_pack_shard(&self.pool, &new_pack_id, shard.shard_index, &err.to_string()).await?;
                    let summary = db::summarize_pack_shards(&self.pool, &new_pack_id).await?;
                    let status = db::resolve_pack_status_for_mode(desired_mode, summary);
                    db::update_pack_status(&self.pool, &new_pack_id, status).await?;
                    return Err(err);
                }
                Err(_) => {
                    let err = RepairError::Timeout {
                        provider: provider.provider_name,
                        duration: self.provider_timeout,
                    };
                    db::requeue_pack_shard(&self.pool, &new_pack_id, shard.shard_index, &err.to_string()).await?;
                    let summary = db::summarize_pack_shards(&self.pool, &new_pack_id).await?;
                    let status = db::resolve_pack_status_for_mode(desired_mode, summary);
                    db::update_pack_status(&self.pool, &new_pack_id, status).await?;
                    return Err(err);
                }
            }
        }

        let summary = db::summarize_pack_shards(&self.pool, &new_pack_id).await?;
        let status = db::resolve_pack_status_for_mode(desired_mode, summary);
        db::update_pack_status(&self.pool, &new_pack_id, status).await?;
        if status == PackStatus::Healthy {
            info!(
                "repair reconcile SWAP start: old_pack={} new_pack={} current_mode={} desired_mode={}",
                pack.pack_id,
                new_pack_id,
                current_mode.as_str(),
                desired_mode.as_str()
            );
            db::link_chunk_to_pack(&self.pool, &pack.chunk_id, &new_pack_id, 0, manifest_size).await?;
            info!(
                "repair reconcile SWAP complete: old_pack={} new_pack={} current_mode={} desired_mode={}",
                pack.pack_id,
                new_pack_id,
                current_mode.as_str(),
                desired_mode.as_str()
            );
        }

        Ok(())
    }

    async fn load_ciphertext_for_pack(&self, pack: &db::PackRecord) -> Result<Vec<u8>, RepairError> {
        match StorageMode::from_str(&pack.storage_mode) {
            StorageMode::Ec2_1 => self.load_ec_ciphertext(pack).await,
            StorageMode::SingleReplica => self.load_single_replica_ciphertext(pack).await,
            StorageMode::LocalOnly => self.load_local_only_ciphertext(pack).await,
        }
    }

    async fn load_ec_ciphertext(&self, pack: &db::PackRecord) -> Result<Vec<u8>, RepairError> {
        let shards = db::get_pack_shards(&self.pool, &pack.pack_id).await?;
        let shard_len = usize::try_from(pack.shard_size)
            .map_err(|_| RepairError::NumericOverflow("pack shard size"))?;
        let mut shard_set: Vec<Option<Vec<u8>>> = vec![None; TOTAL_SHARDS];
        let mut completed = 0usize;

        for shard in shards.into_iter().filter(|shard| shard.status == "COMPLETED") {
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
            completed += 1;
        }

        if completed < DATA_SHARDS {
            return Err(RepairError::InvalidShardLayout(pack.pack_id.clone()));
        }

        let reed_solomon = ReedSolomon::new(DATA_SHARDS, PARITY_SHARDS)?;
        reed_solomon.reconstruct(&mut shard_set)?;

        let mut ciphertext = Vec::with_capacity(
            usize::try_from(pack.cipher_size)
                .map_err(|_| RepairError::NumericOverflow("cipher size"))?,
        );
        for shard in shard_set.iter().take(DATA_SHARDS) {
            let bytes = shard
                .as_ref()
                .ok_or(RepairError::MissingShardRecord("reconstructed data shard missing"))?;
            ciphertext.extend_from_slice(bytes);
        }
        let cipher_size = usize::try_from(pack.cipher_size)
            .map_err(|_| RepairError::NumericOverflow("cipher size"))?;
        ciphertext.truncate(cipher_size);
        Ok(ciphertext)
    }

    async fn load_single_replica_ciphertext(
        &self,
        pack: &db::PackRecord,
    ) -> Result<Vec<u8>, RepairError> {
        let shard = db::get_pack_shards(&self.pool, &pack.pack_id)
            .await?
            .into_iter()
            .find(|shard| shard.status == "COMPLETED")
            .ok_or(RepairError::MissingShardRecord("single replica shard missing"))?;
        let provider = self
            .providers
            .get(&shard.provider)
            .ok_or_else(|| RepairError::InvalidEnv("single replica provider not configured"))?;
        self.download_shard(provider, &shard.object_key, &pack.pack_id, shard.shard_index)
            .await
    }

    async fn load_local_only_ciphertext(
        &self,
        pack: &db::PackRecord,
    ) -> Result<Vec<u8>, RepairError> {
        let manifest_path = local_pack_path(&self.spool_dir, &pack.pack_id);
        let bytes = fs::read(&manifest_path).await?;
        let cipher_size = usize::try_from(pack.cipher_size)
            .map_err(|_| RepairError::NumericOverflow("cipher size"))?;
        let start = omnidrive_core::layout::ChunkRecordPrefix::SIZE;
        let end = start
            .checked_add(cipher_size)
            .ok_or(RepairError::NumericOverflow("cipher range"))?;
        if end > bytes.len() {
            return Err(RepairError::InvalidShardLayout(pack.pack_id.clone()));
        }
        Ok(bytes[start..end].to_vec())
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
        write_ephemeral_bytes(&shard_path, &bytes)
            .await
            .map_err(|err| RepairError::Io(std::io::Error::other(err.to_string())))?;

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
        write_ephemeral_bytes(&shard_path, bytes)
            .await
            .map_err(|err| RepairError::Io(std::io::Error::other(err.to_string())))?;

        info!(
            "repair reconcile upload start: pack={} shard={} provider={} key={}",
            pack_id,
            shard_index,
            provider.provider_name,
            object_key
        );
        let body = ByteStream::from(bytes.to_vec());
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
        info!(
            "repair reconcile upload complete: pack={} shard={} provider={} key={}",
            pack_id,
            shard_index,
            provider.provider_name,
            object_key
        );

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

fn vec_to_array_32(bytes: &[u8], field: &'static str) -> Result<[u8; 32], RepairError> {
    <[u8; 32]>::try_from(bytes)
        .map_err(|_| RepairError::Packer(format!("invalid stored {field} length")))
}

fn vec_to_array_16(bytes: &[u8], field: &'static str) -> Result<[u8; 16], RepairError> {
    <[u8; 16]>::try_from(bytes)
        .map_err(|_| RepairError::Packer(format!("invalid stored {field} length")))
}

fn vec_to_array_12(bytes: &[u8], field: &'static str) -> Result<[u8; 12], RepairError> {
    <[u8; 12]>::try_from(bytes)
        .map_err(|_| RepairError::Packer(format!("invalid stored {field} length")))
}
