use angeld::db;
use angeld::onboarding::seal_provider_secrets;
use angeld::vault::bootstrap_local_vault;
use serde::Deserialize;
use serde_json::Value;
use sqlx::SqlitePool;
use std::fs::File;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::time::sleep;

#[allow(dead_code)]
pub const E2E_PASSPHRASE: &str = "e2e-basic-passphrase";

#[allow(dead_code)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct DiagnosticsHealth {
    pub uptime_seconds: u64,
    pub pending_uploads_queue_size: i64,
    pub last_upload_error: Option<String>,
    pub worker_statuses: WorkerStatuses,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct WorkerStatuses {
    pub uploader: String,
    pub repair: String,
    pub scrubber: String,
    pub gc: String,
    pub watcher: String,
    pub metadata_backup: String,
    pub api: String,
}

#[allow(dead_code)]
pub struct DaemonHarness {
    pub temp_root: PathBuf,
    pub child: Child,
    pub db_url: String,
    pub base_url: String,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub session_token: Option<String>,
}

#[allow(dead_code)]
impl DaemonHarness {
    pub async fn spawn() -> Result<Self, Box<dyn std::error::Error>> {
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

    pub async fn wait_for_api_ready(&self) -> Result<(), Box<dyn std::error::Error>> {
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

    pub async fn health(&self) -> Result<DiagnosticsHealth, Box<dyn std::error::Error>> {
        http_get_json::<DiagnosticsHealth>(
            &format!("{}/api/diagnostics/health", self.base_url),
            None,
        )
        .await
    }

    pub async fn unlock(&mut self) -> Result<Value, Box<dyn std::error::Error>> {
        let resp = http_post_json(
            &format!("{}/api/unlock", self.base_url),
            &serde_json::json!({ "passphrase": E2E_PASSPHRASE }),
            None,
        )
        .await?;
        self.session_token = resp["session_token"].as_str().map(|s| s.to_string());
        Ok(resp)
    }

    pub async fn post_json(
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

    pub async fn connect_db(&self) -> Result<SqlitePool, Box<dyn std::error::Error>> {
        SqlitePool::connect(&self.db_url).await.map_err(Into::into)
    }

    pub fn failure_message(&self, prefix: &str) -> String {
        format!(
            "{prefix}\nstdout:\n{}\nstderr:\n{}",
            std::fs::read_to_string(&self.stdout_path).unwrap_or_default(),
            std::fs::read_to_string(&self.stderr_path).unwrap_or_default()
        )
    }

    pub async fn shutdown(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
        let _ = std::fs::remove_dir_all(&self.temp_root);
    }

    #[allow(dead_code)]
    pub async fn get_json(
        &self,
        path: &str,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        http_get_json::<serde_json::Value>(
            &format!("{}{}", self.base_url, path),
            self.session_token.as_deref(),
        )
        .await
    }

    #[allow(dead_code)]
    pub async fn post(&self, path: &str) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        http_post_raw(
            &format!("{}{}", self.base_url, path),
            &serde_json::Value::Null,
            self.session_token.as_deref(),
        )
        .await
    }

    #[allow(dead_code)]
    pub async fn get_raw(&self, path: &str) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        http_get_raw(&format!("{}{}", self.base_url, path), None).await
    }
}

impl Drop for DaemonHarness {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = std::fs::remove_dir_all(&self.temp_root);
    }
}

#[allow(dead_code)]
pub async fn reserve_port() -> Result<u16, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

#[allow(dead_code)]
pub fn parse_http_url(url: &str) -> std::io::Result<(String, String)> {
    let rest = url.strip_prefix("http://").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "expected http:// URL")
    })?;
    let (host_port, path_rest) = rest
        .split_once('/')
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "URL missing path"))?;
    Ok((host_port.to_string(), format!("/{path_rest}")))
}

#[allow(dead_code)]
pub async fn http_get_json<T: for<'de> Deserialize<'de>>(
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

#[allow(dead_code)]
pub async fn http_post_json(
    url: &str,
    body: &serde_json::Value,
    token: Option<&str>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let raw = http_post_raw(url, body, token).await?;
    Ok(serde_json::from_str(&raw.body)?)
}

#[allow(dead_code)]
pub async fn http_post_raw(
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

#[allow(dead_code)]
pub async fn http_get_raw(
    url: &str,
    token: Option<&str>,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
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
    let response_str = String::from_utf8(response)?;
    let status_line = response_str.lines().next().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "empty HTTP response")
    })?;
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no status code"))?
        .parse()?;
    let body = response_str
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    Ok(HttpResponse { status, body })
}

#[allow(dead_code)]
pub fn create_temp_root() -> io::Result<PathBuf> {
    let unique = format!(
        "angeld-e2e-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let path = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

#[allow(dead_code)]
pub fn normalize_for_sqlite_url(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Seeds mock S3 provider configs into the test DB so the daemon starts in full
/// cloud mode (with repair/scrubber/gc workers). Must be called after `db::init_db`
/// and before spawning the daemon.
///
/// Also bootstraps vault params and sets the sync policy for `watch_root` to
/// PARANOIA (EC_2_1) so ingested files get uploaded with erasure coding.
#[allow(dead_code)]
pub async fn seed_mock_providers(
    pool: &SqlitePool,
    mock_addr: SocketAddr,
    watch_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    bootstrap_local_vault(pool).await?;

    let policy_path = watch_root.to_string_lossy().replace('\\', "/");
    db::set_sync_policy_type_for_path(pool, &policy_path, "PARANOIA").await?;

    let endpoint = format!("http://{mock_addr}");

    let providers = [
        ("cloudflare-r2", "auto", "bucket-r2"),
        ("scaleway", "pl-waw", "bucket-scaleway"),
        ("backblaze-b2", "eu-central-003", "bucket-b2"),
    ];

    for (name, region, bucket) in &providers {
        db::upsert_provider_config(
            pool,
            name,
            &endpoint,
            region,
            bucket,
            true,
            true,
            None,
            Some("VALID"),
            None,
            None,
        )
        .await?;

        let (sealed_key_id, sealed_secret) = seal_provider_secrets("test", "test")?;
        db::upsert_provider_secret(pool, name, &sealed_key_id, &sealed_secret).await?;
    }

    db::set_system_config_value(pool, "onboarding_state", "COMPLETED").await?;
    db::set_system_config_value(pool, "onboarding_mode", "CLOUD_ENABLED").await?;
    db::set_system_config_value(pool, "cloud_enabled", "1").await?;
    db::set_system_config_value(pool, "last_onboarding_step", "completed").await?;

    Ok(())
}
