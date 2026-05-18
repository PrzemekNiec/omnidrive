use serde::Deserialize;
use serde_json::Value;
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

const E2E_PASSPHRASE: &str = "e2e-basic-passphrase";

struct HttpResponse {
    status: u16,
    body: String,
}

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
    session_token: Option<String>,
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
            session_token: None,
        };

        harness.wait_for_api_ready().await?;
        Ok(harness)
    }

    async fn wait_for_api_ready(&self) -> Result<(), Box<dyn std::error::Error>> {
        let deadline = Instant::now() + Duration::from_secs(15);

        loop {
            match http_get_json::<DiagnosticsHealth>(
                &format!("{}/api/diagnostics/health", self.base_url),
                None,
            )
            .await
            {
                Ok(_) => return Ok(()),
                Err(_) if Instant::now() < deadline => sleep(Duration::from_millis(100)).await,
                Err(err) => {
                    return Err(format!(
                        "{}\nlast error: {err}",
                        self.failure_message("API did not become ready")
                    )
                    .into());
                }
            }
        }
    }

    async fn health(&self) -> Result<DiagnosticsHealth, Box<dyn std::error::Error>> {
        http_get_json::<DiagnosticsHealth>(
            &format!("{}/api/diagnostics/health", self.base_url),
            None,
        )
        .await
    }

    /// Unlock the vault and store the session token for subsequent requests.
    /// Lifted from `e2e_recovery.rs::DaemonHandle::unlock` (option a per α.A.b.1 plan).
    async fn unlock(&mut self) -> Result<Value, Box<dyn std::error::Error>> {
        let resp = http_post_json(
            &format!("{}/api/unlock", self.base_url),
            &serde_json::json!({ "passphrase": E2E_PASSPHRASE }),
            None,
        )
        .await?;
        self.session_token = resp["session_token"].as_str().map(|s| s.to_string());
        Ok(resp)
    }

    /// POST JSON body to `path` and return the raw HTTP status code + body string.
    async fn post_json(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        http_post_raw(
            &format!("{}{}", self.base_url, path),
            &body,
            self.session_token.as_deref(),
        )
        .await
    }

    async fn connect_db(&self) -> Result<SqlitePool, Box<dyn std::error::Error>> {
        SqlitePool::connect(&self.db_url).await.map_err(Into::into)
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
async fn happy_path_upload_queue_clears_and_uploader_returns_idle()
-> Result<(), Box<dyn std::error::Error>> {
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
        let completed = matches!(
            job.as_ref().map(|job| job.status.as_str()),
            Some("COMPLETED")
        );

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

fn parse_http_url(url: &str) -> std::io::Result<(String, String)> {
    let rest = url.strip_prefix("http://").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "expected http:// URL")
    })?;
    let (host_port, path_rest) = rest
        .split_once('/')
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "URL missing path"))?;
    Ok((host_port.to_string(), format!("/{path_rest}")))
}

async fn http_get_json<T: for<'de> Deserialize<'de>>(
    url: &str,
    token: Option<&str>,
) -> Result<T, Box<dyn std::error::Error>> {
    let (host_port, path) = parse_http_url(url)?;

    let auth = match token {
        Some(t) => format!("Authorization: Bearer {t}\r\n"),
        None => String::new(),
    };

    let mut stream = TcpStream::connect(host_port.as_str()).await?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host_port}\r\n{auth}Connection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let response = String::from_utf8(response)?;
    let (_, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP response"))?;

    Ok(serde_json::from_str(body)?)
}

/// POST JSON to `url` and parse response body as JSON (used for unlock).
async fn http_post_json(
    url: &str,
    body: &serde_json::Value,
    token: Option<&str>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let raw = http_post_raw(url, body, token).await?;
    Ok(serde_json::from_str(&raw.body)?)
}

/// POST JSON to `url` and return raw status + body (used for endpoint assertions).
async fn http_post_raw(
    url: &str,
    body: &serde_json::Value,
    token: Option<&str>,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    let (host_port, path) = parse_http_url(url)?;
    let body_text = body.to_string();
    let auth = match token {
        Some(t) => format!("Authorization: Bearer {t}\r\n"),
        None => String::new(),
    };
    let mut stream = TcpStream::connect(host_port.as_str()).await?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host_port}\r\n{auth}Connection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body_text.len(),
        body_text
    );
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let response_str = String::from_utf8(response)?;

    // Parse HTTP status line: "HTTP/1.1 204 No Content\r\n..."
    let status_line = response_str
        .lines()
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty HTTP response"))?;
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no status code"))?
        .parse()?;

    let body = response_str
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();

    Ok(HttpResponse { status, body })
}

// ── α.A.b.1 integration tests ──────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_lock_timeout_endpoint_accepts_preset() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let resp = h
        .post_json(
            "/api/auto-lock/timeout",
            serde_json::json!({"idle_timeout_min": 30}),
        )
        .await?;
    assert_eq!(
        resp.status, 204,
        "expected 204 but got {}; body: {}",
        resp.status, resp.body
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_lock_timeout_endpoint_rejects_invalid() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let resp = h
        .post_json(
            "/api/auto-lock/timeout",
            serde_json::json!({"idle_timeout_min": 7}),
        )
        .await?;
    assert_eq!(
        resp.status, 400,
        "expected 400 but got {}; body: {}",
        resp.status, resp.body
    );
    assert!(
        resp.body.contains("invalid_preset"),
        "body missing 'invalid_preset': {}",
        resp.body
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_lock_timeout_endpoint_rejects_unauthenticated()
-> Result<(), Box<dyn std::error::Error>> {
    let h = DaemonHarness::spawn().await?;
    // Do NOT call h.unlock() — session_token remains None.
    let resp = h
        .post_json(
            "/api/auto-lock/timeout",
            serde_json::json!({"idle_timeout_min": 30}),
        )
        .await?;
    assert_eq!(
        resp.status, 401,
        "expected 401 unauthenticated; got {}; body: {}",
        resp.status, resp.body
    );
    Ok(())
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
