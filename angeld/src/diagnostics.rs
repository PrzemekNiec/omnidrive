use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;

static GLOBAL_DIAGNOSTICS: OnceLock<Arc<DaemonDiagnostics>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerKind {
    Uploader,
    Repair,
    Scrubber,
    Gc,
    Watcher,
    MetadataBackup,
    Peer,
    Api,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerStatus {
    Starting = 0,
    Idle = 1,
    Active = 2,
}

impl WorkerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Idle => "idle",
            Self::Active => "active",
        }
    }
}

#[derive(Debug)]
pub struct DiagnosticsSnapshot {
    pub uptime_seconds: u64,
    pub last_upload_error: Option<String>,
    pub uploader: WorkerStatus,
    pub repair: WorkerStatus,
    pub scrubber: WorkerStatus,
    pub gc: WorkerStatus,
    pub watcher: WorkerStatus,
    pub metadata_backup: WorkerStatus,
    pub peer: WorkerStatus,
    pub api: WorkerStatus,
}

pub struct DaemonDiagnostics {
    started_at: Instant,
    last_upload_error: RwLock<Option<String>>,
    uploader: AtomicU8,
    repair: AtomicU8,
    scrubber: AtomicU8,
    gc: AtomicU8,
    watcher: AtomicU8,
    metadata_backup: AtomicU8,
    peer: AtomicU8,
    api: AtomicU8,
}

impl DaemonDiagnostics {
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
            last_upload_error: RwLock::new(None),
            uploader: AtomicU8::new(WorkerStatus::Starting as u8),
            repair: AtomicU8::new(WorkerStatus::Starting as u8),
            scrubber: AtomicU8::new(WorkerStatus::Starting as u8),
            gc: AtomicU8::new(WorkerStatus::Starting as u8),
            watcher: AtomicU8::new(WorkerStatus::Starting as u8),
            metadata_backup: AtomicU8::new(WorkerStatus::Starting as u8),
            peer: AtomicU8::new(WorkerStatus::Starting as u8),
            api: AtomicU8::new(WorkerStatus::Starting as u8),
        }
    }

    pub fn set_worker_status(&self, worker: WorkerKind, status: WorkerStatus) {
        let target = match worker {
            WorkerKind::Uploader => &self.uploader,
            WorkerKind::Repair => &self.repair,
            WorkerKind::Scrubber => &self.scrubber,
            WorkerKind::Gc => &self.gc,
            WorkerKind::Watcher => &self.watcher,
            WorkerKind::MetadataBackup => &self.metadata_backup,
            WorkerKind::Peer => &self.peer,
            WorkerKind::Api => &self.api,
        };
        target.store(status as u8, Ordering::Relaxed);
    }

    pub fn record_upload_error(&self, message: impl Into<String>) {
        if let Ok(mut slot) = self.last_upload_error.write() {
            *slot = Some(message.into());
        }
    }

    pub fn clear_upload_error(&self) {
        if let Ok(mut slot) = self.last_upload_error.write() {
            *slot = None;
        }
    }

    pub fn snapshot(&self) -> DiagnosticsSnapshot {
        DiagnosticsSnapshot {
            uptime_seconds: self.started_at.elapsed().as_secs(),
            last_upload_error: self.last_upload_error.read().ok().and_then(|value| value.clone()),
            uploader: load_status(&self.uploader),
            repair: load_status(&self.repair),
            scrubber: load_status(&self.scrubber),
            gc: load_status(&self.gc),
            watcher: load_status(&self.watcher),
            metadata_backup: load_status(&self.metadata_backup),
            peer: load_status(&self.peer),
            api: load_status(&self.api),
        }
    }
}

fn load_status(slot: &AtomicU8) -> WorkerStatus {
    match slot.load(Ordering::Relaxed) {
        1 => WorkerStatus::Idle,
        2 => WorkerStatus::Active,
        _ => WorkerStatus::Starting,
    }
}

pub fn init_global_diagnostics() -> Arc<DaemonDiagnostics> {
    GLOBAL_DIAGNOSTICS
        .get_or_init(|| Arc::new(DaemonDiagnostics::new()))
        .clone()
}

pub fn global_diagnostics() -> Option<Arc<DaemonDiagnostics>> {
    GLOBAL_DIAGNOSTICS.get().cloned()
}

pub fn set_worker_status(worker: WorkerKind, status: WorkerStatus) {
    if let Some(diagnostics) = global_diagnostics() {
        diagnostics.set_worker_status(worker, status);
    }
}

pub fn record_upload_error(message: impl Into<String>) {
    if let Some(diagnostics) = global_diagnostics() {
        diagnostics.record_upload_error(message);
    }
}

pub fn clear_upload_error() {
    if let Some(diagnostics) = global_diagnostics() {
        diagnostics.clear_upload_error();
    }
}
