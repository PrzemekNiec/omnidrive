use clap::{Parser, Subcommand};
use angeld::autostart;
use angeld::disaster_recovery::{MetadataBackupProviderManager, restore_metadata_from_cloud};
use angeld::runtime_paths::{RuntimePaths, sqlite_db_file_path};
use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::fmt;
use std::path::PathBuf;

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
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
    Maintenance {
        #[command(subcommand)]
        command: MaintenanceCommand,
    },
    Recovery {
        #[command(subcommand)]
        command: RecoveryCommand,
    },
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
}

#[derive(Subcommand)]
enum MaintenanceCommand {
    Status,
    Errors,
}

#[derive(Subcommand)]
enum CacheCommand {
    Status,
}

#[derive(Subcommand)]
enum RecoveryCommand {
    Status,
    #[command(alias = "snapshot-local")]
    BackupNow,
    Restore,
}

#[derive(Subcommand)]
enum ServiceCommand {
    Register,
    Unregister,
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
struct BackupNowResponse {
    uploaded: bool,
}

#[derive(Debug, Deserialize)]
struct RecoveryStatusResponse {
    last_successful_backup: Option<i64>,
    recent_attempts: Vec<MetadataBackupAttemptResponse>,
}

#[derive(Debug, Deserialize)]
struct MetadataBackupAttemptResponse {
    backup_id: String,
    created_at: i64,
    snapshot_version: i64,
    object_key: String,
    provider: String,
    encrypted_size: i64,
    status: String,
    last_error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ScrubStatusResponse {
    total_shards: i64,
    verified_shards: i64,
    healthy_shards: i64,
    corrupted_or_missing: i64,
    verified_light_shards: i64,
    verified_deep_shards: i64,
    last_scrub_timestamp: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ScrubErrorResponse {
    pack_id: String,
    provider: String,
    shard_index: i64,
    last_verified_at: Option<i64>,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CacheStatusResponse {
    total_entries: i64,
    total_bytes: i64,
    max_bytes: u64,
    prefetched_entries: i64,
    hit_count: u64,
    miss_count: u64,
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
        Command::Cache { command } => match command {
            CacheCommand::Status => cache_status(&client, &api_base).await,
        },
        Command::Maintenance { command } => match command {
            MaintenanceCommand::Status => maintenance_status(&client, &api_base).await,
            MaintenanceCommand::Errors => maintenance_errors(&client, &api_base).await,
        },
        Command::Recovery { command } => match command {
            RecoveryCommand::Status => recovery_status(&client, &api_base).await,
            RecoveryCommand::BackupNow => backup_now(&client, &api_base).await,
            RecoveryCommand::Restore => restore_from_cloud().await,
        },
        Command::Service { command } => match command {
            ServiceCommand::Register => register_service_autostart(),
            ServiceCommand::Unregister => unregister_service_autostart(),
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

async fn cache_status(client: &Client, api_base: &str) -> Result<(), CliError> {
    let status: CacheStatusResponse = get_json(client, &format!("{api_base}/api/cache/status")).await?;
    let total_ops = status.hit_count + status.miss_count;
    let hit_ratio = if total_ops == 0 {
        0.0
    } else {
        (status.hit_count as f64 / total_ops as f64) * 100.0
    };

    println!("Cache Performance");
    println!("  entries:          {}", status.total_entries);
    println!(
        "  usage:            {} / {}",
        human_bytes(status.total_bytes as u64),
        human_bytes(status.max_bytes)
    );
    println!("  prefetched:       {}", status.prefetched_entries);
    println!("  hits:             {}", status.hit_count);
    println!("  misses:           {}", status.miss_count);
    println!("  hit ratio:        {:.1}%", hit_ratio);

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

async fn backup_now(client: &Client, api_base: &str) -> Result<(), CliError> {
    let response = client
        .post(format!("{api_base}/api/metadata-backup/backup-now"))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(CliError::Api(format!(
            "backup-now failed with status {}",
            response.status()
        )));
    }

    let backup: BackupNowResponse = response.json().await?;
    println!("Uploaded encrypted metadata backup (uploaded={})", backup.uploaded);
    Ok(())
}

async fn recovery_status(client: &Client, api_base: &str) -> Result<(), CliError> {
    let status: RecoveryStatusResponse =
        get_json(client, &format!("{api_base}/api/metadata-backup/status")).await?;

    println!("Recovery Status");
    println!(
        "  last successful backup: {}",
        status
            .last_successful_backup
            .map(format_timestamp)
            .unwrap_or_else(|| "never".to_string())
    );
    println!();

    if status.recent_attempts.is_empty() {
        println!("No metadata backup attempts recorded.");
        return Ok(());
    }

    println!(
        "{:<24} {:<18} {:<12} {:>12} {:<10}  OBJECT_KEY",
        "BACKUP_ID", "PROVIDER", "STATUS", "SIZE", "VERSION"
    );
    for attempt in status.recent_attempts {
        println!(
            "{:<24} {:<18} {:<12} {:>12} {:<10}  {}",
            truncate(&attempt.backup_id, 24),
            attempt.provider,
            attempt.status,
            human_bytes(attempt.encrypted_size as u64),
            attempt.snapshot_version,
            attempt.object_key
        );
        if let Some(error) = attempt.last_error {
            println!("  error: {}", error);
        }
        println!("  created_at: {}", format_timestamp(attempt.created_at));
    }

    Ok(())
}

async fn maintenance_status(client: &Client, api_base: &str) -> Result<(), CliError> {
    let status: ScrubStatusResponse =
        get_json(client, &format!("{api_base}/api/maintenance/scrub-status")).await?;

    let verification_pct = if status.total_shards > 0 {
        (status.verified_shards as f64 / status.total_shards as f64) * 100.0
    } else {
        0.0
    };
    let healthy_pct = if status.total_shards > 0 {
        (status.healthy_shards as f64 / status.total_shards as f64) * 100.0
    } else {
        0.0
    };

    println!("Vault Integrity");
    println!(
        "  coverage: {:.1}% verified ({}/{})",
        verification_pct, status.verified_shards, status.total_shards
    );
    println!(
        "  healthy:  {:.1}% healthy ({}/{})",
        healthy_pct, status.healthy_shards, status.total_shards
    );
    println!("  errors:   {}", status.corrupted_or_missing);
    println!(
        "  methods:  light={} deep={}",
        status.verified_light_shards, status.verified_deep_shards
    );
    println!(
        "  last scan: {}",
        status
            .last_scrub_timestamp
            .map(format_timestamp)
            .unwrap_or_else(|| "never".to_string())
    );

    let filled = ((verification_pct / 10.0).round() as usize).min(10);
    let empty = 10usize.saturating_sub(filled);
    println!();
    println!(
        "  [{}{}] {:.1}% verified",
        "#".repeat(filled),
        "-".repeat(empty),
        verification_pct
    );

    Ok(())
}

async fn maintenance_errors(client: &Client, api_base: &str) -> Result<(), CliError> {
    let errors: Vec<ScrubErrorResponse> =
        get_json(client, &format!("{api_base}/api/maintenance/scrub-errors")).await?;

    if errors.is_empty() {
        println!("No scrubber errors detected.");
        return Ok(());
    }

    println!(
        "{:<24} {:<18} {:<8} {:<18} STATUS",
        "PACK_ID", "PROVIDER", "SHARD", "LAST_VERIFIED"
    );
    for error in errors {
        println!(
            "{:<24} {:<18} {:<8} {:<18} {}",
            truncate(&error.pack_id, 24),
            error.provider,
            error.shard_index,
            error
                .last_verified_at
                .map(format_timestamp)
                .unwrap_or_else(|| "never".to_string()),
            error.status.unwrap_or_else(|| "UNKNOWN".to_string())
        );
    }

    Ok(())
}

async fn restore_from_cloud() -> Result<(), CliError> {
    let output_db_path = env::var("OMNIDRIVE_DB_PATH")
        .map(PathBuf::from)
        .or_else(|_| {
            env::var("OMNIDRIVE_DB_URL").map(|db_url| {
                sqlite_db_file_path(&db_url).unwrap_or_else(|| parse_db_url_to_path(db_url))
            })
        })
        .unwrap_or_else(|_| {
            RuntimePaths::detect()
                .db_file_path
                .unwrap_or_else(|| PathBuf::from("omnidrive.db"))
        });

    eprint!("Master Password: ");
    let passphrase = rpassword::read_password()
        .map_err(|err| CliError::Api(format!("failed to read password: {err}")))?;

    let provider_manager = MetadataBackupProviderManager::from_env()
        .await
        .map_err(|err| CliError::Api(format!("failed to initialize recovery providers: {err}")))?;

    restore_metadata_from_cloud(&provider_manager, &passphrase, &output_db_path)
        .await
        .map_err(|err| CliError::Api(format!("restore failed: {err}")))?;

    println!("Restored metadata database to {}", output_db_path.display());
    Ok(())
}

fn register_service_autostart() -> Result<(), CliError> {
    let command = autostart::default_current_user_autostart_command()
        .map_err(|err| CliError::Api(format!("autostart command resolution failed: {err}")))?;
    autostart::register_current_user_autostart(&command)
        .map_err(|err| CliError::Api(format!("autostart registration failed: {err}")))?;
    println!("Registered OmniDrive daemon autostart for current user.");
    println!("Command: {}", command);
    Ok(())
}

fn unregister_service_autostart() -> Result<(), CliError> {
    autostart::unregister_current_user_autostart()
        .map_err(|err| CliError::Api(format!("autostart unregistration failed: {err}")))?;
    println!("Unregistered OmniDrive daemon autostart for current user.");
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

fn parse_db_url_to_path(db_url: String) -> PathBuf {
    db_url
        .strip_prefix("sqlite://")
        .or_else(|| db_url.strip_prefix("sqlite:"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(db_url))
}

fn format_timestamp(timestamp_ms: i64) -> String {
    format!("{timestamp_ms} ms since epoch")
}

fn truncate(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        value.to_string()
    } else {
        format!("{}...", &value[..max_len.saturating_sub(3)])
    }
}

fn sync_marker(pin_state: Option<i64>, hydration_state: Option<i64>) -> String {
    match (pin_state.unwrap_or(0), hydration_state.unwrap_or(0)) {
        (1, 1) => "[PH]".to_string(),
        (1, _) => "[P]".to_string(),
        (0, 1) => "[H]".to_string(),
        _ => "[O]".to_string(),
    }
}
