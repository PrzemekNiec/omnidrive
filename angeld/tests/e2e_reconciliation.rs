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
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::sleep;

const PASSPHRASE: &str = "reconcile-e2e-passphrase";

#[derive(Clone)]
struct MockS3State {
    objects: Arc<Mutex<HashMap<(String, String), Vec<u8>>>>,
}

struct ReconciliationEnv {
    temp_root: PathBuf,
    localapp: PathBuf,
    base: PathBuf,
    db_url: String,
    watch_root: PathBuf,
}

struct DaemonHandle {
    child: Child,
    base_url: String,
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

impl ReconciliationEnv {
    async fn create() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_root = create_temp_root()?;
        let localapp = temp_root.join("localapp");
        let base = localapp.join("OmniDrive");
        let watch_root = temp_root.join("watch");
        let db_path = base.join("e2e-reconciliation.db");
        let db_url = format!("sqlite:///{}", normalize_for_sqlite_url(&db_path));

        fs::create_dir_all(base.join("logs")).await?;
        fs::create_dir_all(base.join("Cache")).await?;
        fs::create_dir_all(base.join("Spool")).await?;
        fs::create_dir_all(base.join("download-spool")).await?;
        fs::create_dir_all(&watch_root).await?;

        Ok(Self {
            temp_root,
            localapp,
            base,
            db_url,
            watch_root,
        })
    }

