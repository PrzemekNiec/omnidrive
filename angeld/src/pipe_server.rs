//! Named Pipe IPC server — receives commands from the Shell Extension DLL.
//!
//! Protocol: one JSON-line request per connection, one JSON-line response, then close.
//!
//! Request:  `{"action":"free_space","path":"O:\\Documents\\file.pdf"}\n`
//! Response: `{"ok":true}\n`  or  `{"ok":false,"error":"..."}\n`
//!
//! The server accepts connections only from `explorer.exe` running under the same user
//! account as the `angeld` process.

use crate::db;
use crate::smart_sync;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::NamedPipeServer;
use tracing::{error, info, warn};
use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::core::PCWSTR;

const PIPE_NAME: &str = r"\\.\pipe\omnidrive_shellcmd";

/// `PIPE_UNLIMITED_INSTANCES` would let any process holding `FILE_CREATE_PIPE_INSTANCE`
/// (implied by the `GENERIC_WRITE` the DACL grants the current user) spin up extra
/// instances of this pipe. A fixed pool created once at startup closes that gap: once all
/// `PIPE_INSTANCES` exist, NPFS refuses every further `CreateNamedPipeW` on this name.
const PIPE_INSTANCES: u32 = 4;

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

fn current_user_sddl() -> Result<String, String> {
    let sid = crate::win_acl::current_user_sid_string().map_err(|e| e.to_string())?;
    Ok(format!("D:(A;;GRGW;;;{sid})"))
}

fn client_image_path(pipe: &NamedPipeServer) -> Result<String, String> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::core::PWSTR;

    unsafe {
        let handle = HANDLE(pipe.as_raw_handle());
        let mut pid = 0u32;
        GetNamedPipeClientProcessId(handle, &mut pid).map_err(|e| format!("client pid: {e}"))?;

        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .map_err(|e| format!("open client process: {e}"))?;

        let mut buf = vec![0u16; 4096];
        let mut len = buf.len() as u32;
        let query_result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        query_result.map_err(|e| format!("query image name: {e}"))?;

        Ok(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Compares the full image path, not just the file name, so `C:\Users\x\explorer.exe`
/// is rejected.
fn is_trusted_client(image_path: &str, system_root: &str) -> bool {
    let root = system_root.trim_end_matches(['\\', '/']);
    let expected = format!("{root}\\explorer.exe");
    image_path.eq_ignore_ascii_case(&expected)
}

fn system_root() -> String {
    std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string())
}

/// The DACL admits only the current user: the shell extension DLL runs in-process inside
/// `explorer.exe`, under the same account as `angeld`.
fn create_pipe_instance(pipe_name: &str, first: bool) -> Result<NamedPipeServer, String> {
    use windows::Win32::Security::Authorization::SDDL_REVISION_1;

    unsafe {
        let sddl = current_user_sddl()?;
        let sddl_w: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut sd = windows::Win32::Security::PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_w.as_ptr()),
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

        let open_mode = PIPE_ACCESS_DUPLEX
            | FILE_FLAG_OVERLAPPED
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0)
            };

        let pipe_name_w: Vec<u16> = pipe_name.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = CreateNamedPipeW(
            PCWSTR(pipe_name_w.as_ptr()),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            PIPE_INSTANCES,
            4096, // out buffer
            4096, // in buffer
            0,    // default timeout
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
        NamedPipeServer::from_raw_handle(handle.0).map_err(|e| format!("from_raw_handle: {e}"))
    }
}

/// Starts the Named Pipe server loop.  Spawns one task per incoming connection.
/// Call from `run_daemon()` via `tokio::spawn`.
pub async fn run_pipe_server(pool: SqlitePool) {
    serve(pool, PIPE_NAME).await;
}

