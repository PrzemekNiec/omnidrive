mod common;

use angeld::db;
use angeld::downloader::Downloader;
use angeld::vault::VaultKeyStore;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::put;
use axum::Router;
use serde::Deserialize;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::io;
use std::path::{Path as FsPath, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio::time::sleep;

const PASSPHRASE: &str = "scrubber-repair-e2e-passphrase";
const BUCKET_R2: &str = "bucket-r2";
const BUCKET_SCALEWAY: &str = "bucket-scaleway";
const BUCKET_B2: &str = "bucket-b2";

struct ChaosEnv {
    temp_root: PathBuf,
    localapp: PathBuf,
    base: PathBuf,
    db_url: String,
    watch_root: PathBuf,
    object_root: PathBuf,
}

struct DaemonHandle {
    child: Child,
    base_url: String,
    session_token: Option<String>,
}

struct ScopedEnv {
    previous: HashMap<String, Option<String>>,
}

struct HeartbeatOutcome {
    reads: usize,
    failures: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DiagnosticsHealth {
    worker_statuses: WorkerStatuses,
}

#[derive(Debug, Deserialize)]
struct WorkerStatuses {
    uploader: String,
    repair: String,
    scrubber: String,
    watcher: String,
}

#[derive(Clone, Debug, Deserialize)]
struct FileEntry {
    inode_id: i64,
    path: String,
    current_revision_id: Option<i64>,
}

#[derive(Clone, Debug)]
struct ActivePackSnapshot {
    pack_id: String,
    mode: db::StorageMode,
    status: String,
    shard_count: usize,
}

#[derive(Clone, Debug)]
struct ShardSnapshot {
    provider: String,
    shard_index: i64,
    object_key: String,
    status: String,
    verification_status: Option<String>,
}

#[derive(Clone)]
struct FsMockS3State {
    root: Arc<PathBuf>,
}

impl ChaosEnv {
    async fn create() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_root = create_temp_root()?;
        let localapp = temp_root.join("localapp");
        let base = localapp.join("OmniDrive");
        let watch_root = temp_root.join("watch");
        let object_root = temp_root.join("mock-s3");
        let db_path = base.join("e2e-scrubber-repair.db");
        let db_url = format!("sqlite:///{}", normalize_for_sqlite_url(&db_path));

        fs::create_dir_all(base.join("logs")).await?;
        fs::create_dir_all(base.join("Cache")).await?;
        fs::create_dir_all(base.join("Spool")).await?;
        fs::create_dir_all(base.join("download-spool")).await?;
        fs::create_dir_all(&watch_root).await?;
        fs::create_dir_all(&object_root).await?;

        Ok(Self {
            temp_root,
            localapp,
            base,
            db_url,
            watch_root,
            object_root,
        })
    }

    async fn spawn_daemon(
        &self,
        mock_addr: std::net::SocketAddr,
    ) -> Result<DaemonHandle, Box<dyn std::error::Error>> {
        // Pre-seed the DB with mock provider configs so the daemon starts in
        // full cloud mode (with repair/scrubber/gc workers enabled).
        let pool = db::init_db(&self.db_url).await?;
        common::seed_mock_providers(&pool, mock_addr, &self.watch_root).await?;
        pool.close().await;

        let api_port = reserve_port().await?;
        let base_url = format!("http://127.0.0.1:{api_port}");
        let repo_root = FsPath::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");

        let mut command = Command::new(env!("CARGO_BIN_EXE_angeld"));
        command
            .current_dir(repo_root)
            .arg("--no-sync")
            .env("LOCALAPPDATA", &self.localapp)
            .env("OMNIDRIVE_DB_URL", &self.db_url)
            .env("OMNIDRIVE_WATCH_DIR", &self.watch_root)
            .env("OMNIDRIVE_SPOOL_DIR", self.base.join("Spool"))
            .env("OMNIDRIVE_DOWNLOAD_SPOOL_DIR", self.base.join("download-spool"))
            .env("OMNIDRIVE_CACHE_DIR", self.base.join("Cache"))
            .env("OMNIDRIVE_API_BIND", format!("127.0.0.1:{api_port}"))
            .env("OMNIDRIVE_WATCH_DEBOUNCE_MS", "100")
            .env("OMNIDRIVE_WATCH_RESCAN_MS", "3600000")
            .env("OMNIDRIVE_UPLOAD_POLL_INTERVAL_MS", "100")
            .env("OMNIDRIVE_UPLOAD_TIMEOUT_MS", "5000")
            .env("OMNIDRIVE_UPLOAD_CONNECT_TIMEOUT_MS", "1000")
            .env("OMNIDRIVE_UPLOAD_READ_TIMEOUT_MS", "1000")
            .env("OMNIDRIVE_UPLOAD_RETRY_BASE_MS", "500")
            .env("OMNIDRIVE_UPLOAD_RETRY_MAX_MS", "2000")
            .env("OMNIDRIVE_UPLOAD_BUFFERED", "1")
            .env("OMNIDRIVE_REPAIR_POLL_INTERVAL_MS", "100")
            .env("OMNIDRIVE_REPAIR_TIMEOUT_MS", "5000")
            .env("OMNIDRIVE_REPAIR_CONNECT_TIMEOUT_MS", "1000")
            .env("OMNIDRIVE_REPAIR_READ_TIMEOUT_MS", "1000")
            .env("OMNIDRIVE_SCRUB_POLL_INTERVAL_MS", "60000")
            .env("OMNIDRIVE_SCRUB_TIMEOUT_MS", "5000")
            .env("OMNIDRIVE_SCRUB_CONNECT_TIMEOUT_MS", "1000")
            .env("OMNIDRIVE_SCRUB_READ_TIMEOUT_MS", "1000")
            .env("OMNIDRIVE_SCRUB_BATCH_SIZE", "16")
            .env("OMNIDRIVE_SCRUB_DEEP_MODULUS", "9999")
            .env("OMNIDRIVE_GC_POLL_INTERVAL_MS", "60000")
            .env("OMNIDRIVE_METADATA_BACKUP_INTERVAL_MS", "60000")
            .env("RUST_LOG", "info")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let child = command.spawn()?;
        let handle = DaemonHandle { child, base_url, session_token: None };
        handle.wait_for_api_ready().await?;
        Ok(handle)
    }
}

impl DaemonHandle {
    async fn wait_for_api_ready(&self) -> Result<(), Box<dyn std::error::Error>> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match http_get_json::<DiagnosticsHealth>(&format!(
                "{}/api/diagnostics/health",
                self.base_url
            ), None)
            .await
            {
                Ok(_) => return Ok(()),
                Err(_) if Instant::now() < deadline => sleep(Duration::from_millis(100)).await,
                Err(err) => return Err(format!("daemon API did not become ready: {err}").into()),
            }
        }
    }

    async fn unlock(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let response = http_post_json(
            &format!("{}/api/unlock", self.base_url),
            &serde_json::json!({ "passphrase": PASSPHRASE }),
            None,
        )
        .await?;
        if response["status"] != serde_json::Value::String("UNLOCKED".to_string()) {
            return Err(format!("unlock response was unexpected: {response}").into());
        }
        self.session_token = response["session_token"].as_str().map(|s| s.to_string());
        Ok(())
    }

    async fn list_files(&mut self) -> Result<Vec<FileEntry>, Box<dyn std::error::Error>> {
        self.ensure_running()?;
        http_get_json::<Vec<FileEntry>>(&format!("{}/api/files", self.base_url), self.session_token.as_deref()).await
    }

    async fn health(&mut self) -> Result<DiagnosticsHealth, Box<dyn std::error::Error>> {
        self.ensure_running()?;
        http_get_json::<DiagnosticsHealth>(&format!("{}/api/diagnostics/health", self.base_url), None)
            .await
    }

    async fn scrub_now(&mut self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        self.ensure_running()?;
        http_post_json(
            &format!("{}/api/maintenance/scrub-now", self.base_url),
            &serde_json::json!({}),
            self.session_token.as_deref(),
        )
        .await
    }

    fn ensure_running(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(status) = self.child.try_wait()? {
            return Err(format!("daemon exited unexpectedly with status {status}").into());
        }
        Ok(())
    }

    async fn shutdown(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }
}

