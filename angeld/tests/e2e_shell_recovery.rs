use serde_json::Value;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::time::sleep;

struct ShellHarness {
    temp_root: PathBuf,
    child: Child,
    base_url: String,
    drive_letter: String,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

impl ShellHarness {
    async fn spawn() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_root = create_temp_root()?;
        let localapp = temp_root.join("localapp");
        let base = localapp.join("OmniDrive");
        std::fs::create_dir_all(base.join("logs"))?;
        std::fs::create_dir_all(base.join("Cache"))?;
        std::fs::create_dir_all(base.join("Spool"))?;
        std::fs::create_dir_all(base.join("download-spool"))?;

        let db_path = base.join("e2e-shell.db");
        let db_url = format!("sqlite:///{}", normalize_for_sqlite_url(&db_path));
        let watch_dir = temp_root.join("vault");
        let api_port = reserve_port().await?;
        let base_url = format!("http://127.0.0.1:{api_port}");
        let drive_letter = "W:".to_string();
        let stdout_path = temp_root.join("angeld.stdout.log");
        let stderr_path = temp_root.join("angeld.stderr.log");

        let stdout = File::create(&stdout_path)?;
        let stderr = File::create(&stderr_path)?;

        let child = Command::new(env!("CARGO_BIN_EXE_angeld"))
            .current_dir(&temp_root)
            .env("LOCALAPPDATA", &localapp)
            .env("OMNIDRIVE_RUNTIME_MODE", "installed")
            .env("OMNIDRIVE_DB_URL", &db_url)
            .env("OMNIDRIVE_WATCH_DIR", &watch_dir)
            .env("OMNIDRIVE_SPOOL_DIR", base.join("Spool"))
            .env("OMNIDRIVE_DOWNLOAD_SPOOL_DIR", base.join("download-spool"))
            .env("OMNIDRIVE_CACHE_DIR", base.join("Cache"))
            .env("OMNIDRIVE_LOG_DIR", base.join("logs"))
            .env("OMNIDRIVE_API_BIND", format!("127.0.0.1:{api_port}"))
            .env("OMNIDRIVE_DRIVE_LETTER", &drive_letter)
            .env_remove("OMNIDRIVE_R2_ENDPOINT")
            .env_remove("OMNIDRIVE_R2_REGION")
            .env_remove("OMNIDRIVE_R2_BUCKET")
            .env_remove("OMNIDRIVE_R2_ACCESS_KEY_ID")
            .env_remove("OMNIDRIVE_R2_SECRET_ACCESS_KEY")
            .env_remove("OMNIDRIVE_R2_FORCE_PATH_STYLE")
            .env_remove("OMNIDRIVE_SCALEWAY_ENDPOINT")
            .env_remove("OMNIDRIVE_SCALEWAY_REGION")
            .env_remove("OMNIDRIVE_SCALEWAY_BUCKET")
            .env_remove("OMNIDRIVE_SCALEWAY_ACCESS_KEY_ID")
            .env_remove("OMNIDRIVE_SCALEWAY_SECRET_ACCESS_KEY")
            .env_remove("OMNIDRIVE_SCALEWAY_FORCE_PATH_STYLE")
            .env_remove("OMNIDRIVE_B2_ENDPOINT")
            .env_remove("OMNIDRIVE_B2_REGION")
            .env_remove("OMNIDRIVE_B2_BUCKET")
            .env_remove("OMNIDRIVE_B2_ACCESS_KEY_ID")
            .env_remove("OMNIDRIVE_B2_SECRET_ACCESS_KEY")
            .env_remove("OMNIDRIVE_B2_FORCE_PATH_STYLE")
            .env("RUST_LOG", "info")
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()?;

        let harness = Self {
            temp_root,
            child,
            base_url,
            drive_letter,
            stdout_path,
            stderr_path,
        };
        harness.wait_for_api_ready().await?;
        Ok(harness)
    }

    async fn wait_for_api_ready(&self) -> Result<(), Box<dyn std::error::Error>> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match http_get_json(&format!("{}/api/diagnostics/health", self.base_url)).await {
                Ok(_) => return Ok(()),
                Err(_) if Instant::now() < deadline => sleep(Duration::from_millis(100)).await,
                Err(err) => {
                    return Err(format!(
                        "shell daemon API did not become ready.\nstdout:\n{}\nstderr:\n{}\nlast error: {err}",
                        std::fs::read_to_string(&self.stdout_path).unwrap_or_default(),
                        std::fs::read_to_string(&self.stderr_path).unwrap_or_default()
                    )
                    .into())
                }
            }
        }
    }

    async fn get_json(&self, path: &str) -> Result<Value, Box<dyn std::error::Error>> {
        http_get_json(&format!("{}{}", self.base_url, path)).await
    }

    async fn post_json(&self, path: &str) -> Result<Value, Box<dyn std::error::Error>> {
        http_post_json(&format!("{}{}", self.base_url, path)).await
    }

    async fn shutdown(&mut self) {
        let _ = Command::new("subst").arg(&self.drive_letter).arg("/D").output().await;
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
        let _ = std::fs::remove_dir_all(&self.temp_root);
    }
}

impl Drop for ShellHarness {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = std::fs::remove_dir_all(&self.temp_root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an unrestricted desktop session for subst-backed virtual drive mapping"]
async fn shell_repair_restores_drive_and_context_menu_after_local_drift(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut harness = ShellHarness::spawn().await?;

    let initial = harness.get_json("/api/diagnostics/shell").await?;
    assert_eq!(initial["drive_present"], true);
    assert_eq!(initial["drive_browsable"], true);
    assert_eq!(initial["drive_target_matches"], true);
    assert_eq!(initial["context_menu_registered"], true);

    Command::new("subst")
        .arg(&harness.drive_letter)
        .arg("/D")
        .output()
        .await?;
    Command::new("reg")
        .args([
            "delete",
            r"HKCU\Software\Classes\Directory\shell\OmniDrive",
            "/f",
        ])
        .output()
        .await?;

    let drifted = harness.get_json("/api/diagnostics/shell").await?;
    assert_eq!(drifted["drive_present"], false);
    assert_eq!(drifted["context_menu_registered"], false);

    let repaired = harness.post_json("/api/maintenance/repair-shell").await?;
    assert_eq!(repaired["status"], "ok");
    let actions = repaired["actions"]
        .as_array()
        .ok_or_else(|| io::Error::other("repair-shell actions missing"))?;
    assert!(
        !actions.is_empty(),
        "expected shell repair actions, got none: {}",
        repaired
    );

    let final_state = harness.get_json("/api/diagnostics/shell").await?;
    assert_eq!(final_state["drive_present"], true);
    assert_eq!(final_state["drive_browsable"], true);
    assert_eq!(final_state["drive_target_matches"], true);
    assert_eq!(final_state["context_menu_registered"], true);
    assert_eq!(final_state["duplicate_drive_mappings"], serde_json::json!([]));

    harness.shutdown().await;
    Ok(())
}

async fn reserve_port() -> Result<u16, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

async fn http_get_json(url: &str) -> Result<Value, Box<dyn std::error::Error>> {
    http_request_json("GET", url).await
}

async fn http_post_json(url: &str) -> Result<Value, Box<dyn std::error::Error>> {
    http_request_json("POST", url).await
}

async fn http_request_json(method: &str, url: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let without_scheme = url
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "only http:// URLs are supported"))?;
    let (host_port, path) = without_scheme
        .split_once('/')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing request path"))?;
    let path = format!("/{}", path);

    let mut stream = TcpStream::connect(host_port).await?;
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
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
        "angeld-e2e-shell-{}-{}",
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
