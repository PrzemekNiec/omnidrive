mod acl;
mod api;
mod api_error;
mod aws_http;
mod cache;
mod cloud_guard;
mod config;
mod db;
mod device_identity;
mod diagnostics;
mod identity;
mod disaster_recovery;
mod downloader;
mod gc;
mod ingest;
mod logging;
mod migrator;
mod onboarding;
mod packer;
mod peer;
mod pipe_server;
mod recovery;
mod repair;
mod runtime_paths;
mod scrubber;
mod secure_fs;
mod sharing;
mod shell_integration;
mod shell_state;
mod smart_sync;
mod uploader;
mod vault;
mod virtual_drive;
mod watcher;
mod win_acl;

use crate::api::ApiServer;
use crate::config::AppConfig;
use crate::device_identity::ensure_local_device_identity;
use crate::diagnostics::init_global_diagnostics;
use crate::disaster_recovery::{
    MetadataBackupProviderManager, restore_metadata_from_cloud, start_metadata_backup_worker,
};
use crate::downloader::Downloader;
use crate::gc::GcWorker;
use crate::logging::init_logging;
use crate::onboarding::{
    cleanup_stale_restore_staging, cleanup_stale_uploads, get_active_provider_configs,
    initialize_onboarding_persistence,
};
use crate::packer::{DEFAULT_CHUNK_SIZE, Packer, PackerConfig};
use crate::peer::{PeerClient, PeerService};
use crate::repair::RepairWorker;
use crate::runtime_paths::{RuntimePaths, sqlite_db_file_path};
use crate::scrubber::ScrubberWorker;
use crate::uploader::{UploadWorker, Uploader};
use crate::vault::{VaultKeyStore, bootstrap_local_vault};
use crate::watcher::FileWatcher;
use std::env;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::signal;
use tokio::sync::watch;
use tokio::time::{Duration, sleep};
use tracing::{error, info, warn};

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let runtime_paths = RuntimePaths::detect();
    runtime_paths.export_env_defaults();
    if let Err(err) = init_logging() {
        eprintln!("failed to initialize logging: {err}");
        std::process::exit(1);
    }
    install_panic_hook();

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

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let payload = if let Some(message) = panic_info.payload().downcast_ref::<&str>() {
            (*message).to_string()
        } else if let Some(message) = panic_info.payload().downcast_ref::<String>() {
            message.clone()
        } else {
            "non-string panic payload".to_string()
        };

        error!("panic: {} at {}", payload, location);
        eprintln!("panic: {} at {}", payload, location);
        crate::logging::flush_logs_best_effort();
    }));
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

fn should_dry_run() -> bool {
    env::args().any(|arg| arg == "--dry-run")
}

