use sqlx::SqlitePool;
use std::fmt;
use std::path::Path;

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

#[cfg(windows)]
mod imp {
    use super::SmartSyncError;
    use crate::db::{self, ProjectionFileRecord};
    use sqlx::SqlitePool;
    use std::ffi::OsStr;
    use std::iter;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::ptr;
    use std::sync::OnceLock;
    use std::time::{Duration, UNIX_EPOCH};
    use windows::core::{GUID, HRESULT, PCWSTR};
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::Storage::CloudFilters::{
        CF_CALLBACK_REGISTRATION, CF_CALLBACK_TYPE_NONE, CF_CONNECT_FLAG_NONE, CF_CONNECTION_KEY,
        CF_CREATE_FLAGS, CF_CREATE_FLAG_NONE, CF_CREATE_FLAG_STOP_ON_ERROR, CF_FS_METADATA,
        CF_HARDLINK_POLICY, CF_HARDLINK_POLICY_NONE, CF_HYDRATION_POLICY,
        CF_HYDRATION_POLICY_FULL, CF_HYDRATION_POLICY_MODIFIER,
        CF_HYDRATION_POLICY_MODIFIER_NONE, CF_HYDRATION_POLICY_PRIMARY, CF_INSYNC_POLICY,
        CF_INSYNC_POLICY_NONE, CF_PLACEHOLDER_CREATE_FLAGS, CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC,
        CF_PLACEHOLDER_CREATE_FLAG_SUPERSEDE, CF_PLACEHOLDER_CREATE_INFO,
        CF_PLACEHOLDER_MANAGEMENT_POLICY, CF_PLACEHOLDER_MANAGEMENT_POLICY_CREATE_UNRESTRICTED,
        CF_POPULATION_POLICY, CF_POPULATION_POLICY_FULL, CF_POPULATION_POLICY_MODIFIER,
        CF_POPULATION_POLICY_MODIFIER_NONE, CF_POPULATION_POLICY_PRIMARY, CF_REGISTER_FLAGS,
        CF_REGISTER_FLAG_DISABLE_ON_DEMAND_POPULATION_ON_ROOT, CF_REGISTER_FLAG_NONE,
        CF_REGISTER_FLAG_UPDATE, CF_SYNC_POLICIES, CF_SYNC_REGISTRATION, CfConnectSyncRoot,
        CfCreatePlaceholders, CfRegisterSyncRoot, CfUnregisterSyncRoot,
    };
    use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_ARCHIVE, FILE_BASIC_INFO};

    const PROVIDER_NAME: &str = "OmniDrive";
    const PROVIDER_VERSION: &str = "1.0";
    const SYNC_ROOT_IDENTITY: &[u8] = b"omnidrive-sync-root";
    const PROVIDER_ID: GUID = GUID::from_u128(0xb7a42c2a_4af1_4f4a_a650_0b1308b8f019);
    static CONNECTION_KEY: OnceLock<CF_CONNECTION_KEY> = OnceLock::new();

    #[repr(C)]
    struct PlaceholderIdentity {
        inode_id: i64,
        revision_id: i64,
    }

    pub async fn register_sync_root_public(sync_root_path: &Path) -> Result<(), SmartSyncError> {
        let sync_root = normalize_sync_root_path(sync_root_path)?;
        eprintln!("smart-sync: registering {}", sync_root.display());
        register_sync_root(&sync_root).map_err(|err| {
            SmartSyncError::InvalidPathWithContext("CfRegisterSyncRoot", err.to_string())
        })?;
        eprintln!("smart-sync: connecting {}", sync_root.display());
        connect_sync_root(&sync_root).map_err(|err| {
            SmartSyncError::InvalidPathWithContext("CfConnectSyncRoot", err.to_string())
        })?;
        eprintln!("smart-sync: connected {}", sync_root.display());
        Ok(())
    }

    pub async fn project_vault_to_sync_root(
        pool: &SqlitePool,
        sync_root_path: &Path,
    ) -> Result<(), SmartSyncError> {
        let sync_root = normalize_sync_root_path(sync_root_path)?;
        let files = db::get_active_files_for_projection(pool).await?;
        eprintln!(
            "smart-sync: projecting {} active file placeholders into {}",
            files.len(),
            sync_root.display()
        );

        for file in files {
            create_projection_placeholder(&sync_root, &file)?;
        }

        Ok(())
    }

    fn create_projection_placeholder(
        sync_root: &Path,
        file: &ProjectionFileRecord,
    ) -> Result<(), SmartSyncError> {
        let relative_path = normalize_relative_placeholder_path(&file.path)?;
        let target_path = sync_root.join(&relative_path);
        if target_path.exists() {
            return Ok(());
        }

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

        eprintln!("smart-sync: placeholder ready {}", relative_path);
        Ok(())
    }

    fn register_sync_root(sync_root_path: &Path) -> Result<(), SmartSyncError> {
        let sync_root_wide = wide_path(sync_root_path)?;
        let provider_name_wide = wide_str(OsStr::new(PROVIDER_NAME));
        let provider_version_wide = wide_str(OsStr::new(PROVIDER_VERSION));

        let registration = CF_SYNC_REGISTRATION {
            StructSize: size_of::<CF_SYNC_REGISTRATION>() as u32,
            ProviderName: PCWSTR(provider_name_wide.as_ptr()),
            ProviderVersion: PCWSTR(provider_version_wide.as_ptr()),
            SyncRootIdentity: SYNC_ROOT_IDENTITY.as_ptr().cast(),
            SyncRootIdentityLength: SYNC_ROOT_IDENTITY.len() as u32,
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
        unsafe {
            let _ = CfUnregisterSyncRoot(path);
        }
        let create_result =
            unsafe { CfRegisterSyncRoot(path, &registration, &policies, register_flags(false)) };
        if create_result.is_ok() {
            return Ok(());
        }

        unsafe { CfRegisterSyncRoot(path, &registration, &policies, register_flags(true))? };
        Ok(())
    }

    fn connect_sync_root(sync_root_path: &Path) -> Result<(), SmartSyncError> {
        if CONNECTION_KEY.get().is_some() {
            return Ok(());
        }

        let sync_root_wide = wide_path(sync_root_path)?;
        let callbacks = [CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_NONE,
            Callback: None,
        }];

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
        std::fs::create_dir_all(path).map_err(SmartSyncError::Io)?;
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

    fn ensure_path_inside_user_profile(path: &Path) -> Result<(), SmartSyncError> {
        let user_profile = std::env::var("USERPROFILE")
            .map_err(|_| SmartSyncError::InvalidPath("USERPROFILE is not set"))?;
        let user_profile = PathBuf::from(
            normalized_windows_path_string(Path::new(&user_profile)).map_err(|_| {
                SmartSyncError::InvalidPath("USERPROFILE is not a valid Windows path")
            })?,
        );

        if !path.starts_with(&user_profile) {
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

    fn register_flags(update: bool) -> CF_REGISTER_FLAGS {
        let mut flags = CF_REGISTER_FLAG_DISABLE_ON_DEMAND_POPULATION_ON_ROOT.0;
        if update {
            flags |= CF_REGISTER_FLAG_UPDATE.0;
        } else {
            flags |= CF_REGISTER_FLAG_NONE.0;
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
}
