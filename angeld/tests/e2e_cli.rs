use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod common;

/// `cargo test -p angeld` never rebuilds `omnidrive-cli`, so an existing binary may be
/// stale; the nested build is a no-op when it is current.
fn omnidrive_binary_path() -> PathBuf {
    let path = Path::new(env!("CARGO_BIN_EXE_angeld")).with_file_name("omnidrive.exe");
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root");
    let is_release = path
        .parent()
        .and_then(|dir| dir.file_name())
        .map(|name| name == "release")
        .unwrap_or(false);

    let mut build = Command::new(env!("CARGO"));
    build.args(["build", "-p", "omnidrive-cli"]);
    if is_release {
        build.arg("--release");
    }
    build.current_dir(repo_root);
    let status = build.status().expect("spawn cargo build -p omnidrive-cli");
    assert!(status.success(), "cargo build -p omnidrive-cli failed");

    path
}

struct CliRun {
    success: bool,
    stdout: String,
    stderr: String,
}

fn run_cli(
    binary: &Path,
    localapp: &Path,
    api_base: &str,
    args: &[&str],
    stdin_data: Option<&str>,
) -> CliRun {
    let mut command = Command::new(binary);
    command
        .args(args)
        .env("LOCALAPPDATA", localapp)
        .env("OMNIDRIVE_API_BASE", api_base)
        .env_remove("OMNIDRIVE_API_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().expect("spawn omnidrive CLI");
    if let Some(data) = stdin_data {
        child
            .stdin
            .take()
            .expect("cli stdin handle")
            .write_all(data.as_bytes())
            .expect("write cli stdin");
    } else {
        drop(child.stdin.take());
    }

    let output = child.wait_with_output().expect("wait for omnidrive CLI");
    CliRun {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[tokio::test]
async fn cli_login_then_authenticated_commands() {
    let binary = omnidrive_binary_path();
    let harness = common::DaemonHarness::spawn()
        .await
        .expect("daemon harness spawn");
    let localapp = harness.temp_root.join("localapp");

    let before_login = run_cli(&binary, &localapp, &harness.base_url, &["ls"], None);
    assert!(
        !before_login.success,
        "ls before login must fail: stdout={} stderr={}",
        before_login.stdout, before_login.stderr
    );
    assert!(
        before_login.stderr.contains("401"),
        "stderr={}",
        before_login.stderr
    );
    assert!(
        before_login.stderr.contains("omnidrive login"),
        "stderr={}",
        before_login.stderr
    );

    let login = run_cli(
        &binary,
        &localapp,
        &harness.base_url,
        &["login"],
        Some(&format!("{}\n", common::E2E_PASSPHRASE)),
    );
    assert!(
        login.success,
        "login failed: stdout={} stderr={}",
        login.stdout, login.stderr
    );
    let session_path = localapp.join("OmniDrive").join("cli-session");
    let contents =
        std::fs::read_to_string(&session_path).expect("cli-session file must exist after login");
    let trimmed = contents.trim();
    assert!(!trimmed.is_empty(), "cli-session file must not be empty");
    assert!(
        !trimmed.chars().any(|c| c.is_whitespace()),
        "trimmed token must contain no whitespace: {trimmed:?}"
    );

    let ls = run_cli(&binary, &localapp, &harness.base_url, &["ls"], None);
    assert!(
        ls.success,
        "ls after login failed: stdout={} stderr={}",
        ls.stdout, ls.stderr
    );
    assert!(
        ls.stdout.contains("No active files."),
        "stdout={}",
        ls.stdout
    );

    let pin = run_cli(&binary, &localapp, &harness.base_url, &["pin", "1"], None);
    assert!(!pin.success, "pin against missing inode must fail");
    assert!(pin.stderr.contains("404"), "stderr={}", pin.stderr);
    assert!(!pin.stderr.contains("401"), "stderr={}", pin.stderr);
    assert!(!pin.stderr.contains("403"), "stderr={}", pin.stderr);

    let bad_token = run_cli(
        &binary,
        &localapp,
        &harness.base_url,
        &["--api-token", "invalid", "ls"],
        None,
    );
    assert!(!bad_token.success, "invalid --api-token must fail");
    assert!(
        bad_token.stderr.contains("401"),
        "stderr={}",
        bad_token.stderr
    );

    let status = run_cli(&binary, &localapp, &harness.base_url, &["status"], None);
    assert!(
        status.success,
        "status failed: stdout={} stderr={}",
        status.stdout, status.stderr
    );
}
