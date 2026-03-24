mod api;
mod aws_http;
mod cache;
mod config;
mod db;
mod diagnostics;
mod disaster_recovery;
mod downloader;
mod gc;
mod logging;
mod packer;
mod repair;
mod scrubber;
mod secure_fs;
mod shell_integration;
mod smart_sync;
mod uploader;
mod vault;
mod virtual_drive;
mod watcher;
mod win_acl;

use crate::api::ApiServer;
use crate::diagnostics::init_global_diagnostics;
use crate::disaster_recovery::{MetadataBackupProviderManager, start_metadata_backup_worker};
use crate::downloader::Downloader;
use crate::gc::GcWorker;
use crate::logging::{default_log_dir, init_logging};
use crate::packer::{DEFAULT_CHUNK_SIZE, Packer, PackerConfig};
use crate::repair::RepairWorker;
use crate::scrubber::ScrubberWorker;
use crate::uploader::{UploadWorker, Uploader};
use crate::vault::VaultKeyStore;
use crate::watcher::FileWatcher;
use std::env;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::signal;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() {
    if let Err(err) = init_logging() {
        eprintln!("failed to initialize logging: {err}");
        std::process::exit(1);
    }

    if should_run_r2_smoke_test() {
        if let Err(err) = smoke_test_r2_upload().await {
            error!("r2 smoke test failed: {err}");
            std::process::exit(1);
        }
        return;
    }

    if should_run_upload_diagnostics() {
        if let Err(err) = run_upload_diagnostics().await {
            error!("upload diagnostics failed: {err}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(err) = run_daemon().await {
        error!("angeld failed: {err}");
        std::process::exit(1);
    }
}

fn should_run_r2_smoke_test() -> bool {
    env_flag("OMNIDRIVE_SMOKE_TEST_R2")
}

fn should_run_upload_diagnostics() -> bool {
    env_flag("OMNIDRIVE_DIAG_UPLOADS")
}

fn should_disable_sync() -> bool {
    env::args().any(|arg| arg == "--no-sync")
}

fn env_flag(key: &str) -> bool {
    matches!(
        env::var(key)
            .ok()
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1" | "true" | "yes")
    )
}

async fn smoke_test_r2_upload() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();

    let spool_dir = env_path("OMNIDRIVE_SPOOL_DIR", ".omnidrive/spool");
    let chunk_size = env::var("OMNIDRIVE_CHUNK_SIZE_BYTES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_CHUNK_SIZE);
    let sample_path = spool_dir.join("r2-smoke-test-input.txt");
    let sample_bytes = b"omnidrive r2 smoke test payload\n";

    fs::create_dir_all(&spool_dir).await?;
    fs::write(&sample_path, sample_bytes).await?;

    let pool = db::init_db("sqlite::memory:").await?;
    let inode_id = db::create_inode(
        &pool,
        None,
        "r2-smoke-test-input.txt",
        "FILE",
        i64::try_from(sample_bytes.len())?,
    )
    .await?;

    let vault_keys = VaultKeyStore::new();
    let _ = vault_keys.unlock(&pool, "r2-smoke-test-passphrase").await?;
    let packer = Packer::new(
        pool,
        vault_keys,
        PackerConfig::new(&spool_dir).with_chunk_size(chunk_size),
    )?;
    let pack_result = packer.pack_file(inode_id, &sample_path).await?;
    let pack_path = pack_result
        .pack_path
        .ok_or_else(|| io::Error::other("smoke test packer produced no pack file"))?;

    let uploader = Uploader::from_r2_env().await?;
    let uploaded = uploader.upload_pack(&pack_path).await?;

    info!(
        "uploaded {} to {}/{}",
        pack_path.display(),
        uploaded.bucket,
        uploaded.key
    );
    Ok(())
}

async fn run_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    let diagnostics = init_global_diagnostics();
    let no_sync = should_disable_sync();

    let sync_root = sync_root_path();
    let drive_letter = virtual_drive_letter();
    let db_url = env::var("OMNIDRIVE_DB_URL").unwrap_or_else(|_| "sqlite:omnidrive.db".to_string());
    let spool_dir = env_path("OMNIDRIVE_SPOOL_DIR", ".omnidrive/spool");
    let download_spool_dir = env_path("OMNIDRIVE_DOWNLOAD_SPOOL_DIR", ".omnidrive/download-spool");
    let cache_dir = cache_root_path();
    let log_dir = default_log_dir();
    let db_dir = sqlite_db_directory(&db_url).unwrap_or_else(|| PathBuf::from("."));

    fs::create_dir_all(&spool_dir).await?;
    fs::create_dir_all(&download_spool_dir).await?;
    fs::create_dir_all(&cache_dir).await?;
    fs::create_dir_all(&log_dir).await?;
    fs::create_dir_all(&db_dir).await?;
    if !no_sync {
        fs::create_dir_all(&sync_root).await?;
    }

    win_acl::secure_directory(&cache_dir)
        .map_err(|err| io::Error::other(format!("failed to secure cache dir: {err}")))?;
    win_acl::secure_directory(&spool_dir)
        .map_err(|err| io::Error::other(format!("failed to secure spool dir: {err}")))?;
    win_acl::secure_directory(&download_spool_dir)
        .map_err(|err| io::Error::other(format!("failed to secure download spool dir: {err}")))?;
    if should_secure_db_directory(&db_dir) {
        win_acl::secure_directory(&db_dir)
            .map_err(|err| io::Error::other(format!("failed to secure db dir: {err}")))?;
    } else {
        warn!(
            "skipping db directory ACL hardening for relative or working-directory path {}",
            db_dir.display()
        );
    }

    let pool = db::init_db(&db_url).await?;
    let vault_keys = VaultKeyStore::new();
    let downloader = Arc::new(Downloader::from_env(pool.clone(), vault_keys.clone()).await?);
    if no_sync {
        warn!("starting angeld with --no-sync; skipping Smart Sync and virtual drive bootstrap");
    } else {
        smart_sync::install_hydration_runtime(pool.clone(), downloader)
            .map_err(|err| io::Error::other(format!("smart sync hydration setup failed: {err}")))?;
        smart_sync::register_sync_root(&sync_root).await
            .map_err(|err| io::Error::other(format!("smart sync register failed: {err}")))?;
        smart_sync::project_vault_to_sync_root(&pool, &sync_root).await
            .map_err(|err| io::Error::other(format!("smart sync projection failed: {err}")))?;
        virtual_drive::hide_sync_root(&sync_root)
            .map_err(|err| io::Error::other(format!("virtual drive hide sync root failed: {err}")))?;
        virtual_drive::mount_virtual_drive(&drive_letter, &sync_root)
            .map_err(|err| io::Error::other(format!("virtual drive mount failed: {err}")))?;
        virtual_drive::configure_virtual_drive_appearance(
            &drive_letter,
            "OmniDrive",
            &virtual_drive_icon_path(),
        )
        .map_err(|err| io::Error::other(format!("virtual drive appearance failed: {err}")))?;
        shell_integration::register_explorer_context_menu(
            &drive_letter,
            &shell_api_base(),
            &virtual_drive_icon_path(),
        )
        .map_err(|err| io::Error::other(format!("shell integration registration failed: {err}")))?;
    }

    let worker = UploadWorker::from_env(pool.clone()).await?;
    let repair_worker = RepairWorker::from_env(pool.clone()).await?;
    let scrubber_worker = ScrubberWorker::from_env(pool.clone()).await?;
    let gc_worker = GcWorker::from_env(pool.clone()).await?;
    let metadata_backup_provider_manager =
        Arc::new(MetadataBackupProviderManager::from_env().await?);
    let metadata_backup_worker = start_metadata_backup_worker(
        pool.clone(),
        metadata_backup_provider_manager,
        Arc::new(vault_keys.clone()),
    );
    let watcher = FileWatcher::from_env(pool.clone(), vault_keys.clone()).await?;
    let api = ApiServer::from_env(pool, vault_keys, diagnostics.clone())?;

    if no_sync {
        info!("smart sync bootstrap skipped by --no-sync");
    } else {
        info!("smart sync bootstrap ready at {}", sync_root.display());
        info!("virtual drive mounted at {}", drive_letter);
    }
    info!("upload worker, repair worker, scrubber worker, gc worker, file watcher, and api server started");

    let mut upload_task = tokio::spawn(async move { worker.run().await });
    let mut repair_task = tokio::spawn(async move { repair_worker.run().await });
    let mut scrubber_task = tokio::spawn(async move { scrubber_worker.run().await });
    let mut gc_task = tokio::spawn(async move { gc_worker.run().await });
    let mut metadata_backup_task = metadata_backup_worker;
    let mut api_task = tokio::spawn(async move { api.run().await });
    let watcher_future = watcher.run();
    tokio::pin!(watcher_future);

    let result = tokio::select! {
        result = &mut upload_task => {
            repair_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            let outcome = result??;
            Ok(outcome)
        }
        result = &mut repair_task => {
            upload_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            let outcome = result??;
            Ok(outcome)
        }
        result = &mut scrubber_task => {
            upload_task.abort();
            repair_task.abort();
            gc_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            let outcome = result??;
            Ok(outcome)
        }
        result = &mut gc_task => {
            upload_task.abort();
            repair_task.abort();
            scrubber_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            let outcome = result??;
            Ok(outcome)
        }
        result = &mut metadata_backup_task => {
            upload_task.abort();
            repair_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            api_task.abort();
            result?;
            Ok(())
        }
        result = &mut watcher_future => {
            upload_task.abort();
            repair_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            let outcome = result?;
            Ok(outcome)
        }
        result = &mut api_task => {
            upload_task.abort();
            repair_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            metadata_backup_task.abort();
            let outcome = result??;
            Ok(outcome)
        }
        signal = signal::ctrl_c() => {
            signal?;
            upload_task.abort();
            repair_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            info!("shutdown signal received");
            Ok(())
        }
    };

    if !no_sync {
        if let Err(err) = smart_sync::shutdown_sync_root() {
            warn!("smart sync shutdown warning: {}", err);
        }

        if let Err(err) = virtual_drive::unmount_virtual_drive(&drive_letter) {
            warn!("virtual drive unmount warning for {}: {}", drive_letter, err);
        }
    }

    result
}

async fn run_upload_diagnostics() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();

    let spool_dir = env_path("OMNIDRIVE_SPOOL_DIR", ".omnidrive/spool");
    fs::create_dir_all(&spool_dir).await?;

    let diag_path = spool_dir.join("provider-diagnostic-1kb.txt");
    let payload = vec![b'X'; 1024];
    fs::write(&diag_path, &payload).await?;

    let uploaders = Uploader::all_from_env().await?;

    for uploader in uploaders {
        let key = format!(
            "diagnostics/{}/provider-diagnostic-1kb.txt",
            uploader.provider_name()
        );
        info!(
            "diagnostic start provider={} force_path_style={} key={}",
            uploader.provider_name(),
            uploader.force_path_style(),
            key
        );

        match uploader.upload_debug_file(&diag_path, &key).await {
            Ok(result) => {
                info!(
                    "diagnostic ok provider={} bucket={} key={} etag={:?} version_id={:?}",
                    result.provider, result.bucket, result.key, result.etag, result.version_id
                );
            }
            Err(err) => {
                error!(
                    "diagnostic fail provider={}: {}",
                    uploader.provider_name(),
                    err
                );
            }
        }
    }

    Ok(())
}

