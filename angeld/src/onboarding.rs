#![allow(dead_code)]

use crate::cloud_guard::{self, GuardOperation};
use crate::config::AppConfig;
use crate::disaster_recovery::{self, MetadataBackupProviderManager};
use crate::runtime_paths::RuntimePaths;
use crate::uploader::{KNOWN_PROVIDERS, ProviderConfig};
use crate::{db, db::ProviderConfigRecord};
use aws_config::Region;
use aws_config::timeout::TimeoutConfig;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Credentials;
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::primitives::ByteStream;
use reqwest::Url;
use serde::Serialize;
use sqlx::SqlitePool;
use std::env;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;
use tokio::net::{TcpStream, lookup_host};
use tokio::time::timeout;
use tracing::info;

pub const SYSTEM_CONFIG_ONBOARDING_STATE: &str = "onboarding_state";
pub const SYSTEM_CONFIG_ONBOARDING_MODE: &str = "onboarding_mode";
pub const SYSTEM_CONFIG_LAST_ONBOARDING_STEP: &str = "last_onboarding_step";
pub const SYSTEM_CONFIG_DRAFT_ENV_DETECTED: &str = "draft_env_detected";
pub const SYSTEM_CONFIG_CLOUD_ENABLED: &str = "cloud_enabled";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnboardingState {
    Initial,
    InProgress,
    Completed,
}

impl OnboardingState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "INITIAL",
            Self::InProgress => "IN_PROGRESS",
            Self::Completed => "COMPLETED",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "IN_PROGRESS" => Self::InProgress,
            "COMPLETED" => Self::Completed,
            _ => Self::Initial,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnboardingMode {
    LocalOnly,
    CloudEnabled,
    JoinExisting,
}

impl OnboardingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalOnly => "LOCAL_ONLY",
            Self::CloudEnabled => "CLOUD_ENABLED",
            Self::JoinExisting => "JOIN_EXISTING",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "CLOUD_ENABLED" => Self::CloudEnabled,
            "JOIN_EXISTING" => Self::JoinExisting,
            _ => Self::LocalOnly,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderDraft {
    pub provider_name: String,
    pub endpoint: Option<String>,
    pub region: Option<String>,
    pub bucket: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub force_path_style: bool,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSecretMaterial {
    pub access_key_id: String,
    pub secret_access_key: String,
}

pub(crate) type FullProviderSetup = ProviderConfig;

#[derive(Clone, Debug, Serialize)]
pub struct ValidationReport {
    pub status: String,
    pub message: String,
    pub last_run: i64,
    pub provider_name: String,
    pub endpoint_reachable: bool,
    pub authenticated: bool,
    pub list_objects_ok: bool,
    pub put_object_ok: bool,
    pub delete_object_ok: bool,
    pub error_kind: Option<String>,
}

#[derive(Debug)]
pub enum OnboardingSecretError {
    EmptySecret(&'static str),
    Platform(String),
}

impl fmt::Display for OnboardingSecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySecret(field) => write!(f, "secret field {field} cannot be empty"),
            Self::Platform(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for OnboardingSecretError {}

#[derive(Debug)]
pub enum ProviderError {
    MissingProviderConfig(String),
    MissingSecrets(String),
    InvalidCredentials(String),
    BucketNotFound(String),
    AccessDenied(String),
    EndpointUnreachable(String),
    ClockSkew(String),
    Io(std::io::Error),
    Url(String),
    Aws(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProviderConfig(message)
            | Self::MissingSecrets(message)
            | Self::InvalidCredentials(message)
            | Self::BucketNotFound(message)
            | Self::AccessDenied(message)
            | Self::EndpointUnreachable(message)
            | Self::ClockSkew(message)
            | Self::Url(message)
            | Self::Aws(message) => write!(f, "{message}"),
            Self::Io(err) => write!(f, "i/o error: {err}"),
        }
    }
}

impl std::error::Error for ProviderError {}

impl From<std::io::Error> for ProviderError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug)]
pub enum OnboardingInitError {
    Db(sqlx::Error),
    Secret(OnboardingSecretError),
}

#[derive(Clone, Debug, Serialize)]
pub struct VaultRestoreReport {
    pub status: String,
    pub message: String,
    pub last_run: i64,
    pub provider_name: String,
    pub vault_id: String,
    pub restored_inodes: i64,
    pub restored_revisions: i64,
}

#[derive(Debug)]
pub enum RestoreError {
    MissingProviderConfig(String),
    IncorrectPassphrase(String),
    MetadataNotFound(String),
    NetworkError(String),
    SnapshotApply(String),
    Runtime(String),
    Io(std::io::Error),
}

