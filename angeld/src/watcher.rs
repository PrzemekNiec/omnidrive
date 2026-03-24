#![allow(dead_code)]

use crate::config::AppConfig;
use crate::db;
use crate::diagnostics::{self, WorkerKind, WorkerStatus};
use crate::packer::{DEFAULT_CHUNK_SIZE, Packer, PackerConfig};
use crate::vault::VaultKeyStore;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior, interval, sleep_until};
use tracing::{info, warn};

pub struct FileWatcher {
    pool: SqlitePool,
    watch_roots: Vec<PathBuf>,
    spool_dir: PathBuf,
    packer: Packer,
    debounce_window: StdDuration,
    rescan_interval: StdDuration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileFingerprint {
    size: u64,
    mtime: Option<i64>,
}

#[derive(Debug)]
pub enum WatcherError {
    MissingEnv(&'static str),
    InvalidEnv(&'static str),
    Notify(notify::Error),
    Io(std::io::Error),
    Db(sqlx::Error),
    Packer(crate::packer::PackerError),
    Join(tokio::task::JoinError),
}

impl fmt::Display for WatcherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnv(key) => write!(f, "missing required environment variable {key}"),
            Self::InvalidEnv(key) => write!(f, "invalid environment variable {key}"),
            Self::Notify(err) => write!(f, "notify error: {err}"),
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::Db(err) => write!(f, "sqlite error: {err}"),
            Self::Packer(err) => write!(f, "packer error: {err}"),
            Self::Join(err) => write!(f, "task join error: {err}"),
        }
    }
}

impl std::error::Error for WatcherError {}

impl From<notify::Error> for WatcherError {
    fn from(value: notify::Error) -> Self {
        Self::Notify(value)
    }
}

impl From<std::io::Error> for WatcherError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<sqlx::Error> for WatcherError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

impl From<crate::packer::PackerError> for WatcherError {
    fn from(value: crate::packer::PackerError) -> Self {
        Self::Packer(value)
    }
}

impl From<tokio::task::JoinError> for WatcherError {
    fn from(value: tokio::task::JoinError) -> Self {
        Self::Join(value)
    }
}

