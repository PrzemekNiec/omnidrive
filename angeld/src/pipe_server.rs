//! Named Pipe IPC server — receives commands from the Shell Extension DLL.
//!
//! Protocol: one JSON-line request per connection, one JSON-line response, then close.
//!
//! Request:  `{"action":"free_space","path":"O:\\Documents\\file.pdf"}\n`
//! Response: `{"ok":true}\n`  or  `{"ok":false,"error":"..."}\n`

use crate::db;
use crate::smart_sync;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::NamedPipeServer;
use tracing::{error, info, warn};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HANDLE, LocalFree, HLOCAL};
use windows::Win32::Security::{SECURITY_ATTRIBUTES};
use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
    PIPE_WAIT,
};

const PIPE_NAME: &str = r"\\.\pipe\omnidrive_shellcmd";
const PIPE_NAME_W: &str = "\\\\.\\pipe\\omnidrive_shellcmd\0";
const SDDL_EVERYONE_RW: &str = "D:(A;;GRGW;;;WD)\0";

#[derive(Deserialize)]
struct ShellCommand {
    action: String,
    path: String,
}

#[derive(Serialize)]
struct ShellResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl ShellResponse {
    fn success() -> Self {
        Self {
            ok: true,
            error: None,
        }
    }
    fn fail(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(msg.into()),
        }
    }
}

/// Create a named pipe instance with a permissive ACL so that shell extension DLLs
/// running under different security contexts (e.g. non-elevated Explorer) can connect
/// to an elevated angeld process.
fn create_pipe_instance(first: bool) -> Result<NamedPipeServer, String> {
    use windows::Win32::Security::Authorization::SDDL_REVISION_1;

    unsafe {
        // Build a security descriptor from SDDL granting Everyone read+write.
        let sddl: Vec<u16> = SDDL_EVERYONE_RW.encode_utf16().collect();
        let mut sd = windows::Win32::Security::PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut sd,
            None,
        )
        .map_err(|e| format!("SDDL parse: {e}"))?;

        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };

        let open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED
            | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0) };

        let pipe_name: Vec<u16> = PIPE_NAME_W.encode_utf16().collect();
        let handle = CreateNamedPipeW(
            PCWSTR(pipe_name.as_ptr()),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            4096,  // out buffer
            4096,  // in buffer
            0,     // default timeout
            Some(&sa),
        );

        // Free the security descriptor allocated by ConvertString...
        let _ = LocalFree(Some(HLOCAL(sd.0 as *mut _)));

        if handle == HANDLE(-1isize as *mut _) {
            return Err(format!(
                "CreateNamedPipeW failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        // Wrap in tokio NamedPipeServer (takes ownership of the handle).
        NamedPipeServer::from_raw_handle(handle.0)
            .map_err(|e| format!("from_raw_handle: {e}"))
    }
}

/// Starts the Named Pipe server loop.  Spawns one task per incoming connection.
/// Call from `run_daemon()` via `tokio::spawn`.
pub async fn run_pipe_server(pool: SqlitePool) {
    info!("pipe server starting on {PIPE_NAME}");

    let mut server = match create_pipe_instance(true) {
        Ok(s) => s,
        Err(e) => {
            error!("failed to create named pipe {PIPE_NAME}: {e}");
            return;
        }
    };

    loop {
        // Wait for a client (shell extension DLL) to connect.
        if let Err(e) = server.connect().await {
            error!("pipe accept error: {e}");
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            continue;
        }

        // Hand off the connected pipe to a spawned task.
        let connected = server;

        // Immediately create a new pipe instance for the next client.
        server = match create_pipe_instance(false) {
            Ok(s) => s,
            Err(e) => {
                error!("failed to recreate named pipe: {e}");
                return;
            }
        };

        let pool = pool.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(connected, &pool).await {
                warn!("pipe client error: {e}");
            }
        });
    }
}

async fn handle_connection(
    pipe: tokio::net::windows::named_pipe::NamedPipeServer,
    pool: &SqlitePool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (reader, mut writer) = tokio::io::split(pipe);
    let mut buf_reader = BufReader::new(reader);

    let mut line = String::new();
    let bytes_read = buf_reader.read_line(&mut line).await?;
    if bytes_read == 0 {
        return Ok(()); // client disconnected without sending
    }

    let response = match serde_json::from_str::<ShellCommand>(line.trim()) {
        Ok(cmd) => dispatch_command(cmd, pool).await,
        Err(e) => ShellResponse::fail(format!("invalid json: {e}")),
    };

    let mut resp_bytes = serde_json::to_vec(&response)?;
    resp_bytes.push(b'\n');
    writer.write_all(&resp_bytes).await?;
    writer.shutdown().await?;

    Ok(())
}

async fn dispatch_command(cmd: ShellCommand, pool: &SqlitePool) -> ShellResponse {
    info!(
        "pipe command: action=\"{}\", path=\"{}\"",
        cmd.action, cmd.path
    );

    match cmd.action.as_str() {
        "free_space" => do_free_space(pool, &cmd.path).await,
        "download" => do_download(pool, &cmd.path).await,
        "set_lokalnie" => do_set_policy(pool, &cmd.path, "LOCAL", 1).await,
        "set_combo" => do_set_policy(pool, &cmd.path, "STANDARD", 1).await,
        "set_chmura" => do_set_policy(pool, &cmd.path, "PARANOIA", 0).await,
        "set_forteca" => do_set_policy(pool, &cmd.path, "PARANOIA", 1).await,
        _ => ShellResponse::fail(format!("unknown action: {}", cmd.action)),
    }
}