impl Drop for DaemonHandle {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl ScopedEnv {
    fn new() -> Self {
        Self {
            previous: HashMap::new(),
        }
    }

    fn set(&mut self, key: &str, value: impl Into<String>) {
        if !self.previous.contains_key(key) {
            self.previous
                .insert(key.to_string(), std::env::var(key).ok());
        }
        unsafe {
            std::env::set_var(key, value.into());
        }
    }
}

impl Drop for ScopedEnv {
    fn drop(&mut self) {
        for (key, previous) in self.previous.drain() {
            match previous {
                Some(value) => unsafe {
                    std::env::set_var(&key, value);
                },
                None => unsafe {
                    std::env::remove_var(&key);
                },
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scrubber_detects_missing_shard_and_repair_restores_health_without_read_failures(
) -> Result<(), Box<dyn std::error::Error>> {
    let env = ChaosEnv::create().await?;
    let (mock_addr, mock_server) = spawn_fs_mock_s3(env.object_root.clone()).await?;
    let mut daemon = env.spawn_daemon(mock_addr).await?;
    daemon.unlock().await?;

    let payload = Arc::new(build_payload(1024 * 1024));
    let file_path = env.watch_root.join("chaos.bin");
    fs::write(&file_path, payload.as_slice()).await?;

    let pool = SqlitePool::connect(&env.db_url).await?;
    let (inode_id, revision_id, logical_path) =
        wait_for_file_ingested(&mut daemon, &pool, "chaos.bin").await?;
    let active_pack = current_active_pack(&pool, inode_id).await?;
    assert_eq!(active_pack.mode, db::StorageMode::Ec2_1);
    assert_eq!(active_pack.status, "COMPLETED_HEALTHY");
    assert_eq!(active_pack.shard_count, 3);

    let mut env_guard = ScopedEnv::new();
    env_guard.set("LOCALAPPDATA", env.localapp.to_string_lossy().to_string());
    env_guard.set("OMNIDRIVE_DB_URL", env.db_url.clone());
    env_guard.set(
        "OMNIDRIVE_DOWNLOAD_SPOOL_DIR",
        env.base.join("download-spool").to_string_lossy().to_string(),
    );
    env_guard.set(
        "OMNIDRIVE_CACHE_DIR",
        env.base.join("Cache").to_string_lossy().to_string(),
    );
    env_guard.set("OMNIDRIVE_R2_ENDPOINT", format!("http://{mock_addr}"));
    env_guard.set("OMNIDRIVE_R2_REGION", "auto");
    env_guard.set("OMNIDRIVE_R2_BUCKET", BUCKET_R2);
    env_guard.set("OMNIDRIVE_R2_ACCESS_KEY_ID", "test");
    env_guard.set("OMNIDRIVE_R2_SECRET_ACCESS_KEY", "test");
    env_guard.set("OMNIDRIVE_R2_FORCE_PATH_STYLE", "true");
    env_guard.set("OMNIDRIVE_SCALEWAY_ENDPOINT", format!("http://{mock_addr}"));
    env_guard.set("OMNIDRIVE_SCALEWAY_REGION", "pl-waw");
    env_guard.set("OMNIDRIVE_SCALEWAY_BUCKET", BUCKET_SCALEWAY);
    env_guard.set("OMNIDRIVE_SCALEWAY_ACCESS_KEY_ID", "test");
    env_guard.set("OMNIDRIVE_SCALEWAY_SECRET_ACCESS_KEY", "test");
    env_guard.set("OMNIDRIVE_SCALEWAY_FORCE_PATH_STYLE", "true");
    env_guard.set("OMNIDRIVE_B2_ENDPOINT", format!("http://{mock_addr}"));
    env_guard.set("OMNIDRIVE_B2_REGION", "eu-central-003");
    env_guard.set("OMNIDRIVE_B2_BUCKET", BUCKET_B2);
    env_guard.set("OMNIDRIVE_B2_ACCESS_KEY_ID", "test");
    env_guard.set("OMNIDRIVE_B2_SECRET_ACCESS_KEY", "test");
    env_guard.set("OMNIDRIVE_B2_FORCE_PATH_STYLE", "true");

    let vault_keys = VaultKeyStore::new();
    let _ = vault_keys.unlock(&pool, PASSPHRASE).await?;
    let downloader = Downloader::from_env(pool.clone(), vault_keys).await?;
    assert_eq!(
        downloader
            .read_range(inode_id, revision_id, 0, payload.len() as u64)
            .await?,
        payload.as_slice()
    );

    let sabotaged = choose_sabotage_target(&pool, &active_pack.pack_id).await?;
    let object_path = object_path_for(&env.object_root, &sabotaged.provider, &sabotaged.object_key);
    assert!(
        fs::try_exists(&object_path).await?,
        "expected shard object to exist before sabotage: {}",
        object_path.display()
    );
    eprintln!(
        "chaos-monkey deleting shard pack={} shard={} provider={} path={}",
        active_pack.pack_id,
        sabotaged.shard_index,
        sabotaged.provider,
        object_path.display()
    );

    let heartbeat = spawn_heartbeat(
        downloader.clone(),
        inode_id,
        revision_id,
        payload.clone(),
    );
    fs::remove_file(&object_path).await?;

    wait_for_missing_shard_and_degraded_pack(
        &mut daemon,
        &pool,
        &active_pack.pack_id,
        &sabotaged,
    )
    .await?;
    wait_for_repair_and_rescrub(
        &mut daemon,
        &pool,
        &active_pack.pack_id,
        &sabotaged,
        &object_path,
    )
    .await?;

    heartbeat.0.store(true, Ordering::SeqCst);
    let outcome = heartbeat.1.await?;
    assert!(
        outcome.failures.is_empty(),
        "heartbeat read failures during chaos test for {}: {}",
        logical_path,
        outcome.failures.join(" | ")
    );
    assert!(
        outcome.reads >= 1,
        "heartbeat performed too few reads during chaos test: {}",
        outcome.reads
    );

    assert_eq!(
        downloader
            .read_range(inode_id, revision_id, 0, payload.len() as u64)
            .await?,
        payload.as_slice(),
        "final read after scrubber/repair returned unexpected bytes"
    );

    let final_pack = current_active_pack(&pool, inode_id).await?;
    assert_eq!(final_pack.pack_id, active_pack.pack_id);
    assert_eq!(final_pack.mode, db::StorageMode::Ec2_1);
    assert_eq!(final_pack.status, "COMPLETED_HEALTHY");

    let final_health = daemon.health().await?;
    assert_eq!(final_health.worker_statuses.uploader, "idle");
    assert_eq!(final_health.worker_statuses.repair, "idle");
    assert_eq!(final_health.worker_statuses.scrubber, "idle");
    assert_eq!(final_health.worker_statuses.watcher, "idle");

    daemon.shutdown().await;
    mock_server.abort();
    let _ = fs::remove_dir_all(&env.temp_root).await;
    Ok(())
}

fn choose_bucket(provider: &str) -> &'static str {
    match provider {
        "cloudflare-r2" => BUCKET_R2,
        "scaleway" => BUCKET_SCALEWAY,
        "backblaze-b2" => BUCKET_B2,
        _ => BUCKET_R2,
    }
}

fn object_path_for(root: &FsPath, provider: &str, object_key: &str) -> PathBuf {
    let mut path = root.join(choose_bucket(provider));
    for segment in object_key.split('/') {
        if !segment.is_empty() {
            path = path.join(segment);
        }
    }
    path
}

async fn choose_sabotage_target(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<ShardSnapshot, Box<dyn std::error::Error>> {
    let shards = db::get_pack_shards(pool, pack_id).await?;
    let shard = shards
        .into_iter()
        .find(|shard| shard.shard_role == "DATA")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "pack shard missing"))?;
    Ok(ShardSnapshot {
        provider: shard.provider,
        shard_index: shard.shard_index,
        object_key: shard.object_key,
        status: shard.status,
        verification_status: shard.last_verification_status,
    })
}

async fn wait_for_missing_shard_and_degraded_pack(
    daemon: &mut DaemonHandle,
    pool: &SqlitePool,
    pack_id: &str,
    sabotaged: &ShardSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        daemon.ensure_running()?;
        let _ = daemon.scrub_now().await?;

        let pack = db::get_pack(pool, pack_id)
            .await?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "pack missing after sabotage"))?;
        let shard = shard_snapshot(pool, pack_id, sabotaged.shard_index).await?;
        if pack.status == "COMPLETED_DEGRADED"
            && shard.status == "FAILED"
            && shard.verification_status.as_deref() == Some("MISSING")
        {
            return Ok(());
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "scrubber did not mark pack {} degraded in time; pack_status={} shard_status={} verification_status={:?}",
                pack_id,
                pack.status,
                shard.status,
                shard.verification_status
            )
            .into());
        }
        sleep(Duration::from_millis(250)).await;
    }
}