async fn next_pipe_instance(pipe_name: &str, first: bool) -> NamedPipeServer {
    loop {
        match create_pipe_instance(pipe_name, first) {
            Ok(s) => return s,
            Err(e) => {
                warn!("failed to create named pipe {pipe_name}: {e}, retrying in 5s");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

async fn serve(pool: SqlitePool, pipe_name: &str) {
    info!("pipe server starting on {pipe_name}");
    let system_root = system_root();

    let mut instances = Vec::with_capacity(PIPE_INSTANCES as usize);
    instances.push(next_pipe_instance(pipe_name, true).await);
    for _ in 1..PIPE_INSTANCES {
        instances.push(next_pipe_instance(pipe_name, false).await);
    }

    for server in instances {
        let pool = pool.clone();
        let system_root = system_root.clone();
        tokio::spawn(serve_instance(server, pool, system_root));
    }

    std::future::pending::<()>().await;
}

async fn serve_instance(mut server: NamedPipeServer, pool: SqlitePool, system_root: String) {
    loop {
        // Wait for a client (shell extension DLL) to connect to this pool slot.
        if let Err(e) = server.connect().await {
            error!("pipe accept error: {e}");
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let _ = server.disconnect();
            continue;
        }

        if let Err(e) = handle_connection(&mut server, &pool, &system_root).await {
            warn!("pipe client error: {e}");
        }

        if let Err(e) = server.disconnect() {
            warn!("pipe disconnect error: {e}");
        }
    }
}

async fn handle_connection(
    pipe: &mut NamedPipeServer,
    pool: &SqlitePool,
    system_root: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let trusted = match client_image_path(pipe) {
        Ok(path) if is_trusted_client(&path, system_root) => true,
        Ok(path) => {
            warn!("rejected pipe client: untrusted image path \"{path}\"");
            false
        }
        Err(e) => {
            warn!("rejected pipe client: could not verify identity: {e}");
            false
        }
    };

    if !trusted {
        let mut resp_bytes = serde_json::to_vec(&ShellResponse::fail("untrusted client"))?;
        resp_bytes.push(b'\n');
        pipe.write_all(&resp_bytes).await?;
        wait_for_client_close(pipe).await;
        return Ok(());
    }

    let mut line = String::new();
    let bytes_read = {
        let mut buf_reader = BufReader::new(&mut *pipe);
        buf_reader.read_line(&mut line).await?
    };
    if bytes_read == 0 {
        return Ok(()); // client disconnected without sending
    }

    let response = match serde_json::from_str::<ShellCommand>(line.trim()) {
        Ok(cmd) => dispatch_command(cmd, pool).await,
        Err(e) => ShellResponse::fail(format!("invalid json: {e}")),
    };

    let mut resp_bytes = serde_json::to_vec(&response)?;
    resp_bytes.push(b'\n');
    pipe.write_all(&resp_bytes).await?;
    wait_for_client_close(pipe).await;

    Ok(())
}

/// `DisconnectNamedPipe` drops any bytes the client has not yet read, so the response
/// written above would be lost if we disconnected immediately. Waiting for the client to
/// close its handle (read returns 0 or errors) guarantees delivery before the instance is
/// recycled for the next connection.
async fn wait_for_client_close(pipe: &mut NamedPipeServer) {
    let mut buf = [0u8; 64];
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match pipe.read(&mut buf).await {
                Ok(0) => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    })
    .await;
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
async fn resolve_path(pool: &SqlitePool, raw_path: &str) -> Result<ResolvedTarget, ShellResponse> {
    let logical = normalize_path(raw_path)
        .ok_or_else(|| ShellResponse::fail(format!("invalid path: {raw_path}")))?;

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

    let drive_letter = crate::virtual_drive::mounted_drive_letter()
        .or_else(|| std::env::var("OMNIDRIVE_DRIVE_LETTER").ok())
        .unwrap_or_else(|| "O:".to_string());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_pipe_name() -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        format!(
            "\\\\.\\pipe\\omnidrive_shellcmd_test_{}_{}",
            std::process::id(),
            n
        )
    }

    #[test]
    fn sddl_grants_only_current_user() {
        let sddl = current_user_sddl().expect("current_user_sddl");
        assert!(sddl.starts_with("D:(A;;GRGW;;;S-1-"), "sddl={sddl}");
        assert!(!sddl.contains(";WD)"), "sddl={sddl}");
    }

    #[tokio::test]
    async fn pipe_dacl_has_single_ace_for_current_user() {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
            SE_KERNEL_OBJECT,
        };
        use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
        use windows::core::PWSTR;

        let name = test_pipe_name();
        let server = create_pipe_instance(&name, true).expect("create test pipe instance");
        let expected_sid = current_user_sddl()
            .unwrap()
            .trim_start_matches("D:(A;;GRGW;;;")
            .trim_end_matches(')')
            .to_string();

        unsafe {
            let handle = HANDLE(server.as_raw_handle());
            let mut sd = PSECURITY_DESCRIPTOR::default();
            let err = GetSecurityInfo(
                handle,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                Some(&mut sd),
            );
            assert_eq!(err.0, 0, "GetSecurityInfo failed: {}", err.0);

            let mut sddl_ptr = PWSTR::null();
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                sd,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut sddl_ptr,
                None,
            )
            .expect("stringify security descriptor");
            let sddl = crate::win_acl::pwstr_to_string(sddl_ptr).unwrap();
            let _ = LocalFree(Some(HLOCAL(sddl_ptr.0 as *mut _)));
            let _ = LocalFree(Some(HLOCAL(sd.0 as *mut _)));

            assert_eq!(sddl.matches("(A;").count(), 1, "sddl={sddl}");
            assert!(sddl.contains(&expected_sid), "sddl={sddl}");
            assert!(!sddl.contains(";;;WD)"), "sddl={sddl}");
        }
    }

    #[test]
    fn is_trusted_client_table() {
        let root = "C:\\Windows";
        assert!(is_trusted_client("C:\\Windows\\explorer.exe", root));
        assert!(is_trusted_client("c:\\windows\\EXPLORER.EXE", root));
        assert!(!is_trusted_client(
            "C:\\Windows\\System32\\notepad.exe",
            root
        ));
        assert!(!is_trusted_client("C:\\Users\\x\\explorer.exe", root));
        assert!(!is_trusted_client("C:\\Windows\\explorer.exe.bak", root));
        assert!(is_trusted_client(
            "C:\\Windows\\explorer.exe",
            "C:\\Windows\\"
        ));
    }

    #[tokio::test]
    async fn second_instance_from_same_user_is_refused() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let pipe_name = test_pipe_name();
        let server_name = pipe_name.clone();
        tokio::spawn(async move {
            serve(pool, &server_name).await;
        });

        let wait_client_path = pipe_name.clone();
        tokio::task::spawn_blocking(move || {
            for _ in 0..50 {
                if let Ok(file) = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&wait_client_path)
                {
                    drop(file);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            panic!("pipe never became available");
        })
        .await
        .unwrap();

        let pipe_name_w: Vec<u16> = pipe_name.encode_utf16().chain(std::iter::once(0)).collect();

        use windows::Win32::System::Pipes::PIPE_UNLIMITED_INSTANCES;
        for max_instances in [PIPE_UNLIMITED_INSTANCES, 4] {
            let handle = unsafe {
                CreateNamedPipeW(
                    PCWSTR(pipe_name_w.as_ptr()),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    max_instances,
                    4096,
                    4096,
                    0,
                    None,
                )
            };
            assert_eq!(
                handle,
                HANDLE(-1isize as *mut _),
                "expected INVALID_HANDLE_VALUE for nMaxInstances={max_instances}"
            );
            let err = std::io::Error::last_os_error();
            println!(
                "second_instance_from_same_user_is_refused: nMaxInstances={max_instances} err={err:?}"
            );
        }
    }

    #[tokio::test]
    async fn pool_instances_are_reused_after_disconnect() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let pipe_name = test_pipe_name();
        let server_name = pipe_name.clone();
        tokio::spawn(async move {
            serve(pool, &server_name).await;
        });

        for i in 0..6 {
            let client_path = pipe_name.clone();
            let response = tokio::task::spawn_blocking(move || -> String {
                use std::io::{BufRead, BufReader, Write};

                let mut file = None;
                for _ in 0..50 {
                    match std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&client_path)
                    {
                        Ok(f) => {
                            file = Some(f);
                            break;
                        }
                        Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
                    }
                }
                let mut file = file.expect("failed to open test pipe as client");
                let _ = file.write_all(b"{\"action\":\"free_space\",\"path\":\"O:\\\\x\"}\n");
                let mut reader = BufReader::new(file);
                let mut line = String::new();
                reader.read_line(&mut line).expect("read response");
                line
            })
            .await
            .unwrap();

            let parsed: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
            assert_eq!(parsed["ok"], false, "client {i}: response={response}");
            assert_eq!(
                parsed["error"], "untrusted client",
                "client {i}: response={response}"
            );
        }
    }

    #[tokio::test]
    async fn server_rejects_non_explorer_client() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let pipe_name = test_pipe_name();
        let server_name = pipe_name.clone();
        tokio::spawn(async move {
            serve(pool, &server_name).await;
        });

        let client_path = pipe_name.clone();
        let response = tokio::task::spawn_blocking(move || -> String {
            use std::io::{BufRead, BufReader, Write};

            let mut file = None;
            for _ in 0..50 {
                match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&client_path)
                {
                    Ok(f) => {
                        file = Some(f);
                        break;
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
                }
            }
            let mut file = file.expect("failed to open test pipe as client");
            let _ = file.write_all(b"{\"action\":\"free_space\",\"path\":\"O:\\\\x\"}\n");
            let mut reader = BufReader::new(file);
            let mut line = String::new();
            reader.read_line(&mut line).expect("read response");
            line
        })
        .await
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(parsed["ok"], false, "response={response}");
        assert_eq!(parsed["error"], "untrusted client", "response={response}");
    }
}