impl RestoreError {
    pub fn human_readable_reason(&self) -> &'static str {
        match self {
            Self::IncorrectPassphrase(_) => {
                "Niepoprawne hasło Skarbca."
            }
            Self::MetadataNotFound(_) => {
                "Nie znaleziono kopii metadanych dla wybranego dostawcy. Najpierw prześlij metadane z urządzenia głównego."
            }
            Self::NetworkError(_) => {
                "OmniDrive nie mógł połączyć się z wybranym dostawcą podczas pobierania metadanych."
            }
            Self::MissingProviderConfig(_) => {
                "Wybrany dostawca nie jest skonfigurowany z poprawnymi danymi dostępowymi na tym urządzeniu."
            }
            Self::SnapshotApply(_) | Self::Runtime(_) | Self::Io(_) => {
                "OmniDrive nie mógł bezpiecznie zastosować odtworzonej migawki Skarbca na tym urządzeniu."
            }
        }
    }
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProviderConfig(message)
            | Self::IncorrectPassphrase(message)
            | Self::MetadataNotFound(message)
            | Self::NetworkError(message)
            | Self::SnapshotApply(message)
            | Self::Runtime(message) => write!(f, "{message}"),
            Self::Io(err) => write!(f, "i/o error: {err}"),
        }
    }
}

impl std::error::Error for RestoreError {}