/// Dehydrate a file (unpin → sync placeholder pin state → dehydrate).
async fn do_free_space(pool: &SqlitePool, raw_path: &str) -> ShellResponse {
    let t = match resolve_path(pool, raw_path).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    if let Err(e) = db::set_pin_state(pool, t.inode_id, 0).await {
        return ShellResponse::fail(format!("db error: {e}"));
    }

    if let Err(e) =
        smart_sync::sync_placeholder_pin_state(pool, &t.sync_root, t.inode_id, true).await
    {
        return ShellResponse::fail(format!("dehydrate error: {e}"));
    }

    ShellResponse::success()
}

/// Hydrate a file (pin → hydrate placeholder now).
async fn do_download(pool: &SqlitePool, raw_path: &str) -> ShellResponse {
    let t = match resolve_path(pool, raw_path).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    if let Err(e) = db::set_pin_state(pool, t.inode_id, 1).await {
        return ShellResponse::fail(format!("db error: {e}"));
    }

    if let Err(e) = smart_sync::hydrate_placeholder_now(pool, &t.sync_root, t.inode_id).await {
        return ShellResponse::fail(format!("hydrate error: {e}"));
    }

    ShellResponse::success()
}

/// Set protection level: policy_type (LOCAL/STANDARD/PARANOIA) + pin_state (0/1).
async fn do_set_policy(
    pool: &SqlitePool,
    raw_path: &str,
    policy_type: &str,
    pin_state: i64,
) -> ShellResponse {
    let t = match resolve_path(pool, raw_path).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    // 1. Set sync policy for this path
    if let Err(e) = db::set_sync_policy_type_for_path(pool, &t.logical_path, policy_type).await {
        return ShellResponse::fail(format!("set policy error: {e}"));
    }

    // 2. Set pin state
    if let Err(e) = db::set_pin_state(pool, t.inode_id, pin_state).await {
        return ShellResponse::fail(format!("db error: {e}"));
    }

    // 3. Enforce pin state on the placeholder
    if pin_state == 1 {
        if let Err(e) = smart_sync::hydrate_placeholder_now(pool, &t.sync_root, t.inode_id).await {
            return ShellResponse::fail(format!("hydrate error: {e}"));
        }
    } else if let Err(e) =
        smart_sync::sync_placeholder_pin_state(pool, &t.sync_root, t.inode_id, true).await
    {
        return ShellResponse::fail(format!("dehydrate error: {e}"));
    }

    info!(
        "policy set: path=\"{}\", type={}, pin={}",
        t.logical_path, policy_type, pin_state
    );
    ShellResponse::success()
}

/// Resolved target: inode_id, logical DB path, sync_root on disk.
struct ResolvedTarget {
    inode_id: i64,
    logical_path: String,
    sync_root: PathBuf,
}

/// Resolve an O:\ or SyncRoot path to inode_id + logical path + sync_root.
async fn resolve_path(
    pool: &SqlitePool,
    raw_path: &str,
) -> Result<ResolvedTarget, ShellResponse> {
    let logical = normalize_path(raw_path).ok_or_else(|| {
        ShellResponse::fail(format!("invalid path: {raw_path}"))
    })?;

    let inode_id = db::resolve_path(pool, &logical)
        .await
        .map_err(|e| ShellResponse::fail(format!("db error: {e}")))?
        .ok_or_else(|| ShellResponse::fail("inode_not_found"))?;

    let inode = db::get_inode_by_id(pool, inode_id)
        .await
        .map_err(|e| ShellResponse::fail(format!("db error: {e}")))?
        .ok_or_else(|| ShellResponse::fail("inode_not_found"))?;

    if inode.kind != "FILE" {
        return Err(ShellResponse::fail(format!(
            "not a file (kind={})",
            inode.kind
        )));
    }

    let sync_root = crate::runtime_paths::RuntimePaths::detect().sync_root;
    Ok(ResolvedTarget {
        inode_id,
        logical_path: logical,
        sync_root,
    })
}

/// Strip O:\ or SyncRoot prefix, normalise to forward-slash relative path.
/// Mirror of `normalize_filesystem_api_path` in api.rs.
fn normalize_path(raw_path: &str) -> Option<String> {
    let trimmed = raw_path.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }

    let drive_letter =
        std::env::var("OMNIDRIVE_DRIVE_LETTER").unwrap_or_else(|_| "O:".to_string());
    let drive_prefix = format!(
        "{}\\",
        drive_letter
            .trim()
            .trim_end_matches('\\')
            .trim_end_matches('/')
            .to_ascii_uppercase()
    );

    let sync_root = crate::runtime_paths::RuntimePaths::detect().sync_root;
    let sync_root_rendered = sync_root.to_string_lossy().replace('/', "\\");
    let sync_root_upper = sync_root_rendered.to_ascii_uppercase();

    let candidate = trimmed.replace('/', "\\");
    let candidate_upper = candidate.to_ascii_uppercase();

    let relative = if candidate_upper.starts_with(&drive_prefix) {
        candidate[drive_prefix.len()..].to_string()
    } else if candidate_upper.starts_with(&(sync_root_upper.clone() + "\\")) {
        candidate[(sync_root_rendered.len() + 1)..].to_string()
    } else {
        candidate
    };

    let normalized = relative
        .trim_start_matches('\\')
        .trim_start_matches('/')
        .replace('\\', "/");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}
