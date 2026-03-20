mod api;
mod config;
mod db;
mod downloader;
mod gc;
mod packer;
mod repair;
mod uploader;
mod vault;
mod watcher;

use crate::api::ApiServer;
use crate::gc::GcWorker;
use crate::packer::{DEFAULT_CHUNK_SIZE, Packer, PackerConfig};
use crate::repair::RepairWorker;
use crate::uploader::{UploadWorker, Uploader};
use crate::vault::VaultKeyStore;
use crate::watcher::FileWatcher;
use std::env;
use std::io;
use std::path::PathBuf;
use tokio::fs;
use tokio::signal;

#[tokio::main]
async fn main() {
    if should_run_r2_smoke_test() {
        if let Err(err) = smoke_test_r2_upload().await {
            eprintln!("r2 smoke test failed: {err}");
            std::process::exit(1);
        }
        return;
    }

    if should_run_upload_diagnostics() {
        if let Err(err) = run_upload_diagnostics().await {
            eprintln!("upload diagnostics failed: {err}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(err) = run_daemon().await {
        eprintln!("angeld failed: {err}");
        std::process::exit(1);
    }
}

fn should_run_r2_smoke_test() -> bool {
    env_flag("OMNIDRIVE_SMOKE_TEST_R2")
}

fn should_run_upload_diagnostics() -> bool {
    env_flag("OMNIDRIVE_DIAG_UPLOADS")
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

    println!(
        "uploaded {} to {}/{}",
        pack_path.display(),
        uploaded.bucket,
        uploaded.key
    );
    Ok(())
}

async fn run_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();

    let db_url = env::var("OMNIDRIVE_DB_URL").unwrap_or_else(|_| "sqlite:omnidrive.db".to_string());
    let pool = db::init_db(&db_url).await?;
    let vault_keys = VaultKeyStore::new();
    let worker = UploadWorker::from_env(pool.clone()).await?;
    let repair_worker = RepairWorker::from_env(pool.clone()).await?;
    let gc_worker = GcWorker::from_env(pool.clone()).await?;
    let watcher = FileWatcher::from_env(pool.clone(), vault_keys.clone()).await?;
    let api = ApiServer::from_env(pool, vault_keys)?;

    println!("upload worker, repair worker, gc worker, file watcher, and api server started");

    let mut upload_task = tokio::spawn(async move { worker.run().await });
    let mut repair_task = tokio::spawn(async move { repair_worker.run().await });
    let mut gc_task = tokio::spawn(async move { gc_worker.run().await });
    let mut api_task = tokio::spawn(async move { api.run().await });
    let watcher_future = watcher.run();
    tokio::pin!(watcher_future);

    tokio::select! {
        result = &mut upload_task => {
            repair_task.abort();
            gc_task.abort();
            api_task.abort();
            let outcome = result??;
            Ok(outcome)
        }
        result = &mut repair_task => {
            upload_task.abort();
            gc_task.abort();
            api_task.abort();
            let outcome = result??;
            Ok(outcome)
        }
        result = &mut gc_task => {
            upload_task.abort();
            repair_task.abort();
            api_task.abort();
            let outcome = result??;
            Ok(outcome)
        }
        result = &mut watcher_future => {
            upload_task.abort();
            repair_task.abort();
            gc_task.abort();
            api_task.abort();
            let outcome = result?;
            Ok(outcome)
        }
        result = &mut api_task => {
            upload_task.abort();
            repair_task.abort();
            gc_task.abort();
            let outcome = result??;
            Ok(outcome)
        }
        signal = signal::ctrl_c() => {
            signal?;
            upload_task.abort();
            repair_task.abort();
            gc_task.abort();
            api_task.abort();
            println!("shutdown signal received");
            Ok(())
        }
    }
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
        println!(
            "diagnostic start provider={} force_path_style={} key={}",
            uploader.provider_name(),
            uploader.force_path_style(),
            key
        );

        match uploader.upload_debug_file(&diag_path, &key).await {
            Ok(result) => {
                println!(
                    "diagnostic ok provider={} bucket={} key={} etag={:?} version_id={:?}",
                    result.provider, result.bucket, result.key, result.etag, result.version_id
                );
            }
            Err(err) => {
                eprintln!(
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
