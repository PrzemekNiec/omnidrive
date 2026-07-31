use crate::downloader::Downloader;
use serde::Serialize;
use sqlx::SqlitePool;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug)]
pub enum SmartSyncError {
    Io(std::io::Error),
    Sqlx(sqlx::Error),
    InvalidPath(&'static str),
    InvalidPathWithContext(&'static str, String),
    #[cfg_attr(windows, allow(dead_code))]
    UnsupportedPlatform,
    #[cfg(windows)]
    Windows(windows::core::Error),
}

impl fmt::Display for SmartSyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::Sqlx(err) => write!(f, "database error: {err}"),
            Self::InvalidPath(reason) => write!(f, "invalid sync root path: {reason}"),
            Self::InvalidPathWithContext(step, detail) => write!(f, "{step} failed: {detail}"),
            Self::UnsupportedPlatform => {
                write!(f, "smart sync bootstrap is only supported on Windows")
            }
            #[cfg(windows)]
            Self::Windows(err) => write!(f, "windows error: {err}"),
        }
    }
}

impl std::error::Error for SmartSyncError {}

impl From<std::io::Error> for SmartSyncError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<sqlx::Error> for SmartSyncError {
    fn from(value: sqlx::Error) -> Self {
        Self::Sqlx(value)
    }
}