fn is_e2e_test_mode() -> bool {
    env_flag("OMNIDRIVE_E2E_TEST_MODE")
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
    let dry_run_flag = should_dry_run();
    let e2e_test_mode = is_e2e_test_mode();
    let runtime_paths = RuntimePaths::detect();
    runtime_paths.export_env_defaults();

    let sync_root = runtime_paths.sync_root.clone();
    let plain_local_drive_target = runtime_paths
        .default_watch_dir
        .clone()
        .unwrap_or_else(|| sync_root.clone());
    let preferred_drive_letter = virtual_drive_letter();
    runtime_paths
        .bootstrap_directories(false)
        .await?;
    runtime_paths.secure_runtime_directories()?;
    let database_missing_on_start = runtime_paths
        .db_file_path
        .as_ref()
        .map(|path| !path.exists())
        .unwrap_or(false);

    let just_restored = maybe_auto_restore_database(&runtime_paths.db_url).await?;

    let pool = db::init_db(&runtime_paths.db_url).await?;
    let dry_run_active = dry_run_flag || env_flag("OMNIDRIVE_DRY_RUN");
    cloud_guard::sync_runtime_flags(&pool, dry_run_active).await?;
    if dry_run_active {
        info!("DRY-RUN mode active: cloud operations will not perform external S3 side effects");
    }
    initialize_onboarding_persistence(&pool).await?;
    cleanup_stale_restore_staging(&runtime_paths).await;
    match cleanup_stale_uploads(&pool).await {
        Ok(actions) => {
            for action in &actions {
                info!("[ONBOARDING] startup multipart cleanup: {}", action);
            }
        }
        Err(err) => {
            warn!("[ONBOARDING] startup multipart cleanup failed: {}", err);
        }
    }
    let active_provider_configs = match get_active_provider_configs(&pool).await {
        Ok(configs) => configs,
        Err(err) => {
            warn!("failed to load active providers from DB: {}", err);
            Vec::new()
        }
    };
    let remote_providers_configured = !active_provider_configs.is_empty();
    shell_state::set_cloud_mode_hint(remote_providers_configured);
    let smart_sync_enabled = !no_sync && remote_providers_configured;
    runtime_paths
        .bootstrap_directories(smart_sync_enabled)
        .await?;

    if remote_providers_configured {
        let has_provider = |name: &str| {
            active_provider_configs
                .iter()
                .any(|config| config.provider_name == name)
        };
        let mut provider_labels = Vec::new();
        if has_provider("cloudflare-r2") {
            provider_labels.push("R2");
        }
        if has_provider("backblaze-b2") {
            provider_labels.push("B2");
        }
        if has_provider("scaleway") {
            provider_labels.push("Scaleway");
        }
        info!(
            "Active providers loaded from DB: [{}]",
            provider_labels.join(", ")
        );
    } else {
        warn!("starting OmniDrive in setup/local-only mode: no remote providers configured");
    }
    let local_vault_bootstrapped = bootstrap_default_local_vault(
        &pool,
        &runtime_paths,
        database_missing_on_start,
        just_restored,
    )
    .await?;
    let app_config = AppConfig::from_env();
    let local_device = ensure_local_device_identity(&pool, &app_config).await?;
    let local_vault_id = db::get_vault_params(&pool)
        .await?
        .map(|record| record.vault_id)
        .unwrap_or_else(|| "local-vault".to_string());

    // Epic 34.0b: migrate single-user vault to multi-user schema
    match db::migrate_single_to_multi_user(&pool, &local_vault_id).await {
        Ok(true) => info!("migrated single-user vault to multi-user schema (owner={})", local_device.device_id),
        Ok(false) => {} // already migrated or no device yet
        Err(e) => warn!("multi-user migration failed (non-fatal): {e}"),
    }

    let vault_keys = VaultKeyStore::new();
    let (provider_reload_tx, provider_reload_rx) = watch::channel(0u64);
    if no_sync && e2e_test_mode {
        let worker =
            UploadWorker::from_onboarding_db(pool.clone(), Some(provider_reload_rx.clone()))
                .await?;
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Repair,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Scrubber,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Gc,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Watcher,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::MetadataBackup,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Peer,
            crate::diagnostics::WorkerStatus::Idle,
        );
        let api = ApiServer::from_env(pool, vault_keys, diagnostics.clone(), None, None)?;

        warn!(
            "starting angeld in OMNIDRIVE_E2E_TEST_MODE with --no-sync; only uploader and API workers are enabled"
        );
        info!("smart sync bootstrap skipped by --no-sync");
        info!(
            "e2e test mode enabled: repair, scrubber, gc, watcher, and metadata backup workers are disabled"
        );

        let mut upload_task = tokio::spawn(async move { worker.run().await });
        let mut api_task = tokio::spawn(async move { api.run().await });

        let result = tokio::select! {
            result = &mut upload_task => {
                api_task.abort();
                result??;
                Ok(())
            }
            result = &mut api_task => {
                upload_task.abort();
                result??;
                Ok(())
            }
            signal = signal::ctrl_c() => {
                signal?;
                upload_task.abort();
                api_task.abort();
                info!("shutdown signal received");
                Ok(())
            }
        };

        return result;
    }

    if e2e_test_mode {
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Uploader,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Repair,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Scrubber,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Gc,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Watcher,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::MetadataBackup,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Peer,
            crate::diagnostics::WorkerStatus::Idle,
        );

        let mut smart_sync_ready = false;
        match smart_sync::register_sync_root(&sync_root).await {
            Ok(()) => {
                smart_sync_ready = true;
                if just_restored {
                    info!(
                        "just-restored database detected; forcing recursive placeholder projection into {}",
                        sync_root.display()
                    );
                }
                if let Err(err) =
                    project_sync_root_with_retry(&pool, &sync_root, just_restored).await
                {
                    warn!(
                        "smart sync bootstrap warning: projection failed for {}: {}",
                        sync_root.display(),
                        err
                    );
                    smart_sync_ready = false;
                }
            }
            Err(err) => {
                warn!(
                    "smart sync bootstrap warning: registration failed for {}: {}",
                    sync_root.display(),
                    err
                );
                if just_restored {
                    info!(
                        "just-restored database detected; attempting placeholder projection despite registration warning into {}",
                        sync_root.display()
                    );
                }
                match project_sync_root_with_retry(&pool, &sync_root, just_restored).await {
                    Ok(()) => {
                        info!(
                            "smart sync fallback projection succeeded at {} after registration warning",
                            sync_root.display()
                        );
                        smart_sync_ready = true;
                    }
                    Err(project_err) => {
                        warn!(
                            "smart sync bootstrap warning: fallback projection failed for {}: {}",
                            sync_root.display(),
                            project_err
                        );
                    }
                }
            }
        }

        let api = ApiServer::from_env(pool, vault_keys, diagnostics.clone(), None, None)?;
        if smart_sync_ready {
            info!("smart sync bootstrap ready at {}", sync_root.display());
        } else {
            warn!("smart sync bootstrap warning at {}", sync_root.display());
        }
        info!(
            "e2e test mode enabled: background provider workers and virtual drive bootstrap are disabled"
        );

        let mut api_task = tokio::spawn(async move { api.run().await });
        let result = tokio::select! {
            result = &mut api_task => {
                result??;
                Ok(())
            }
            signal = signal::ctrl_c() => {
                signal?;
                api_task.abort();
                info!("shutdown signal received");
                Ok(())
            }
        };

        if let Err(err) = smart_sync::shutdown_sync_root() {
            warn!("smart sync shutdown warning: {}", err);
        }
        if let Err(err) = smart_sync::unregister_sync_root(&sync_root) {
            warn!(
                "smart sync unregister warning for {}: {}",
                sync_root.display(),
                err
            );
        }

        return result;
    }

    let downloader = Arc::new(
        Downloader::from_provider_configs(
            pool.clone(),
            vault_keys.clone(),
            env_path("OMNIDRIVE_DOWNLOAD_SPOOL_DIR", ".omnidrive/download-spool"),
            Duration::from_millis(120_000),
            active_provider_configs.clone(),
        )
        .await?,
    );
    downloader
        .set_peer_client(PeerClient::new(
            pool.clone(),
            local_device.device_id.clone(),
            local_vault_id.clone(),
        ))
        .await;
    if !no_sync {
        let _ = virtual_drive::unmount_virtual_drive(&preferred_drive_letter);
    }
    let drive_letter = if no_sync {
        preferred_drive_letter.clone()
    } else {
        virtual_drive::select_mount_drive_letter(&preferred_drive_letter)
            .unwrap_or(preferred_drive_letter.clone())
    };
    if no_sync {
        warn!("starting angeld with --no-sync; skipping Smart Sync and virtual drive bootstrap");
    } else if smart_sync_enabled {
        smart_sync::install_hydration_runtime(pool.clone(), downloader.clone())
            .map_err(|err| io::Error::other(format!("smart sync hydration setup failed: {err}")))?;
        smart_sync::register_sync_root(&sync_root)
            .await
            .map_err(|err| io::Error::other(format!("smart sync register failed: {err}")))?;
        if just_restored {
            info!(
                "just-restored database detected; forcing recursive placeholder projection into {}",
                sync_root.display()
            );
        }
        project_sync_root_with_retry(&pool, &sync_root, just_restored)
            .await
            .map_err(|err| io::Error::other(format!("smart sync projection failed: {err}")))?;
        virtual_drive::hide_sync_root(&sync_root).map_err(|err| {
            io::Error::other(format!("virtual drive hide sync root failed: {err}"))
        })?;
        virtual_drive::mount_virtual_drive(&drive_letter, &sync_root)
            .map_err(|err| io::Error::other(format!("virtual drive mount failed: {err}")))?;
    } else {
        info!(
            "setup/local-only mode: skipping Smart Sync and mounting a plain local drive view at {}",
            plain_local_drive_target.display()
        );
        virtual_drive::mount_virtual_drive(&drive_letter, &plain_local_drive_target)
            .map_err(|err| io::Error::other(format!("virtual drive mount failed: {err}")))?;
    }

    if !no_sync {
        match shell_state::startup_recover_shell() {
            Ok(report) => {
                if report.actions.is_empty() && report.shell_state.is_healthy() {
                    info!(
                        "startup shell audit healthy for {} (target={}, browsable={}, context_menu={}, icon={}, label={})",
                        report.shell_state.preferred_drive_letter,
                        report.shell_state.expected_target,
                        report.shell_state.drive_browsable,
                        report.shell_state.context_menu_registered,
                        report.shell_state.drive_icon_registered,
                        report.shell_state.drive_label_registered
                    );
                } else if report.actions.is_empty() {
                    warn!(
                        "startup shell audit detected residual drift for {} (target_matches={}, browsable={}, autostart={}, context_menu={}, icon={}, label={})",
                        report.shell_state.preferred_drive_letter,
                        report.shell_state.drive_target_matches,
                        report.shell_state.drive_browsable,
                        report.shell_state.autostart_registered,
                        report.shell_state.context_menu_registered,
                        report.shell_state.drive_icon_registered,
                        report.shell_state.drive_label_registered
                    );
                } else {
                    for action in &report.actions {
                        info!("startup shell recovery: {}", action);
                    }
                    info!(
                        "startup shell recovery complete for {} (target={})",
                        report.shell_state.preferred_drive_letter,
                        report.shell_state.expected_target
                    );
                }
            }
            Err(err) => {
                warn!(
                    "startup shell recovery warning for {}: {}",
                    drive_letter, err
                );
            }
        }

        if smart_sync_enabled {
            match smart_sync::audit_sync_root_state(&sync_root) {
                Ok(snapshot) if snapshot.registered_for_provider && snapshot.connected => {
                    info!(
                        "startup sync-root audit healthy for {} (registered={}, connected={})",
                        snapshot.path, snapshot.registered_for_provider, snapshot.connected
                    );
                }
                Ok(snapshot) => {
                    warn!(
                        "startup sync-root audit detected drift for {} (registered={}, registered_for_provider={}, connected={})",
                        snapshot.path,
                        snapshot.registered,
                        snapshot.registered_for_provider,
                        snapshot.connected
                    );
                    match smart_sync::repair_sync_root(&pool, &sync_root).await {
                        Ok(report) => {
                            for action in &report.actions {
                                info!("startup sync-root recovery: {}", action);
                            }
                            info!(
                                "startup sync-root recovery complete for {}",
                                report.sync_root_state.path
                            );
                        }
                        Err(err) => {
                            warn!(
                                "startup sync-root recovery warning for {}: {}",
                                sync_root.display(),
                                err
                            );
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        "startup sync-root audit warning for {}: {}",
                        sync_root.display(),
                        err
                    );
                }
            }
        }
    }

    let watcher = FileWatcher::from_env(pool.clone(), vault_keys.clone()).await?;
    let api = ApiServer::from_env(
        pool.clone(),
        vault_keys.clone(),
        diagnostics.clone(),
        Some(downloader.clone()),
        Some(provider_reload_tx.clone()),
    )?;
    let peer_service = PeerService::new(
        pool.clone(),
        downloader.clone(),
        local_device.clone(),
        local_vault_id.clone(),
        app_config.peer_port,
        app_config.peer_discovery_port,
        Duration::from_millis(app_config.peer_discovery_interval_ms),
    );

    if no_sync {
        info!("smart sync bootstrap skipped by --no-sync");
    } else if smart_sync_enabled {
        info!("smart sync bootstrap ready at {}", sync_root.display());
        info!("virtual drive mounted at {}", drive_letter);
    } else {
        info!(
            "plain local vault drive mounted at {} -> {}",
            drive_letter,
            plain_local_drive_target.display()
        );
    }
    if local_vault_bootstrapped {
        info!("default local vault bootstrap is ready");
    }
    info!(
        "local device identity ready: {} ({})",
        local_device.device_name, local_device.device_id
    );
    if !remote_providers_configured {
        let worker =
            UploadWorker::from_onboarding_db(pool.clone(), Some(provider_reload_rx.clone()))
                .await?;
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Repair,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Scrubber,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Gc,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::MetadataBackup,
            crate::diagnostics::WorkerStatus::Idle,
        );
        diagnostics::set_worker_status(
            crate::diagnostics::WorkerKind::Peer,
            crate::diagnostics::WorkerStatus::Idle,
        );
        info!(
            "setup/local-only mode enabled: repair/scrub/gc/metadata workers are idle until remote providers are configured"
        );
        info!("file watcher and api server started");

        let mut upload_task = tokio::spawn(async move { worker.run().await });
        let mut api_task = tokio::spawn(async move { api.run().await });
        let mut peer_task = tokio::spawn(async move { peer_service.run().await });
        let _pipe_task = tokio::spawn(pipe_server::run_pipe_server(pool.clone()));
        let watcher_future = watcher.run();
        tokio::pin!(watcher_future);

        let result = tokio::select! {
            result = &mut upload_task => {
                api_task.abort();
                peer_task.abort();
                result??;
                Ok(())
            }
            result = &mut watcher_future => {
                upload_task.abort();
                api_task.abort();
                peer_task.abort();
                result?;
                Ok(())
            }
            result = &mut api_task => {
                upload_task.abort();
                peer_task.abort();
                result??;
                Ok(())
            }
            result = &mut peer_task => {
                upload_task.abort();
                api_task.abort();
                result??;
                Ok(())
            }
            signal = signal::ctrl_c() => {
                signal?;
                upload_task.abort();
                api_task.abort();
                peer_task.abort();
                info!("shutdown signal received");
                Ok(())
            }
        };

        if !no_sync {
            if smart_sync_enabled {
                if let Err(err) = smart_sync::shutdown_sync_root() {
                    warn!("smart sync shutdown warning: {}", err);
                }

                if e2e_test_mode
                    && let Err(err) = smart_sync::unregister_sync_root(&sync_root) {
                        warn!(
                            "smart sync unregister warning for {}: {}",
                            sync_root.display(),
                            err
                        );
                    }
            }

            if let Err(err) = virtual_drive::unmount_virtual_drive(&drive_letter) {
                warn!(
                    "virtual drive unmount warning for {}: {}",
                    drive_letter, err
                );
            }
        }

        return result;
    }

    let worker =
        UploadWorker::from_onboarding_db(pool.clone(), Some(provider_reload_rx.clone())).await?;
    let repair_worker = RepairWorker::from_onboarding_db(pool.clone()).await?;
    let scrubber_worker = ScrubberWorker::from_onboarding_db(pool.clone()).await?;
    let gc_worker = GcWorker::from_onboarding_db(pool.clone()).await?;
    let ingest_spool_dir = env_path("OMNIDRIVE_SPOOL_DIR", ".omnidrive/spool");
    let ingest_worker =
        ingest::IngestWorker::new(pool.clone(), vault_keys.clone(), ingest_spool_dir, sync_root.clone());
    let metadata_backup_provider_manager =
        Arc::new(MetadataBackupProviderManager::from_onboarding_db_all(&pool).await?);
    let metadata_backup_worker = start_metadata_backup_worker(
        pool.clone(),
        metadata_backup_provider_manager,
        Arc::new(vault_keys.clone()),
    );
    info!(
        "upload worker, repair worker, scrubber worker, gc worker, ingest worker, file watcher, and api server started"
    );

    let mut upload_task = tokio::spawn(async move { worker.run().await });
    let mut repair_task = tokio::spawn(async move { repair_worker.run().await });
    let mut scrubber_task = tokio::spawn(async move { scrubber_worker.run().await });
    let mut gc_task = tokio::spawn(async move { gc_worker.run().await });
    let mut ingest_task = tokio::spawn(async move { ingest_worker.run().await });
    let mut metadata_backup_task = metadata_backup_worker;
    let mut api_task = tokio::spawn(async move { api.run().await });
    let mut peer_task = tokio::spawn(async move { peer_service.run().await });
    let _pipe_task = tokio::spawn(pipe_server::run_pipe_server(pool.clone()));

    // Periodic cleanup of expired share password tokens (every 5 minutes)
    let cleanup_pool = pool.clone();
    let _token_cleanup_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            match db::cleanup_expired_share_tokens(&cleanup_pool).await {
                Ok(count) if count > 0 => {
                    tracing::debug!("cleaned up {count} expired share password tokens");
                }
                Err(err) => {
                    tracing::warn!("failed to clean up share tokens: {err}");
                }
                _ => {}
            }
        }
    });
    let watcher_future = watcher.run();
    tokio::pin!(watcher_future);

    let result = tokio::select! {
        result = &mut upload_task => {
            repair_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            ingest_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            peer_task.abort();
            result??;
            Ok(())
        }
        result = &mut repair_task => {
            upload_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            ingest_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            peer_task.abort();
            result??;
            Ok(())
        }
        result = &mut scrubber_task => {
            upload_task.abort();
            repair_task.abort();
            gc_task.abort();
            ingest_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            peer_task.abort();
            result??;
            Ok(())
        }
        result = &mut gc_task => {
            upload_task.abort();
            repair_task.abort();
            scrubber_task.abort();
            ingest_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            peer_task.abort();
            result??;
            Ok(())
        }
        result = &mut ingest_task => {
            upload_task.abort();
            repair_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            peer_task.abort();
            result??;
            Ok(())
        }
        result = &mut metadata_backup_task => {
            upload_task.abort();
            repair_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            ingest_task.abort();
            api_task.abort();
            peer_task.abort();
            result?;
            Ok(())
        }
        result = &mut watcher_future => {
            upload_task.abort();
            repair_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            ingest_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            peer_task.abort();
            result?;
            Ok(())
        }
        result = &mut api_task => {
            upload_task.abort();
            repair_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            ingest_task.abort();
            metadata_backup_task.abort();
            peer_task.abort();
            result??;
            Ok(())
        }
        result = &mut peer_task => {
            upload_task.abort();
            repair_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            ingest_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            result??;
            Ok(())
        }
        signal = signal::ctrl_c() => {
            signal?;
            upload_task.abort();
            repair_task.abort();
            scrubber_task.abort();
            gc_task.abort();
            ingest_task.abort();
            metadata_backup_task.abort();
            api_task.abort();
            peer_task.abort();
            info!("shutdown signal received");
            Ok(())
        }
    };

    if !no_sync {
        if smart_sync_enabled {
            if let Err(err) = smart_sync::shutdown_sync_root() {
                warn!("smart sync shutdown warning: {}", err);
            }

            if e2e_test_mode
                && let Err(err) = smart_sync::unregister_sync_root(&sync_root) {
                    warn!(
                        "smart sync unregister warning for {}: {}",
                        sync_root.display(),
                        err
                    );
                }
        }

        if let Err(err) = virtual_drive::unmount_virtual_drive(&drive_letter) {
            warn!(
                "virtual drive unmount warning for {}: {}",
                drive_letter, err
            );
        }
    }

    result
}