impl FileWatcher {
    pub async fn from_env(
        pool: SqlitePool,
        vault_keys: VaultKeyStore,
    ) -> Result<Self, WatcherError> {
        let _ = dotenvy::dotenv();

        let app_config = AppConfig::from_env();
        let spool_dir = env_path("OMNIDRIVE_SPOOL_DIR", ".omnidrive/spool");
        let chunk_size = env::var("OMNIDRIVE_CHUNK_SIZE_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(DEFAULT_CHUNK_SIZE);
        let debounce_window = duration_from_env("OMNIDRIVE_WATCH_DEBOUNCE_MS", 750);
        let rescan_interval = duration_from_env("OMNIDRIVE_WATCH_RESCAN_MS", 30_000);

        let spool_dir = normalize_path(&spool_dir)?;
        fs::create_dir_all(&spool_dir).await?;

        let mut policies = db::list_sync_policies(&pool).await?;
        if policies.is_empty() {
            let watch_dir = app_config
                .default_watch_dir
                .ok_or(WatcherError::MissingEnv("OMNIDRIVE_WATCH_DIR"))?;
            let watch_dir = normalize_path(&watch_dir)?;
            fs::create_dir_all(&watch_dir).await?;
            let policy_path = path_to_policy_key(&watch_dir)?;
            db::upsert_sync_policy(&pool, &policy_path, true, true).await?;
            policies = db::list_sync_policies(&pool).await?;
        }

        let mut watch_roots = Vec::new();
        for policy in policies {
            let root = normalize_path(Path::new(&policy.path_prefix))?;
            fs::create_dir_all(&root).await?;
            if !watch_roots.iter().any(|existing| existing == &root) {
                watch_roots.push(root);
            }
        }

        let packer = Packer::new(
            pool.clone(),
            vault_keys,
            PackerConfig::new(&spool_dir).with_chunk_size(chunk_size),
        )?;

        Ok(Self {
            pool,
            watch_roots,
            spool_dir,
            packer,
            debounce_window,
            rescan_interval,
        })
    }

    pub async fn run(self) -> Result<(), WatcherError> {
        diagnostics::set_worker_status(WorkerKind::Watcher, WorkerStatus::Starting);
        let (tx, mut rx) = mpsc::unbounded_channel::<notify::Result<Event>>();
        let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |result| {
            let _ = tx.send(result);
        })?;

        let watch_roots = self.watch_roots.clone();
        for root in watch_roots.clone() {
            watcher.watch(root.as_path(), RecursiveMode::Recursive)?;
        }
        if let Err(err) = self.scan_existing_files().await {
            if is_vault_locked_error(&err) {
                warn!("watcher initial scan skipped while vault is locked");
            } else {
                return Err(err);
            }
        }
        diagnostics::set_worker_status(WorkerKind::Watcher, WorkerStatus::Idle);

        let mut processed_files = HashMap::new();
        let mut pending_paths: HashMap<PathBuf, Instant> = HashMap::new();
        let mut rescan_tick = interval(self.rescan_interval);
        rescan_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        rescan_tick.tick().await;

        loop {
            if let Some(next_due) = next_due_instant(&pending_paths) {
                tokio::select! {
                    maybe_event = rx.recv() => {
                        let Some(event_result) = maybe_event else {
                            break;
                        };
                        self.handle_event(event_result?, &mut pending_paths)?;
                    }
                    _ = sleep_until(next_due) => {
                        diagnostics::set_worker_status(WorkerKind::Watcher, WorkerStatus::Active);
                        self.flush_ready_paths(&mut pending_paths, &mut processed_files).await;
                        diagnostics::set_worker_status(WorkerKind::Watcher, WorkerStatus::Idle);
                    }
                    _ = rescan_tick.tick() => {
                        diagnostics::set_worker_status(WorkerKind::Watcher, WorkerStatus::Active);
                        if let Err(err) = self.scan_existing_files().await {
                            if is_vault_locked_error(&err) {
                                warn!("watcher periodic scan skipped while vault is locked");
                            } else {
                                warn!("watcher periodic scan failed: {err}");
                            }
                        }
                        diagnostics::set_worker_status(WorkerKind::Watcher, WorkerStatus::Idle);
                    }
                }
            } else {
                tokio::select! {
                    maybe_event = rx.recv() => {
                        let Some(event_result) = maybe_event else {
                            break;
                        };
                        self.handle_event(event_result?, &mut pending_paths)?;
                    }
                    _ = rescan_tick.tick() => {
                        diagnostics::set_worker_status(WorkerKind::Watcher, WorkerStatus::Active);
                        if let Err(err) = self.scan_existing_files().await {
                            if is_vault_locked_error(&err) {
                                warn!("watcher periodic scan skipped while vault is locked");
                            } else {
                                warn!("watcher periodic scan failed: {err}");
                            }
                        }
                        diagnostics::set_worker_status(WorkerKind::Watcher, WorkerStatus::Idle);
                    }
                }
            }
        }

        Ok(())
    }

    async fn scan_existing_files(&self) -> Result<(), WatcherError> {
        let mut processed_files = HashMap::new();
        let watch_roots = self.watch_roots.clone();
        for root in watch_roots {
            self.process_event_path(root.clone(), &mut processed_files)
                .await?;
        }
        Ok(())
    }

    fn handle_event(
        &self,
        event: Event,
        pending_paths: &mut HashMap<PathBuf, Instant>,
    ) -> Result<(), WatcherError> {
        if !is_relevant_event(&event.kind) {
            return Ok(());
        }

        for path in event.paths {
            let path = normalize_path(&path)?;
            if self.should_ignore_path(&path) {
                continue;
            }

            pending_paths.insert(path, Instant::now() + self.debounce_window);
        }

        Ok(())
    }