async fn wait_for_repair_and_rescrub(
    daemon: &mut DaemonHandle,
    pool: &SqlitePool,
    pack_id: &str,
    sabotaged: &ShardSnapshot,
    object_path: &FsPath,
) -> Result<(), Box<dyn std::error::Error>> {
    let repair_deadline = Instant::now() + Duration::from_secs(30);
    let mut saw_repair_active = false;
    loop {
        daemon.ensure_running()?;
        let health = daemon.health().await?;
        if health.worker_statuses.repair == "active" {
            saw_repair_active = true;
        }

        let pack = db::get_pack(pool, pack_id)
            .await?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "pack missing during repair"))?;
        if pack.status == "COMPLETED_HEALTHY" && fs::try_exists(object_path).await? {
            break;
        }

        if Instant::now() >= repair_deadline {
            return Err(format!(
                "repair did not restore pack {} in time; pack_status={} shard_restored={} saw_repair_active={}",
                pack_id,
                pack.status,
                fs::try_exists(object_path).await?,
                saw_repair_active
            )
            .into());
        }
        sleep(Duration::from_millis(100)).await;
    }

    let scrub_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        daemon.ensure_running()?;
        let _ = daemon.scrub_now().await?;
        let shard = shard_snapshot(pool, pack_id, sabotaged.shard_index).await?;
        if shard.status == "COMPLETED"
            && shard.verification_status.as_deref() == Some("HEALTHY")
        {
            return Ok(());
        }

        if Instant::now() >= scrub_deadline {
            return Err(format!(
                "post-repair scrub did not return shard {}:{} to HEALTHY; shard_status={} verification_status={:?}",
                sabotaged.provider,
                sabotaged.shard_index,
                shard.status,
                shard.verification_status
            )
            .into());
        }
        sleep(Duration::from_millis(250)).await;
    }
}

