use crate::downloader::Downloader;
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
    use super::SmartSyncError;
    use crate::db::{self, ProjectionFileRecord};
    use crate::downloader::Downloader;
    use sqlx::SqlitePool;
    use std::ffi::OsStr;
    use std::iter;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Component;
    use std::path::{Path, PathBuf};
    use std::ptr;
    use std::sync::{Arc, OnceLock};
    use std::time::{Duration, UNIX_EPOCH};
    use tokio::runtime::Handle;
    use tracing::{info, warn};
    use windows::core::{GUID, HRESULT, PCWSTR};
    use windows::Win32::Foundation::{HANDLE, NTSTATUS, S_OK};
    use windows::Win32::Storage::CloudFilters::{
        CfConnectSyncRoot, CfCreatePlaceholders, CfDisconnectSyncRoot, CfExecute,
        CfHydratePlaceholder,
        CfRegisterSyncRoot, CfSetPinState, CfUnregisterSyncRoot, CfUpdatePlaceholder,
        CF_CALLBACK_INFO, CF_CALLBACK_PARAMETERS,
        CF_CALLBACK_REGISTRATION, CF_CALLBACK_TYPE_CANCEL_FETCH_DATA,
        CF_CALLBACK_TYPE_FETCH_DATA, CF_CALLBACK_TYPE_NONE, CF_CONNECT_FLAG_NONE,
        CF_CONNECTION_KEY, CF_CREATE_FLAGS, CF_CREATE_FLAG_NONE, CF_CREATE_FLAG_STOP_ON_ERROR,
        CF_HYDRATE_FLAGS,
        CF_FILE_RANGE, CF_FS_METADATA, CF_HARDLINK_POLICY, CF_HARDLINK_POLICY_NONE,
        CF_HYDRATION_POLICY, CF_HYDRATION_POLICY_FULL, CF_HYDRATION_POLICY_MODIFIER,
        CF_HYDRATION_POLICY_MODIFIER_NONE, CF_HYDRATION_POLICY_PRIMARY, CF_INSYNC_POLICY,
        CF_INSYNC_POLICY_NONE, CF_OPERATION_INFO, CF_OPERATION_PARAMETERS, CF_PIN_STATE,
        CF_PIN_STATE_PINNED, CF_PIN_STATE_UNPINNED, CF_OPERATION_TRANSFER_DATA_FLAGS,
        CF_OPERATION_TYPE_TRANSFER_DATA, CF_PLACEHOLDER_CREATE_FLAGS,
        CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC, CF_PLACEHOLDER_CREATE_FLAG_SUPERSEDE,
        CF_PLACEHOLDER_CREATE_INFO, CF_PLACEHOLDER_MANAGEMENT_POLICY,
        CF_PLACEHOLDER_MANAGEMENT_POLICY_CREATE_UNRESTRICTED, CF_POPULATION_POLICY,
        CF_POPULATION_POLICY_FULL, CF_POPULATION_POLICY_MODIFIER,
        CF_POPULATION_POLICY_MODIFIER_NONE, CF_POPULATION_POLICY_PRIMARY, CF_REGISTER_FLAGS,
        CF_REGISTER_FLAG_NONE, CF_REGISTER_FLAG_UPDATE, CF_SET_PIN_FLAG_NONE, CF_SET_PIN_FLAGS, CF_SYNC_POLICIES,
        CF_SYNC_REGISTRATION, CF_UPDATE_FLAG_DEHYDRATE, CF_UPDATE_FLAG_NONE, CF_UPDATE_FLAGS,
    };
    use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_ARCHIVE, FILE_BASIC_INFO};
    use windows::Win32::UI::Shell::{SHCNE_UPDATEITEM, SHCNF_PATHW, SHChangeNotify};

    const PROVIDER_NAME: &str = "OmniDrive";
    const PROVIDER_VERSION: &str = "1.0";
    const ACCOUNT_NAME: &str = "UserVault";
    const PROVIDER_ID: GUID = GUID::from_u128(0xb7a42c2a_4af1_4f4a_a650_0b1308b8f019);
    const STATUS_UNSUCCESSFUL: i32 = 0xC0000001u32 as i32;
    const STATUS_SUCCESS: i32 = 0;
    static CONNECTION_KEY: OnceLock<CF_CONNECTION_KEY> = OnceLock::new();
    static HYDRATION_CONTEXT: OnceLock<HydrationContext> = OnceLock::new();

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

    unsafe extern "system" fn fetch_data_callback(
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
            let _ = complete_transfer_failure_from_request(&request);
            return;
        };

        context.runtime.spawn(async move {
            let offset = match u64::try_from(request.offset) {
                Ok(value) => value,
                Err(_) => {
                    warn!(
                        "smart-sync: invalid negative offset for inode={}, revision={}",
                        request.inode_id, request.revision_id
                    );
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
                    let _ = complete_transfer_failure_from_request(&request);
                    return;
                }
            };

            match context
                .downloader
                .read_range(
                    request.inode_id,
                    request.revision_id,
                    offset,
                    length,
                )
                .await
            {
                Ok(bytes) => {
                    if let Err(err) = complete_transfer_success(&request, &bytes) {
                        warn!(
                            "smart-sync: transfer writeback failed for inode={}, revision={}: {}",
                            request.inode_id, request.revision_id, err
                        );
                        let _ = complete_transfer_failure_from_request(&request);
                        return;
                    }

                    if let Err(err) = db::set_hydration_state(&context.pool, request.inode_id, 1).await {
                        warn!(
                            "smart-sync: failed to persist hydration state for inode={}: {}",
                            request.inode_id, err
                        );
                    }
                    if let Ok(path) = projection_path_for_inode(&context.pool, request.inode_id).await {
                        notify_shell_path_changed(&path);
                    }
                }
                Err(err) => {
                    warn!(
                        "smart-sync: read_range failed for inode={}, revision={}, offset={}, length={}: {}",
                        request.inode_id, request.revision_id, request.offset, request.length, err
                    );
                    let _ = complete_transfer_failure_from_request(&request);
                }
            }
        });
    }

    unsafe extern "system" fn cancel_fetch_data_callback(
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

    pub async fn register_sync_root_public(sync_root_path: &Path) -> Result<(), SmartSyncError> {
        let sync_root = normalize_sync_root_path(sync_root_path)?;
        info!("smart-sync: registering {}", sync_root.display());
        register_sync_root(&sync_root).map_err(|err| {
            SmartSyncError::InvalidPathWithContext("CfRegisterSyncRoot", err.to_string())
        })?;
        info!("smart-sync: connecting {}", sync_root.display());
        connect_sync_root(&sync_root).map_err(|err| {
            SmartSyncError::InvalidPathWithContext("CfConnectSyncRoot", err.to_string())
        })?;
        info!("smart-sync: connected {}", sync_root.display());
        Ok(())
    }

    pub fn shutdown_sync_root() -> Result<(), SmartSyncError> {
        if let Some(connection_key) = CONNECTION_KEY.get().copied() {
            unsafe {
                let _ = CfDisconnectSyncRoot(connection_key);
            }
        }
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
            notify_shell_path_changed(&target_path);
            evicted += 1;
        }

        Ok(evicted)
    }

    fn create_projection_placeholder(
        sync_root: &Path,
        file: &ProjectionFileRecord,
        pinned: bool,
    ) -> Result<(), SmartSyncError> {
        let relative_path = normalize_relative_placeholder_path(&file.path)?;
        let target_path = sync_root.join(&relative_path);
        if !target_path.exists() {
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let sync_root_wide = wide_path(sync_root)?;
            let relative_name_wide = wide_str(OsStr::new(&relative_path));
            let file_time = file_time_from_unix_millis(file.created_at)?;
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

            unsafe {
                CfCreatePlaceholders(
                    PCWSTR(sync_root_wide.as_ptr()),
                    &mut placeholder,
                    create_flags(),
                    Some(&mut entries_processed),
                )?;
            }

            if entries_processed != 1 {
                return Err(SmartSyncError::InvalidPathWithContext(
                    "CfCreatePlaceholders",
                    format!("expected one entry for {relative_path}, got {entries_processed}"),
                ));
            }

            if placeholder[0].Result != S_OK {
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
        Ok(())
    }

    fn register_sync_root(sync_root_path: &Path) -> Result<(), SmartSyncError> {
        let sync_root_wide = wide_path(sync_root_path)?;
        let provider_name_wide = wide_str(OsStr::new(PROVIDER_NAME));
        let provider_version_wide = wide_str(OsStr::new(PROVIDER_VERSION));
        let sync_root_identity = sync_root_identity_bytes();

        let registration = CF_SYNC_REGISTRATION {
            StructSize: size_of::<CF_SYNC_REGISTRATION>() as u32,
            ProviderName: PCWSTR(provider_name_wide.as_ptr()),
            ProviderVersion: PCWSTR(provider_version_wide.as_ptr()),
            SyncRootIdentity: sync_root_identity.as_ptr().cast(),
            SyncRootIdentityLength: sync_root_identity.len() as u32,
            FileIdentity: ptr::null(),
            FileIdentityLength: 0,
            ProviderId: PROVIDER_ID,
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
        let initial_result =
            unsafe { CfRegisterSyncRoot(path, &registration, &policies, register_flags(false)) };
        if initial_result.is_ok() {
            return Ok(());
        }

        let first_error = initial_result
            .err()
            .map(|err| err.to_string())
            .unwrap_or_else(|| "unknown register error".to_string());
        warn!(
            "smart-sync: initial register failed for {} (provider={}, account={ACCOUNT_NAME}): {}",
            sync_root_path.display(),
            PROVIDER_NAME,
            first_error
        );

        unsafe {
            let _ = CfUnregisterSyncRoot(path);
        }

        unsafe { CfRegisterSyncRoot(path, &registration, &policies, register_flags(true))? };
        Ok(())
    }

    fn connect_sync_root(sync_root_path: &Path) -> Result<(), SmartSyncError> {
        if CONNECTION_KEY.get().is_some() {
            return Ok(());
        }

        let sync_root_wide = wide_path(sync_root_path)?;
        let callbacks = [
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
            let metadata = std::fs::metadata(path).map_err(SmartSyncError::Io)?;
            if !metadata.is_dir() {
                return Err(SmartSyncError::InvalidPath(
                    "sync root path exists and is not a directory",
                ));
            }

            if directory_is_empty(path)? {
                std::fs::remove_dir(path).map_err(SmartSyncError::Io)?;
            }
        }

        std::fs::create_dir_all(path).map_err(SmartSyncError::Io)?;
        assert_sync_root_writable(path)?;
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

    fn sync_root_identity_bytes() -> Vec<u8> {
        let vault_id = std::env::var("OMNIDRIVE_VAULT_ID")
            .unwrap_or_else(|_| "default-vault".to_string());
        format!("{PROVIDER_NAME}|{ACCOUNT_NAME}|{vault_id}")
            .into_bytes()
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

    fn placeholder_create_flags() -> CF_PLACEHOLDER_CREATE_FLAGS {
        CF_PLACEHOLDER_CREATE_FLAGS(
            CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC.0 | CF_PLACEHOLDER_CREATE_FLAG_SUPERSEDE.0,
        )
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
                    .join("SyncRoot")
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

    fn directory_is_empty(path: &Path) -> Result<bool, SmartSyncError> {
        let mut entries = std::fs::read_dir(path).map_err(SmartSyncError::Io)?;
        Ok(entries.next().is_none())
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