impl From<std::io::Error> for RestoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl fmt::Display for OnboardingInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Db(err) => write!(f, "sqlite error: {err}"),
            Self::Secret(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for OnboardingInitError {}

impl From<sqlx::Error> for OnboardingInitError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

impl From<OnboardingSecretError> for OnboardingInitError {
    fn from(value: OnboardingSecretError) -> Self {
        Self::Secret(value)
    }
}

impl ProviderDraft {
    pub fn is_complete(&self) -> bool {
        self.endpoint
            .as_ref()
            .is_some_and(|value| !value.is_empty())
            && self.region.as_ref().is_some_and(|value| !value.is_empty())
            && self.bucket.as_ref().is_some_and(|value| !value.is_empty())
            && self
                .access_key_id
                .as_ref()
                .is_some_and(|value| !value.is_empty())
            && self
                .secret_access_key
                .as_ref()
                .is_some_and(|value| !value.is_empty())
    }
}

pub fn detect_env_provider_drafts() -> Vec<ProviderDraft> {
    let _ = dotenvy::dotenv();

    [
        (
            "cloudflare-r2",
            "OMNIDRIVE_R2_ENDPOINT",
            "OMNIDRIVE_R2_REGION",
            "auto",
            "OMNIDRIVE_R2_BUCKET",
            "OMNIDRIVE_R2_ACCESS_KEY_ID",
            "OMNIDRIVE_R2_SECRET_ACCESS_KEY",
            "OMNIDRIVE_R2_FORCE_PATH_STYLE",
        ),
        (
            "backblaze-b2",
            "OMNIDRIVE_B2_ENDPOINT",
            "OMNIDRIVE_B2_REGION",
            "eu-central-003",
            "OMNIDRIVE_B2_BUCKET",
            "OMNIDRIVE_B2_ACCESS_KEY_ID",
            "OMNIDRIVE_B2_SECRET_ACCESS_KEY",
            "OMNIDRIVE_B2_FORCE_PATH_STYLE",
        ),
        (
            "scaleway",
            "OMNIDRIVE_SCALEWAY_ENDPOINT",
            "OMNIDRIVE_SCALEWAY_REGION",
            "pl-waw",
            "OMNIDRIVE_SCALEWAY_BUCKET",
            "OMNIDRIVE_SCALEWAY_ACCESS_KEY_ID",
            "OMNIDRIVE_SCALEWAY_SECRET_ACCESS_KEY",
            "OMNIDRIVE_SCALEWAY_FORCE_PATH_STYLE",
        ),
    ]
    .into_iter()
    .filter_map(
        |(
            provider_name,
            endpoint_key,
            region_key,
            default_region,
            bucket_key,
            access_key_key,
            secret_key_key,
            force_path_style_key,
        )| {
            let endpoint = env_value(endpoint_key);
            let bucket = env_value(bucket_key);
            let access_key_id = env_value(access_key_key);
            let secret_access_key = env_value(secret_key_key);
            let region = env_value(region_key).or_else(|| Some(default_region.to_string()));
            let force_path_style = env_flag(force_path_style_key);

            let any_present = endpoint.is_some()
                || bucket.is_some()
                || access_key_id.is_some()
                || secret_access_key.is_some();
            if !any_present {
                return None;
            }

            Some(ProviderDraft {
                provider_name: provider_name.to_string(),
                endpoint,
                region,
                bucket,
                access_key_id,
                secret_access_key,
                force_path_style,
                source: ".env".to_string(),
            })
        },
    )
    .collect()
}

pub(crate) fn provider_config_from_env(provider_name: &str) -> Option<ProviderConfig> {
    let _ = dotenvy::dotenv();
    match provider_name {
        "cloudflare-r2" => ProviderConfig::from_r2_env().ok(),
        "backblaze-b2" => ProviderConfig::from_b2_env().ok(),
        "scaleway" => ProviderConfig::from_scaleway_env().ok(),
        _ => None,
    }
}

/// Reset onboarding state back to initial (wizard will reappear).
/// Provider configs and secrets are preserved so the user does not have to re-enter them.
pub async fn reset_onboarding(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    db::set_system_config_value(pool, SYSTEM_CONFIG_ONBOARDING_STATE, OnboardingState::Initial.as_str()).await?;
    db::set_system_config_value(pool, SYSTEM_CONFIG_ONBOARDING_MODE, OnboardingMode::LocalOnly.as_str()).await?;
    db::set_system_config_value(pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP, "welcome").await?;
    db::set_system_config_value(pool, SYSTEM_CONFIG_CLOUD_ENABLED, "0").await?;
    tracing::info!("[ONBOARDING] onboarding state has been reset — wizard will reappear on next dashboard load");
    Ok(())
}

pub async fn initialize_onboarding_persistence(
    pool: &SqlitePool,
) -> Result<(), OnboardingInitError> {
    ensure_onboarding_defaults(pool).await?;

    let onboarding_completed = db::get_system_config_value(pool, SYSTEM_CONFIG_ONBOARDING_STATE)
        .await?
        .is_some_and(|value| OnboardingState::from_str(&value) == OnboardingState::Completed);

    if !onboarding_completed {
        sync_env_provider_drafts_to_db(pool).await?;
    }

    Ok(())
}

pub async fn sync_env_provider_drafts_to_db(
    pool: &SqlitePool,
) -> Result<usize, OnboardingInitError> {
    let drafts = detect_env_provider_drafts();
    db::set_system_config_value(
        pool,
        SYSTEM_CONFIG_DRAFT_ENV_DETECTED,
        if drafts.is_empty() { "0" } else { "1" },
    )
    .await?;

    let mut imported = 0usize;
    for draft in drafts {
        if should_import_draft(
            db::get_provider_config(pool, &draft.provider_name)
                .await?
                .as_ref(),
        ) {
            db::upsert_provider_config(
                pool,
                &draft.provider_name,
                draft.endpoint.as_deref().unwrap_or(""),
                draft.region.as_deref().unwrap_or(""),
                draft.bucket.as_deref().unwrap_or(""),
                draft.force_path_style,
                false,
                Some(&draft.source),
                None,
                None,
                None,
            )
            .await?;

            if let (Some(access_key_id), Some(secret_access_key)) = (
                draft.access_key_id.as_deref(),
                draft.secret_access_key.as_deref(),
            ) {
                let (sealed_access_key_id, sealed_secret_access_key) =
                    seal_provider_secrets(access_key_id, secret_access_key)?;
                db::upsert_provider_secret(
                    pool,
                    &draft.provider_name,
                    &sealed_access_key_id,
                    &sealed_secret_access_key,
                )
                .await?;
            }

            imported += 1;
        }
    }

    Ok(imported)
}

pub async fn validate_persisted_provider_connection(
    pool: &SqlitePool,
    provider_name: &str,
) -> Result<ValidationReport, ProviderError> {
    let config = load_provider_config_from_onboarding_db(pool, provider_name).await?;
    let secrets = ProviderSecretMaterial {
        access_key_id: config.access_key_id.clone(),
        secret_access_key: config.secret_access_key.clone(),
    };
    validate_provider_connection(pool, config, secrets).await
}

pub(crate) async fn load_provider_config_from_onboarding_db(
    pool: &SqlitePool,
    provider_name: &str,
) -> Result<ProviderConfig, ProviderError> {
    let config_record = db::get_provider_config(pool, provider_name)
        .await
        .map_err(|err| ProviderError::Aws(format!("nie udało się wczytać konfiguracji dostawcy: {err}")))?
        .ok_or_else(|| {
            ProviderError::MissingProviderConfig(format!(
                "brak konfiguracji dostawcy {provider_name}"
            ))
        })?;
    let secret_record = db::get_provider_secret(pool, provider_name)
        .await
        .map_err(|err| ProviderError::Aws(format!("nie udało się wczytać sekretu dostawcy: {err}")))?
        .ok_or_else(|| {
            ProviderError::MissingSecrets(format!("brak sekretu dostawcy {provider_name}"))
        })?;
    let secrets = unseal_provider_secrets(
        &secret_record.access_key_id_ciphertext,
        &secret_record.secret_access_key_ciphertext,
    )
    .map_err(|err| ProviderError::MissingSecrets(err.to_string()))?;
    Ok(provider_config_from_record(&config_record, &secrets))
}

pub(crate) async fn get_active_provider_configs(
    pool: &SqlitePool,
) -> Result<Vec<FullProviderSetup>, ProviderError> {
    let records = db::list_provider_configs(pool)
        .await
        .map_err(|err| ProviderError::Aws(format!("nie udało się pobrać listy konfiguracji dostawców: {err}")))?;
    let mut configs = Vec::new();

    for record in records {
        if record.enabled == 0 {
            continue;
        }
        if !KNOWN_PROVIDERS.contains(&record.provider_name.as_str()) {
            continue;
        }

        let config = load_provider_config_from_onboarding_db(pool, &record.provider_name).await?;
        configs.push(config);
    }

    Ok(configs)
}

pub(crate) async fn validate_provider_connection(
    pool: &SqlitePool,
    config: ProviderConfig,
    secrets: ProviderSecretMaterial,
) -> Result<ValidationReport, ProviderError> {
    info!(
        "[ONBOARDING] Testing connection to {} at {}",
        config.provider_name, config.endpoint
    );

    let endpoint_reachable = probe_endpoint_reachability(&config.endpoint).await?;
    info!(
        "[ONBOARDING] Reachability probe for {} succeeded",
        config.provider_name
    );
    let client = build_validation_client(&config).await;

    match cloud_guard::current_decision(
        pool,
        GuardOperation::Read {
            count: 1,
            estimated_egress_bytes: 0,
        },
    )
    .await
    .map_err(|err| ProviderError::Aws(err.to_string()))?
    {
        cloud_guard::GuardDecision::Allowed => {}
        cloud_guard::GuardDecision::DryRun { message }
        | cloud_guard::GuardDecision::Suspended { reason: message }
        | cloud_guard::GuardDecision::QuotaExceeded { reason: message } => {
            return Err(ProviderError::AccessDenied(message));
        }
    }

    client
        .head_bucket()
        .bucket(&config.bucket)
        .send()
        .await
        .map_err(|err| classify_provider_error("authentication", &config, &err))?;
    info!(
        "[ONBOARDING] Authentication probe for {} succeeded",
        config.provider_name
    );

    match cloud_guard::current_decision(
        pool,
        GuardOperation::Read {
            count: 1,
            estimated_egress_bytes: 0,
        },
    )
    .await
    .map_err(|err| ProviderError::Aws(err.to_string()))?
    {
        cloud_guard::GuardDecision::Allowed => {}
        cloud_guard::GuardDecision::DryRun { message }
        | cloud_guard::GuardDecision::Suspended { reason: message }
        | cloud_guard::GuardDecision::QuotaExceeded { reason: message } => {
            return Err(ProviderError::AccessDenied(message));
        }
    }

    client
        .list_objects_v2()
        .bucket(&config.bucket)
        .max_keys(1)
        .send()
        .await
        .map_err(|err| classify_provider_error("list", &config, &err))?;
    info!(
        "[ONBOARDING] ListObjects probe for {} succeeded",
        config.provider_name
    );

    let probe_key = format!(
        ".omnidrive_probe/{}_{}",
        config.provider_name,
        unix_timestamp_millis()
    );
    match cloud_guard::current_decision(pool, GuardOperation::Write { count: 1 })
        .await
        .map_err(|err| ProviderError::Aws(err.to_string()))?
    {
        cloud_guard::GuardDecision::Allowed => {}
        cloud_guard::GuardDecision::DryRun { message }
        | cloud_guard::GuardDecision::Suspended { reason: message }
        | cloud_guard::GuardDecision::QuotaExceeded { reason: message } => {
            return Err(ProviderError::AccessDenied(message));
        }
    }

    client
        .put_object()
        .bucket(&config.bucket)
        .key(&probe_key)
        .body(ByteStream::from(vec![]))
        .send()
        .await
        .map_err(|err| classify_provider_error("put", &config, &err))?;
    info!(
        "[ONBOARDING] PutObject probe for {} succeeded",
        config.provider_name
    );

    match cloud_guard::current_decision(pool, GuardOperation::Write { count: 1 })
        .await
        .map_err(|err| ProviderError::Aws(err.to_string()))?
    {
        cloud_guard::GuardDecision::Allowed => {}
        cloud_guard::GuardDecision::DryRun { message }
        | cloud_guard::GuardDecision::Suspended { reason: message }
        | cloud_guard::GuardDecision::QuotaExceeded { reason: message } => {
            return Err(ProviderError::AccessDenied(message));
        }
    }

    client
        .delete_object()
        .bucket(&config.bucket)
        .key(&probe_key)
        .send()
        .await
        .map_err(|err| classify_provider_error("delete", &config, &err))?;
    info!(
        "[ONBOARDING] DeleteObject probe for {} succeeded",
        config.provider_name
    );

    let _ = secrets;

    info!(
        "[ONBOARDING] Connection test for {} completed successfully",
        config.provider_name
    );

    Ok(ValidationReport {
        status: "OK".to_string(),
        message: format!(
            "Połączenie zweryfikowane pomyślnie dla {} (bucket: {}).",
            config.provider_name, config.bucket
        ),
        last_run: unix_timestamp_millis(),
        provider_name: config.provider_name.to_string(),
        endpoint_reachable,
        authenticated: true,
        list_objects_ok: true,
        put_object_ok: true,
        delete_object_ok: true,
        error_kind: None,
    })
}

pub async fn perform_vault_restore(
    pool: &SqlitePool,
    runtime_paths: &RuntimePaths,
    passphrase: &str,
    provider_id: &str,
) -> Result<VaultRestoreReport, RestoreError> {
    let provider_id = provider_id.trim();
    if provider_id.is_empty() {
        return Err(RestoreError::MissingProviderConfig(
            "provider_id cannot be empty".to_string(),
        ));
    }
    if passphrase.trim().is_empty() {
        return Err(RestoreError::IncorrectPassphrase(
            "passphrase cannot be empty".to_string(),
        ));
    }

    let staging_path = restore_staging_path(runtime_paths);
    if let Some(parent) = staging_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    if fs::try_exists(&staging_path).await.unwrap_or(false) {
        let _ = fs::remove_file(&staging_path).await;
    }

    let provider_manager = MetadataBackupProviderManager::from_onboarding_db(pool, provider_id)
        .await
        .map_err(|err| map_restore_bootstrap_error(provider_id, err))?;

    let restore_result = disaster_recovery::restore_metadata_from_cloud(
        &provider_manager,
        passphrase,
        &staging_path,
    )
    .await;

    if let Err(err) = restore_result {
        let _ = fs::remove_file(&staging_path).await;
        return Err(map_restore_download_error(provider_id, err));
    }

    let apply_result = db::graft_restored_metadata_snapshot(pool, &staging_path)
        .await
        .map_err(|err| {
            RestoreError::SnapshotApply(format!(
                "failed to apply restored metadata snapshot from {}: {err}",
                staging_path.display()
            ))
        });
    let _ = fs::remove_file(&staging_path).await;
    let applied = apply_result?;

    info!(
        "[RESTORE] Vault ID grafted successfully: {}",
        applied.vault_id
    );

    Ok(VaultRestoreReport {
        status: "OK".to_string(),
        message: format!(
            "Vault metadata restored from {}. {} inode(s) and {} revision(s) were imported.",
            provider_id, applied.restored_inodes, applied.restored_revisions
        ),
        last_run: unix_timestamp_millis(),
        provider_name: provider_id.to_string(),
        vault_id: applied.vault_id,
        restored_inodes: applied.restored_inodes,
        restored_revisions: applied.restored_revisions,
    })
}

pub async fn cleanup_stale_uploads(pool: &SqlitePool) -> Result<Vec<String>, ProviderError> {
    let dry_run_active = AppConfig::from_env().dry_run_active
        || db::get_system_config_value(pool, cloud_guard::SYSTEM_CONFIG_DRY_RUN_ACTIVE)
            .await
            .map_err(|err| ProviderError::Aws(format!("failed to load dry-run flag: {err}")))?
            .is_some_and(|value| value == "1");
    if dry_run_active {
        return Ok(vec![
            "[DRY-RUN] Skipping stale multipart upload cleanup.".to_string(),
        ]);
    }
    if !env_flag("OMNIDRIVE_ENABLE_MULTIPART_CLEANUP") {
        return Ok(vec![
            "Skipping stale multipart upload cleanup (set OMNIDRIVE_ENABLE_MULTIPART_CLEANUP=1 to enable).".to_string(),
        ]);
    }

    let mut actions = Vec::new();
    for provider_name in crate::uploader::KNOWN_PROVIDERS {
        let config = match load_provider_config_from_onboarding_db(pool, provider_name).await {
            Ok(config) => config,
            Err(ProviderError::MissingProviderConfig(_))
            | Err(ProviderError::MissingSecrets(_)) => continue,
            Err(err) => return Err(err),
        };
        let client = build_validation_client(&config).await;
        let listed = client
            .list_multipart_uploads()
            .bucket(&config.bucket)
            .max_uploads(100)
            .send()
            .await
            .map_err(|err| classify_provider_error("list_multipart_uploads", &config, &err))?;

        let uploads = listed.uploads();
        if uploads.is_empty() {
            continue;
        }

        for upload in uploads {
            let Some(key) = upload.key() else {
                continue;
            };
            let Some(upload_id) = upload.upload_id() else {
                continue;
            };
            client
                .abort_multipart_upload()
                .bucket(&config.bucket)
                .key(key)
                .upload_id(upload_id)
                .send()
                .await
                .map_err(|err| classify_provider_error("abort_multipart_upload", &config, &err))?;
            actions.push(format!(
                "aborted stale multipart upload {} on {}",
                key, provider_name
            ));
        }
    }

    Ok(actions)
}

pub fn seal_provider_secrets(
    access_key_id: &str,
    secret_access_key: &str,
) -> Result<(Vec<u8>, Vec<u8>), OnboardingSecretError> {
    if access_key_id.trim().is_empty() {
        return Err(OnboardingSecretError::EmptySecret("access_key_id"));
    }
    if secret_access_key.trim().is_empty() {
        return Err(OnboardingSecretError::EmptySecret("secret_access_key"));
    }

    Ok((
        protect_for_current_user(access_key_id.as_bytes())?,
        protect_for_current_user(secret_access_key.as_bytes())?,
    ))
}

pub fn unseal_provider_secrets(
    access_key_id_ciphertext: &[u8],
    secret_access_key_ciphertext: &[u8],
) -> Result<ProviderSecretMaterial, OnboardingSecretError> {
    let access_key_id = String::from_utf8(unprotect_for_current_user(access_key_id_ciphertext)?)
        .map_err(|err| {
            OnboardingSecretError::Platform(format!("invalid UTF-8 in access key material: {err}"))
        })?;
    let secret_access_key = String::from_utf8(unprotect_for_current_user(
        secret_access_key_ciphertext,
    )?)
    .map_err(|err| {
        OnboardingSecretError::Platform(format!("invalid UTF-8 in secret key material: {err}"))
    })?;

    Ok(ProviderSecretMaterial {
        access_key_id,
        secret_access_key,
    })
}

fn env_value(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_flag(key: &str) -> bool {
    matches!(
        env::var(key)
            .ok()
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1" | "true" | "yes")
    )
}

async fn ensure_onboarding_defaults(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    if db::get_system_config_value(pool, SYSTEM_CONFIG_ONBOARDING_STATE)
        .await?
        .is_none()
    {
        db::set_system_config_value(
            pool,
            SYSTEM_CONFIG_ONBOARDING_STATE,
            OnboardingState::Initial.as_str(),
        )
        .await?;
    }

    if db::get_system_config_value(pool, SYSTEM_CONFIG_ONBOARDING_MODE)
        .await?
        .is_none()
    {
        db::set_system_config_value(
            pool,
            SYSTEM_CONFIG_ONBOARDING_MODE,
            OnboardingMode::LocalOnly.as_str(),
        )
        .await?;
    }

    if db::get_system_config_value(pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP)
        .await?
        .is_none()
    {
        db::set_system_config_value(pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP, "welcome").await?;
    }

    if db::get_system_config_value(pool, SYSTEM_CONFIG_CLOUD_ENABLED)
        .await?
        .is_none()
    {
        db::set_system_config_value(pool, SYSTEM_CONFIG_CLOUD_ENABLED, "0").await?;
    }

    if db::get_system_config_value(pool, SYSTEM_CONFIG_DRAFT_ENV_DETECTED)
        .await?
        .is_none()
    {
        db::set_system_config_value(pool, SYSTEM_CONFIG_DRAFT_ENV_DETECTED, "0").await?;
    }

    Ok(())
}

fn should_import_draft(existing: Option<&ProviderConfigRecord>) -> bool {
    match existing {
        None => true,
        Some(record) => record.draft_source.as_deref() == Some(".env"),
    }
}

fn provider_config_from_record(
    record: &ProviderConfigRecord,
    secrets: &ProviderSecretMaterial,
) -> ProviderConfig {
    ProviderConfig {
        provider_name: provider_name_static(&record.provider_name),
        endpoint: record.endpoint.clone(),
        region: record.region.clone(),
        bucket: record.bucket.clone(),
        access_key_id: secrets.access_key_id.clone(),
        secret_access_key: secrets.secret_access_key.clone(),
        force_path_style: record.force_path_style != 0,
    }
}

fn provider_name_static(provider_name: &str) -> &'static str {
    match provider_name {
        "cloudflare-r2" => "cloudflare-r2",
        "backblaze-b2" => "backblaze-b2",
        "scaleway" => "scaleway",
        _ => "unknown",
    }
}

async fn build_validation_client(config: &ProviderConfig) -> Client {
    let timeout_config = TimeoutConfig::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .operation_attempt_timeout(Duration::from_secs(30))
        .operation_timeout(Duration::from_secs(45))
        .build();
    let shared_config = crate::aws_http::load_shared_config(
        Region::new(config.region.clone()),
        timeout_config.clone(),
        config.endpoint.starts_with("http://"),
    )
    .await;

    let s3_config = aws_sdk_s3::config::Builder::from(&shared_config)
        .credentials_provider(Credentials::new(
            config.access_key_id.clone(),
            config.secret_access_key.clone(),
            None,
            None,
            config.provider_name,
        ))
        .endpoint_url(&config.endpoint)
        .region(Region::new(config.region.clone()))
        .timeout_config(timeout_config)
        .force_path_style(config.force_path_style)
        .build();

    Client::from_conf(s3_config)
}

async fn probe_endpoint_reachability(endpoint: &str) -> Result<bool, ProviderError> {
    let url = Url::parse(endpoint)
        .map_err(|err| ProviderError::Url(format!("invalid endpoint URL: {err}")))?;
    let host = url
        .host_str()
        .ok_or_else(|| ProviderError::Url("endpoint URL is missing a hostname".to_string()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| ProviderError::Url("endpoint URL is missing a known port".to_string()))?;

    let addrs: Vec<_> = lookup_host((host, port))
        .await
        .map_err(|err| {
            ProviderError::EndpointUnreachable(format!("failed to resolve {host}:{port}: {err}"))
        })?
        .collect();
    if addrs.is_empty() {
        return Err(ProviderError::EndpointUnreachable(format!(
            "no network addresses resolved for {host}:{port}"
        )));
    }

    timeout(Duration::from_secs(5), TcpStream::connect(addrs[0]))
        .await
        .map_err(|_| {
            ProviderError::EndpointUnreachable(format!("przekroczono czas łączenia z {host}:{port}"))
        })?
        .map_err(|err| {
            ProviderError::EndpointUnreachable(format!("nie udało się połączyć z {host}:{port}: {err}"))
        })?;

    Ok(true)
}

fn classify_provider_error<E>(
    phase: &'static str,
    config: &ProviderConfig,
    err: &E,
) -> ProviderError
where
    E: ProvideErrorMetadata + fmt::Display,
{
    let code = err.code().map(|value| value.to_ascii_lowercase());
    let message = err
        .message()
        .map(|value| value.to_string())
        .unwrap_or_else(|| err.to_string());
    let lower = message.to_ascii_lowercase();

    if lower.contains("skew") || lower.contains("requesttimetooskewed") {
        return ProviderError::ClockSkew(format!(
            "Błędny czas systemowy (clock skew) podczas testu {phase} dla {}: {}",
            config.provider_name, message
        ));
    }

    if matches!(code.as_deref(), Some("nosuchbucket") | Some("notfound"))
        || lower.contains("bucket") && lower.contains("not found")
    {
        return ProviderError::BucketNotFound(format!(
            "Nie znaleziono kontenera (bucket) {} podczas testu {phase} dla {}: {}",
            config.bucket, config.provider_name, message
        ));
    }

    if matches!(
        code.as_deref(),
        Some("invalidaccesskeyid")
            | Some("signaturedoesnotmatch")
            | Some("invalidtoken")
            | Some("expiredtoken")
    ) {
        return ProviderError::InvalidCredentials(format!(
            "Nieprawidłowe dane dostępowe podczas testu {phase} dla {}: {}",
            config.provider_name, message
        ));
    }

    if matches!(code.as_deref(), Some("accessdenied")) {
        return match phase {
            "authentication" => ProviderError::InvalidCredentials(format!(
                "Nieprawidłowe dane dostępowe (authentication) dla {}: {}",
                config.provider_name, message
            )),
            _ => ProviderError::AccessDenied(format!(
                "Brak uprawnień do bucketa podczas testu {phase} dla {}: {}",
                config.provider_name, message
            )),
        };
    }

    if lower.contains("dns")
        || lower.contains("connection refused")
        || lower.contains("timed out")
        || lower.contains("unreachable")
    {
        return ProviderError::EndpointUnreachable(format!(
            "Endpoint jest nieosiągalny podczas testu {phase} dla {} ({}): {}",
            config.provider_name, config.endpoint, message
        ));
    }

    ProviderError::Aws(format!(
        "Test {phase} dla {} nie powiódł się: {}",
        config.provider_name, message
    ))
}

fn restore_staging_path(runtime_paths: &RuntimePaths) -> PathBuf {
    runtime_paths
        .runtime_base_dir
        .join(format!("restore-staging-{}.db", unix_timestamp_millis()))
}

/// Remove leftover `restore-staging-*.db` files from failed/interrupted restore attempts.
pub async fn cleanup_stale_restore_staging(runtime_paths: &RuntimePaths) {
    let base = &runtime_paths.runtime_base_dir;
    let mut read_dir = match tokio::fs::read_dir(base).await {
        Ok(rd) => rd,
        Err(_) => return,
    };
    let mut removed = 0usize;
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("restore-staging-") && name.ends_with(".db") {
            if tokio::fs::remove_file(entry.path()).await.is_ok() {
                removed += 1;
            }
        }
    }
    if removed > 0 {
        tracing::info!("[ONBOARDING] cleaned up {removed} stale restore-staging file(s)");
    }
}

fn map_restore_bootstrap_error(
    provider_id: &str,
    err: disaster_recovery::DisasterRecoveryError,
) -> RestoreError {
    match err {
        disaster_recovery::DisasterRecoveryError::NoConfiguredProviders => {
            RestoreError::MissingProviderConfig(format!(
                "dostawca {provider_id} nie jest skonfigurowany z poprawnymi danymi dostępowymi"
            ))
        }
        other => RestoreError::Runtime(format!(
            "nie udało się zainicjalizować restore dla dostawcy {provider_id}: {other}"
        )),
    }
}

fn map_restore_download_error(
    provider_id: &str,
    err: disaster_recovery::DisasterRecoveryError,
) -> RestoreError {
    match err {
        disaster_recovery::DisasterRecoveryError::BackupDecryptFailed => {
            RestoreError::IncorrectPassphrase(format!(
                "kopia metadanych od {provider_id} nie mogła zostać odszyfrowana podanym hasłem"
            ))
        }
        disaster_recovery::DisasterRecoveryError::DownloadFailed(errors) => {
            let joined = errors.join(" | ");
            let lower = joined.to_ascii_lowercase();
            if lower.contains("nosuchkey")
                || lower.contains("not found")
                || lower.contains("404")
                || lower.contains("latest.db.enc")
            {
                RestoreError::MetadataNotFound(format!(
                    "nie znaleziono kopii metadanych u dostawcy {provider_id}: {joined}"
                ))
            } else if lower.contains("timed out")
                || lower.contains("dns")
                || lower.contains("connection")
                || lower.contains("unreachable")
            {
                RestoreError::NetworkError(format!(
                    "nie udało się pobrać kopii metadanych od dostawcy {provider_id}: {joined}"
                ))
            } else {
                RestoreError::Runtime(format!(
                    "pobieranie restore od dostawcy {provider_id} nie powiodło się: {joined}"
                ))
            }
        }
        other => RestoreError::Runtime(format!("restore od dostawcy {provider_id} nie powiódł się: {other}")),
    }
}

fn unix_timestamp_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(windows)]
fn protect_for_current_user(plaintext: &[u8]) -> Result<Vec<u8>, OnboardingSecretError> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };
    use windows::core::PCWSTR;

    let input = CRYPT_INTEGER_BLOB {
        cbData: plaintext.len() as u32,
        pbData: plaintext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|err| {
            OnboardingSecretError::Platform(format!("CryptProtectData failed: {err}"))
        })?;

        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData as _)));
        Ok(bytes)
    }
}