async fn shard_snapshot(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
) -> Result<ShardSnapshot, Box<dyn std::error::Error>> {
    let shard = db::get_pack_shards(pool, pack_id)
        .await?
        .into_iter()
        .find(|shard| shard.shard_index == shard_index)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "shard missing"))?;
    Ok(ShardSnapshot {
        provider: shard.provider,
        shard_index: shard.shard_index,
        object_key: shard.object_key,
        status: shard.status,
        verification_status: shard.last_verification_status,
    })
}

fn spawn_heartbeat(
    downloader: Downloader,
    inode_id: i64,
    revision_id: i64,
    payload: Arc<Vec<u8>>,
) -> (Arc<AtomicBool>, JoinHandle<HeartbeatOutcome>) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    let task = tokio::spawn(async move {
        let mut reads = 0usize;
        let mut failures = Vec::new();
        while !stop_flag.load(Ordering::SeqCst) {
            match downloader
                .read_range(inode_id, revision_id, 0, payload.len() as u64)
                .await
            {
                Ok(bytes) => {
                    reads += 1;
                    if bytes != payload.as_slice() {
                        failures.push(format!(
                            "read returned {} bytes with mismatched payload",
                            bytes.len()
                        ));
                    }
                }
                Err(err) => failures.push(err.to_string()),
            }
            sleep(Duration::from_millis(100)).await;
        }
        HeartbeatOutcome { reads, failures }
    });
    (stop, task)
}