    async fn spawn_daemon(&self, mock_addr: std::net::SocketAddr) -> Result<DaemonHandle, Box<dyn std::error::Error>> {
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
            .env("OMNIDRIVE_GC_POLL_INTERVAL_MS", "60000")
            .env("OMNIDRIVE_METADATA_BACKUP_INTERVAL_MS", "60000")
            .env("OMNIDRIVE_R2_ENDPOINT", format!("http://{mock_addr}"))
            .env("OMNIDRIVE_R2_REGION", "auto")
            .env("OMNIDRIVE_R2_BUCKET", "bucket-r2")
            .env("OMNIDRIVE_R2_ACCESS_KEY_ID", "test")
            .env("OMNIDRIVE_R2_SECRET_ACCESS_KEY", "test")
            .env("OMNIDRIVE_R2_FORCE_PATH_STYLE", "true")
            .env("OMNIDRIVE_SCALEWAY_ENDPOINT", format!("http://{mock_addr}"))
            .env("OMNIDRIVE_SCALEWAY_REGION", "pl-waw")
            .env("OMNIDRIVE_SCALEWAY_BUCKET", "bucket-scaleway")
            .env("OMNIDRIVE_SCALEWAY_ACCESS_KEY_ID", "test")
            .env("OMNIDRIVE_SCALEWAY_SECRET_ACCESS_KEY", "test")
            .env("OMNIDRIVE_SCALEWAY_FORCE_PATH_STYLE", "true")
            .env("OMNIDRIVE_B2_ENDPOINT", format!("http://{mock_addr}"))
            .env("OMNIDRIVE_B2_REGION", "eu-central-003")
            .env("OMNIDRIVE_B2_BUCKET", "bucket-b2")
            .env("OMNIDRIVE_B2_ACCESS_KEY_ID", "test")
            .env("OMNIDRIVE_B2_SECRET_ACCESS_KEY", "test")
            .env("OMNIDRIVE_B2_FORCE_PATH_STYLE", "true")
            .env("RUST_LOG", "info")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let child = command.spawn()?;
        let handle = DaemonHandle { child, base_url };
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
            ))
            .await
            {
                Ok(_) => return Ok(()),
                Err(_) if Instant::now() < deadline => sleep(Duration::from_millis(100)).await,
                Err(err) => return Err(format!("daemon API did not become ready: {err}").into()),
            }
        }
    }

    async fn unlock(&self) -> Result<(), Box<dyn std::error::Error>> {
        let response = http_post_json(
            &format!("{}/api/unlock", self.base_url),
            &serde_json::json!({ "passphrase": PASSPHRASE }),
        )
        .await?;
        if response["status"] != serde_json::Value::String("UNLOCKED".to_string()) {
            return Err(format!("unlock response was unexpected: {response}").into());
        }
        Ok(())
    }

    async fn list_files(&mut self) -> Result<Vec<FileEntry>, Box<dyn std::error::Error>> {
        self.ensure_running()?;
        http_get_json::<Vec<FileEntry>>(&format!("{}/api/files", self.base_url)).await
    }

    fn ensure_running(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(status) = self.child.try_wait()? {
            return Err(format!("daemon exited unexpectedly with status {status}").into());
        }
        Ok(())
    }

    async fn health(&mut self) -> Result<DiagnosticsHealth, Box<dyn std::error::Error>> {
        self.ensure_running()?;
        http_get_json::<DiagnosticsHealth>(&format!("{}/api/diagnostics/health", self.base_url))
            .await
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
async fn policy_reconciliation_keeps_reads_alive_during_atomic_pack_swaps(
) -> Result<(), Box<dyn std::error::Error>> {
    let env = ReconciliationEnv::create().await?;
    let (mock_addr, mock_server) = spawn_mock_s3().await?;
    let mut daemon = env.spawn_daemon(mock_addr).await?;
    daemon.unlock().await?;

    let payload = Arc::new(build_payload(256 * 1024));
    let file_path = env.watch_root.join("atomic.txt");
    fs::write(&file_path, payload.as_slice()).await?;

    let pool = SqlitePool::connect(&env.db_url).await?;
    let (inode_id, revision_id, logical_path) =
        wait_for_file_ingested(&mut daemon, &pool, "atomic.txt").await?;
    let canonical_path = db::get_inode_path(&pool, inode_id)
        .await?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "inode path missing"))?;

    let initial = wait_for_active_pack_mode(&mut daemon, &pool, inode_id, db::StorageMode::Ec2_1)
        .await?;
    assert_eq!(initial.shard_count, 3);
    assert_eq!(initial.status, "COMPLETED_HEALTHY");

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
    env_guard.set("OMNIDRIVE_R2_BUCKET", "bucket-r2");
    env_guard.set("OMNIDRIVE_R2_ACCESS_KEY_ID", "test");
    env_guard.set("OMNIDRIVE_R2_SECRET_ACCESS_KEY", "test");
    env_guard.set("OMNIDRIVE_R2_FORCE_PATH_STYLE", "true");
    env_guard.set("OMNIDRIVE_SCALEWAY_ENDPOINT", format!("http://{mock_addr}"));
    env_guard.set("OMNIDRIVE_SCALEWAY_REGION", "pl-waw");
    env_guard.set("OMNIDRIVE_SCALEWAY_BUCKET", "bucket-scaleway");
    env_guard.set("OMNIDRIVE_SCALEWAY_ACCESS_KEY_ID", "test");
    env_guard.set("OMNIDRIVE_SCALEWAY_SECRET_ACCESS_KEY", "test");
    env_guard.set("OMNIDRIVE_SCALEWAY_FORCE_PATH_STYLE", "true");
    env_guard.set("OMNIDRIVE_B2_ENDPOINT", format!("http://{mock_addr}"));
    env_guard.set("OMNIDRIVE_B2_REGION", "eu-central-003");
    env_guard.set("OMNIDRIVE_B2_BUCKET", "bucket-b2");
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

    let transitions = [
        ("STANDARD", db::StorageMode::SingleReplica, 1usize),
        ("LOCAL", db::StorageMode::LocalOnly, 0usize),
        ("PARANOIA", db::StorageMode::Ec2_1, 3usize),
    ];

    for (policy_type, expected_mode, expected_shards) in transitions {
        let previous = current_active_pack(&pool, inode_id).await?;
        db::set_sync_policy_type_for_path(&pool, &canonical_path, policy_type).await?;
        let heartbeat = spawn_heartbeat(
            downloader.clone(),
            inode_id,
            revision_id,
            payload.clone(),
        );

        let next = wait_for_transition(
            &mut daemon,
            &pool,
            inode_id,
            expected_mode,
            &previous.pack_id,
        )
        .await?;
        sleep(Duration::from_millis(350)).await;
        heartbeat.0.store(true, Ordering::SeqCst);
        let outcome = heartbeat.1.await?;
        assert!(
            outcome.failures.is_empty(),
            "heartbeat read failures during {} transition for {}: {}",
            previous.mode.as_str(),
            logical_path,
            outcome.failures.join(" | ")
        );
        assert!(
            outcome.reads >= 2,
            "heartbeat performed too few reads during {} transition: {}",
            policy_type,
            outcome.reads
        );

        assert_eq!(next.mode, expected_mode);
        assert_eq!(next.shard_count, expected_shards);
        assert_ne!(next.pack_id, previous.pack_id);
        assert!(
            db::get_pack(&pool, &previous.pack_id).await?.is_some(),
            "old pack {} disappeared before GC confirmation",
            previous.pack_id
        );
        assert_eq!(
            active_pack_location_count(&pool, &previous.pack_id).await?,
            0,
            "old pack {} is still active after swap",
            previous.pack_id
        );
        let orphaned = db::get_orphaned_pack_ids(&pool, 128).await?;
        assert!(
            orphaned.iter().any(|pack_id| pack_id == &previous.pack_id),
            "old pack {} was not marked as orphaned/ready for GC after swap to {}",
            previous.pack_id,
            next.pack_id
        );
        assert_eq!(
            downloader
                .read_range(inode_id, revision_id, 0, payload.len() as u64)
                .await?,
            payload.as_slice(),
            "final read after transition {} returned unexpected bytes",
            policy_type
        );
    }

    let final_health = daemon.health().await?;
    assert_eq!(final_health.worker_statuses.uploader, "idle");
    assert_eq!(final_health.worker_statuses.repair, "idle");
    assert_eq!(final_health.worker_statuses.watcher, "idle");

    daemon.shutdown().await;
    mock_server.abort();
    let _ = fs::remove_dir_all(&env.temp_root).await;
    Ok(())
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
    let mut last_active_details = None;
    loop {
        daemon.ensure_running()?;
        let files = daemon.list_files().await?;
        let current_paths = files.iter().map(|file| file.path.clone()).collect::<Vec<_>>();
        if let Some(file) = files
            .into_iter()
            .find(|file| file.path.replace('\\', "/").ends_with(expected_path))
        {
            if let Some(revision_id) = file.current_revision_id {
                if let Ok(active) = current_active_pack(pool, file.inode_id).await {
                    last_active_details = Some(format!(
                        "pack={} mode={} status={} shards={}",
                        active.pack_id,
                        active.mode.as_str(),
                        active.status,
                        active.shard_count
                    ));
                    if active.mode == db::StorageMode::Ec2_1
                        && active.status == "COMPLETED_HEALTHY"
                    {
                        return Ok((file.inode_id, revision_id, file.path));
                    }
                }
            }
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "watcher/uploader did not ingest {} in time; api_files={:?}; active={:?}",
                expected_path,
                current_paths,
                last_active_details
            )
            .into());
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

