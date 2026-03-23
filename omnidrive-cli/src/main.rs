use clap::{Parser, Subcommand};
use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::fmt;

#[derive(Parser)]
#[command(name = "omnidrive")]
#[command(about = "CLI client for the local OmniDrive daemon API")]
struct Cli {
    #[arg(long, global = true)]
    api_base: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Status,
    Ls,
    History { inode_id: i64 },
    Restore { inode_id: i64, revision_id: i64 },
    Pin { inode_id: i64 },
    Unpin { inode_id: i64 },
    Recovery {
        #[command(subcommand)]
        command: RecoveryCommand,
    },
}

#[derive(Subcommand)]
enum RecoveryCommand {
    SnapshotLocal { output_path: String },
}

#[derive(Debug)]
enum CliError {
    Http(reqwest::Error),
    Api(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(err) => write!(f, "http error: {err}"),
            Self::Api(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<reqwest::Error> for CliError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

#[derive(Debug, Deserialize)]
struct QuotaResponse {
    max_physical_bytes_per_provider: u64,
    providers: Vec<ProviderQuotaResponse>,
}

#[derive(Debug, Deserialize)]
struct ProviderQuotaResponse {
    provider: String,
    used_physical_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct VaultHealthResponse {
    total_packs: i64,
    healthy_packs: i64,
    degraded_packs: i64,
    unreadable_packs: i64,
}

#[derive(Debug, Deserialize)]
struct FileEntryResponse {
    inode_id: i64,
    path: String,
    size: i64,
    current_revision_id: Option<i64>,
    current_revision_created_at: Option<i64>,
    smart_sync_pin_state: Option<i64>,
    smart_sync_hydration_state: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct FileRevisionResponse {
    revision_id: i64,
    inode_id: i64,
    created_at: i64,
    size: i64,
    is_current: bool,
    immutable_until: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RestoreRevisionResponse {
    inode_id: i64,
    revision_id: i64,
    restored: bool,
}

#[derive(Debug, Deserialize)]
struct SmartSyncActionResponse {
    inode_id: i64,
    pin_state: i64,
    hydration_state: i64,
}

#[derive(Debug, Deserialize)]
struct SnapshotLocalResponse {
    output_path: String,
    created: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let api_base = cli
        .api_base
        .or_else(|| env::var("OMNIDRIVE_API_BASE").ok())
        .unwrap_or_else(|| "http://127.0.0.1:8787".to_string());
    let client = Client::new();

    let result = match cli.command {
        Command::Status => status(&client, &api_base).await,
        Command::Ls => list_files(&client, &api_base).await,
        Command::History { inode_id } => history(&client, &api_base, inode_id).await,
        Command::Restore {
            inode_id,
            revision_id,
        } => restore(&client, &api_base, inode_id, revision_id).await,
        Command::Pin { inode_id } => pin(&client, &api_base, inode_id).await,
        Command::Unpin { inode_id } => unpin(&client, &api_base, inode_id).await,
        Command::Recovery { command } => match command {
            RecoveryCommand::SnapshotLocal { output_path } => {
                snapshot_local(&client, &api_base, &output_path).await
            }
        },
    };

    if let Err(err) = result {
        eprintln!("omnidrive: {err}");
        std::process::exit(1);
    }
}

async fn status(client: &Client, api_base: &str) -> Result<(), CliError> {
    let quota: QuotaResponse = get_json(client, &format!("{api_base}/api/quota")).await?;
    let health: VaultHealthResponse =
        get_json(client, &format!("{api_base}/api/health/vault")).await?;

    println!("Vault Health");
    println!("  total packs:      {}", health.total_packs);
    println!("  healthy packs:    {}", health.healthy_packs);
    println!("  degraded packs:   {}", health.degraded_packs);
    println!("  unreadable packs: {}", health.unreadable_packs);
    println!();
    println!(
        "Provider Usage (limit per provider: {} / {})",
        quota.max_physical_bytes_per_provider,
        human_bytes(quota.max_physical_bytes_per_provider)
    );
    println!(
        "{:<18} {:>16} {:>14}",
        "PROVIDER", "USED_BYTES", "USED_HUMAN"
    );
    for provider in quota.providers {
        println!(
            "{:<18} {:>16} {:>14}",
            provider.provider,
            provider.used_physical_bytes,
            human_bytes(provider.used_physical_bytes)
        );
    }

    Ok(())
}

async fn list_files(client: &Client, api_base: &str) -> Result<(), CliError> {
    let files: Vec<FileEntryResponse> = get_json(client, &format!("{api_base}/api/files")).await?;

    if files.is_empty() {
        println!("No active files.");
        return Ok(());
    }

    println!(
        "{:<8} {:<10} {:>12} {:>12} {:>14}  PATH",
        "SYNC", "INODE_ID", "SIZE", "REVISION", "CREATED_AT"
    );
    for file in files {
        println!(
            "{:<8} {:<10} {:>12} {:>12} {:>14}  {}",
            sync_marker(file.smart_sync_pin_state, file.smart_sync_hydration_state),
            file.inode_id,
            file.size,
            file.current_revision_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            file.current_revision_created_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            file.path
        );
    }

    Ok(())
}

async fn pin(client: &Client, api_base: &str, inode_id: i64) -> Result<(), CliError> {
    let response = client
        .post(format!("{api_base}/api/files/{inode_id}/pin"))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(CliError::Api(format!(
            "pin failed with status {}",
            response.status()
        )));
    }

    let result: SmartSyncActionResponse = response.json().await?;
    println!(
        "Pinned inode {} (pin_state={}, hydration_state={})",
        result.inode_id, result.pin_state, result.hydration_state
    );
    Ok(())
}

async fn unpin(client: &Client, api_base: &str, inode_id: i64) -> Result<(), CliError> {
    let response = client
        .post(format!("{api_base}/api/files/{inode_id}/unpin"))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(CliError::Api(format!(
            "unpin failed with status {}",
            response.status()
        )));
    }

    let result: SmartSyncActionResponse = response.json().await?;
    println!(
        "Unpinned inode {} (pin_state={}, hydration_state={})",
        result.inode_id, result.pin_state, result.hydration_state
    );
    Ok(())
}

async fn history(client: &Client, api_base: &str, inode_id: i64) -> Result<(), CliError> {
    let revisions: Vec<FileRevisionResponse> = get_json(
        client,
        &format!("{api_base}/api/files/{inode_id}/revisions"),
    )
    .await?;

    if revisions.is_empty() {
        println!("No revisions for inode {inode_id}.");
        return Ok(());
    }

    println!(
        "{:<12} {:>10} {:>14} {:>12} {:>10} {:>16}",
        "REVISION_ID", "INODE_ID", "CREATED_AT", "SIZE", "CURRENT", "IMMUTABLE_UNTIL"
    );
    for revision in revisions {
        println!(
            "{:<12} {:>10} {:>14} {:>12} {:>10} {:>16}",
            revision.revision_id,
            revision.inode_id,
            revision.created_at,
            revision.size,
            if revision.is_current { "yes" } else { "no" },
            revision
                .immutable_until
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string())
        );
    }

    Ok(())
}

async fn restore(
    client: &Client,
    api_base: &str,
    inode_id: i64,
    revision_id: i64,
) -> Result<(), CliError> {
    let response = client
        .post(format!(
            "{api_base}/api/files/{inode_id}/revisions/{revision_id}/restore"
        ))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(CliError::Api(format!(
            "restore failed with status {}",
            response.status()
        )));
    }

    let restored: RestoreRevisionResponse = response.json().await?;
    println!(
        "Restored inode {} to revision {} (restored={})",
        restored.inode_id, restored.revision_id, restored.restored
    );

    Ok(())
}

async fn snapshot_local(
    client: &Client,
    api_base: &str,
    output_path: &str,
) -> Result<(), CliError> {
    let response = client
        .post(format!("{api_base}/api/recovery/snapshot-local"))
        .json(&serde_json::json!({ "output_path": output_path }))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(CliError::Api(format!(
            "snapshot-local failed with status {}",
            response.status()
        )));
    }

    let snapshot: SnapshotLocalResponse = response.json().await?;
    println!(
        "Created metadata snapshot at {} (created={})",
        snapshot.output_path, snapshot.created
    );
    Ok(())
}

async fn get_json<T: for<'de> Deserialize<'de>>(client: &Client, url: &str) -> Result<T, CliError> {
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(CliError::Api(format!(
            "request to {url} failed with status {}",
            response.status()
        )));
    }
    Ok(response.json::<T>().await?)
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    format!("{value:.2} {}", UNITS[unit])
}

fn sync_marker(pin_state: Option<i64>, hydration_state: Option<i64>) -> String {
    match (pin_state.unwrap_or(0), hydration_state.unwrap_or(0)) {
        (1, 1) => "[PH]".to_string(),
        (1, _) => "[P]".to_string(),
        (0, 1) => "[H]".to_string(),
        _ => "[O]".to_string(),
    }
}
