use angeld::db;
use angeld::vault::VaultKeyStore;
use serde_json::Value;
use std::io;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::time::sleep;

const PASSPHRASE: &str = "dr-e2e-passphrase";

struct RecoveryEnv {
    temp_root: PathBuf,
    localapp: PathBuf,
    base: PathBuf,
    db_path: PathBuf,
    db_url: String,
    backup_dir: PathBuf,
    sync_root: PathBuf,
    test_prefix: String,
}

struct DaemonHandle {
    child: Child,
    base_url: String,
    sync_root: PathBuf,
}

impl RecoveryEnv {
    async fn create() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_root = create_temp_root()?;
        let localapp = temp_root.join("localapp");
        let base = localapp.join("OmniDrive");
        let backup_dir = temp_root.join("metadata-backup-store");
        let db_path = base.join("e2e-recovery.db");
        let db_url = format!("sqlite:///{}", normalize_for_sqlite_url(&db_path));
        let test_prefix = format!(
            "dr-e2e-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
        );
        let real_localapp = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
        let sync_root = PathBuf::from(real_localapp).join("OmniDrive").join("SyncRoot");

        tokio::fs::create_dir_all(base.join("logs")).await?;
        tokio::fs::create_dir_all(base.join("Cache")).await?;
        tokio::fs::create_dir_all(base.join("Spool")).await?;
        tokio::fs::create_dir_all(base.join("download-spool")).await?;
        tokio::fs::create_dir_all(&backup_dir).await?;
        Ok(Self {
            temp_root,
            localapp,
            base,
            db_path,
            db_url,
            backup_dir,
            sync_root,
            test_prefix,
        })
    }

    async fn seed_database(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let pool = db::init_db(&self.db_url).await?;
        let keys = VaultKeyStore::new();
        let _ = keys.unlock(&pool, PASSPHRASE).await?;

        let root_dir = db::create_inode(&pool, None, &self.test_prefix, "DIR", 0).await?;
        let alpha = db::create_inode(&pool, Some(root_dir), "alpha.txt", "FILE", 128).await?;
        db::create_file_revision(&pool, alpha, 128, None).await?;

        let beta = db::create_inode(&pool, Some(root_dir), "beta.txt", "FILE", 256).await?;
        db::create_file_revision(&pool, beta, 256, None).await?;

        let nested_dir = db::create_inode(&pool, Some(root_dir), "nested", "DIR", 0).await?;
        let gamma = db::create_inode(&pool, Some(nested_dir), "gamma.txt", "FILE", 512).await?;
        db::create_file_revision(&pool, gamma, 512, None).await?;

        let delta = db::create_inode(&pool, Some(nested_dir), "delta.txt", "FILE", 1024).await?;
        db::create_file_revision(&pool, delta, 1024, None).await?;

        Ok(vec![
            format!("{}/alpha.txt", self.test_prefix),
            format!("{}/beta.txt", self.test_prefix),
            format!("{}/nested/gamma.txt", self.test_prefix),
            format!("{}/nested/delta.txt", self.test_prefix),
        ])
    }

    async fn spawn_daemon(
        &self,
        auto_restore: bool,
    ) -> Result<DaemonHandle, Box<dyn std::error::Error>> {
        let _ = angeld::smart_sync::unregister_sync_root(&self.sync_root);
        clear_sync_root_contents(&self.sync_root).await?;
        let api_port = reserve_port().await?;
        let base_url = format!("http://127.0.0.1:{api_port}");
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");

        let mut command = Command::new(env!("CARGO_BIN_EXE_angeld"));
        command
            .current_dir(repo_root)
            .env("LOCALAPPDATA", &self.localapp)
            .env("OMNIDRIVE_DB_URL", &self.db_url)
            .env("OMNIDRIVE_SYNC_ROOT", &self.sync_root)
            .env("OMNIDRIVE_SPOOL_DIR", self.base.join("Spool"))
            .env("OMNIDRIVE_DOWNLOAD_SPOOL_DIR", self.base.join("download-spool"))
            .env("OMNIDRIVE_CACHE_DIR", self.base.join("Cache"))
            .env("OMNIDRIVE_API_BIND", format!("127.0.0.1:{api_port}"))
            .env("OMNIDRIVE_DRIVE_LETTER", "Y:")
            .env("OMNIDRIVE_E2E_TEST_MODE", "1")
            .env("OMNIDRIVE_METADATA_BACKUP_DIR", &self.backup_dir)
            .env("RUST_LOG", "info")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        if auto_restore {
            command.env("OMNIDRIVE_AUTO_RESTORE_PASSPHRASE", PASSPHRASE);
        }

        let child = command.spawn()?;
        let handle = DaemonHandle {
            child,
            base_url,
            sync_root: self.sync_root.clone(),
        };
        handle.wait_for_api_ready().await?;
        Ok(handle)
    }
}

