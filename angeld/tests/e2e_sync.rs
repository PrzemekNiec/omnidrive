use serde::Deserialize;
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
    worker_statuses: WorkerStatuses,
}

#[derive(Debug, Deserialize)]
struct WorkerStatuses {
    api: String,
}

struct SyncHarness {
    temp_root: PathBuf,
    child: Child,
    base_url: String,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

impl SyncHarness {
    async fn spawn() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_root = create_temp_root()?;
        let localapp = temp_root.join("localapp");
        let base = localapp.join("OmniDrive");
        std::fs::create_dir_all(base.join("logs"))?;
        std::fs::create_dir_all(base.join("Cache"))?;
        std::fs::create_dir_all(base.join("Spool"))?;
        std::fs::create_dir_all(base.join("download-spool"))?;

        let db_path = base.join("e2e-sync.db");
        let db_url = format!("sqlite:///{}", normalize_for_sqlite_url(&db_path));
        let real_localapp = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
        let sync_root = PathBuf::from(real_localapp).join("OmniDrive").join("OmniSync");
        std::fs::create_dir_all(&sync_root)?;

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
            .env("LOCALAPPDATA", &localapp)
            .env("OMNIDRIVE_DB_URL", &db_url)
            .env("OMNIDRIVE_SYNC_ROOT", &sync_root)
            .env("OMNIDRIVE_SPOOL_DIR", base.join("Spool"))
            .env("OMNIDRIVE_DOWNLOAD_SPOOL_DIR", base.join("download-spool"))
            .env("OMNIDRIVE_CACHE_DIR", base.join("Cache"))
            .env("OMNIDRIVE_API_BIND", format!("127.0.0.1:{api_port}"))
            .env("OMNIDRIVE_DRIVE_LETTER", "Y:")
            .env("OMNIDRIVE_E2E_TEST_MODE", "1")
            .env("RUST_LOG", "trace")
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()?;

        let harness = Self {
            temp_root,
            child,
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
            ))
            .await
            {
                Ok(_) => return Ok(()),
                Err(_) if Instant::now() < deadline => sleep(Duration::from_millis(100)).await,
                Err(err) => {
                    return Err(format!(
                        "sync daemon API did not become ready.\nstdout:\n{}\nstderr:\n{}\nlast error: {err}",
                        std::fs::read_to_string(&self.stdout_path).unwrap_or_default(),
                        std::fs::read_to_string(&self.stderr_path).unwrap_or_default()
                    )
                    .into())
                }
            }
        }
    }

    async fn health(&self) -> Result<DiagnosticsHealth, Box<dyn std::error::Error>> {
        Ok(http_get_json::<DiagnosticsHealth>(&format!(
            "{}/api/diagnostics/health",
            self.base_url
        ))
        .await?)
    }

    async fn shutdown(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
        let _ = std::fs::remove_dir_all(&self.temp_root);
    }
}

impl Drop for SyncHarness {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = std::fs::remove_dir_all(&self.temp_root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_stack_sync_root_registers_and_api_reaches_listening_state(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut harness = SyncHarness::spawn().await?;
    let health = harness.health().await?;
    assert_eq!(health.worker_statuses.api, "idle");
    assert!(health.uptime_seconds <= 30);

    let stdout = std::fs::read_to_string(&harness.stdout_path).unwrap_or_default();
    let stderr = std::fs::read_to_string(&harness.stderr_path).unwrap_or_default();
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        combined.contains("smart sync bootstrap ready")
            || combined.contains("smart sync bootstrap warning"),
        "missing sync-root bootstrap log\nstdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );

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
    let response = String::from_utf8(response)?;
    let (_, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP response"))?;

    Ok(serde_json::from_str(body)?)
}

fn create_temp_root() -> io::Result<PathBuf> {
    let unique = format!(
        "angeld-e2e-sync-{}-{}",
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
