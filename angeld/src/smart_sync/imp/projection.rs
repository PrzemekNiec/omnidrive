use super::super::SmartSyncError;
use super::callbacks::*;
use super::paths::*;
use super::placeholder::*;
use super::state::*;
use crate::db;
use crate::db::ProjectionFileRecord;
use sqlx::SqlitePool;
use std::iter;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use tracing::error;
use tracing::info;
use tracing::trace;
use tracing::warn;
use windows::Win32::Foundation::S_OK;
use windows::Win32::Storage::CloudFilters::CF_CONVERT_FLAG_NONE;
use windows::Win32::Storage::CloudFilters::CF_CREATE_FLAG_NONE;
use windows::Win32::Storage::CloudFilters::CF_CREATE_FLAG_STOP_ON_ERROR;
use windows::Win32::Storage::CloudFilters::CF_CREATE_FLAGS;
use windows::Win32::Storage::CloudFilters::CF_FS_METADATA;
use windows::Win32::Storage::CloudFilters::CF_PIN_STATE_PINNED;
use windows::Win32::Storage::CloudFilters::CF_PIN_STATE_UNPINNED;
use windows::Win32::Storage::CloudFilters::CF_PLACEHOLDER_CREATE_FLAGS;
use windows::Win32::Storage::CloudFilters::CF_PLACEHOLDER_CREATE_INFO;
use windows::Win32::Storage::CloudFilters::CfConvertToPlaceholder;
use windows::Win32::Storage::CloudFilters::CfCreatePlaceholders;
use windows::Win32::Storage::FileSystem::CreateFileW;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_ARCHIVE;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NOT_CONTENT_INDEXED;
use windows::Win32::Storage::FileSystem::FILE_BASIC_INFO;
use windows::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
use windows::Win32::Storage::FileSystem::FILE_GENERIC_READ;
use windows::Win32::Storage::FileSystem::FILE_GENERIC_WRITE;
use windows::Win32::Storage::FileSystem::FILE_SHARE_READ;
use windows::Win32::Storage::FileSystem::FILE_SHARE_WRITE;
use windows::Win32::Storage::FileSystem::OPEN_EXISTING;
use windows::core::HRESULT;
use windows::core::PCWSTR;

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

    let mut failed = 0usize;
    for file in files {
        // Read the previously projected revision BEFORE ensure_* overwrites it:
        // that comparison is the only way to tell whether an existing placeholder
        // still points at stale content.
        let projected_revision = db::get_smart_sync_state(pool, file.inode_id)
            .await?
            .map(|state| state.revision_id);
        let state = db::ensure_smart_sync_state(pool, file.inode_id, file.revision_id).await?;
        let revision_changed =
            projected_revision.is_some_and(|revision| revision != file.revision_id);

        // One unprojectable file must not stop the whole vault from mounting.
        // Before this guard a single cfapi error aborted projection for every
        // remaining file and left the virtual drive unmounted.
        if let Err(err) =
            create_projection_placeholder(&sync_root, &file, state.pin_state != 0, revision_changed)
        {
            failed += 1;
            warn!(
                "smart-sync: projection failed for inode={} revision={} path='{}': {}",
                file.inode_id, file.revision_id, file.path, err
            );
        }
    }

    if failed > 0 {
        warn!(
            "smart-sync: {} file(s) could not be projected into {}; the rest of the vault is mounted",
            failed,
            sync_root.display()
        );
    }

    Ok(())
}

pub(super) fn create_projection_placeholder(
    sync_root: &Path,
    file: &ProjectionFileRecord,
    pinned: bool,
    revision_changed: bool,
) -> Result<(), SmartSyncError> {
    let relative_path = normalize_relative_placeholder_path(&file.path)?;
    let target_path = sync_root.join(&relative_path);
    // Always ensure parent directories are cloud-file placeholders,
    // even if the file placeholder itself already exists. Without this,
    // directories created by std::fs::create_dir_all in a prior session
    // remain plain folders and cldflt.sys blocks enumeration.
    let file_time = file_time_from_unix_millis(file.created_at)?;
    ensure_placeholder_directory_chain(sync_root, &relative_path, file_time).map_err(|err| {
        SmartSyncError::InvalidPathWithContext(
            "ensure_placeholder_directory_chain",
            format!("{relative_path}: {err}"),
        )
    })?;

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
                    FileAttributes: FILE_ATTRIBUTE_ARCHIVE.0 | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED.0,
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
                    relative_path, placeholder[0].Result.0 as u32
                ),
            ));
        }

        info!("smart-sync: placeholder ready {}", relative_path);
    } else if revision_changed {
        update_placeholder_revision(
            &target_path,
            file.inode_id,
            file.revision_id,
            file.size,
            file_time,
        )
        .map_err(|err| {
            SmartSyncError::InvalidPathWithContext(
                "update_placeholder_revision",
                format!("{relative_path}: {err}"),
            )
        })?;
        info!(
            "smart-sync: placeholder repointed to revision {} for {}",
            file.revision_id, relative_path
        );
    }

    apply_pin_state(
        &target_path,
        if pinned {
            CF_PIN_STATE_PINNED
        } else {
            CF_PIN_STATE_UNPINNED
        },
    )
    .map_err(|err| {
        SmartSyncError::InvalidPathWithContext("apply_pin_state", format!("{relative_path}: {err}"))
    })?;

    // New placeholder is in-sync with the cloud (its content matches the known revision).
    if let Err(err) = mark_in_sync(&target_path, true) {
        warn!(
            "smart-sync: mark_in_sync for new placeholder {} failed: {}",
            relative_path, err
        );
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

pub(super) async fn projection_path_for_inode(
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

/// Convert an existing regular directory into a cfapi placeholder so that
/// the cloud-files minifilter does not block directory enumeration.
fn convert_directory_to_placeholder(path: &Path) {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
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
    let result =
        unsafe { CfConvertToPlaceholder(handle, None, 0, CF_CONVERT_FLAG_NONE, None, None) };
    unsafe {
        windows::Win32::Foundation::CloseHandle(handle).ok();
    }
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

fn create_flags() -> CF_CREATE_FLAGS {
    CF_CREATE_FLAGS(CF_CREATE_FLAG_NONE.0 | CF_CREATE_FLAG_STOP_ON_ERROR.0)
}

fn placeholder_create_flags() -> CF_PLACEHOLDER_CREATE_FLAGS {
    CF_PLACEHOLDER_CREATE_FLAGS(0)
}
