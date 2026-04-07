use crate::db;
use crate::diagnostics::{self, WorkerKind, WorkerStatus};
use crate::packer::{PackResult, Packer, PackerConfig};
use crate::vault::VaultKeyStore;
use serde::Serialize;
use sqlx::SqlitePool;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

// ── Ingest State Machine ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum IngestState {
    Pending,
    Chunking,
    Uploading,
    Ghosted,
    Failed,
}

impl IngestState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Chunking => "CHUNKING",
            Self::Uploading => "UPLOADING",
            Self::Ghosted => "GHOSTED",
            Self::Failed => "FAILED",
        }
    }

    pub fn from_db(s: &str) -> Option<Self> {
        match s {
            "PENDING" => Some(Self::Pending),
            "CHUNKING" => Some(Self::Chunking),
            "UPLOADING" => Some(Self::Uploading),
            "GHOSTED" => Some(Self::Ghosted),
            "FAILED" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn valid_transitions(self) -> &'static [IngestState] {
        match self {
            Self::Pending => &[Self::Chunking, Self::Failed],
            Self::Chunking => &[Self::Uploading, Self::Failed],
            Self::Uploading => &[Self::Ghosted, Self::Failed],
            Self::Ghosted => &[],
            Self::Failed => &[Self::Pending],
        }
    }

    pub fn can_transition_to(self, target: IngestState) -> bool {
        self.valid_transitions().contains(&target)
    }
}

impl fmt::Display for IngestState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Ingest Job (in-memory representation) ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct IngestJob {
    pub id: i64,
    pub file_path: String,
    pub file_size: i64,
    pub state: IngestState,
    pub bytes_processed: i64,
    pub attempt_count: i64,
    pub error_message: Option<String>,
}

impl IngestJob {
    pub fn from_row(row: &db::IngestJobRow) -> Option<Self> {
        let state = IngestState::from_db(&row.state)?;
        Some(Self {
            id: row.id,
            file_path: row.file_path.clone(),
            file_size: row.file_size,
            state,
            bytes_processed: row.bytes_processed,
            attempt_count: row.attempt_count,
            error_message: row.error_message.clone(),
        })
    }
}

// ── Ingest Error ──────────────────────────────────────────────────────

#[derive(Debug)]
pub enum IngestError {
    Db(sqlx::Error),
    InvalidTransition { from: IngestState, to: IngestState },
    Io(std::io::Error),
    Packer(crate::packer::PackerError),
}

impl fmt::Display for IngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Db(err) => write!(f, "database error: {err}"),
            Self::InvalidTransition { from, to } => {
                write!(f, "invalid state transition: {from} → {to}")
            }
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::Packer(err) => write!(f, "packer error: {err}"),
        }
    }
}

impl std::error::Error for IngestError {}

impl From<sqlx::Error> for IngestError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

impl From<std::io::Error> for IngestError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<crate::packer::PackerError> for IngestError {
    fn from(value: crate::packer::PackerError) -> Self {
        Self::Packer(value)
    }
}

// ── State transition helper ───────────────────────────────────────────

async fn transition(
    pool: &SqlitePool,
    job_id: i64,
    from: IngestState,
    to: IngestState,
) -> Result<(), IngestError> {
    if !from.can_transition_to(to) {
        return Err(IngestError::InvalidTransition { from, to });
    }
    let changed = db::transition_ingest_job(pool, job_id, from.as_str(), to.as_str()).await?;
    if !changed {
        return Err(IngestError::InvalidTransition { from, to });
    }
    Ok(())
}

// ── Background Worker ─────────────────────────────────────────────────

const UPLOAD_POLL_INTERVAL: Duration = Duration::from_secs(2);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(600);

pub struct IngestWorker {
    pool: SqlitePool,
    packer: Packer,
    poll_interval: Duration,
}