#[cfg(windows)]
impl From<windows::core::Error> for SmartSyncError {
    fn from(value: windows::core::Error) -> Self {
        Self::Windows(value)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SyncRootStateSnapshot {
    pub path: String,
    pub path_exists: bool,
    pub registered: bool,
    pub registered_for_provider: bool,
    pub connected: bool,
    pub provider_name: Option<String>,
    pub provider_version: Option<String>,
    pub identity: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SyncRootRepairReport {
    pub actions: Vec<String>,
    pub sync_root_state: SyncRootStateSnapshot,
}

pub async fn register_sync_root(sync_root_path: &Path) -> Result<(), SmartSyncError> {
    #[cfg(windows)]
    {
        imp::register_sync_root_public(sync_root_path).await
    }

    #[cfg(not(windows))]
    {
        let _ = sync_root_path;
        Err(SmartSyncError::UnsupportedPlatform)
    }
}

pub fn audit_sync_root_state(
    sync_root_path: &Path,
) -> Result<SyncRootStateSnapshot, SmartSyncError> {
    #[cfg(windows)]
    {
        imp::audit_sync_root_state(sync_root_path)
    }

    #[cfg(not(windows))]
    {
        let _ = sync_root_path;
        Err(SmartSyncError::UnsupportedPlatform)
    }
}

pub async fn repair_sync_root(
    pool: &SqlitePool,
    sync_root_path: &Path,
) -> Result<SyncRootRepairReport, SmartSyncError> {
    #[cfg(windows)]
    {
        imp::repair_sync_root(pool, sync_root_path).await
    }

    #[cfg(not(windows))]
    {
        let _ = pool;
        let _ = sync_root_path;
        Err(SmartSyncError::UnsupportedPlatform)
    }
}

pub fn shutdown_sync_root() -> Result<(), SmartSyncError> {
    #[cfg(windows)]
    {
        imp::shutdown_sync_root()
    }

    #[cfg(not(windows))]
    {
        Err(SmartSyncError::UnsupportedPlatform)
    }
}

pub fn unregister_sync_root(sync_root_path: &Path) -> Result<(), SmartSyncError> {
    #[cfg(windows)]
    {
        imp::unregister_sync_root(sync_root_path)
    }

    #[cfg(not(windows))]
    {
        let _ = sync_root_path;
        Err(SmartSyncError::UnsupportedPlatform)
    }
}

pub fn install_hydration_runtime(
    pool: SqlitePool,
    downloader: Arc<Downloader>,
) -> Result<(), SmartSyncError> {
    #[cfg(windows)]
    {
        imp::install_hydration_runtime(pool, downloader)
    }

    #[cfg(not(windows))]
    {
        let _ = pool;
        let _ = downloader;
        Err(SmartSyncError::UnsupportedPlatform)
    }
}

/// Security (P0): Full vault lock sequence —
/// 1. Recursive dehydrate of every file in OmniSync (removes decrypted cache)
/// 2. CfDisconnectSyncRoot (sync provider gone)
/// 3. CfUnregisterSyncRoot (removes CF reparse tags / registry entry)
/// 4. Unmounts the virtual drive from Explorer
pub async fn dismount_after_lock(sync_root_path: &Path) -> Result<(), SmartSyncError> {
    #[cfg(windows)]
    {
        imp::dismount_after_lock(sync_root_path).await
    }
    #[cfg(not(windows))]
    {
        let _ = sync_root_path;
        Ok(())
    }
}

/// Vault unlock sequence —
/// 1. CfRegisterSyncRoot + CfConnectSyncRoot
/// 2. Project all vault files as dehydrated placeholders
///
/// Caller is responsible for virtual drive hide + mount afterwards.
pub async fn mount_after_unlock(
    pool: &SqlitePool,
    sync_root_path: &Path,
) -> Result<(), SmartSyncError> {
    #[cfg(windows)]
    {
        imp::mount_after_unlock(pool, sync_root_path).await
    }
    #[cfg(not(windows))]
    {
        let _ = (pool, sync_root_path);
        Ok(())
    }
}

pub async fn project_vault_to_sync_root(
    pool: &SqlitePool,
    sync_root_path: &Path,
) -> Result<(), SmartSyncError> {
    #[cfg(windows)]
    {
        imp::project_vault_to_sync_root(pool, sync_root_path).await
    }

    #[cfg(not(windows))]
    {
        let _ = pool;
        let _ = sync_root_path;
        Err(SmartSyncError::UnsupportedPlatform)
    }
}

#[allow(dead_code)]
pub async fn evict_unpinned_hydrated_files(
    pool: &SqlitePool,
    sync_root_path: &Path,
) -> Result<usize, SmartSyncError> {
    #[cfg(windows)]
    {
        imp::evict_unpinned_hydrated_files(pool, sync_root_path).await
    }

    #[cfg(not(windows))]
    {
        let _ = pool;
        let _ = sync_root_path;
        Err(SmartSyncError::UnsupportedPlatform)
    }
}

pub async fn sync_placeholder_pin_state(
    pool: &SqlitePool,
    sync_root_path: &Path,
    inode_id: i64,
    dehydrate_immediately: bool,
) -> Result<(), SmartSyncError> {
    #[cfg(windows)]
    {
        imp::sync_placeholder_pin_state(pool, sync_root_path, inode_id, dehydrate_immediately).await
    }

    #[cfg(not(windows))]
    {
        let _ = pool;
        let _ = sync_root_path;
        let _ = inode_id;
        let _ = dehydrate_immediately;
        Err(SmartSyncError::UnsupportedPlatform)
    }
}

/// Convert an existing real file into a cfapi placeholder and dehydrate it.
/// Used by the ingest pipeline after upload completes (Epic 35.1c).
/// The file is converted in-place: CfConvertToPlaceholder + dehydrate.
/// If anything fails, the original file remains untouched.
pub async fn convert_to_ghost(
    pool: &SqlitePool,
    sync_root_path: &Path,
    inode_id: i64,
    revision_id: i64,
    file_size: i64,
) -> Result<(), SmartSyncError> {
    #[cfg(windows)]
    {
        imp::convert_to_ghost(pool, sync_root_path, inode_id, revision_id, file_size).await
    }

    #[cfg(not(windows))]
    {
        let _ = (pool, sync_root_path, inode_id, revision_id, file_size);
        Err(SmartSyncError::UnsupportedPlatform)
    }
}

pub async fn hydrate_placeholder_now(
    pool: &SqlitePool,
    sync_root_path: &Path,
    inode_id: i64,
) -> Result<(), SmartSyncError> {
    #[cfg(windows)]
    {
        imp::hydrate_placeholder_now(pool, sync_root_path, inode_id).await
    }

    #[cfg(not(windows))]
    {
        let _ = pool;
        let _ = sync_root_path;
        let _ = inode_id;
        Err(SmartSyncError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
mod imp;