impl DaemonHandle {
    async fn wait_for_api_ready(&self) -> Result<(), Box<dyn std::error::Error>> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match http_get_json(&format!("{}/api/diagnostics/health", self.base_url)).await {
                Ok(_) => return Ok(()),
                Err(_) if Instant::now() < deadline => sleep(Duration::from_millis(100)).await,
                Err(err) => {
                    return Err(format!(
                        "daemon API did not become ready; daemon stdout/stderr are inherited by the test process.\nlast error: {err}"
                    )
                    .into())
                }
            }
        }
    }

    async fn post_json(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        http_post_json(&format!("{}{}", self.base_url, path), body).await
    }

    async fn get_json(&self, path: &str) -> Result<Value, Box<dyn std::error::Error>> {
        http_get_json(&format!("{}{}", self.base_url, path)).await
    }

    async fn shutdown(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
        let _ = angeld::smart_sync::unregister_sync_root(&self.sync_root);
    }
}

impl Drop for DaemonHandle {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = angeld::smart_sync::unregister_sync_root(&self.sync_root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disaster_recovery_rebuilds_local_db_inventory_after_total_db_loss(
) -> Result<(), Box<dyn std::error::Error>> {
    let env = RecoveryEnv::create().await?;
    let mut expected_paths = env.seed_database().await?;
    expected_paths.sort();

    let mut first = env.spawn_daemon(false).await?;
    first
        .post_json(
            "/api/unlock",
            &serde_json::json!({ "passphrase": PASSPHRASE }),
        )
        .await?;

    let original_files = filtered_file_inventory(
        first.get_json("/api/files").await?,
        &env.test_prefix,
    )?;
    assert_eq!(original_files, expected_paths);

    let backup_response = first
        .post_json("/api/recovery/backup-now", &serde_json::json!({}))
        .await?;
    assert_eq!(backup_response["uploaded"], Value::Bool(true));

    wait_for_backup_completion(&first).await?;
    let backup_artifact = env
        .backup_dir
        .join("_omnidrive")
        .join("system")
        .join("metadata")
        .join("latest.db.enc");
    assert!(tokio::fs::try_exists(&backup_artifact).await?);

    first.shutdown().await;

    if tokio::fs::try_exists(&env.db_path).await? {
        tokio::fs::remove_file(&env.db_path).await?;
    }
    if tokio::fs::try_exists(&env.base.join("Cache")).await? {
        tokio::fs::remove_dir_all(env.base.join("Cache")).await?;
    }
    tokio::fs::create_dir_all(env.base.join("Cache")).await?;
    clear_test_sync_root_subtree(&env.sync_root, &env.test_prefix).await?;

    let mut second = env.spawn_daemon(true).await?;
    let restored_files = filtered_file_inventory(
        second.get_json("/api/files").await?,
        &env.test_prefix,
    )?;
    assert_eq!(restored_files, expected_paths);
    let restored_placeholders =
        wait_for_placeholder_tree(&env.sync_root, &env.test_prefix, expected_paths.len()).await?;
    assert_eq!(restored_placeholders, expected_paths);
    assert_placeholder_attributes(&env.sync_root, &restored_placeholders)?;

    second.shutdown().await;
    let _ = tokio::fs::remove_dir_all(&env.temp_root).await;
    Ok(())
}

async fn clear_test_sync_root_subtree(
    sync_root: &Path,
    prefix: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let subtree = sync_root.join(prefix);
    if tokio::fs::try_exists(&subtree).await? {
        tokio::fs::remove_dir_all(&subtree).await?;
    }
    Ok(())
}

async fn clear_sync_root_contents(sync_root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if tokio::fs::try_exists(sync_root).await? {
        tokio::fs::remove_dir_all(sync_root).await?;
    }
    Ok(())
}

async fn wait_for_backup_completion(
    handle: &DaemonHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let status = handle.get_json("/api/recovery/status").await?;
        let recent = status["recent_attempts"].as_array().cloned().unwrap_or_default();
        let has_completed = recent
            .iter()
            .any(|entry| entry["status"].as_str() == Some("COMPLETED"));
        if status["last_successful_backup"].as_i64().is_some() && has_completed {
            return Ok(());
        }

        if Instant::now() >= deadline {
            return Err(format!("metadata backup did not complete in time: {}", status).into());
        }
        sleep(Duration::from_millis(100)).await;
    }
}

fn filtered_file_inventory(
    payload: Value,
    prefix: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut files = payload
        .as_array()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "expected file array"))?
        .iter()
        .filter_map(|entry| entry["path"].as_str())
        .filter(|path| path.starts_with(prefix) && path.ends_with(".txt"))
        .map(|path| path.to_string())
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