impl IngestWorker {
    pub fn new(pool: SqlitePool, vault_keys: VaultKeyStore, spool_dir: PathBuf) -> Self {
        let poll_ms: u64 = std::env::var("OMNIDRIVE_INGEST_POLL_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2_000);
        let chunk_size: usize = std::env::var("OMNIDRIVE_CHUNK_SIZE_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(crate::packer::DEFAULT_CHUNK_SIZE);

        let packer = Packer::new(
            pool.clone(),
            vault_keys,
            PackerConfig::new(&spool_dir).with_chunk_size(chunk_size),
        )
        .expect("ingest: packer initialization failed");

        Self {
            pool,
            packer,
            poll_interval: Duration::from_millis(poll_ms),
        }
    }

    /// Crash recovery: reset any jobs that were mid-flight (CHUNKING / UPLOADING)
    /// back to PENDING so they get retried on next cycle.
    async fn recover_interrupted_jobs(&self) -> Result<(), IngestError> {
        let reset_count = db::reset_interrupted_ingest_jobs(&self.pool).await?;
        if reset_count > 0 {
            warn!(
                "ingest: recovered {} interrupted job(s) back to PENDING",
                reset_count
            );
        }
        Ok(())
    }

    /// Main loop: poll for PENDING jobs, drive them through the state machine.
    pub async fn run(self) -> Result<(), IngestError> {
        self.recover_interrupted_jobs().await?;
        info!(
            "ingest: worker started, poll interval = {:?}",
            self.poll_interval
        );
        diagnostics::set_worker_status(WorkerKind::Ingest, WorkerStatus::Idle);

        loop {
            let job_row = db::get_next_pending_ingest_job(&self.pool).await?;

            let Some(row) = job_row else {
                diagnostics::set_worker_status(WorkerKind::Ingest, WorkerStatus::Idle);
                sleep(self.poll_interval).await;
                continue;
            };

            let Some(job) = IngestJob::from_row(&row) else {
                error!(
                    "ingest: job {} has unrecognized state '{}', skipping",
                    row.id, row.state
                );
                db::fail_ingest_job(&self.pool, row.id, "unrecognized state").await?;
                continue;
            };

            diagnostics::set_worker_status(WorkerKind::Ingest, WorkerStatus::Active);
            info!(
                "ingest: processing job {} — {} ({} bytes)",
                job.id, job.file_path, job.file_size
            );

            if let Err(err) = self.process_job(&job).await {
                error!("ingest: job {} failed — {}", job.id, err);
                let msg = format!("{err}");
                let _ = db::fail_ingest_job(&self.pool, job.id, &msg).await;
            }
        }
    }

    /// Drive a single job through PENDING → CHUNKING → UPLOADING → GHOSTED.
    /// Each phase is a guarded DB transition first, work second.
    /// If the process crashes between transition and work completion,
    /// `recover_interrupted_jobs` will reset it back to PENDING on next start.
    async fn process_job(&self, job: &IngestJob) -> Result<(), IngestError> {
        // ── Phase 1: PENDING → CHUNKING ───────────────────────────────
        transition(&self.pool, job.id, IngestState::Pending, IngestState::Chunking).await?;
        info!("ingest: job {} → CHUNKING", job.id);

        let pack_result = self.do_chunking(job).await?;

        // ── Phase 2: CHUNKING → UPLOADING ─────────────────────────────
        transition(
            &self.pool,
            job.id,
            IngestState::Chunking,
            IngestState::Uploading,
        )
        .await?;
        info!("ingest: job {} → UPLOADING", job.id);

        self.do_uploading(job, &pack_result).await?;

        // ── Phase 3: UPLOADING → GHOSTED ──────────────────────────────
        transition(
            &self.pool,
            job.id,
            IngestState::Uploading,
            IngestState::Ghosted,
        )
        .await?;
        info!(
            "ingest: job {} → GHOSTED (complete, {} chunks, {} packs)",
            job.id,
            pack_result.chunk_count,
            pack_result.pack_ids.len()
        );

        // TODO(35.1c): replace original file with cfapi placeholder

        Ok(())
    }

    /// CHUNKING phase: ensure inode exists, call Packer to chunk + encrypt + spool.
    /// Packer handles: SHA-256, DEK get/create, V2 AES-GCM encryption, erasure coding,
    /// shard files on disk, DB records (file_revisions, chunk_refs, packs, pack_shards,
    /// upload_jobs queued for UploadWorker).
    async fn do_chunking(&self, job: &IngestJob) -> Result<PackResult, IngestError> {
        let source_path = Path::new(&job.file_path);
        if !source_path.exists() {
            return Err(IngestError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("source file no longer exists: {}", job.file_path),
            )));
        }

        // Ensure inode hierarchy exists in DB for this file path.
        let metadata = tokio::fs::metadata(source_path).await?;
        let file_size = i64::try_from(metadata.len()).unwrap_or(i64::MAX);
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_millis() as i64)
            });
        let inode_id =
            ensure_inode_path_for_ingest(&self.pool, &job.file_path, file_size, mtime).await?;

        info!(
            "ingest: job {} — inode_id={}, calling packer for {}",
            job.id, inode_id, job.file_path
        );

        // Packer does the heavy lifting: chunking, DEK, encrypt, sharding, DB records.
        let pack_result = self.packer.pack_file(inode_id, source_path).await?;

        // Update progress to reflect completed chunking
        db::update_ingest_progress(&self.pool, job.id, job.file_size).await?;

        info!(
            "ingest: job {} — packed {} chunks into {} pack(s), revision={:?}, logical={} encrypted={}",
            job.id,
            pack_result.chunk_count,
            pack_result.pack_ids.len(),
            pack_result.revision_id,
            pack_result.logical_size,
            pack_result.encrypted_size,
        );

        Ok(pack_result)
    }

    /// UPLOADING phase: wait for UploadWorker to finish uploading all packs.
    /// The Packer already queued upload_jobs — we just poll until they all complete.
    async fn do_uploading(
        &self,
        job: &IngestJob,
        pack_result: &PackResult,
    ) -> Result<(), IngestError> {
        if pack_result.pack_ids.is_empty() {
            info!("ingest: job {} — no packs to upload (empty file or dedup)", job.id);
            return Ok(());
        }

        let deadline = tokio::time::Instant::now() + UPLOAD_TIMEOUT;

        loop {
            if tokio::time::Instant::now() > deadline {
                return Err(IngestError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "upload timeout after {:?} for job {}",
                        UPLOAD_TIMEOUT, job.id
                    ),
                )));
            }

            let mut all_done = true;
            let mut any_failed = false;

            for pack_id in &pack_result.pack_ids {
                let summary = db::summarize_pack_shards(&self.pool, pack_id).await?;
                let status = db::resolve_pack_status(summary);

                match status {
                    db::PackStatus::Healthy | db::PackStatus::Degraded => {
                        // This pack is done
                    }
                    db::PackStatus::Unreadable => {
                        any_failed = true;
                    }
                    db::PackStatus::Uploading => {
                        all_done = false;
                    }
                }
            }

            if any_failed {
                return Err(IngestError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "one or more packs failed upload for job {}",
                        job.id
                    ),
                )));
            }

            if all_done {
                info!(
                    "ingest: job {} — all {} pack(s) uploaded successfully",
                    job.id,
                    pack_result.pack_ids.len()
                );
                return Ok(());
            }

            sleep(UPLOAD_POLL_INTERVAL).await;
        }
    }
}

// ── Inode path helper ─────────────────────────────────────────────────

/// Build the inode hierarchy for a file path.  Mirrors the watcher logic:
/// split path into segments, upsert each as DIR except the last which is FILE.
async fn ensure_inode_path_for_ingest(
    pool: &SqlitePool,
    file_path: &str,
    file_size: i64,
    file_mtime: Option<i64>,
) -> Result<i64, IngestError> {
    let normalized = file_path.replace('\\', "/");
    let segments: Vec<&str> = normalized
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    if segments.is_empty() {
        return Err(IngestError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("empty path after normalization: {file_path}"),
        )));
    }

    let mut parent_id: Option<i64> = None;
    for (i, segment) in segments.iter().enumerate() {
        let is_last = i == segments.len() - 1;
        let kind = if is_last { "FILE" } else { "DIR" };
        let size = if is_last { file_size } else { 0 };
        let mtime = if is_last { file_mtime } else { None };
        let inode_id = db::upsert_inode(pool, parent_id, segment, kind, size, mtime).await?;
        parent_id = Some(inode_id);
    }

    parent_id.ok_or_else(|| {
        IngestError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no inode created",
        ))
    })
}
