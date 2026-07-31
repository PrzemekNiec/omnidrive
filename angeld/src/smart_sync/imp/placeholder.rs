use super::super::SmartSyncError;
use super::callbacks::*;
use super::paths::*;
use super::projection::*;
use crate::db;
use sqlx::SqlitePool;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use tracing::info;
use tracing::warn;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::CloudFilters::CF_CONVERT_FLAG_NONE;
use windows::Win32::Storage::CloudFilters::CF_FILE_RANGE;
use windows::Win32::Storage::CloudFilters::CF_HYDRATE_FLAGS;
use windows::Win32::Storage::CloudFilters::CF_IN_SYNC_STATE_IN_SYNC;
use windows::Win32::Storage::CloudFilters::CF_IN_SYNC_STATE_NOT_IN_SYNC;
use windows::Win32::Storage::CloudFilters::CF_PIN_STATE;
use windows::Win32::Storage::CloudFilters::CF_PIN_STATE_PINNED;
use windows::Win32::Storage::CloudFilters::CF_PIN_STATE_UNPINNED;
use windows::Win32::Storage::CloudFilters::CF_SET_IN_SYNC_FLAG_NONE;
use windows::Win32::Storage::CloudFilters::CF_SET_PIN_FLAG_NONE;
use windows::Win32::Storage::CloudFilters::CF_SET_PIN_FLAGS;
use windows::Win32::Storage::CloudFilters::CF_UPDATE_FLAG_DEHYDRATE;
use windows::Win32::Storage::CloudFilters::CF_UPDATE_FLAG_NONE;
use windows::Win32::Storage::CloudFilters::CF_UPDATE_FLAGS;
use windows::Win32::Storage::CloudFilters::CfConvertToPlaceholder;
use windows::Win32::Storage::CloudFilters::CfHydratePlaceholder;
use windows::Win32::Storage::CloudFilters::CfSetInSyncState;
use windows::Win32::Storage::CloudFilters::CfSetPinState;
use windows::Win32::Storage::CloudFilters::CfUpdatePlaceholder;
use windows::Win32::UI::Shell::SHCNE_UPDATEITEM;
use windows::Win32::UI::Shell::SHCNF_PATHW;
use windows::Win32::UI::Shell::SHChangeNotify;
use windows::core::PCWSTR;

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
    if target_path.exists()
        && let Err(err) = mark_in_sync(&target_path, true)
    {
        warn!(
            "smart-sync: mark_in_sync after pin state sync failed for inode={}: {}",
            inode_id, err
        );
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
    db::set_hydration_state(pool, inode_id, state.hydration_state.max(1)).await?;

    // Hydrated + pinned = fully synced, show green checkmark.
    if let Err(err) = mark_in_sync(&target_path, true) {
        warn!(
            "smart-sync: mark_in_sync after hydrate_now failed for inode={}: {}",
            inode_id, err
        );
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
        warn!(
            "smart-sync: mark_in_sync after ghost swap failed for inode={}: {}",
            inode_id, err
        );
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

pub(super) fn apply_pin_state(path: &Path, pin_state: CF_PIN_STATE) -> Result<(), SmartSyncError> {
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
pub(super) fn mark_in_sync(path: &Path, in_sync: bool) -> Result<(), SmartSyncError> {
    let file = open_placeholder_handle(path)?;
    let state = if in_sync {
        CF_IN_SYNC_STATE_IN_SYNC
    } else {
        CF_IN_SYNC_STATE_NOT_IN_SYNC
    };
    unsafe {
        CfSetInSyncState(as_handle(&file), state, CF_SET_IN_SYNC_FLAG_NONE, None)?;
    }
    Ok(())
}

#[allow(dead_code)]
pub(super) fn dehydrate_placeholder(path: &Path) -> Result<(), SmartSyncError> {
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
        CfHydratePlaceholder(as_handle(&file), 0, i64::MAX, CF_HYDRATE_FLAGS(0), None)?;
    }
    Ok(())
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

pub(super) fn notify_shell_path_changed(path: &Path) {
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