#[cfg(windows)]
fn unprotect_for_current_user(ciphertext: &[u8]) -> Result<Vec<u8>, OnboardingSecretError> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };
    use windows::core::PWSTR;

    let input = CRYPT_INTEGER_BLOB {
        cbData: ciphertext.len() as u32,
        pbData: ciphertext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let mut description = PWSTR::null();

    unsafe {
        CryptUnprotectData(
            &input,
            Some(&mut description),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|err| {
            OnboardingSecretError::Platform(format!("CryptUnprotectData failed: {err}"))
        })?;

        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData as _)));
        if !description.is_null() {
            let _ = LocalFree(Some(HLOCAL(description.0 as _)));
        }
        Ok(bytes)
    }
}

#[cfg(not(windows))]
fn protect_for_current_user(_plaintext: &[u8]) -> Result<Vec<u8>, OnboardingSecretError> {
    Err(OnboardingSecretError::Platform(
        "provider secret sealing is only implemented on Windows".to_string(),
    ))
}

#[cfg(not(windows))]
fn unprotect_for_current_user(_ciphertext: &[u8]) -> Result<Vec<u8>, OnboardingSecretError> {
    Err(OnboardingSecretError::Platform(
        "provider secret unsealing is only implemented on Windows".to_string(),
    ))
}
