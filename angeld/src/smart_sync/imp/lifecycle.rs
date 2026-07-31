use super::super::SmartSyncError;
use super::paths::*;
use super::placeholder::*;
use super::projection::*;
use super::registration::*;
use super::state::*;
use sqlx::SqlitePool;
use std::path::Path;
use tracing::info;
use tracing::trace;
use tracing::warn;

/// Security P0 lock sequence:
/// 1. Recursive dehydrate of every file under sync_root (wipes CF cache)
/// 2. CfDisconnectSyncRoot (callbacks stop)
/// 3. CfUnregisterSyncRoot (removes CF reparse state)
///
/// Virtual drive unmount is handled by the caller after this returns.
pub async fn dismount_after_lock(sync_root_path: &Path) -> Result<(), SmartSyncError> {
    let sync_root = normalize_sync_root_path(sync_root_path)?;

    // 1. Recursive dehydrate — wipe every decrypted byte from the CF cache.
    info!(
        "[LOCK] starting recursive dehydration of {}",
        sync_root.display()
    );
    flush_smart_sync_logs();
    dehydrate_directory_recursive(&sync_root);
    info!("[LOCK] recursive dehydration finished");
    flush_smart_sync_logs();

    // 2. Disconnect sync provider.
    shutdown_sync_root()?;

    // 3. Unregister sync root.
    unregister_sync_root(sync_root_path)?;

    info!("[LOCK] CF sync root torn down");
    flush_smart_sync_logs();
    Ok(())
}

/// Vault unlock sequence:
/// 1. CfRegisterSyncRoot + CfConnectSyncRoot
/// 2. Project all vault files as dehydrated placeholders
///
/// Virtual drive hide + mount is handled by the caller after this returns.
pub async fn mount_after_unlock(
    pool: &SqlitePool,
    sync_root_path: &Path,
) -> Result<(), SmartSyncError> {
    // 1. Register + connect sync root.
    register_sync_root_public(sync_root_path).await?;

    // 2. Project all vault files as dehydrated placeholders.
    project_vault_to_sync_root(pool, sync_root_path).await?;

    info!("[UNLOCK] CF sync root ready");
    flush_smart_sync_logs();
    Ok(())
}

/// Recursive filesystem walk — dehydrates every CF placeholder found under `dir`.
/// Non-placeholder files (e.g. user-dropped regular files) produce a warning but
/// are skipped; we never delete user data.
fn dehydrate_directory_recursive(dir: &Path) {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(err) => {
            warn!("[LOCK] cannot read dir {}: {}", dir.display(), err);
            return;
        }
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            dehydrate_directory_recursive(&path);
        } else if path.is_file()
            && let Err(err) = dehydrate_placeholder(&path)
        {
            // Non-placeholder files (e.g. user-dropped regular files) will fail here;
            // that's expected — log at trace level and continue.
            trace!("[LOCK] dehydrate skipped {}: {}", path.display(), err);
        }
    }
}