async fn wait_for_placeholder_tree(
    sync_root: &Path,
    prefix: &str,
    expected_count: usize,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let inventory = collect_sync_root_files(sync_root, prefix)?;
        if inventory.len() == expected_count {
            return Ok(inventory);
        }

        if Instant::now() >= deadline {
            let tree_dump = dump_sync_root_tree(sync_root)?;
            return Err(format!(
                "placeholder projection did not complete in time under {}\ncurrent sync root tree:\n{}",
                sync_root.display(),
                tree_dump
            )
            .into());
        }
        sleep(Duration::from_millis(100)).await;
    }
}

fn collect_sync_root_files(
    sync_root: &Path,
    prefix: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let root = sync_root.join(prefix);
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }

    collect_sync_root_files_recursive(sync_root, &root, &mut files)?;
    files.sort();
    Ok(files)
}

fn dump_sync_root_tree(sync_root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut lines = Vec::new();
    if !sync_root.exists() {
        return Ok("<sync root does not exist>".to_string());
    }

    dump_sync_root_tree_recursive(sync_root, sync_root, &mut lines)?;
    if lines.is_empty() {
        Ok("<sync root is empty>".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

fn dump_sync_root_tree_recursive(
    sync_root: &Path,
    current: &Path,
    lines: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        let relative = path
            .strip_prefix(sync_root)?
            .to_string_lossy()
            .replace('\\', "/");
        #[cfg(windows)]
        let attrs = metadata.file_attributes();
        #[cfg(not(windows))]
        let attrs = 0u32;
        lines.push(format!(
            "{} [{} attrs=0x{:08x}]",
            relative,
            if metadata.is_dir() { "dir" } else { "file" },
            attrs
        ));
        if metadata.is_dir() {
            dump_sync_root_tree_recursive(sync_root, &path, lines)?;
        }
    }
    Ok(())
}

fn collect_sync_root_files_recursive(
    sync_root: &Path,
    current: &Path,
    files: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            collect_sync_root_files_recursive(sync_root, &path, files)?;
        } else {
            let relative = path
                .strip_prefix(sync_root)?
                .to_string_lossy()
                .replace('\\', "/");
            files.push(relative);
        }
    }
    Ok(())
}

#[cfg(windows)]
fn assert_placeholder_attributes(
    sync_root: &Path,
    paths: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    const FILE_ATTRIBUTE_OFFLINE: u32 = 0x0000_1000;
    const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x0000_0200;
    const FILE_ATTRIBUTE_PINNED: u32 = 0x0008_0000;
    const FILE_ATTRIBUTE_UNPINNED: u32 = 0x0010_0000;
    const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x0004_0000;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;

    for relative in paths {
        let path = sync_root.join(relative.replace('/', "\\"));
        let attrs = std::fs::metadata(&path)?.file_attributes();
        let is_placeholder = (attrs & FILE_ATTRIBUTE_REPARSE_POINT) != 0
            || (attrs & FILE_ATTRIBUTE_OFFLINE) != 0
            || (attrs & FILE_ATTRIBUTE_SPARSE_FILE) != 0
            || (attrs & FILE_ATTRIBUTE_PINNED) != 0
            || (attrs & FILE_ATTRIBUTE_UNPINNED) != 0
            || (attrs & FILE_ATTRIBUTE_RECALL_ON_OPEN) != 0
            || (attrs & FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) != 0;
        assert!(
            is_placeholder,
            "plik {} nie wygląda jak placeholder CFAPI, attrs=0x{:08x}",
            path.display(),
            attrs
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn assert_placeholder_attributes(
    _sync_root: &Path,
    _paths: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

async fn reserve_port() -> Result<u16, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

async fn http_get_json(url: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let (host_port, path) = split_http_url(url)?;
    let mut stream = TcpStream::connect(&host_port).await?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    let response = read_http_body(&mut stream).await?;
    Ok(serde_json::from_str(&response)?)
}

async fn http_post_json(url: &str, body: &Value) -> Result<Value, Box<dyn std::error::Error>> {
    let (host_port, path) = split_http_url(url)?;
    let body_text = body.to_string();
    let mut stream = TcpStream::connect(&host_port).await?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body_text.len(),
        body_text
    );
    stream.write_all(request.as_bytes()).await?;
    let response = read_http_body(&mut stream).await?;
    Ok(serde_json::from_str(&response)?)
}

async fn read_http_body(stream: &mut TcpStream) -> Result<String, Box<dyn std::error::Error>> {
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let response = String::from_utf8(response)?;
    let (_, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP response"))?;
    Ok(body.to_string())
}

fn split_http_url(url: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let without_scheme = url
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "only http:// URLs are supported"))?;
    let (host_port, path) = without_scheme
        .split_once('/')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing request path"))?;
    Ok((host_port.to_string(), format!("/{}", path)))
}

fn create_temp_root() -> io::Result<PathBuf> {
    let unique = format!(
        "angeld-e2e-recovery-{}-{}",
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