async fn wait_for_file_ingested(
    daemon: &mut DaemonHandle,
    pool: &SqlitePool,
    expected_path: &str,
) -> Result<(i64, i64, String), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        daemon.ensure_running()?;
        let files = daemon.list_files().await?;
        if let Some(file) = files
            .into_iter()
            .find(|file| file.path.replace('\\', "/").ends_with(expected_path))
        {
            if let Some(revision_id) = file.current_revision_id {
                let active = current_active_pack(pool, file.inode_id).await?;
                if active.mode == db::StorageMode::Ec2_1
                    && active.status == "COMPLETED_HEALTHY"
                {
                    return Ok((file.inode_id, revision_id, file.path));
                }
            }
        }

        if Instant::now() >= deadline {
            return Err(format!("watcher/uploader did not ingest {expected_path} in time").into());
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn current_active_pack(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<ActivePackSnapshot, Box<dyn std::error::Error>> {
    let locations = db::get_file_chunk_locations(pool, inode_id).await?;
    let location = locations
        .first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "file chunk location missing"))?;
    let pack = db::get_pack(pool, &location.pack_id)
        .await?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "active pack missing"))?;
    let shards = db::get_pack_shards(pool, &pack.pack_id).await?;
    Ok(ActivePackSnapshot {
        pack_id: pack.pack_id,
        mode: db::StorageMode::from_str(&pack.storage_mode),
        status: pack.status,
        shard_count: shards.len(),
    })
}