async fn bootstrap_default_local_vault(
    pool: &sqlx::SqlitePool,
    runtime_paths: &RuntimePaths,
    database_missing_on_start: bool,
    just_restored: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut bootstrapped = false;

    if database_missing_on_start && !just_restored
        && bootstrap_local_vault(pool).await? {
            info!(
                "initialized default local vault metadata in {}",
                runtime_paths
                    .db_file_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| runtime_paths.db_url.clone())
            );
            bootstrapped = true;
        }

    if db::list_sync_policies(pool).await?.is_empty()
        && let Some(default_watch_dir) = &runtime_paths.default_watch_dir {
            fs::create_dir_all(default_watch_dir).await?;
            let policy_path = absolute_path_to_policy_key(default_watch_dir)?;
            db::set_sync_policy_type_for_path(pool, &policy_path, "LOCAL").await?;
            info!(
                "bootstrapped default local vault watch root at {}",
                default_watch_dir.display()
            );
            bootstrapped = true;
        }

    Ok(bootstrapped)
}

async fn project_sync_root_with_retry(
    pool: &sqlx::SqlitePool,
    sync_root: &std::path::Path,
    just_restored: bool,
) -> Result<(), crate::smart_sync::SmartSyncError> {
    let attempts = if just_restored { 8 } else { 5 };
    let delay = Duration::from_millis(250);
    let mut last_err = None;

    for attempt in 1..=attempts {
        match smart_sync::project_vault_to_sync_root(pool, sync_root).await {
            Ok(()) => return Ok(()),
            Err(err) => {
                let error_text = err.to_string();
                let retryable = is_retryable_cloud_projection_error(&err.to_string());
                let fatal = is_fatal_cloud_projection_error(&error_text);
                error!(
                    "smart sync projection attempt {}/{} failed for {}: {}",
                    attempt,
                    attempts,
                    sync_root.display(),
                    error_text
                );
                if fatal {
                    error!(
                        "smart sync projection encountered a fatal error for {} and will not retry",
                        sync_root.display()
                    );
                    return Err(err);
                }
                if !retryable || attempt == attempts {
                    return Err(err);
                }
                warn!(
                    "smart sync projection retry {}/{} for {} after error: {}",
                    attempt,
                    attempts,
                    sync_root.display(),
                    err
                );
                last_err = Some(err);
                sleep(delay).await;
            }
        }
    }

    Err(last_err.expect("projection retry loop should capture the last error"))
}

