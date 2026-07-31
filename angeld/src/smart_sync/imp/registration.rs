use super::super::SmartSyncError;
use super::super::SyncRootRepairReport;
use super::super::SyncRootStateSnapshot;
use super::callbacks::*;
use super::paths::*;
use super::projection::*;
use super::state::*;
use crate::win_acl;
use sha2::Digest;
use sha2::Sha256;
use sqlx::SqlitePool;
use std::ffi::OsStr;
use std::mem::size_of;
use std::os::windows::fs::MetadataExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::ptr;
use tracing::info;
use tracing::trace;
use tracing::warn;
use windows::Win32::Storage::CloudFilters::CF_CALLBACK_REGISTRATION;
use windows::Win32::Storage::CloudFilters::CF_CALLBACK_TYPE_CANCEL_FETCH_DATA;
use windows::Win32::Storage::CloudFilters::CF_CALLBACK_TYPE_FETCH_DATA;
use windows::Win32::Storage::CloudFilters::CF_CALLBACK_TYPE_FETCH_PLACEHOLDERS;
use windows::Win32::Storage::CloudFilters::CF_CALLBACK_TYPE_NONE;
use windows::Win32::Storage::CloudFilters::CF_CONNECT_FLAG_NONE;
use windows::Win32::Storage::CloudFilters::CF_HARDLINK_POLICY;
use windows::Win32::Storage::CloudFilters::CF_HARDLINK_POLICY_NONE;
use windows::Win32::Storage::CloudFilters::CF_HYDRATION_POLICY;
use windows::Win32::Storage::CloudFilters::CF_HYDRATION_POLICY_FULL;
use windows::Win32::Storage::CloudFilters::CF_HYDRATION_POLICY_MODIFIER;
use windows::Win32::Storage::CloudFilters::CF_HYDRATION_POLICY_MODIFIER_NONE;
use windows::Win32::Storage::CloudFilters::CF_HYDRATION_POLICY_PRIMARY;
use windows::Win32::Storage::CloudFilters::CF_INSYNC_POLICY;
use windows::Win32::Storage::CloudFilters::CF_INSYNC_POLICY_NONE;
use windows::Win32::Storage::CloudFilters::CF_PLACEHOLDER_MANAGEMENT_POLICY;
use windows::Win32::Storage::CloudFilters::CF_PLACEHOLDER_MANAGEMENT_POLICY_CREATE_UNRESTRICTED;
use windows::Win32::Storage::CloudFilters::CF_POPULATION_POLICY;
use windows::Win32::Storage::CloudFilters::CF_POPULATION_POLICY_MODIFIER;
use windows::Win32::Storage::CloudFilters::CF_POPULATION_POLICY_MODIFIER_NONE;
use windows::Win32::Storage::CloudFilters::CF_POPULATION_POLICY_PARTIAL;
use windows::Win32::Storage::CloudFilters::CF_POPULATION_POLICY_PRIMARY;
use windows::Win32::Storage::CloudFilters::CF_REGISTER_FLAG_NONE;
use windows::Win32::Storage::CloudFilters::CF_REGISTER_FLAG_UPDATE;
use windows::Win32::Storage::CloudFilters::CF_REGISTER_FLAGS;
use windows::Win32::Storage::CloudFilters::CF_SYNC_POLICIES;
use windows::Win32::Storage::CloudFilters::CF_SYNC_REGISTRATION;
use windows::Win32::Storage::CloudFilters::CF_SYNC_ROOT_INFO_STANDARD;
use windows::Win32::Storage::CloudFilters::CF_SYNC_ROOT_STANDARD_INFO;
use windows::Win32::Storage::CloudFilters::CfConnectSyncRoot;
use windows::Win32::Storage::CloudFilters::CfDisconnectSyncRoot;
use windows::Win32::Storage::CloudFilters::CfGetSyncRootInfoByPath;
use windows::Win32::Storage::CloudFilters::CfRegisterSyncRoot;
use windows::Win32::Storage::CloudFilters::CfUnregisterSyncRoot;
use windows::core::GUID;
use windows::core::PCWSTR;

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
            // PARTIAL: the filter does not require full directory enumeration before
            // allowing user-initiated file creation (drops) in the sync root.
            Primary: CF_POPULATION_POLICY_PRIMARY(CF_POPULATION_POLICY_PARTIAL.0),
            Modifier: CF_POPULATION_POLICY_MODIFIER(CF_POPULATION_POLICY_MODIFIER_NONE.0),
        },
        InSync: CF_INSYNC_POLICY(CF_INSYNC_POLICY_NONE.0),
        HardLink: CF_HARDLINK_POLICY(CF_HARDLINK_POLICY_NONE.0),
        PlaceholderManagement: CF_PLACEHOLDER_MANAGEMENT_POLICY(
            CF_PLACEHOLDER_MANAGEMENT_POLICY_CREATE_UNRESTRICTED.0,
        ),
    };

    let path = PCWSTR(sync_root_wide.as_ptr());
    if inspect_existing_sync_root(
        sync_root_path,
        path,
        &provider_name,
        &provider_version,
        &sync_root_identity,
    ) {
        info!(
            "smart-sync: existing sync root detected, updating policies for {}",
            sync_root_path.display()
        );
        // Always push the latest CF_SYNC_POLICIES (e.g. after a PARTIAL→FULL change).
        // CF_REGISTER_FLAG_UPDATE keeps existing placeholders intact.
        unsafe {
            let _ = CfRegisterSyncRoot(path, &registration, &policies, register_flags(true));
        }
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

    log_registration_context(
        sync_root_path,
        &registration,
        register_flags(false),
        "initial",
    );

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

    log_registration_context(
        sync_root_path,
        &registration,
        register_flags(true),
        "update",
    );
    unsafe { CfRegisterSyncRoot(path, &registration, &policies, register_flags(true))? };
    Ok(())
}