async fn spawn_fs_mock_s3(
    root: PathBuf,
) -> Result<(std::net::SocketAddr, JoinHandle<()>), Box<dyn std::error::Error>> {
    let state = FsMockS3State {
        root: Arc::new(root),
    };
    let app = Router::new()
        .route(
            "/{*path}",
            put(mock_put_object)
                .get(mock_get_object)
                .head(mock_head_object)
                .delete(mock_delete_object),
        )
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((addr, server))
}

async fn mock_put_object(
    State(state): State<FsMockS3State>,
    Path(path): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let (bucket, key) = split_bucket_and_key(&path);
    let object_path = object_path_under_root(state.root.as_ref(), bucket, key);
    if let Some(parent) = object_path.parent() {
        if let Err(err) = fs::create_dir_all(parent).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("create_dir_all failed: {err}"),
            )
                .into_response();
        }
    }
    eprintln!("mock-s3 PUT {bucket}/{key} bytes={}", body.len());
    if let Err(err) = fs::write(&object_path, body).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("write failed: {err}"),
        )
            .into_response();
    }
    StatusCode::OK.into_response()
}

async fn mock_head_object(
    State(state): State<FsMockS3State>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    let (bucket, key) = split_bucket_and_key(&path);
    let object_path = object_path_under_root(state.root.as_ref(), bucket, key);
    eprintln!("mock-s3 HEAD {bucket}/{key}");
    match fs::metadata(&object_path).await {
        Ok(metadata) => (
            StatusCode::OK,
            [("content-length", metadata.len().to_string())],
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn mock_get_object(
    State(state): State<FsMockS3State>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    let (bucket, key) = split_bucket_and_key(&path);
    let object_path = object_path_under_root(state.root.as_ref(), bucket, key);
    eprintln!("mock-s3 GET {bucket}/{key}");
    match fs::read(&object_path).await {
        Ok(bytes) => (StatusCode::OK, bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn mock_delete_object(
    State(state): State<FsMockS3State>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    let (bucket, key) = split_bucket_and_key(&path);
    let object_path = object_path_under_root(state.root.as_ref(), bucket, key);
    eprintln!("mock-s3 DELETE {bucket}/{key}");
    match fs::remove_file(&object_path).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::NO_CONTENT.into_response(),
    }
}

fn object_path_under_root(root: &FsPath, bucket: &str, key: &str) -> PathBuf {
    let mut path = root.join(bucket);
    for segment in key.split('/') {
        if !segment.is_empty() {
            path = path.join(segment);
        }
    }
    path
}

fn split_bucket_and_key(path: &str) -> (&str, &str) {
    let trimmed = path.trim_start_matches('/');
    let mut segments = trimmed.splitn(2, '/');
    let bucket = segments.next().unwrap_or_default();
    let key = segments.next().unwrap_or_default();
    (bucket, key)
}

fn build_payload(size: usize) -> Vec<u8> {
    (0..size).map(|index| ((index * 31) % 251) as u8).collect()
}

fn create_temp_root() -> io::Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let root = std::env::temp_dir().join(format!("angeld-e2e-scrubber-repair-{nanos}"));
    std::fs::create_dir_all(&root)?;
    Ok(root)
}

fn normalize_for_sqlite_url(path: &FsPath) -> String {
    path.to_string_lossy().replace('\\', "/")
}

async fn reserve_port() -> Result<u16, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

async fn http_get_json<T: for<'de> Deserialize<'de>>(
    url: &str,
    token: Option<&str>,
) -> Result<T, Box<dyn std::error::Error>> {
    let without_scheme = url
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "only http:// URLs are supported"))?;
    let (host_port, path) = without_scheme
        .split_once('/')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing request path"))?;
    let path = format!("/{}", path);

    let auth = match token {
        Some(t) => format!("Authorization: Bearer {t}\r\n"),
        None => String::new(),
    };

    let mut stream = TcpStream::connect(host_port).await?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host_port}\r\n{auth}Connection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    parse_http_json_response(&response)
}

async fn http_post_json(
    url: &str,
    body: &serde_json::Value,
    token: Option<&str>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let without_scheme = url
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "only http:// URLs are supported"))?;
    let (host_port, path) = without_scheme
        .split_once('/')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing request path"))?;
    let path = format!("/{}", path);
    let body_bytes = serde_json::to_vec(body)?;

    let auth = match token {
        Some(t) => format!("Authorization: Bearer {t}\r\n"),
        None => String::new(),
    };

    let mut stream = TcpStream::connect(host_port).await?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host_port}\r\n{auth}Connection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body_bytes.len()
    );
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(&body_bytes).await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    parse_http_json_response(&response)
}

fn parse_http_json_response<T: for<'de> Deserialize<'de>>(
    response: &[u8],
) -> Result<T, Box<dyn std::error::Error>> {
    let text = std::str::from_utf8(response)?;
    let (status_line, remainder) = text
        .split_once("\r\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP response"))?;
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing status code"))?
        .parse::<u16>()?;
    let (_, body) = remainder
        .split_once("\r\n\r\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing response body"))?;
    if !(200..300).contains(&status_code) {
        return Err(format!("HTTP {status_code}: {body}").into());
    }
    Ok(serde_json::from_str(body)?)
}