async fn wait_for_active_pack_mode(
    daemon: &mut DaemonHandle,
    pool: &SqlitePool,
    inode_id: i64,
    expected_mode: db::StorageMode,
) -> Result<ActivePackSnapshot, Box<dyn std::error::Error>> {
    wait_for_transition(daemon, pool, inode_id, expected_mode, "").await
}

async fn wait_for_transition(
    daemon: &mut DaemonHandle,
    pool: &SqlitePool,
    inode_id: i64,
    expected_mode: db::StorageMode,
    previous_pack_id: &str,
) -> Result<ActivePackSnapshot, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        daemon.ensure_running()?;
        let snapshot = current_active_pack(pool, inode_id).await?;
        let changed = previous_pack_id.is_empty() || snapshot.pack_id != previous_pack_id;
        if changed && snapshot.mode == expected_mode && snapshot.status == "COMPLETED_HEALTHY" {
            return Ok(snapshot);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "reconciliation to {} did not complete in time (previous_pack={}, current_pack={}, current_mode={}, current_status={})",
                expected_mode.as_str(),
                previous_pack_id,
                snapshot.pack_id,
                snapshot.mode.as_str(),
                snapshot.status
            )
            .into());
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn active_pack_location_count(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<i64, Box<dyn std::error::Error>> {
    Ok(sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM pack_locations
        WHERE pack_id = ?
        "#,
    )
    .bind(pack_id)
    .fetch_one(pool)
    .await?)
}

async fn spawn_mock_s3(
) -> Result<(std::net::SocketAddr, JoinHandle<()>), Box<dyn std::error::Error>> {
    let state = MockS3State {
        objects: Arc::new(Mutex::new(HashMap::new())),
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
    State(state): State<MockS3State>,
    Path(path): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let (bucket, key) = split_bucket_and_key(&path);
    eprintln!("mock-s3 PUT {bucket}/{key} bytes={}", body.len());
    state
        .objects
        .lock()
        .await
        .insert((bucket.to_string(), key.to_string()), body.to_vec());
    StatusCode::OK
}

async fn mock_head_object(
    State(state): State<MockS3State>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    let (bucket, key) = split_bucket_and_key(&path);
    eprintln!("mock-s3 HEAD {bucket}/{key}");
    let objects = state.objects.lock().await;
    if let Some(bytes) = objects.get(&(bucket.to_string(), key.to_string())) {
        (
            StatusCode::OK,
            [("content-length", bytes.len().to_string())],
        )
            .into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn mock_get_object(
    State(state): State<MockS3State>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    let (bucket, key) = split_bucket_and_key(&path);
    eprintln!("mock-s3 GET {bucket}/{key}");
    let objects = state.objects.lock().await;
    if let Some(bytes) = objects.get(&(bucket.to_string(), key.to_string())) {
        (StatusCode::OK, bytes.clone()).into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn mock_delete_object(
    State(state): State<MockS3State>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    let (bucket, key) = split_bucket_and_key(&path);
    eprintln!("mock-s3 DELETE {bucket}/{key}");
    state
        .objects
        .lock()
        .await
        .remove(&(bucket.to_string(), key.to_string()));
    StatusCode::NO_CONTENT
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
    let root = std::env::temp_dir().join(format!("angeld-e2e-reconciliation-{nanos}"));
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
) -> Result<T, Box<dyn std::error::Error>> {
    let without_scheme = url
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "only http:// URLs are supported"))?;
    let (host_port, path) = without_scheme
        .split_once('/')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing request path"))?;
    let path = format!("/{}", path);

    let mut stream = TcpStream::connect(host_port).await?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    parse_http_json_response(&response)
}

async fn http_post_json(
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let without_scheme = url
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "only http:// URLs are supported"))?;
    let (host_port, path) = without_scheme
        .split_once('/')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing request path"))?;
    let path = format!("/{}", path);
    let body_bytes = serde_json::to_vec(body)?;

    let mut stream = TcpStream::connect(host_port).await?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
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