fn env_path(key: &str, default: &str) -> PathBuf {
    env::var(key)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(default))
}

fn sync_root_path() -> PathBuf {
    env::var("OMNIDRIVE_SYNC_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            env::var("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    env::var("USERPROFILE")
                        .map(PathBuf::from)
                        .unwrap_or_else(|_| PathBuf::from(r"C:\Users\Default"))
                })
                .join("OmniDrive")
                .join("SyncRoot")
        })
}

fn virtual_drive_letter() -> String {
    env::var("OMNIDRIVE_DRIVE_LETTER").unwrap_or_else(|_| "O:".to_string())
}

fn virtual_drive_icon_path() -> PathBuf {
    env::var("OMNIDRIVE_DRIVE_ICON")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("icons").join("omnidrive.ico"))
}

fn cache_root_path() -> PathBuf {
    env::var("OMNIDRIVE_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            env::var("LOCALAPPDATA")
                .map(|root| PathBuf::from(root).join("OmniDrive").join("Cache"))
                .unwrap_or_else(|_| PathBuf::from(".omnidrive").join("cache"))
        })
}

fn sqlite_db_directory(db_url: &str) -> Option<PathBuf> {
    if db_url.contains(":memory:") {
        return None;
    }

    let raw = db_url
        .strip_prefix("sqlite://")
        .or_else(|| db_url.strip_prefix("sqlite:"))
        .unwrap_or(db_url);

    if raw.is_empty() {
        return None;
    }

    let normalized = if raw.len() >= 4
        && raw.starts_with('/')
        && raw.as_bytes().get(2) == Some(&b':')
    {
        &raw[1..]
    } else {
        raw
    };

    let path = PathBuf::from(normalized);
    Some(
        path.parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".")),
    )
}

fn should_secure_db_directory(path: &std::path::Path) -> bool {
    let normalized = path.as_os_str().to_string_lossy().trim().to_string();
    if normalized.is_empty() || normalized == "." {
        return false;
    }

    if let Ok(current_dir) = std::env::current_dir() {
        if let (Ok(candidate), Ok(current)) =
            (std::fs::canonicalize(path), std::fs::canonicalize(current_dir))
        {
            if candidate == current {
                return false;
            }
        }
    }

    true
}

fn shell_api_base() -> String {
    let bind = env::var("OMNIDRIVE_API_BIND").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    let host_port = bind
        .strip_prefix("0.0.0.0:")
        .map(|port| format!("127.0.0.1:{port}"))
        .unwrap_or(bind);
    format!("http://{host_port}")
}
