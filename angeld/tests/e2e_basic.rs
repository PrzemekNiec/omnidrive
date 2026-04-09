use serde::Deserialize;
use sqlx::SqlitePool;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::time::sleep;

#[derive(Debug, Deserialize)]
struct DiagnosticsHealth {
    uptime_seconds: u64,
    pending_uploads_queue_size: i64,
    last_upload_error: Option<String>,
    worker_statuses: WorkerStatuses,
}

#[derive(Debug, Deserialize)]
struct WorkerStatuses {
    uploader: String,
    repair: String,
    scrubber: String,
    gc: String,
    watcher: String,
    metadata_backup: String,
    api: String,
}

struct DaemonHarness {
    temp_root: PathBuf,
    child: Child,
    db_url: String,
    base_url: String,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

impl DaemonHarness {
    async fn spawn() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_root = create_temp_root()?;
        let localapp = temp_root.join("localapp");
        let base = localapp.join("OmniDrive");
        std::fs::create_dir_all(base.join("logs"))?;
        std::fs::create_dir_all(base.join("Cache"))?;
        std::fs::create_dir_all(base.join("Spool"))?;
        std::fs::create_dir_all(base.join("download-spool"))?;

        let db_path = base.join("e2e-basic.db");
        let db_url = format!("sqlite:///{}", normalize_for_sqlite_url(&db_path));
        let api_port = reserve_port().await?;
        let base_url = format!("http://127.0.0.1:{api_port}");
        let stdout_path = temp_root.join("angeld.stdout.log");
        let stderr_path = temp_root.join("angeld.stderr.log");

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");

        let stdout = File::create(&stdout_path)?;
        let stderr = File::create(&stderr_path)?;

        let child = Command::new(env!("CARGO_BIN_EXE_angeld"))
            .current_dir(repo_root)
            .arg("--no-sync")
            .env("LOCALAPPDATA", &localapp)
            .env("OMNIDRIVE_DB_URL", &db_url)
            .env("OMNIDRIVE_SPOOL_DIR", base.join("Spool"))
            .env("OMNIDRIVE_DOWNLOAD_SPOOL_DIR", base.join("download-spool"))
            .env("OMNIDRIVE_CACHE_DIR", base.join("Cache"))
            .env("OMNIDRIVE_API_BIND", format!("127.0.0.1:{api_port}"))
            .env("OMNIDRIVE_E2E_TEST_MODE", "1")
            .env("OMNIDRIVE_ALLOW_EMPTY_UPLOADERS", "1")
            .env("OMNIDRIVE_UPLOAD_POLL_INTERVAL_MS", "100")
            .env("OMNIDRIVE_UPLOAD_TEST_PROCESS_DELAY_MS", "400")
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()?;

        let harness = Self {
            temp_root,
            child,
            db_url,
            base_url,
            stdout_path,
            stderr_path,
        };

        harness.wait_for_api_ready().await?;
        Ok(harness)
    }

    async fn wait_for_api_ready(&self) -> Result<(), Box<dyn std::error::Error>> {
        let deadline = Instant::now() + Duration::from_secs(15);

        loop {
            match http_get_json::<DiagnosticsHealth>(&format!(
                "{}/api/diagnostics/health",
                self.base_url
            ), None)
            .await
            {
                Ok(_) => return Ok(()),
                Err(_) if Instant::now() < deadline => sleep(Duration::from_millis(100)).await,
                Err(err) => {
                    return Err(format!("{}\nlast error: {err}", self.failure_message("API did not become ready")).into())
                }
            }
        }
    }

    async fn health(&self) -> Result<DiagnosticsHealth, Box<dyn std::error::Error>> {
        Ok(http_get_json::<DiagnosticsHealth>(&format!(
            "{}/api/diagnostics/health",
            self.base_url
        ), None)
        .await?)
    }

    async fn connect_db(&self) -> Result<SqlitePool, Box<dyn std::error::Error>> {
        Ok(SqlitePool::connect(&self.db_url).await?)
    }

    fn failure_message(&self, prefix: &str) -> String {
        format!(
            "{prefix}\nstdout:\n{}\nstderr:\n{}",
            std::fs::read_to_string(&self.stdout_path).unwrap_or_default(),
            std::fs::read_to_string(&self.stderr_path).unwrap_or_default()
        )
    }

    async fn shutdown(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
        let _ = std::fs::remove_dir_all(&self.temp_root);
    }
}

impl Drop for DaemonHarness {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = std::fs::remove_dir_all(&self.temp_root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn happy_path_upload_queue_clears_and_uploader_returns_idle(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut harness = DaemonHarness::spawn().await?;
    let initial = harness.health().await?;
    assert_eq!(initial.pending_uploads_queue_size, 0);
    assert_eq!(initial.worker_statuses.uploader, "idle");

    let pool = harness.connect_db().await?;
    let pack_id = "e2e-local-only-pack";
    sqlx::query(
        r#"
        INSERT INTO packs (
            pack_id,
            chunk_id,
            plaintext_hash,
            storage_mode,
            encryption_version,
            ec_scheme,
            logical_size,
            cipher_size,
            shard_size,
            nonce,
            gcm_tag,
            status
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(pack_id)
    .bind(vec![1u8, 2, 3, 4])
    .bind("e2e-local-only-hash")
    .bind("LOCAL_ONLY")
    .bind(1_i64)
    .bind("local_only")
    .bind(0_i64)
    .bind(0_i64)
    .bind(0_i64)
    .bind(Vec::<u8>::new())
    .bind(Vec::<u8>::new())
    .bind("UPLOADING")
    .execute(&pool)
    .await?;
    angeld::db::queue_pack_for_upload(&pool, pack_id).await?;

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut saw_active = false;
    let mut saw_idle_after_active = false;

    loop {
        if harness.child.try_wait()?.is_some() {
            return Err(harness
                .failure_message("daemon exited before upload queue test completed")
                .into());
        }

        let health = harness.health().await?;
        if health.worker_statuses.uploader == "active" {
            saw_active = true;
        }
        if saw_active && health.worker_statuses.uploader == "idle" {
            saw_idle_after_active = true;
        }

        let job = angeld::db::get_upload_job_by_pack_id(&pool, pack_id).await?;
        let completed = matches!(job.as_ref().map(|job| job.status.as_str()), Some("COMPLETED"));

        if health.pending_uploads_queue_size == 0 && completed && saw_idle_after_active {
            assert!(health.uptime_seconds <= 30);
            assert!(health.last_upload_error.is_none());
            assert_eq!(health.worker_statuses.api, "idle");
            assert_eq!(health.worker_statuses.repair, "idle");
            assert_eq!(health.worker_statuses.scrubber, "idle");
            assert_eq!(health.worker_statuses.gc, "idle");
            assert_eq!(health.worker_statuses.watcher, "idle");
            assert_eq!(health.worker_statuses.metadata_backup, "idle");
            break;
        }

        if Instant::now() >= deadline {
            return Err(harness
                .failure_message("upload queue did not clear in time")
                .into());
        }

        sleep(Duration::from_millis(100)).await;
    }

    harness.shutdown().await;
    Ok(())
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
    let response = String::from_utf8(response)?;
    let (_, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP response"))?;

    Ok(serde_json::from_str(body)?)
}

fn create_temp_root() -> io::Result<PathBuf> {
    let unique = format!(
        "angeld-e2e-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let path = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn normalize_for_sqlite_url(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