    async fn flush_ready_paths(
        &self,
        pending_paths: &mut HashMap<PathBuf, Instant>,
        processed_files: &mut HashMap<PathBuf, FileFingerprint>,
    ) {
        let now = Instant::now();
        let ready_paths: Vec<PathBuf> = pending_paths
            .iter()
            .filter_map(|(path, due)| {
                if *due <= now {
                    Some(path.clone())
                } else {
                    None
                }
            })
            .collect();

        for path in ready_paths {
            pending_paths.remove(&path);
            if let Err(err) = self.process_event_path(path, processed_files).await {
                warn!("watcher failed to process event: {err}");
            }
        }
    }

    async fn process_event_path(
        &self,
        path: PathBuf,
        processed_files: &mut HashMap<PathBuf, FileFingerprint>,
    ) -> Result<(), WatcherError> {
        let path = normalize_path(&path)?;
        if self.should_ignore_path(&path) {
            return Ok(());
        }

        let metadata = match fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.handle_deleted_path(&path, processed_files).await?;
                return Ok(());
            }
            Err(err) => return Err(err.into()),
        };

        if metadata.is_dir() {
            let files = collect_files_recursively(path).await?;
            for file in files {
                if let Err(err) = self.process_file(file.clone(), processed_files).await {
                    if is_vault_locked_error(&err) {
                        warn!("watcher skipped {} while vault is locked", file.display());
                        continue;
                    }
                    return Err(err);
                }
            }
            return Ok(());
        }

        if metadata.is_file() {
            if let Err(err) = self.process_file(path.clone(), processed_files).await {
                if is_vault_locked_error(&err) {
                    warn!("watcher skipped {} while vault is locked", path.display());
                    return Ok(());
                }
                return Err(err);
            }
        }

        Ok(())
    }

    async fn process_file(
        &self,
        file_path: PathBuf,
        processed_files: &mut HashMap<PathBuf, FileFingerprint>,
    ) -> Result<(), WatcherError> {
        if self.should_ignore_path(&file_path) {
            return Ok(());
        }

        let metadata = fs::metadata(&file_path).await?;
        if !metadata.is_file() {
            return Ok(());
        }

        let policy_path = path_to_policy_key(&file_path)?;
        let policy = db::find_sync_policy_for_path(&self.pool, &policy_path)
            .await?
            .ok_or(WatcherError::InvalidEnv("sync_policy_for_path"))?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|time| unix_timestamp_ms(time).ok());
        let fingerprint = FileFingerprint {
            size: metadata.len(),
            mtime,
        };

        if processed_files
            .get(&file_path)
            .is_some_and(|previous| previous == &fingerprint)
        {
            return Ok(());
        }

        let size = i64::try_from(metadata.len())
            .map_err(|_| WatcherError::InvalidEnv("file_size_overflow"))?;
        let inode_id =
            ensure_inode_path_from_db_path(&self.pool, &policy_path, size, mtime).await?;

        if policy.enable_versioning == 0
            && db::get_current_file_revision(&self.pool, inode_id)
                .await?
                .is_some()
        {
            db::delete_file_chunks(&self.pool, inode_id).await?;
        }

        let pack_result = self.packer.pack_file(inode_id, &file_path).await?;
        processed_files.insert(file_path.clone(), fingerprint);
        if let Some(pack_id) = pack_result.pack_id {
            info!("watcher packed {} into {}", file_path.display(), pack_id);
        }

        Ok(())
    }

    async fn handle_deleted_path(
        &self,
        path: &Path,
        processed_files: &mut HashMap<PathBuf, FileFingerprint>,
    ) -> Result<(), WatcherError> {
        processed_files.remove(path);

        let db_path = path_to_policy_key(path)?;
        let Some(inode_id) = db::resolve_path(&self.pool, &db_path).await? else {
            return Ok(());
        };
        let Some(inode) = db::get_inode_by_id(&self.pool, inode_id).await? else {
            return Ok(());
        };

        if inode.kind != "FILE" {
            return Ok(());
        }

        db::delete_file_chunks(&self.pool, inode_id).await?;
        db::delete_inode_record(&self.pool, inode_id).await?;
        info!("watcher removed {} from sqlite", path.display());
        Ok(())
    }

    fn should_ignore_path(&self, path: &Path) -> bool {
        path.starts_with(&self.spool_dir)
    }
}