fn is_retryable_cloud_projection_error(message: &str) -> bool {
    message.contains("0x8007017C")
        || message.contains("Operacja w chmurze jest nieprawid")
        || message.contains("cloud operation is invalid")
}

fn is_fatal_cloud_projection_error(message: &str) -> bool {
    message.contains("0x80070186")
        || message.contains("0x80070057")
        || message.contains("invalid arguments")
        || message.contains("only supported for files in the cloud sync root")
        || message
            .contains("obsługiwana tylko w przypadku plików w katalogu głównym synchronizacji")
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

fn virtual_drive_letter() -> String {
    env::var("OMNIDRIVE_DRIVE_LETTER").unwrap_or_else(|_| "O:".to_string())
}

async fn maybe_auto_restore_database(db_url: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(passphrase) = env::var("OMNIDRIVE_AUTO_RESTORE_PASSPHRASE").ok() else {
        return Ok(false);
    };
    let Some(db_path) = sqlite_db_file_path(db_url) else {
        return Ok(false);
    };

    if fs::try_exists(&db_path).await? {
        return Ok(false);
    }

    let provider_manager = MetadataBackupProviderManager::from_env().await?;
    warn!(
        "local SQLite database is missing; attempting automatic metadata restore into {}",
        db_path.display()
    );
    restore_metadata_from_cloud(&provider_manager, &passphrase, &db_path).await?;
    info!(
        "automatic metadata restore completed for {}",
        db_path.display()
    );
    Ok(true)
}

fn absolute_path_to_policy_key(path: &std::path::Path) -> io::Result<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    let normalized = absolute.to_string_lossy().replace('\\', "/");
    let segments: Vec<&str> = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return Err(io::Error::other("invalid empty policy path"));
    }
    Ok(segments.join("/"))
}
