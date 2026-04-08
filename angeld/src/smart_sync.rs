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

pub fn audit_sync_root_state(sync_root_path: &Path) -> Result<SyncRootStateSnapshot, SmartSyncError> {
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

pub async fn reset_placeholders_after_unlock(
    pool: &SqlitePool,
    sync_root_path: &Path,
) -> Result<(), SmartSyncError> {
    #[cfg(windows)]
    {
        imp::reset_placeholders_after_unlock(pool, sync_root_path).await
    }

    #[cfg(not(windows))]
    {
        let _ = pool;
        let _ = sync_root_path;
        Err(SmartSyncError::UnsupportedPlatform)
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
mod imp {
    use super::{SmartSyncError, SyncRootRepairReport, SyncRootStateSnapshot};
    use crate::db::{self, ProjectionFileRecord};
    use crate::downloader::Downloader;
    use crate::win_acl;
    use sqlx::SqlitePool;
    use sha2::{Digest, Sha256};
    use std::ffi::OsStr;
    use std::iter;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use std::os::windows::io::AsRawHandle;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::path::Component;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::ptr;
    use std::sync::{Arc, OnceLock};
    use std::time::{Duration, UNIX_EPOCH};
    use tokio::runtime::Handle;
    use tracing::{error, info, trace, warn};
    use windows::core::{GUID, HRESULT, PCWSTR};
    use windows::Win32::Foundation::{HANDLE, NTSTATUS, RPC_E_CHANGED_MODE, S_FALSE, S_OK};
    use windows::Win32::Storage::CloudFilters::{
        CfConnectSyncRoot, CfConvertToPlaceholder, CfCreatePlaceholders,
        CfDisconnectSyncRoot, CfExecute, CfGetSyncRootInfoByPath,
        CfHydratePlaceholder, CfSetInSyncState,
        CfRegisterSyncRoot, CfSetPinState, CfUnregisterSyncRoot, CfUpdatePlaceholder,
        CF_CONVERT_FLAG_NONE,
        CF_IN_SYNC_STATE_IN_SYNC, CF_IN_SYNC_STATE_NOT_IN_SYNC,
        CF_SET_IN_SYNC_FLAG_NONE,
        CF_CALLBACK_INFO, CF_CALLBACK_PARAMETERS,
        CF_CALLBACK_REGISTRATION, CF_CALLBACK_TYPE_CANCEL_FETCH_DATA,
        CF_CALLBACK_TYPE_FETCH_DATA, CF_CALLBACK_TYPE_FETCH_PLACEHOLDERS,
        CF_CALLBACK_TYPE_NONE, CF_CONNECT_FLAG_NONE,
        CF_CONNECTION_KEY, CF_CREATE_FLAGS, CF_CREATE_FLAG_NONE, CF_CREATE_FLAG_STOP_ON_ERROR,
        CF_HYDRATE_FLAGS,
        CF_FILE_RANGE, CF_FS_METADATA, CF_HARDLINK_POLICY, CF_HARDLINK_POLICY_NONE,
        CF_HYDRATION_POLICY, CF_HYDRATION_POLICY_FULL, CF_HYDRATION_POLICY_MODIFIER,
        CF_HYDRATION_POLICY_MODIFIER_NONE, CF_HYDRATION_POLICY_PRIMARY, CF_INSYNC_POLICY,
        CF_INSYNC_POLICY_NONE, CF_OPERATION_INFO, CF_OPERATION_PARAMETERS, CF_PIN_STATE,
        CF_PIN_STATE_PINNED, CF_PIN_STATE_UNPINNED, CF_OPERATION_TRANSFER_DATA_FLAGS,
        CF_OPERATION_TRANSFER_PLACEHOLDERS_FLAGS,
        CF_OPERATION_TYPE_TRANSFER_DATA, CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS,
        CF_PLACEHOLDER_CREATE_FLAGS,
        CF_PLACEHOLDER_CREATE_INFO, CF_PLACEHOLDER_MANAGEMENT_POLICY,
        CF_PLACEHOLDER_MANAGEMENT_POLICY_CREATE_UNRESTRICTED, CF_POPULATION_POLICY,
        CF_POPULATION_POLICY_FULL, CF_POPULATION_POLICY_MODIFIER,
        CF_POPULATION_POLICY_MODIFIER_NONE, CF_POPULATION_POLICY_PRIMARY, CF_REGISTER_FLAGS,
        CF_REGISTER_FLAG_NONE, CF_REGISTER_FLAG_UPDATE, CF_SET_PIN_FLAG_NONE, CF_SET_PIN_FLAGS, CF_SYNC_POLICIES,
        CF_SYNC_REGISTRATION, CF_SYNC_ROOT_INFO_STANDARD, CF_SYNC_ROOT_STANDARD_INFO,
        CF_UPDATE_FLAG_DEHYDRATE, CF_UPDATE_FLAG_NONE, CF_UPDATE_FLAGS,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_ARCHIVE, FILE_BASIC_INFO,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
    use windows::Win32::UI::Shell::{SHCNE_UPDATEITEM, SHCNF_PATHW, SHChangeNotify};

    const PROVIDER_NAME: &str = "OmniDrive";
    const PROVIDER_VERSION: &str = "1.0";
    const ACCOUNT_NAME: &str = "UserVault";
    const PROVIDER_ID: GUID = GUID::from_u128(0xb7a42c2a_4af1_4f4a_a650_0b1308b8f019);
    const STATUS_UNSUCCESSFUL: i32 = 0xC0000001u32 as i32;
    const STATUS_SUCCESS: i32 = 0;
    static CONNECTION_KEY: OnceLock<CF_CONNECTION_KEY> = OnceLock::new();
    static HYDRATION_CONTEXT: OnceLock<HydrationContext> = OnceLock::new();

    struct ExistingSyncRootInfo {
        provider_name: String,
        provider_version: String,
        identity_bytes: Vec<u8>,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct PlaceholderIdentity {
        inode_id: i64,
        revision_id: i64,
    }

    #[derive(Clone)]
    struct HydrationContext {
        pool: SqlitePool,
        runtime: Handle,
        downloader: Arc<Downloader>,
    }

    #[derive(Clone, Copy)]
    struct HydrationRequest {
        connection_key: CF_CONNECTION_KEY,
        transfer_key: i64,
        request_key: i64,
        inode_id: i64,
        revision_id: i64,
        offset: i64,
        length: i64,
    }

    struct ComApartmentGuard {
        should_uninitialize: bool,
    }

    pub fn install_hydration_runtime(
        pool: SqlitePool,
        downloader: Arc<Downloader>,
    ) -> Result<(), SmartSyncError> {
        let context = HydrationContext {
            pool,
            runtime: Handle::current(),
            downloader,
        };

        let _ = HYDRATION_CONTEXT.set(context);
        Ok(())
    }

    fn flush_smart_sync_logs() {
        crate::logging::flush_logs_best_effort();
    }

    unsafe extern "system" fn fetch_data_callback(
        callback_info: *const CF_CALLBACK_INFO,
        callback_parameters: *const CF_CALLBACK_PARAMETERS,
    ) {
        let result = catch_unwind(AssertUnwindSafe(|| unsafe {
            fetch_data_callback_inner(callback_info, callback_parameters)
        }));
        if let Err(panic_payload) = result {
            log_callback_panic("FETCH_DATA", panic_payload);
            if !callback_info.is_null() && !callback_parameters.is_null() {
                let callback_info = unsafe { &*callback_info };
                let callback_parameters = unsafe { &*callback_parameters };
                let fetch = unsafe { callback_parameters.Anonymous.FetchData };
                let _ = complete_transfer_failure(
                    callback_info,
                    fetch.RequiredFileOffset,
                    fetch.RequiredLength,
                );
            }
        }
    }

    unsafe fn fetch_data_callback_inner(
        callback_info: *const CF_CALLBACK_INFO,
        callback_parameters: *const CF_CALLBACK_PARAMETERS,
    ) {
        if callback_info.is_null() || callback_parameters.is_null() {
            return;
        }

        let callback_info = unsafe { &*callback_info };
        let callback_parameters = unsafe { &*callback_parameters };
        let fetch = unsafe { callback_parameters.Anonymous.FetchData };

        let Some(identity) = decode_file_identity(
            callback_info.FileIdentity,
            callback_info.FileIdentityLength,
        ) else {
            warn!(
                "smart-sync: hydration requested with invalid identity, request_key={}",
                callback_info.RequestKey
            );
            let _ = complete_transfer_failure(
                callback_info,
                fetch.RequiredFileOffset,
                fetch.RequiredLength,
            );
            return;
        };

        let request = HydrationRequest {
            connection_key: callback_info.ConnectionKey,
            transfer_key: callback_info.TransferKey,
            request_key: callback_info.RequestKey,
            inode_id: identity.inode_id,
            revision_id: identity.revision_id,
            offset: fetch.RequiredFileOffset,
            length: fetch.RequiredLength,
        };

        info!(
            "Hydration requested for inode: {}, revision: {}, offset: {}, length: {}",
            request.inode_id, request.revision_id, request.offset, request.length
        );

        let Some(context) = HYDRATION_CONTEXT.get().cloned() else {
            warn!(
                "smart-sync: hydration runtime missing, request_key={}",
                request.request_key
            );
            flush_smart_sync_logs();
            let _ = complete_transfer_failure_from_request(&request);
            return;
        };

        if !context.downloader.has_remote_providers() {
            warn!(
                "smart-sync: no remote providers configured for request_key={}, inode={}, revision={}; returning empty hydration result in setup/local-only mode",
                request.request_key,
                request.inode_id,
                request.revision_id
            );
            flush_smart_sync_logs();
            let _ = complete_transfer_success(&request, &[]);
            return;
        }

        context.runtime.spawn(async move {
            let offset = match u64::try_from(request.offset) {
                Ok(value) => value,
                Err(_) => {
                    warn!(
                        "smart-sync: invalid negative offset for inode={}, revision={}",
                        request.inode_id, request.revision_id
                    );
                    flush_smart_sync_logs();
                    let _ = complete_transfer_failure_from_request(&request);
                    return;
                }
            };
            let length = match u64::try_from(request.length) {
                Ok(value) => value,
                Err(_) => {
                    warn!(
                        "smart-sync: invalid negative length for inode={}, revision={}",
                        request.inode_id, request.revision_id
                    );
                    flush_smart_sync_logs();
                    let _ = complete_transfer_failure_from_request(&request);
                    return;
                }
            };

            // Streamed hydration: download + decrypt one chunk at a time,
            // feed each slice to Windows immediately via CfExecute, then
            // drop the chunk before loading the next.  Peak RAM ≤ 1 chunk.
            let stream_result = context
                .downloader
                .read_range_streamed(
                    request.inode_id,
                    request.revision_id,
                    offset,
                    length,
                    |chunk_offset, chunk_bytes| {
                        let file_offset = i64::try_from(chunk_offset).map_err(|_| {
                            crate::downloader::DownloaderError::NumericOverflow("chunk offset")
                        })?;
                        complete_transfer_chunk(&request, file_offset, chunk_bytes).map_err(
                            |err| {
                                crate::downloader::DownloaderError::Io(std::io::Error::other(
                                    format!("CfExecute transfer failed: {err}"),
                                ))
                            },
                        )
                    },
                )
                .await;

            match stream_result {
                Ok(()) => {
                    if let Err(err) = db::set_hydration_state(&context.pool, request.inode_id, 1).await {
                        warn!(
                            "smart-sync: failed to persist hydration state for inode={}: {}",
                            request.inode_id, err
                        );
                    }
                    if let Ok(path) = projection_path_for_inode(&context.pool, request.inode_id).await {
                        if let Err(err) = mark_in_sync(&path, true) {
                            warn!("smart-sync: mark_in_sync after hydration failed for inode={}: {}", request.inode_id, err);
                        }
                        notify_shell_path_changed(&path);
                    }
                }
                Err(err) => {
                    if !context.downloader.has_remote_providers() {
                        warn!(
                            "smart-sync: local-only setup mode could not hydrate inode={}, revision={} without configured remote providers: {}",
                            request.inode_id, request.revision_id, err
                        );
                    }
                    warn!(
                        "smart-sync: streamed hydration failed for inode={}, revision={}, offset={}, length={}: {}",
                        request.inode_id, request.revision_id, request.offset, request.length, err
                    );
                    flush_smart_sync_logs();
                    let _ = complete_transfer_failure_from_request(&request);
                }
            }
        });
    }

    /// Callback for FETCH_PLACEHOLDERS.  We pre-create all placeholders at
    /// startup, so there is nothing new to return.  However, we MUST call
    /// CfExecute with CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS (zero entries)
    /// to complete the request — otherwise the minifilter blocks directory
    /// enumeration indefinitely, causing Explorer timeouts.
    unsafe extern "system" fn fetch_placeholders_callback(
        callback_info: *const CF_CALLBACK_INFO,
        _callback_parameters: *const CF_CALLBACK_PARAMETERS,
    ) {
        let result = catch_unwind(AssertUnwindSafe(|| unsafe {
            fetch_placeholders_callback_inner(callback_info)
        }));
        if let Err(panic_payload) = result {
            log_callback_panic("FETCH_PLACEHOLDERS", panic_payload);
        }
    }

    unsafe fn fetch_placeholders_callback_inner(
        callback_info: *const CF_CALLBACK_INFO,
    ) {
        if callback_info.is_null() {
            return;
        }
        let info = unsafe { &*callback_info };

        trace!("smart-sync: FETCH_PLACEHOLDERS callback invoked, completing with zero entries");

        let operation_info = CF_OPERATION_INFO {
            StructSize: size_of::<CF_OPERATION_INFO>() as u32,
            Type: CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS,
            ConnectionKey: info.ConnectionKey,
            TransferKey: info.TransferKey,
            CorrelationVector: ptr::null(),
            SyncStatus: ptr::null(),
            RequestKey: info.RequestKey,
        };

        let mut operation_parameters = CF_OPERATION_PARAMETERS {
            ParamSize: size_of::<CF_OPERATION_PARAMETERS>() as u32,
            ..Default::default()
        };
        operation_parameters.Anonymous.TransferPlaceholders =
            windows::Win32::Storage::CloudFilters::CF_OPERATION_PARAMETERS_0_4 {
                Flags: CF_OPERATION_TRANSFER_PLACEHOLDERS_FLAGS(0),
                CompletionStatus: NTSTATUS(STATUS_SUCCESS),
                PlaceholderTotalCount: 0,
                PlaceholderArray: ptr::null_mut(),
                PlaceholderCount: 0,
                EntriesProcessed: 0,
            };

        if let Err(err) = unsafe { CfExecute(&operation_info, &mut operation_parameters) } {
            warn!("smart-sync: FETCH_PLACEHOLDERS CfExecute failed: {}", err);
        }
    }

    unsafe extern "system" fn cancel_fetch_data_callback(
        callback_info: *const CF_CALLBACK_INFO,
        callback_parameters: *const CF_CALLBACK_PARAMETERS,
    ) {
        let result = catch_unwind(AssertUnwindSafe(|| unsafe {
            cancel_fetch_data_callback_inner(callback_info, callback_parameters)
        }));
        if let Err(panic_payload) = result {
            log_callback_panic("CANCEL_FETCH_DATA", panic_payload);
        }
    }

    unsafe fn cancel_fetch_data_callback_inner(
        callback_info: *const CF_CALLBACK_INFO,
        callback_parameters: *const CF_CALLBACK_PARAMETERS,
    ) {
        if callback_info.is_null() || callback_parameters.is_null() {
            return;
        }

        let callback_info = unsafe { &*callback_info };
        let callback_parameters = unsafe { &*callback_parameters };
        let cancel = unsafe { callback_parameters.Anonymous.Cancel };
        let fetch = unsafe { cancel.Anonymous.FetchData };

        let identity = decode_file_identity(
            callback_info.FileIdentity,
            callback_info.FileIdentityLength,
        );

        match identity {
            Some(identity) => {
                warn!(
                    "smart-sync: hydration canceled for inode={}, revision={}, offset={}, length={}",
                    identity.inode_id,
                    identity.revision_id,
                    fetch.FileOffset,
                    fetch.Length
                );
            }
            None => {
                warn!(
                    "smart-sync: hydration canceled for unknown identity, offset={}, length={}",
                    fetch.FileOffset,
                    fetch.Length
                );
            }
        }
    }

    fn log_callback_panic(callback_name: &str, panic_payload: Box<dyn std::any::Any + Send>) {
        let message = if let Some(text) = panic_payload.downcast_ref::<&str>() {
            (*text).to_string()
        } else if let Some(text) = panic_payload.downcast_ref::<String>() {
            text.clone()
        } else {
            "non-string panic payload".to_string()
        };

        error!(
            "smart-sync: {} callback panicked: {}",
            callback_name,
            message
        );
        eprintln!("smart-sync: {} callback panicked: {}", callback_name, message);
        crate::logging::flush_logs_best_effort();
    }

    pub async fn register_sync_root_public(sync_root_path: &Path) -> Result<(), SmartSyncError> {
        let _com_guard = initialize_com_apartment()?;
        let sync_root = normalize_sync_root_path(sync_root_path)?;
        debug_log_sync_root_security(&sync_root);
        info!("smart-sync: registering {}", sync_root.display());
        flush_smart_sync_logs();
        if let Err(register_err) = register_sync_root(&sync_root) {
            warn!(
                "smart-sync: register attempt failed for {}, trying direct connect fallback: {}",
                sync_root.display(),
                register_err
            );
            flush_smart_sync_logs();
            connect_sync_root(&sync_root).map_err(|connect_err| {
                SmartSyncError::InvalidPathWithContext(
                    "CfRegisterSyncRoot",
                    format!(
                        "{}; connect fallback also failed: {}",
                        register_err, connect_err
                    ),
                )
            })?;
            info!(
                "smart-sync: connect fallback succeeded for {} after registration warning",
                sync_root.display()
            );
            flush_smart_sync_logs();
            return Ok(());
        }
        info!("smart-sync: connecting {}", sync_root.display());
        flush_smart_sync_logs();
        connect_sync_root(&sync_root).map_err(|err| {
            SmartSyncError::InvalidPathWithContext("CfConnectSyncRoot", err.to_string())
        })?;
        info!("smart-sync: connected {}", sync_root.display());
        flush_smart_sync_logs();
        Ok(())
    }

    fn initialize_com_apartment() -> Result<ComApartmentGuard, SmartSyncError> {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr == S_OK || hr == S_FALSE {
            Ok(ComApartmentGuard {
                should_uninitialize: true,
            })
        } else if hr == RPC_E_CHANGED_MODE {
            trace!("smart-sync: COM apartment already initialized in a different mode");
            Ok(ComApartmentGuard {
                should_uninitialize: false,
            })
        } else {
            Err(SmartSyncError::Windows(hr.into()))
        }
    }

    impl Drop for ComApartmentGuard {
        fn drop(&mut self) {
            if self.should_uninitialize {
                unsafe { CoUninitialize() };
            }
        }
    }

    pub fn shutdown_sync_root() -> Result<(), SmartSyncError> {
        if let Some(connection_key) = CONNECTION_KEY.get().copied() {
            unsafe {
                let _ = CfDisconnectSyncRoot(connection_key);
            }
        }
        Ok(())
    }

    pub fn unregister_sync_root(sync_root_path: &Path) -> Result<(), SmartSyncError> {
        let sync_root = normalize_sync_root_path(sync_root_path)?;
        let sync_root_wide = wide_path(&sync_root)?;
        match unsafe { CfUnregisterSyncRoot(PCWSTR(sync_root_wide.as_ptr())) } {
            Ok(()) => {
                info!("smart-sync: unregistered {}", sync_root.display());
                Ok(())
            }
            Err(err) => {
                trace!(
                    "smart-sync: unregister skipped/failed for {}: {}",
                    sync_root.display(),
                    err
                );
                Ok(())
            }
        }
    }

    pub async fn reset_placeholders_after_unlock(
        pool: &SqlitePool,
        sync_root_path: &Path,
    ) -> Result<(), SmartSyncError> {
        let sync_root = normalize_sync_root_path(sync_root_path)?;
        let files = db::get_active_files_for_projection(pool).await?;
        info!(
            "[UNLOCK] resetting {} placeholders after vault unlock in {}",
            files.len(),
            sync_root.display()
        );
        flush_smart_sync_logs();

        for file in &files {
            let relative_path = normalize_relative_placeholder_path(&file.path)?;
            let target_path = sync_root.join(&relative_path);
            if target_path.exists() {
                // Remove the stale placeholder so it can be recreated fresh.
                // Windows cached the "vault is locked" failure on this file.
                if let Err(err) = std::fs::remove_file(&target_path) {
                    warn!(
                        "[UNLOCK] failed to remove stale placeholder {}: {}",
                        target_path.display(),
                        err
                    );
                }
            }
        }

        // Re-create all placeholders from scratch
        for file in &files {
            let state = db::ensure_smart_sync_state(pool, file.inode_id, file.revision_id).await?;
            create_projection_placeholder(&sync_root, file, state.pin_state != 0)?;
        }

        info!("[UNLOCK] placeholder reset complete, {} files ready for hydration", files.len());
        flush_smart_sync_logs();
        Ok(())
    }

    pub async fn project_vault_to_sync_root(
        pool: &SqlitePool,
        sync_root_path: &Path,
    ) -> Result<(), SmartSyncError> {
        let sync_root = normalize_sync_root_path(sync_root_path)?;
        let files = db::get_active_files_for_projection(pool).await?;
        info!(
            "smart-sync: projecting {} active file placeholders into {}",
            files.len(),
            sync_root.display()
        );
        flush_smart_sync_logs();

        if files.is_empty() {
            trace!(
                "smart-sync: projection skipped for {} because there are no active file placeholders",
                sync_root.display()
            );
            flush_smart_sync_logs();
            return Ok(());
        }

        for file in files {
            let state = db::ensure_smart_sync_state(pool, file.inode_id, file.revision_id).await?;
            create_projection_placeholder(&sync_root, &file, state.pin_state != 0)?;
        }

        Ok(())
    }

    pub async fn sync_placeholder_pin_state(
        pool: &SqlitePool,
        sync_root_path: &Path,
        inode_id: i64,
        dehydrate_immediately: bool,
    ) -> Result<(), SmartSyncError> {
        let sync_root = normalize_sync_root_path(sync_root_path)?;
        let file = db::get_active_file_for_projection_by_inode(pool, inode_id)
            .await?
            .ok_or_else(|| {
                SmartSyncError::InvalidPathWithContext(
                    "smart sync",
                    format!("inode {inode_id} has no current revision for projection"),
                )
            })?;
        let state = db::ensure_smart_sync_state(pool, file.inode_id, file.revision_id).await?;
        let relative_path = normalize_relative_placeholder_path(&file.path)?;
        let target_path = sync_root.join(relative_path);
        if !target_path.exists() {
            create_projection_placeholder(&sync_root, &file, state.pin_state != 0)?;
        } else {
            apply_pin_state(
                &target_path,
                if state.pin_state != 0 {
                    CF_PIN_STATE_PINNED
                } else {
                    CF_PIN_STATE_UNPINNED
                },
            )?;
        }

        if dehydrate_immediately && state.pin_state == 0 {
            if target_path.exists() && state.hydration_state != 0 {
                dehydrate_placeholder(&target_path)?;
            }
            db::set_hydration_state(pool, inode_id, 0).await?;
        }

        // After pin/dehydrate changes, placeholder is still in-sync with cloud.
        if target_path.exists() {
            if let Err(err) = mark_in_sync(&target_path, true) {
                warn!("smart-sync: mark_in_sync after pin state sync failed for inode={}: {}", inode_id, err);
            }
        }

        notify_shell_path_changed(&target_path);

        Ok(())
    }

    pub async fn hydrate_placeholder_now(
        pool: &SqlitePool,
        sync_root_path: &Path,
        inode_id: i64,
    ) -> Result<(), SmartSyncError> {
        let sync_root = normalize_sync_root_path(sync_root_path)?;
        let file = db::get_active_file_for_projection_by_inode(pool, inode_id)
            .await?
            .ok_or_else(|| {
                SmartSyncError::InvalidPathWithContext(
                    "smart sync",
                    format!("inode {inode_id} has no current revision for projection"),
                )
            })?;
        let state = db::ensure_smart_sync_state(pool, file.inode_id, file.revision_id).await?;
        let relative_path = normalize_relative_placeholder_path(&file.path)?;
        let target_path = sync_root.join(relative_path);
        if !target_path.exists() {
            create_projection_placeholder(&sync_root, &file, true)?;
        } else {
            apply_pin_state(&target_path, CF_PIN_STATE_PINNED)?;
        }

        hydrate_placeholder(&target_path)?;
        db::set_pin_state(pool, inode_id, 1).await?;
        db::set_hydration_state(pool, inode_id, state.hydration_state.max(1))
            .await?;

        // Hydrated + pinned = fully synced, show green checkmark.
        if let Err(err) = mark_in_sync(&target_path, true) {
            warn!("smart-sync: mark_in_sync after hydrate_now failed for inode={}: {}", inode_id, err);
        }

        notify_shell_path_changed(&target_path);
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn evict_unpinned_hydrated_files(
        pool: &SqlitePool,
        sync_root_path: &Path,
    ) -> Result<usize, SmartSyncError> {
        let sync_root = normalize_sync_root_path(sync_root_path)?;
        let candidates = db::list_unpinned_hydrated_files_for_eviction(pool).await?;
        let mut evicted = 0usize;

        for candidate in candidates {
            let relative_path = normalize_relative_placeholder_path(&candidate.path)?;
            let target_path = sync_root.join(&relative_path);
            if !target_path.exists() {
                let _ = db::set_hydration_state(pool, candidate.inode_id, 0).await;
                continue;
            }

            if let Err(err) = dehydrate_placeholder(&target_path) {
                warn!(
                    "smart-sync: failed to dehydrate {}: {}",
                    target_path.display(),
                    err
                );
                continue;
            }

            db::set_hydration_state(pool, candidate.inode_id, 0).await?;
            // Evicted but still in-sync with cloud (content is in remote storage).
            let _ = mark_in_sync(&target_path, true);
            notify_shell_path_changed(&target_path);
            evicted += 1;
        }

        Ok(evicted)
    }

    /// Convert an existing real file to a cfapi cloud placeholder and dehydrate it.
    /// Steps: CfConvertToPlaceholder (with identity blob) → CfUpdatePlaceholder(DEHYDRATE)
    /// → update smart_sync_state → shell notification.
    pub async fn convert_to_ghost(
        pool: &SqlitePool,
        sync_root_path: &Path,
        inode_id: i64,
        revision_id: i64,
        file_size: i64,
    ) -> Result<(), SmartSyncError> {
        let sync_root = normalize_sync_root_path(sync_root_path)?;

        let file = db::get_active_file_for_projection_by_inode(pool, inode_id)
            .await?
            .ok_or_else(|| {
                SmartSyncError::InvalidPathWithContext(
                    "convert_to_ghost",
                    format!("inode {inode_id} has no active projection record"),
                )
            })?;

        let relative_path = normalize_relative_placeholder_path(&file.path)?;
        let target_path = sync_root.join(&relative_path);

        if !target_path.exists() {
            return Err(SmartSyncError::InvalidPathWithContext(
                "convert_to_ghost",
                format!("file does not exist at {}", target_path.display()),
            ));
        }

        // Safety check: verify file size matches what was ingested.
        let meta = std::fs::metadata(&target_path)?;
        let current_size = meta.len() as i64;
        if current_size != file_size {
            return Err(SmartSyncError::InvalidPathWithContext(
                "convert_to_ghost",
                format!(
                    "file size changed during ingest (expected {file_size}, got {current_size}), aborting ghost swap"
                ),
            ));
        }

        // Step 1: Convert the real file to a cloud placeholder.
        let identity = PlaceholderIdentity {
            inode_id,
            revision_id,
        };
        let identity_bytes = unsafe {
            std::slice::from_raw_parts(
                (&identity as *const PlaceholderIdentity).cast::<u8>(),
                size_of::<PlaceholderIdentity>(),
            )
        };

        let ph_handle = open_placeholder_handle(&target_path)?;
        unsafe {
            CfConvertToPlaceholder(
                as_handle(&ph_handle),
                Some(identity_bytes.as_ptr().cast()),
                identity_bytes.len() as u32,
                CF_CONVERT_FLAG_NONE,
                None,
                None,
            )?;
        }
        drop(ph_handle);

        // Step 2: Dehydrate — remove local data, keep cloud shell.
        dehydrate_placeholder(&target_path)?;

        // Step 3: Mark in-sync — dehydrated ghost is still in-sync with cloud
        // (its revision matches). This makes Explorer show the cloud icon.
        if let Err(err) = mark_in_sync(&target_path, true) {
            warn!("smart-sync: mark_in_sync after ghost swap failed for inode={}: {}", inode_id, err);
        }

        // Step 4: Update DB — mark as dehydrated (hydration_state=0, unpinned).
        db::ensure_smart_sync_state(pool, inode_id, revision_id).await?;
        db::set_hydration_state(pool, inode_id, 0).await?;

        notify_shell_path_changed(&target_path);

        info!(
            "smart-sync: ghost swap complete for inode={} rev={} at {}",
            inode_id, revision_id, relative_path,
        );

        Ok(())
    }

    fn create_projection_placeholder(
        sync_root: &Path,
        file: &ProjectionFileRecord,
        pinned: bool,
    ) -> Result<(), SmartSyncError> {
        let relative_path = normalize_relative_placeholder_path(&file.path)?;
        let target_path = sync_root.join(&relative_path);
        // Always ensure parent directories are cloud-file placeholders,
        // even if the file placeholder itself already exists. Without this,
        // directories created by std::fs::create_dir_all in a prior session
        // remain plain folders and cldflt.sys blocks enumeration.
        let file_time = file_time_from_unix_millis(file.created_at)?;
        ensure_placeholder_directory_chain(sync_root, &relative_path, file_time)?;

        if !target_path.exists() {
            let base_directory = target_path.parent().unwrap_or(sync_root);
            let base_directory_wide = wide_path(base_directory)?;
            let file_name = target_path.file_name().ok_or(SmartSyncError::InvalidPath(
                "placeholder target is missing a file name",
            ))?;
            let relative_name_wide = wide_str(file_name);
            let identity = PlaceholderIdentity {
                inode_id: file.inode_id,
                revision_id: file.revision_id,
            };
            let identity_bytes = unsafe {
                std::slice::from_raw_parts(
                    (&identity as *const PlaceholderIdentity).cast::<u8>(),
                    size_of::<PlaceholderIdentity>(),
                )
            };
            let mut entries_processed = 0u32;

            let mut placeholder = [CF_PLACEHOLDER_CREATE_INFO {
                RelativeFileName: PCWSTR(relative_name_wide.as_ptr()),
                FsMetadata: CF_FS_METADATA {
                    BasicInfo: FILE_BASIC_INFO {
                        CreationTime: file_time,
                        LastAccessTime: file_time,
                        LastWriteTime: file_time,
                        ChangeTime: file_time,
                        FileAttributes: FILE_ATTRIBUTE_ARCHIVE.0,
                    },
                    FileSize: file.size,
                },
                FileIdentity: identity_bytes.as_ptr().cast(),
                FileIdentityLength: identity_bytes.len() as u32,
                Flags: placeholder_create_flags(),
                Result: HRESULT(0),
                CreateUsn: 0,
            }];

            let create_result = unsafe {
                CfCreatePlaceholders(
                    PCWSTR(base_directory_wide.as_ptr()),
                    &mut placeholder,
                    create_flags(),
                    Some(&mut entries_processed),
                )
            };
            if let Err(err) = create_result {
                error!(
                    "smart-sync: CfCreatePlaceholders failed for file '{}' in base {} (sync root {}): {}",
                    relative_path,
                    base_directory.display(),
                    sync_root.display(),
                    err
                );
                return Err(SmartSyncError::Windows(err));
            }

            if entries_processed != 1 {
                return Err(SmartSyncError::InvalidPathWithContext(
                    "CfCreatePlaceholders",
                    format!("expected one entry for {relative_path}, got {entries_processed}"),
                ));
            }

            if placeholder[0].Result != S_OK {
                error!(
                    "smart-sync: file placeholder '{}' failed with HRESULT 0x{:08X} in base {} (sync root {})",
                    relative_path,
                    placeholder[0].Result.0 as u32,
                    base_directory.display(),
                    sync_root.display()
                );
                return Err(SmartSyncError::InvalidPathWithContext(
                    "CfCreatePlaceholders",
                    format!(
                        "placeholder {} failed with HRESULT 0x{:08X}",
                        relative_path,
                        placeholder[0].Result.0 as u32
                    ),
                ));
            }

            info!("smart-sync: placeholder ready {}", relative_path);
        }

        apply_pin_state(&target_path, if pinned { CF_PIN_STATE_PINNED } else { CF_PIN_STATE_UNPINNED })?;

        // New placeholder is in-sync with the cloud (its content matches the known revision).
        if let Err(err) = mark_in_sync(&target_path, true) {
            warn!("smart-sync: mark_in_sync for new placeholder {} failed: {}", relative_path, err);
        }

        Ok(())
    }

    fn ensure_placeholder_directory_chain(
        sync_root: &Path,
        relative_file_path: &str,
        file_time: i64,
    ) -> Result<(), SmartSyncError> {
        let _ = file_time;
        let mut current = PathBuf::new();

        if let Some(parent) = Path::new(relative_file_path).parent() {
            for component in parent.components() {
                let Component::Normal(segment) = component else {
                    continue;
                };
                current.push(segment);
                let target_path = sync_root.join(&current);
                if target_path.exists() {
                    // Directory exists — convert to placeholder if not already one.
                    convert_directory_to_placeholder(&target_path);
                    continue;
                }

                std::fs::create_dir_all(&target_path)?;
                convert_directory_to_placeholder(&target_path);
                info!(
                    "smart-sync: directory placeholder ready {} under {}",
                    current.display(),
                    sync_root.display()
                );
            }
        }

        Ok(())
    }

    fn register_sync_root(sync_root_path: &Path) -> Result<(), SmartSyncError> {
        std::fs::create_dir_all(sync_root_path).map_err(SmartSyncError::Io)?;
        let sync_root_wide = wide_path(sync_root_path)?;
        let provider_name = sync_provider_name();
        let provider_version = sync_provider_version();
        let provider_id = sync_provider_id();
        let sync_root_identity = sync_root_identity_bytes();
        let provider_name_wide = wide_str(OsStr::new(&provider_name));
        let provider_version_wide = wide_str(OsStr::new(&provider_version));

        let registration = CF_SYNC_REGISTRATION {
            StructSize: size_of::<CF_SYNC_REGISTRATION>() as u32,
            ProviderName: PCWSTR(provider_name_wide.as_ptr()),
            ProviderVersion: PCWSTR(provider_version_wide.as_ptr()),
            SyncRootIdentity: sync_root_identity.as_ptr().cast(),
            SyncRootIdentityLength: sync_root_identity.len() as u32,
            FileIdentity: ptr::null(),
            FileIdentityLength: 0,
            ProviderId: provider_id,
        };

        let policies = CF_SYNC_POLICIES {
            StructSize: size_of::<CF_SYNC_POLICIES>() as u32,
            Hydration: CF_HYDRATION_POLICY {
                Primary: CF_HYDRATION_POLICY_PRIMARY(CF_HYDRATION_POLICY_FULL.0),
                Modifier: CF_HYDRATION_POLICY_MODIFIER(CF_HYDRATION_POLICY_MODIFIER_NONE.0),
            },
            Population: CF_POPULATION_POLICY {
                Primary: CF_POPULATION_POLICY_PRIMARY(CF_POPULATION_POLICY_FULL.0),
                Modifier: CF_POPULATION_POLICY_MODIFIER(CF_POPULATION_POLICY_MODIFIER_NONE.0),
            },
            InSync: CF_INSYNC_POLICY(CF_INSYNC_POLICY_NONE.0),
            HardLink: CF_HARDLINK_POLICY(CF_HARDLINK_POLICY_NONE.0),
            PlaceholderManagement: CF_PLACEHOLDER_MANAGEMENT_POLICY(
                CF_PLACEHOLDER_MANAGEMENT_POLICY_CREATE_UNRESTRICTED.0,
            ),
        };

        let path = PCWSTR(sync_root_wide.as_ptr());
        if inspect_existing_sync_root(sync_root_path, path, &provider_name, &provider_version, &sync_root_identity) {
            info!(
                "smart-sync: existing sync root detected, registration skipped for {}",
                sync_root_path.display()
            );
            return Ok(());
        }

        assert_sync_root_writable(sync_root_path)?;
        trace!(
            "smart-sync: defensive unregister before register for {}",
            sync_root_path.display()
        );
        unsafe {
            let _ = CfUnregisterSyncRoot(path);
        }
        let initial_result =
            unsafe { CfRegisterSyncRoot(path, &registration, &policies, register_flags(false)) };
        if initial_result.is_ok() {
            return Ok(());
        }

        log_registration_context(sync_root_path, &registration, register_flags(false), "initial");

        let first_error = initial_result
            .err()
            .map(|err| err.to_string())
            .unwrap_or_else(|| "unknown register error".to_string());
        warn!(
            "smart-sync: initial register failed for {} (provider={}, account={ACCOUNT_NAME}): {}",
            sync_root_path.display(),
            provider_name,
            first_error
        );

        unsafe {
            let _ = CfUnregisterSyncRoot(path);
        }

        log_registration_context(sync_root_path, &registration, register_flags(true), "update");
        unsafe { CfRegisterSyncRoot(path, &registration, &policies, register_flags(true))? };
        Ok(())
    }

    fn inspect_existing_sync_root(
        sync_root_path: &Path,
        path: PCWSTR,
        expected_provider_name: &str,
        expected_provider_version: &str,
        expected_identity: &[u8],
    ) -> bool {
        match get_existing_sync_root_info(sync_root_path, path) {
            Ok(Some(info)) => {
                let identity_matches = info.identity_bytes == expected_identity;
                let provider_name_matches =
                    info.provider_name.eq_ignore_ascii_case(expected_provider_name);
                let provider_version_matches = info.provider_version == expected_provider_version;
                if provider_name_matches && provider_version_matches && identity_matches {
                    true
                } else {
                    trace!(
                        "smart-sync: existing root metadata mismatch for {} => expected provider_name='{}', provider_version='{}', identity='{}'",
                        sync_root_path.display(),
                        expected_provider_name,
                        expected_provider_version,
                        String::from_utf8_lossy(expected_identity)
                    );
                    false
                }
            }
            Ok(None) => false,
            Err(err) => {
                trace!(
                    "smart-sync: existing root inspection failed for {}: {}",
                    sync_root_path.display(),
                    err
                );
                false
            }
        }
    }

    fn get_existing_sync_root_info(
        sync_root_path: &Path,
        path: PCWSTR,
    ) -> Result<Option<ExistingSyncRootInfo>, SmartSyncError> {
        let mut buffer = vec![0u8; size_of::<CF_SYNC_ROOT_STANDARD_INFO>() + 512];
        let mut returned = 0u32;
        let result = unsafe {
            CfGetSyncRootInfoByPath(
                path,
                CF_SYNC_ROOT_INFO_STANDARD,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                Some(&mut returned),
            )
        };

        match result {
            Ok(()) => {
                let info = unsafe { &*(buffer.as_ptr() as *const CF_SYNC_ROOT_STANDARD_INFO) };
                let provider_name = utf16_trimmed(&info.ProviderName);
                let provider_version = utf16_trimmed(&info.ProviderVersion);
                let identity_len = usize::try_from(info.SyncRootIdentityLength).unwrap_or(0);
                let identity_ptr = info.SyncRootIdentity.as_ptr();
                let identity_bytes =
                    unsafe { std::slice::from_raw_parts(identity_ptr, identity_len) }.to_vec();
                trace!(
                    "smart-sync: CfGetSyncRootInfoByPath found existing root for {} => provider_name='{}', provider_version='{}', file_id={}, identity_len={}, identity={}",
                    sync_root_path.display(),
                    provider_name,
                    provider_version,
                    info.SyncRootFileId,
                    info.SyncRootIdentityLength,
                    String::from_utf8_lossy(&identity_bytes)
                );
                Ok(Some(ExistingSyncRootInfo {
                    provider_name,
                    provider_version,
                    identity_bytes,
                }))
            }
            Err(err) => {
                trace!(
                    "smart-sync: CfGetSyncRootInfoByPath reported no reusable root for {}: {}",
                    sync_root_path.display(),
                    err
                );
                Ok(None)
            }
        }
    }

    fn log_registration_context(
        sync_root_path: &Path,
        registration: &CF_SYNC_REGISTRATION,
        flags: CF_REGISTER_FLAGS,
        phase: &str,
    ) {
        trace!(
            "smart-sync: register context [{}] path={}, provider_name='{}', provider_version='{}', provider_id={:?}, sync_root_identity_len={}, flags=0x{:x}",
            phase,
            sync_root_path.display(),
            sync_provider_name(),
            sync_provider_version(),
            registration.ProviderId,
            registration.SyncRootIdentityLength,
            flags.0
        );
    }

    fn sync_provider_name() -> String {
        std::env::var("OMNIDRIVE_SYNC_PROVIDER_NAME")
            .unwrap_or_else(|_| PROVIDER_NAME.to_string())
    }

    fn sync_provider_version() -> String {
        std::env::var("OMNIDRIVE_SYNC_PROVIDER_VERSION")
            .unwrap_or_else(|_| PROVIDER_VERSION.to_string())
    }

    fn sync_root_identity_bytes() -> Vec<u8> {
        std::env::var("OMNIDRIVE_SYNC_ROOT_IDENTITY")
            .unwrap_or_else(|_| "OmniDrive_Vault".to_string())
            .into_bytes()
    }

    fn sync_provider_id() -> GUID {
        if let Ok(seed) = std::env::var("OMNIDRIVE_SYNC_PROVIDER_ID_SEED") {
            let digest = Sha256::digest(seed.as_bytes());
            let mut bytes = [0u8; 16];
            bytes.copy_from_slice(&digest[..16]);
            bytes[6] = (bytes[6] & 0x0F) | 0x40;
            bytes[8] = (bytes[8] & 0x3F) | 0x80;
            return GUID::from_u128(u128::from_be_bytes(bytes));
        }
        PROVIDER_ID
    }

    fn utf16_trimmed(raw: &[u16]) -> String {
        let len = raw.iter().position(|ch| *ch == 0).unwrap_or(raw.len());
        String::from_utf16_lossy(&raw[..len])
    }

    fn debug_log_sync_root_security(path: &Path) {
        let owner_output = powershell_literal_output(
            path,
            "$acl = Get-Acl -LiteralPath __PATH__; $acl.Owner",
        );
        let acl_output = Command::new("icacls").arg(path).output();

        match owner_output {
            Ok(owner) => trace!(
                "smart-sync: sync root owner for {} => {}",
                path.display(),
                owner.trim()
            ),
            Err(err) => trace!(
                "smart-sync: failed to read sync root owner for {}: {}",
                path.display(),
                err
            ),
        }

        match acl_output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                trace!(
                    "smart-sync: sync root ACL dump for {} => status={:?}, stdout={}, stderr={}",
                    path.display(),
                    output.status.code(),
                    stdout.trim(),
                    stderr.trim()
                );
            }
            Err(err) => trace!(
                "smart-sync: failed to dump sync root ACLs for {}: {}",
                path.display(),
                err
            ),
        }
    }

    fn powershell_literal_output(path: &Path, script_template: &str) -> Result<String, SmartSyncError> {
        let escaped = path
            .display()
            .to_string()
            .replace('\'', "''");
        let script = script_template.replace("__PATH__", &format!("'{}'", escaped));
        let output = Command::new("powershell.exe")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(script)
            .output()
            .map_err(SmartSyncError::Io)?;

        if !output.status.success() {
            return Err(SmartSyncError::InvalidPathWithContext(
                "sync root security debug",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn connect_sync_root(sync_root_path: &Path) -> Result<(), SmartSyncError> {
        if CONNECTION_KEY.get().is_some() {
            return Ok(());
        }

        let sync_root_wide = wide_path(sync_root_path)?;
        let callbacks = [
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_FETCH_PLACEHOLDERS,
                Callback: Some(fetch_placeholders_callback),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_FETCH_DATA,
                Callback: Some(fetch_data_callback),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_CANCEL_FETCH_DATA,
                Callback: Some(cancel_fetch_data_callback),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NONE,
                Callback: None,
            },
        ];

        let connection = unsafe {
            CfConnectSyncRoot(
                PCWSTR(sync_root_wide.as_ptr()),
                callbacks.as_ptr(),
                None,
                CF_CONNECT_FLAG_NONE,
            )?
        };
        let _ = CONNECTION_KEY.set(connection);
        Ok(())
    }

    pub fn audit_sync_root_state(sync_root_path: &Path) -> Result<SyncRootStateSnapshot, SmartSyncError> {
        let provider_name = sync_provider_name();
        let provider_version = sync_provider_version();
        let expected_identity = sync_root_identity_bytes();
        let path_exists = sync_root_path.exists();
        let existing = if path_exists {
            let sync_root_wide = wide_path(sync_root_path)?;
            get_existing_sync_root_info(sync_root_path, PCWSTR(sync_root_wide.as_ptr()))?
        } else {
            None
        };

        let registered = existing.is_some();
        let registered_for_provider = existing
            .as_ref()
            .map(|info| {
                info.provider_name.eq_ignore_ascii_case(&provider_name)
                    && info.provider_version == provider_version
                    && info.identity_bytes == expected_identity
            })
            .unwrap_or(false);

        Ok(SyncRootStateSnapshot {
            path: sync_root_path.display().to_string(),
            path_exists,
            registered,
            registered_for_provider,
            connected: CONNECTION_KEY.get().is_some(),
            provider_name: existing.as_ref().map(|info| info.provider_name.clone()),
            provider_version: existing.as_ref().map(|info| info.provider_version.clone()),
            identity: existing
                .as_ref()
                .map(|info| String::from_utf8_lossy(&info.identity_bytes).to_string()),
        })
    }

    pub async fn repair_sync_root(
        pool: &SqlitePool,
        sync_root_path: &Path,
    ) -> Result<SyncRootRepairReport, SmartSyncError> {
        let mut actions = Vec::new();
        let state = audit_sync_root_state(sync_root_path)?;

        if CONNECTION_KEY.get().is_some() {
            shutdown_sync_root()?;
            actions.push(format!(
                "disconnected existing sync root connection for {}",
                sync_root_path.display()
            ));
        }

        if state.registered && !state.registered_for_provider {
            unregister_sync_root(sync_root_path)?;
            actions.push(format!(
                "unregistered stale sync root registration for {}",
                sync_root_path.display()
            ));
        }

        register_sync_root_public(sync_root_path).await?;
        actions.push(format!(
            "registered and connected sync root {}",
            sync_root_path.display()
        ));

        project_vault_to_sync_root(pool, sync_root_path).await?;
        actions.push(format!(
            "projected vault into sync root {}",
            sync_root_path.display()
        ));

        Ok(SyncRootRepairReport {
            actions,
            sync_root_state: audit_sync_root_state(sync_root_path)?,
        })
    }

    fn normalize_sync_root_path(path: &Path) -> Result<PathBuf, SmartSyncError> {
        prepare_sync_root_directory(path)?;
        let canonical = path.canonicalize().map_err(SmartSyncError::Io)?;
        let normalized = normalized_windows_path_string(&canonical)?;
        let normalized = PathBuf::from(normalized);
        if !normalized.is_absolute() {
            return Err(SmartSyncError::InvalidPath(
                "normalized sync root must be absolute",
            ));
        }
        ensure_path_inside_user_profile(&normalized)?;
        Ok(normalized)
    }

    fn prepare_sync_root_directory(path: &Path) -> Result<(), SmartSyncError> {
        if path.exists() {
            trace!("smart-sync: sync root exists before prep: {}", path.display());
            let metadata = std::fs::metadata(path).map_err(SmartSyncError::Io)?;
            if !metadata.is_dir() {
                return Err(SmartSyncError::InvalidPath(
                    "sync root path exists and is not a directory",
                ));
            }

            let attrs = metadata.file_attributes();
            trace!(
                "smart-sync: existing sync root attrs for {} => 0x{:x}",
                path.display(),
                attrs
            );
        }

        trace!("smart-sync: creating sync root directory {}", path.display());
        std::fs::create_dir_all(path).map_err(SmartSyncError::Io)?;
        trace!("smart-sync: created sync root directory {}", path.display());
        if let Err(err) = win_acl::prepare_sync_root_directory(path) {
            return Err(SmartSyncError::InvalidPathWithContext(
                "sync root acl preparation",
                err.to_string(),
            ));
        }
        trace!("smart-sync: prepared sync root ACLs {}", path.display());
        Ok(())
    }

    fn ensure_path_inside_user_profile(path: &Path) -> Result<(), SmartSyncError> {
        let user_profile = std::env::var("USERPROFILE")
            .map_err(|_| SmartSyncError::InvalidPath("USERPROFILE is not set"))?;
        let user_profile = PathBuf::from(
            normalized_windows_path_string(Path::new(&user_profile)).map_err(|_| {
                SmartSyncError::InvalidPath("USERPROFILE is not a valid Windows path")
            })?,
        );

        if !starts_with_case_insensitive(path, &user_profile) {
            return Err(SmartSyncError::InvalidPath(
                "sync root must be inside the current user profile",
            ));
        }

        Ok(())
    }

    fn normalize_relative_placeholder_path(path: &str) -> Result<String, SmartSyncError> {
        let normalized = path.replace('\\', "/").trim_start_matches('/').trim().to_string();

        if normalized.is_empty() {
            return Err(SmartSyncError::InvalidPath(
                "placeholder path cannot be empty",
            ));
        }

        if normalized
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == ".." || segment.contains(':'))
        {
            return Err(SmartSyncError::InvalidPath(
                "placeholder path contains invalid segments",
            ));
        }

        Ok(normalized.replace('/', "\\"))
    }

    fn normalized_windows_path_string(path: &Path) -> Result<String, SmartSyncError> {
        let raw = path.as_os_str().to_string_lossy().replace('/', "\\");
        let without_verbatim = raw.strip_prefix(r"\\?\").unwrap_or(&raw);
        let without_leading = if without_verbatim.starts_with('\\')
            && without_verbatim.len() >= 4
            && without_verbatim.as_bytes()[2] == b':'
            && without_verbatim.as_bytes()[3] == b'\\'
        {
            &without_verbatim[1..]
        } else {
            without_verbatim
        };

        if without_leading.len() < 3
            || without_leading.as_bytes()[1] != b':'
            || without_leading.as_bytes()[2] != b'\\'
        {
            return Err(SmartSyncError::InvalidPath(
                "path must resolve to a drive-qualified Windows path",
            ));
        }

        Ok(without_leading.to_string())
    }

    fn file_time_from_unix_millis(unix_millis: i64) -> Result<i64, SmartSyncError> {
        if unix_millis < 0 {
            return Err(SmartSyncError::InvalidPath("negative unix timestamp"));
        }

        const WINDOWS_EPOCH_OFFSET_SECS: u64 = 11_644_473_600;
        let duration = Duration::from_millis(
            u64::try_from(unix_millis)
                .map_err(|_| SmartSyncError::InvalidPath("negative unix timestamp"))?,
        );
        let system_time = UNIX_EPOCH
            .checked_add(duration)
            .ok_or(SmartSyncError::InvalidPath("timestamp overflow"))?;
        let duration = system_time
            .duration_since(UNIX_EPOCH)
            .map_err(|_| SmartSyncError::InvalidPath("system time before unix epoch"))?;
        let ticks = (duration.as_secs() + WINDOWS_EPOCH_OFFSET_SECS)
            .saturating_mul(10_000_000)
            .saturating_add(u64::from(duration.subsec_nanos() / 100));
        i64::try_from(ticks).map_err(|_| SmartSyncError::InvalidPath("timestamp overflow"))
    }

    fn wide_path(path: &Path) -> Result<Vec<u16>, SmartSyncError> {
        if !path.is_absolute() {
            return Err(SmartSyncError::InvalidPath("path must be absolute"));
        }
        Ok(wide_str(path.as_os_str()))
    }

    fn wide_str(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(iter::once(0)).collect()
    }

    fn open_placeholder_handle(path: &Path) -> Result<std::fs::File, SmartSyncError> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        Ok(file)
    }

    fn as_handle(file: &std::fs::File) -> HANDLE {
        HANDLE(file.as_raw_handle())
    }

    fn decode_file_identity(
        identity_ptr: *const core::ffi::c_void,
        identity_len: u32,
    ) -> Option<PlaceholderIdentity> {
        if identity_ptr.is_null() || identity_len as usize != size_of::<PlaceholderIdentity>() {
            return None;
        }

        let identity = unsafe { ptr::read_unaligned(identity_ptr.cast::<PlaceholderIdentity>()) };
        Some(identity)
    }

    fn complete_transfer_success(
        request: &HydrationRequest,
        bytes: &[u8],
    ) -> Result<(), SmartSyncError> {
        let operation_info = CF_OPERATION_INFO {
            StructSize: size_of::<CF_OPERATION_INFO>() as u32,
            Type: CF_OPERATION_TYPE_TRANSFER_DATA,
            ConnectionKey: request.connection_key,
            TransferKey: request.transfer_key,
            CorrelationVector: ptr::null(),
            SyncStatus: ptr::null(),
            RequestKey: request.request_key,
        };

        let mut operation_parameters = CF_OPERATION_PARAMETERS {
            ParamSize: size_of::<CF_OPERATION_PARAMETERS>() as u32,
            ..Default::default()
        };
        operation_parameters.Anonymous.TransferData =
            windows::Win32::Storage::CloudFilters::CF_OPERATION_PARAMETERS_0_0 {
                Flags: CF_OPERATION_TRANSFER_DATA_FLAGS(0),
                CompletionStatus: NTSTATUS(STATUS_SUCCESS),
                Buffer: bytes.as_ptr().cast(),
                Offset: request.offset,
                Length: i64::try_from(bytes.len())
                    .map_err(|_| SmartSyncError::InvalidPath("range length overflow"))?,
            };

        unsafe {
            CfExecute(&operation_info, &mut operation_parameters)?;
        }

        Ok(())
    }

    /// Transfer a single chunk slice to Windows at an explicit file offset.
    /// Called once per chunk during streamed hydration — peak RAM ≤ 1 chunk.
    fn complete_transfer_chunk(
        request: &HydrationRequest,
        file_offset: i64,
        bytes: &[u8],
    ) -> Result<(), SmartSyncError> {
        let operation_info = CF_OPERATION_INFO {
            StructSize: size_of::<CF_OPERATION_INFO>() as u32,
            Type: CF_OPERATION_TYPE_TRANSFER_DATA,
            ConnectionKey: request.connection_key,
            TransferKey: request.transfer_key,
            CorrelationVector: ptr::null(),
            SyncStatus: ptr::null(),
            RequestKey: request.request_key,
        };

        let mut operation_parameters = CF_OPERATION_PARAMETERS {
            ParamSize: size_of::<CF_OPERATION_PARAMETERS>() as u32,
            ..Default::default()
        };
        operation_parameters.Anonymous.TransferData =
            windows::Win32::Storage::CloudFilters::CF_OPERATION_PARAMETERS_0_0 {
                Flags: CF_OPERATION_TRANSFER_DATA_FLAGS(0),
                CompletionStatus: NTSTATUS(STATUS_SUCCESS),
                Buffer: bytes.as_ptr().cast(),
                Offset: file_offset,
                Length: i64::try_from(bytes.len())
                    .map_err(|_| SmartSyncError::InvalidPath("chunk length overflow"))?,
            };

        unsafe {
            CfExecute(&operation_info, &mut operation_parameters)?;
        }

        Ok(())
    }

    fn complete_transfer_failure(
        callback_info: &CF_CALLBACK_INFO,
        offset: i64,
        length: i64,
    ) -> Result<(), SmartSyncError> {
        let request = HydrationRequest {
            connection_key: callback_info.ConnectionKey,
            transfer_key: callback_info.TransferKey,
            request_key: callback_info.RequestKey,
            inode_id: 0,
            revision_id: 0,
            offset,
            length,
        };
        complete_transfer_failure_from_request(&request)
    }

    fn complete_transfer_failure_from_request(
        request: &HydrationRequest,
    ) -> Result<(), SmartSyncError> {
        let operation_info = CF_OPERATION_INFO {
            StructSize: size_of::<CF_OPERATION_INFO>() as u32,
            Type: CF_OPERATION_TYPE_TRANSFER_DATA,
            ConnectionKey: request.connection_key,
            TransferKey: request.transfer_key,
            CorrelationVector: ptr::null(),
            SyncStatus: ptr::null(),
            RequestKey: request.request_key,
        };

        let mut operation_parameters = CF_OPERATION_PARAMETERS {
            ParamSize: size_of::<CF_OPERATION_PARAMETERS>() as u32,
            ..Default::default()
        };
        operation_parameters.Anonymous.TransferData =
            windows::Win32::Storage::CloudFilters::CF_OPERATION_PARAMETERS_0_0 {
                Flags: CF_OPERATION_TRANSFER_DATA_FLAGS(0),
                CompletionStatus: NTSTATUS(STATUS_UNSUCCESSFUL),
                Buffer: ptr::null(),
                Offset: request.offset,
                Length: request.length.max(0),
            };

        unsafe {
            CfExecute(&operation_info, &mut operation_parameters)?;
        }

        Ok(())
    }

    fn register_flags(update: bool) -> CF_REGISTER_FLAGS {
        let mut flags = CF_REGISTER_FLAG_NONE.0;
        if update {
            flags |= CF_REGISTER_FLAG_UPDATE.0;
        }
        CF_REGISTER_FLAGS(flags)
    }

    fn create_flags() -> CF_CREATE_FLAGS {
        CF_CREATE_FLAGS(CF_CREATE_FLAG_NONE.0 | CF_CREATE_FLAG_STOP_ON_ERROR.0)
    }

    /// Convert an existing regular directory into a cfapi placeholder so that
    /// the cloud-files minifilter does not block directory enumeration.
    fn convert_directory_to_placeholder(path: &Path) {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(iter::once(0)).collect();
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        };
        let handle = match handle {
            Ok(h) => h,
            Err(err) => {
                warn!(
                    "smart-sync: cannot open dir {} for placeholder conversion: {}",
                    path.display(),
                    err
                );
                return;
            }
        };
        let result = unsafe {
            CfConvertToPlaceholder(
                handle,
                None,
                0,
                CF_CONVERT_FLAG_NONE,
                None,
                None,
            )
        };
        unsafe { windows::Win32::Foundation::CloseHandle(handle).ok(); }
        match result {
            Ok(()) => {
                info!(
                    "smart-sync: converted dir to placeholder: {}",
                    path.display()
                );
            }
            Err(ref err) if err.code() == HRESULT(0x8007017Cu32 as i32) => {
                // ERROR_CLOUD_FILE_INVALID_REQUEST — directory is already a placeholder.
                trace!(
                    "smart-sync: dir {} is already a placeholder, skipping",
                    path.display()
                );
            }
            Err(err) => {
                warn!(
                    "smart-sync: CfConvertToPlaceholder for dir {} failed: {}",
                    path.display(),
                    err
                );
            }
        }
    }

    fn placeholder_create_flags() -> CF_PLACEHOLDER_CREATE_FLAGS {
        CF_PLACEHOLDER_CREATE_FLAGS(0)
    }

    fn apply_pin_state(path: &Path, pin_state: CF_PIN_STATE) -> Result<(), SmartSyncError> {
        let file = open_placeholder_handle(path)?;
        unsafe {
            CfSetPinState(
                as_handle(&file),
                pin_state,
                CF_SET_PIN_FLAGS(CF_SET_PIN_FLAG_NONE.0),
                None,
            )?;
        }
        Ok(())
    }

    /// Mark a placeholder as in-sync (or not) with the cloud.
    /// This drives the native cfapi overlay icons in Explorer:
    /// - IN_SYNC + hydrated → green checkmark
    /// - IN_SYNC + dehydrated → cloud icon
    /// - NOT_IN_SYNC → blue sync arrows / warning
    fn mark_in_sync(path: &Path, in_sync: bool) -> Result<(), SmartSyncError> {
        let file = open_placeholder_handle(path)?;
        let state = if in_sync {
            CF_IN_SYNC_STATE_IN_SYNC
        } else {
            CF_IN_SYNC_STATE_NOT_IN_SYNC
        };
        unsafe {
            CfSetInSyncState(
                as_handle(&file),
                state,
                CF_SET_IN_SYNC_FLAG_NONE,
                None,
            )?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn dehydrate_placeholder(path: &Path) -> Result<(), SmartSyncError> {
        let file = open_placeholder_handle(path)?;
        let mut update_usn = 0i64;
        unsafe {
            CfUpdatePlaceholder(
                as_handle(&file),
                None,
                None,
                0,
                Option::<&[CF_FILE_RANGE]>::None,
                CF_UPDATE_FLAGS(CF_UPDATE_FLAG_DEHYDRATE.0 | CF_UPDATE_FLAG_NONE.0),
                Some(&mut update_usn),
                None,
            )?;
        }
        Ok(())
    }

    fn hydrate_placeholder(path: &Path) -> Result<(), SmartSyncError> {
        let file = open_placeholder_handle(path)?;
        unsafe {
            CfHydratePlaceholder(
                as_handle(&file),
                0,
                i64::MAX,
                CF_HYDRATE_FLAGS(0),
                None,
            )?;
        }
        Ok(())
    }

    async fn projection_path_for_inode(
        pool: &SqlitePool,
        inode_id: i64,
    ) -> Result<PathBuf, SmartSyncError> {
        let sync_root = std::env::var("OMNIDRIVE_SYNC_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| {
                        std::env::var("USERPROFILE")
                            .map(PathBuf::from)
                            .unwrap_or_else(|_| PathBuf::from(r"C:\Users\Default"))
                    })
                    .join("OmniDrive")
                    .join("OmniSync")
            });
        let sync_root = normalize_sync_root_path(&sync_root)?;
        let file = db::get_active_file_for_projection_by_inode(pool, inode_id)
            .await?
            .ok_or_else(|| {
                SmartSyncError::InvalidPathWithContext(
                    "smart sync",
                    format!("inode {inode_id} has no current revision for projection"),
                )
            })?;
        let relative_path = normalize_relative_placeholder_path(&file.path)?;
        Ok(sync_root.join(relative_path))
    }

    fn notify_shell_path_changed(path: &Path) {
        if let Ok(wide) = wide_path(path) {
            unsafe {
                SHChangeNotify(
                    SHCNE_UPDATEITEM,
                    SHCNF_PATHW,
                    Some(PCWSTR(wide.as_ptr()).0 as _),
                    None,
                );
            }
        }
    }

    fn assert_sync_root_writable(path: &Path) -> Result<(), SmartSyncError> {
        let probe = path.join(".omnidrive_acl_probe");
        std::fs::write(&probe, b"ok").map_err(SmartSyncError::Io)?;
        std::fs::remove_file(&probe).map_err(SmartSyncError::Io)?;
        Ok(())
    }

    fn starts_with_case_insensitive(path: &Path, prefix: &Path) -> bool {
        let path_parts: Vec<String> = path
            .components()
            .filter_map(normalized_component)
            .collect();
        let prefix_parts: Vec<String> = prefix
            .components()
            .filter_map(normalized_component)
            .collect();

        path_parts.len() >= prefix_parts.len()
            && path_parts
                .iter()
                .zip(prefix_parts.iter())
                .all(|(left, right)| left == right)
    }

    fn normalized_component(component: Component<'_>) -> Option<String> {
        match component {
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().to_ascii_lowercase()),
            Component::RootDir => Some("\\".to_string()),
            Component::Normal(value) => Some(value.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        }
    }
}