async fn ensure_inode_path(
    pool: &SqlitePool,
    relative_path: &Path,
    file_size: i64,
    file_mtime: Option<i64>,
) -> Result<i64, WatcherError> {
    let mut parent_id = None;
    let mut components = relative_path.components().peekable();

    while let Some(component) = components.next() {
        let name = component.as_os_str().to_string_lossy();
        let is_last = components.peek().is_none();
        let kind = if is_last { "FILE" } else { "DIR" };
        let size = if is_last { file_size } else { 0 };
        let mtime = if is_last { file_mtime } else { None };

        let inode_id = db::upsert_inode(pool, parent_id, &name, kind, size, mtime).await?;
        parent_id = Some(inode_id);
    }

    parent_id.ok_or(WatcherError::InvalidEnv("relative_path"))
}

async fn ensure_inode_path_from_db_path(
    pool: &SqlitePool,
    db_path: &str,
    file_size: i64,
    file_mtime: Option<i64>,
) -> Result<i64, WatcherError> {
    let mut parent_id = None;
    let mut segments = db_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .peekable();

    while let Some(name) = segments.next() {
        let is_last = segments.peek().is_none();
        let kind = if is_last { "FILE" } else { "DIR" };
        let size = if is_last { file_size } else { 0 };
        let mtime = if is_last { file_mtime } else { None };
        let inode_id = db::upsert_inode(pool, parent_id, name, kind, size, mtime).await?;
        parent_id = Some(inode_id);
    }

    parent_id.ok_or(WatcherError::InvalidEnv("db_path"))
}

async fn collect_files_recursively(root: PathBuf) -> Result<Vec<PathBuf>, WatcherError> {
    let mut files = Vec::new();
    let mut stack = vec![root];

    while let Some(dir) = stack.pop() {
        let mut entries = fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let metadata = entry.metadata().await?;
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file() {
                files.push(path);
            }
        }
    }

    Ok(files)
}

fn is_relevant_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

fn env_path(key: &str, default: &str) -> PathBuf {
    env::var(key)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(default))
}

fn duration_from_env(key: &str, default_ms: u64) -> StdDuration {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(StdDuration::from_millis)
        .unwrap_or_else(|| StdDuration::from_millis(default_ms))
}

fn next_due_instant(pending_paths: &HashMap<PathBuf, Instant>) -> Option<Instant> {
    pending_paths.values().copied().min()
}

fn is_vault_locked_error(err: &WatcherError) -> bool {
    matches!(
        err,
        WatcherError::Packer(crate::packer::PackerError::Vault(
            crate::vault::VaultError::Locked
        ))
    )
}

fn normalize_path(path: &Path) -> Result<PathBuf, WatcherError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };

    Ok(absolute)
}

fn unix_timestamp_ms(time: SystemTime) -> Result<i64, WatcherError> {
    let millis = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| WatcherError::InvalidEnv("system_time_before_epoch"))?
        .as_millis();

    i64::try_from(millis).map_err(|_| WatcherError::InvalidEnv("timestamp_overflow"))
}

fn path_to_policy_key(path: &Path) -> Result<String, WatcherError> {
    let absolute = normalize_path(path)?;
    let normalized = absolute.to_string_lossy().replace('\\', "/");
    let segments: Vec<String> = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_string())
        .collect();
    if segments.is_empty() {
        return Err(WatcherError::InvalidEnv("path_to_policy_key"));
    }

    Ok(segments.join("/"))
}