fn connect_sync_root(sync_root_path: &Path) -> Result<(), SmartSyncError> {
    {
        let guard = CONNECTION_KEY.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Ok(());
        }
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
    *CONNECTION_KEY.lock().unwrap_or_else(|e| e.into_inner()) = Some(connection);
    info!("smart-sync: connected {}", sync_root_path.display());
    Ok(())
}

pub fn shutdown_sync_root() -> Result<(), SmartSyncError> {
    let key_opt = CONNECTION_KEY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(connection_key) = key_opt {
        unsafe {
            let _ = CfDisconnectSyncRoot(connection_key);
        }
        info!("smart-sync: disconnected sync provider");
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

struct ExistingSyncRootInfo {
    provider_name: String,
    provider_version: String,
    identity_bytes: Vec<u8>,
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
            let provider_name_matches = info
                .provider_name
                .eq_ignore_ascii_case(expected_provider_name);
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
    std::env::var("OMNIDRIVE_SYNC_PROVIDER_NAME").unwrap_or_else(|_| PROVIDER_NAME.to_string())
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
    let owner_output =
        powershell_literal_output(path, "$acl = Get-Acl -LiteralPath __PATH__; $acl.Owner");
    let acl_output = Command::new("icacls")
        .arg(path)
        .creation_flags(CREATE_NO_WINDOW)
        .output();

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
    let escaped = path.display().to_string().replace('\'', "''");
    let script = script_template.replace("__PATH__", &format!("'{}'", escaped));
    let output = Command::new("powershell.exe")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(script)
        .creation_flags(CREATE_NO_WINDOW)
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

pub fn audit_sync_root_state(
    sync_root_path: &Path,
) -> Result<SyncRootStateSnapshot, SmartSyncError> {
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
        connected: CONNECTION_KEY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some(),
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

    if CONNECTION_KEY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
    {
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

pub(super) fn prepare_sync_root_directory(path: &Path) -> Result<(), SmartSyncError> {
    if path.exists() {
        trace!(
            "smart-sync: sync root exists before prep: {}",
            path.display()
        );
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

    trace!(
        "smart-sync: creating sync root directory {}",
        path.display()
    );
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

fn assert_sync_root_writable(path: &Path) -> Result<(), SmartSyncError> {
    let probe = path.join(".omnidrive_acl_probe");
    std::fs::write(&probe, b"ok").map_err(SmartSyncError::Io)?;
    std::fs::remove_file(&probe).map_err(SmartSyncError::Io)?;
    Ok(())
}

fn register_flags(update: bool) -> CF_REGISTER_FLAGS {
    let mut flags = CF_REGISTER_FLAG_NONE.0;
    if update {
        flags |= CF_REGISTER_FLAG_UPDATE.0;
    }
    CF_REGISTER_FLAGS(flags)
}
