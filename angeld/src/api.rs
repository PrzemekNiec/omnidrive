use crate::cache;
use crate::cloud_guard;
use crate::config::AppConfig;
use crate::db;
use crate::device_identity::ensure_local_device_identity;
use crate::diagnostics::{DaemonDiagnostics, WorkerKind, WorkerStatus};
use crate::disaster_recovery;
use crate::downloader::Downloader;
use crate::onboarding::{
    OnboardingMode, OnboardingState, SYSTEM_CONFIG_CLOUD_ENABLED, SYSTEM_CONFIG_DRAFT_ENV_DETECTED,
    SYSTEM_CONFIG_LAST_ONBOARDING_STEP, SYSTEM_CONFIG_ONBOARDING_MODE,
    SYSTEM_CONFIG_ONBOARDING_STATE, ValidationReport, VaultRestoreReport, cleanup_stale_uploads,
    perform_vault_restore, seal_provider_secrets, validate_persisted_provider_connection,
};
use crate::peer;
use crate::repair::{self, RepairError};
use crate::runtime_paths::RuntimePaths;
use crate::scrubber;
use crate::shell_state;
use crate::smart_sync;
use crate::uploader::KNOWN_PROVIDERS;
use crate::vault::{VaultError, VaultKeyStore};
use crate::virtual_drive;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use tracing::{error, info, warn};

#[derive(Clone)]
struct ApiState {
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
    diagnostics: Arc<DaemonDiagnostics>,
    downloader: Option<Arc<Downloader>>,
    runtime_reload_tx: Option<watch::Sender<u64>>,
}

pub struct ApiServer {
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
    diagnostics: Arc<DaemonDiagnostics>,
    downloader: Option<Arc<Downloader>>,
    runtime_reload_tx: Option<watch::Sender<u64>>,
    bind_addr: SocketAddr,
}

#[derive(Debug)]
pub enum ApiError {
    InvalidBindAddress(String),
    Io(std::io::Error),
}

#[derive(Serialize)]
struct TransferResponse {
    job_id: i64,
    pack_id: String,
    status: String,
    attempts: i64,
    providers: Vec<ProviderTransferResponse>,
}

#[derive(Serialize)]
struct ProviderTransferResponse {
    provider: String,
    status: String,
    attempts: i64,
    last_error: Option<String>,
    bucket: Option<String>,
    object_key: Option<String>,
    etag: Option<String>,
    version_id: Option<String>,
    last_attempt_at: Option<i64>,
    updated_at: Option<i64>,
    completed_at: Option<i64>,
}

#[derive(Serialize)]
struct ProviderHealthResponse {
    provider: String,
    connection_status: String,
    last_attempt_status: Option<String>,
    last_attempt_at: Option<i64>,
    last_success_at: Option<i64>,
    last_error: Option<String>,
}

#[derive(Deserialize)]
struct UnlockRequest {
    passphrase: String,
}

#[derive(Serialize)]
struct UnlockResponse {
    status: String,
    initialized: bool,
}

#[derive(Serialize)]
struct VaultHealthResponse {
    total_packs: i64,
    healthy_packs: i64,
    degraded_packs: i64,
    unreadable_packs: i64,
}

#[derive(Serialize)]
struct DeleteFileResponse {
    inode_id: i64,
    deleted: bool,
}

#[derive(Serialize)]
struct FileEntryResponse {
    inode_id: i64,
    path: String,
    size: i64,
    current_revision_id: Option<i64>,
    current_revision_created_at: Option<i64>,
    smart_sync_pin_state: Option<i64>,
    smart_sync_hydration_state: Option<i64>,
}

#[derive(Serialize)]
struct FileRevisionResponse {
    revision_id: i64,
    inode_id: i64,
    created_at: i64,
    size: i64,
    is_current: bool,
    immutable_until: Option<i64>,
    device_id: Option<String>,
    parent_revision_id: Option<i64>,
    origin: String,
    conflict_reason: Option<String>,
}

#[derive(Serialize)]
struct RestoreRevisionResponse {
    inode_id: i64,
    revision_id: i64,
    restored: bool,
    conflict_copy_inode_id: Option<i64>,
    conflict_copy_revision_id: Option<i64>,
    conflict_copy_name: Option<String>,
}

#[derive(Serialize)]
struct ConflictCopyResponse {
    inode_id: i64,
    source_revision_id: i64,
    conflict_copy_inode_id: i64,
    conflict_copy_revision_id: i64,
    conflict_copy_name: String,
    conflict_id: i64,
}

#[derive(Serialize)]
struct QuotaResponse {
    max_physical_bytes_per_provider: u64,
    providers: Vec<ProviderQuotaResponse>,
}

#[derive(Serialize)]
struct CacheStatusResponse {
    total_entries: i64,
    total_bytes: i64,
    max_bytes: u64,
    prefetched_entries: i64,
    hit_count: u64,
    miss_count: u64,
}

#[derive(Serialize)]
struct ProviderQuotaResponse {
    provider: String,
    used_physical_bytes: u64,
}

#[derive(Serialize)]
struct SmartSyncStatusResponse {
    inode_id: i64,
    revision_id: i64,
    pin_state: i64,
    hydration_state: i64,
}

#[derive(Serialize)]
struct SmartSyncActionResponse {
    inode_id: i64,
    pin_state: i64,
    hydration_state: i64,
}

#[derive(Deserialize)]
struct FilesystemPolicyRequest {
    path: String,
    policy_type: String,
}

#[derive(Deserialize)]
struct FilesystemPathRequest {
    path: String,
}

#[derive(Serialize)]
struct FilesystemPolicyResponse {
    inode_id: i64,
    path: String,
    policy_type: String,
    repair_reconciliation_scheduled: bool,
}

#[derive(Deserialize)]
struct SnapshotLocalRequest {
    output_path: String,
}

#[derive(Serialize)]
struct SnapshotLocalResponse {
    output_path: String,
    created: bool,
}

#[derive(Serialize)]
struct RecoveryStatusResponse {
    last_successful_backup: Option<i64>,
    recent_attempts: Vec<MetadataBackupAttemptResponse>,
}

#[derive(Serialize)]
struct ScrubStatusResponse {
    total_shards: i64,
    verified_shards: i64,
    healthy_shards: i64,
    corrupted_or_missing: i64,
    verified_light_shards: i64,
    verified_deep_shards: i64,
    last_scrub_timestamp: Option<i64>,
}

#[derive(Serialize)]
struct ScrubErrorResponse {
    pack_id: String,
    provider: String,
    shard_index: i64,
    last_verified_at: Option<i64>,
    status: Option<String>,
}

#[derive(Serialize)]
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

#[derive(Serialize)]
struct OnboardingProviderStatusResponse {
    provider_name: String,
    endpoint: String,
    region: String,
    bucket: String,
    force_path_style: bool,
    enabled: bool,
    draft_source: Option<String>,
    last_test_status: Option<String>,
    last_test_error: Option<String>,
    last_test_at: Option<i64>,
    access_key_status: String,
    secret_key_status: String,
}

#[derive(Serialize)]
struct OnboardingStatusResponse {
    onboarding_state: String,
    onboarding_mode: String,
    current_step: String,
    draft_env_detected: bool,
    cloud_enabled: bool,
    device_name: Option<String>,
    device_id: Option<String>,
    providers: Vec<OnboardingProviderStatusResponse>,
}

#[derive(Deserialize)]
struct SetupIdentityRequest {
    device_name: String,
}

#[derive(Serialize)]
struct SetupIdentityResponse {
    device_id: String,
    device_name: String,
}

#[derive(Deserialize)]
struct SetupProviderRequest {
    provider_name: String,
    endpoint: String,
    region: String,
    bucket: String,
    force_path_style: Option<bool>,
    enabled: Option<bool>,
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
}

#[derive(Serialize)]
struct SetupProviderResponse {
    provider_name: String,
    enabled: bool,
    access_key_status: String,
    secret_key_status: String,
    validation: ValidationReport,
}

#[derive(Deserialize)]
struct JoinExistingRequest {
    passphrase: String,
    provider_id: String,
}

#[derive(Serialize)]
struct JoinExistingResponse {
    onboarding_state: String,
    onboarding_mode: String,
    cloud_enabled: bool,
    restore: VaultRestoreReport,
}

#[derive(Serialize)]
struct CompleteOnboardingResponse {
    onboarding_state: String,
    onboarding_mode: String,
    cloud_enabled: bool,
}

#[derive(Serialize)]
struct DiagnosticsHealthResponse {
    uptime_seconds: u64,
    pending_uploads_queue_size: i64,
    last_upload_error: Option<String>,
    cache_size_bytes: i64,
    cache_hit_count: u64,
    cache_miss_count: u64,
    worker_statuses: DiagnosticsWorkerStatusesResponse,
}

#[derive(Serialize)]
struct DiagnosticsWorkerStatusesResponse {
    uploader: String,
    repair: String,
    scrubber: String,
    gc: String,
    watcher: String,
    metadata_backup: String,
    peer: String,
    api: String,
}

#[derive(Clone, Copy)]
enum MaintenanceLevel {
    Ok,
    Warn,
    Error,
}

impl MaintenanceLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Serialize)]
struct MaintenanceStatus<T>
where
    T: Serialize,
{
    status: String,
    message: String,
    last_run: i64,
    #[serde(flatten)]
    details: T,
}

#[derive(Serialize)]
struct MaintenanceOverviewResponse {
    health: MaintenanceOverviewItem,
    shell: MaintenanceOverviewItem,
    sync_root: MaintenanceOverviewItem,
    backup: MaintenanceOverviewItem,
}

#[derive(Serialize)]
struct MaintenanceOverviewItem {
    status: String,
    message: String,
    last_run: i64,
}

#[derive(Serialize)]
struct StorageCostResponse {
    status: String,
    message: String,
    last_run: i64,
    logical_bytes: u64,
    physical_bytes: u64,
    physical_to_logical_ratio: f64,
    estimated_monthly_cost_usd: f64,
    estimated_provider_bytes_avoided: u64,
    estimated_paranoia_physical_bytes: u64,
    reconcile_backlog_packs: usize,
    orphaned_packs: i64,
    orphaned_physical_bytes: u64,
    gc_candidate_packs: i64,
    cloud_guard_status: String,
    cloud_guard_message: String,
    dry_run_active: bool,
    cloud_suspended: bool,
    cloud_suspend_reason: Option<String>,
    session_read_ops: i64,
    session_write_ops: i64,
    session_egress_bytes: i64,
    daily_read_ops: i64,
    daily_write_ops: i64,
    daily_egress_bytes: i64,
    daily_read_ops_limit: i64,
    daily_write_ops_limit: i64,
    daily_egress_bytes_limit: i64,
    read_quota_percent: f64,
    write_quota_percent: f64,
    egress_quota_percent: f64,
    providers: Vec<StorageCostProviderResponse>,
    storage_modes: Vec<StorageCostModeResponse>,
}

#[derive(Serialize)]
struct StorageCostProviderResponse {
    provider: String,
    used_physical_bytes: u64,
    usage_share_percent: f64,
    estimated_monthly_cost_usd: f64,
    configured_cost_per_gib_month: f64,
}

#[derive(Serialize)]
struct StorageCostModeResponse {
    storage_mode: String,
    active_packs: i64,
    logical_bytes: u64,
    physical_bytes: u64,
    estimated_paranoia_physical_bytes: u64,
    estimated_provider_bytes_avoided: u64,
    estimated_monthly_cost_usd: f64,
}

impl ApiServer {
    pub fn from_env(
        pool: SqlitePool,
        vault_keys: VaultKeyStore,
        diagnostics: Arc<DaemonDiagnostics>,
        downloader: Option<Arc<Downloader>>,
        runtime_reload_tx: Option<watch::Sender<u64>>,
    ) -> Result<Self, ApiError> {
        let _ = dotenvy::dotenv();

        let bind_addr = env::var("OMNIDRIVE_API_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8787".to_string())
            .parse::<SocketAddr>()
            .map_err(|_| ApiError::InvalidBindAddress("OMNIDRIVE_API_BIND".to_string()))?;

        Ok(Self {
            pool,
            vault_keys,
            diagnostics,
            downloader,
            runtime_reload_tx,
            bind_addr,
        })
    }

    pub async fn run(self) -> Result<(), ApiError> {
        let diagnostics = self.diagnostics.clone();
        let state = ApiState {
            pool: self.pool,
            vault_keys: self.vault_keys,
            diagnostics: diagnostics.clone(),
            downloader: self.downloader,
            runtime_reload_tx: self.runtime_reload_tx,
        };
        let app = Router::new()
            .route("/", get(get_index))
            .route("/wizard.js", get(get_wizard_js))
            .route("/api/onboarding/status", get(get_onboarding_status))
            .route(
                "/api/onboarding/bootstrap-local",
                post(post_bootstrap_local),
            )
            .route("/api/onboarding/setup-identity", post(post_setup_identity))
            .route("/api/onboarding/setup-provider", post(post_setup_provider))
            .route("/api/onboarding/join-existing", post(post_join_existing))
            .route("/api/onboarding/complete", post(post_complete_onboarding))
            .route("/api/transfers", get(get_transfers))
            .route("/api/health", get(get_health))
            .route("/api/diagnostics/health", get(get_diagnostics_health))
            .route("/api/diagnostics/shell", get(get_shell_state))
            .route("/api/diagnostics/sync-root", get(get_sync_root_state))
            .route("/api/maintenance/status", get(get_maintenance_status))
            .route(
                "/api/maintenance/diagnostics",
                get(get_maintenance_diagnostics),
            )
            .route("/api/storage/cost", get(get_storage_cost))
            .route("/api/multidevice/status", get(get_multidevice_status))
            .route("/api/health/vault", get(get_vault_health))
            .route("/api/files", get(get_files))
            .route("/api/files/{inode_id}", delete(delete_file))
            .route(
                "/api/files/{inode_id}/sync_status",
                get(get_file_sync_status),
            )
            .route("/api/files/{inode_id}/pin", post(pin_file))
            .route("/api/files/{inode_id}/unpin", post(unpin_file))
            .route("/api/filesystem/set-policy", post(set_filesystem_policy))
            .route("/api/filesystem/pin", post(pin_filesystem_path))
            .route("/api/filesystem/unpin", post(unpin_filesystem_path))
            .route("/api/files/{inode_id}/revisions", get(get_file_revisions))
            .route(
                "/api/files/{inode_id}/revisions/{revision_id}/restore",
                post(restore_file_revision),
            )
            .route(
                "/api/files/{inode_id}/revisions/{revision_id}/materialize-conflict-copy",
                post(materialize_conflict_copy),
            )
            .route("/api/quota", get(get_quota))
            .route("/api/cache/status", get(get_cache_status))
            .route("/api/maintenance/scrub-status", get(get_scrub_status))
            .route("/api/maintenance/scrub-errors", get(get_scrub_errors))
            .route("/api/maintenance/scrub-now", post(post_scrub_now))
            .route("/api/maintenance/repair-now", post(post_repair_now))
            .route("/api/maintenance/reconcile-now", post(post_reconcile_now))
            .route("/api/maintenance/repair-shell", post(post_repair_shell))
            .route(
                "/api/maintenance/repair-sync-root",
                post(post_repair_sync_root),
            )
            .route("/api/recovery/status", get(get_recovery_status))
            .route("/api/recovery/backup-now", post(post_backup_now))
            .route("/api/recovery/snapshot-local", post(post_snapshot_local))
            .route("/api/unlock", post(post_unlock))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(self.bind_addr)
            .await
            .map_err(ApiError::Io)?;
        diagnostics.set_worker_status(WorkerKind::Api, WorkerStatus::Idle);
        info!("api server listening on http://{}", self.bind_addr);

        axum::serve(listener, app).await.map_err(ApiError::Io)
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBindAddress(key) => {
                write!(f, "invalid bind address in environment variable {key}")
            }
            Self::Io(err) => write!(f, "api server i/o error: {err}"),
        }
    }
}

impl std::error::Error for ApiError {}

async fn get_index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn get_wizard_js() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../static/wizard.js"),
    )
}

async fn get_onboarding_status(State(state): State<ApiState>) -> impl IntoResponse {
    match build_onboarding_status_response(&state).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn post_bootstrap_local(State(state): State<ApiState>) -> impl IntoResponse {
    let result = async {
        db::set_system_config_value(
            &state.pool,
            SYSTEM_CONFIG_ONBOARDING_STATE,
            OnboardingState::InProgress.as_str(),
        )
        .await?;
        db::set_system_config_value(
            &state.pool,
            SYSTEM_CONFIG_ONBOARDING_MODE,
            OnboardingMode::LocalOnly.as_str(),
        )
        .await?;
        db::set_system_config_value(&state.pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP, "identity")
            .await?;
        db::set_system_config_value(&state.pool, SYSTEM_CONFIG_CLOUD_ENABLED, "0").await?;
        build_onboarding_status_response(&state).await
    }
    .await;

    match result {
        Ok(response) => {
            match cleanup_stale_uploads(&state.pool).await {
                Ok(actions) => {
                    for action in &actions {
                        info!("[ONBOARDING] complete multipart cleanup: {}", action);
                    }
                }
                Err(err) => {
                    error!("[ONBOARDING] complete multipart cleanup failed: {}", err);
                }
            }
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(err) => internal_server_error(err),
    }
}

async fn post_setup_identity(
    State(state): State<ApiState>,
    Json(request): Json<SetupIdentityRequest>,
) -> impl IntoResponse {
    let device_name = request.device_name.trim();
    if device_name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_device_name",
                "message": "device_name cannot be empty"
            })),
        )
            .into_response();
    }

    let result = async {
        let mut app_config = AppConfig::from_env();
        app_config.device_name_override = Some(device_name.to_string());
        let local_device = ensure_local_device_identity(&state.pool, &app_config).await?;
        db::update_local_device_name(&state.pool, device_name).await?;
        db::set_system_config_value(
            &state.pool,
            SYSTEM_CONFIG_ONBOARDING_STATE,
            OnboardingState::InProgress.as_str(),
        )
        .await?;
        db::set_system_config_value(&state.pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP, "providers")
            .await?;

        Ok::<_, Box<dyn std::error::Error>>(SetupIdentityResponse {
            device_id: local_device.device_id,
            device_name: device_name.to_string(),
        })
    }
    .await;

    match result {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => internal_server_error(io_error(err)),
    }
}

async fn post_setup_provider(
    State(state): State<ApiState>,
    Json(request): Json<SetupProviderRequest>,
) -> impl IntoResponse {
    let provider_name = request.provider_name.trim();
    if !KNOWN_PROVIDERS.contains(&provider_name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "unsupported_provider",
                "provider_name": provider_name,
            })),
        )
            .into_response();
    }

    if request.endpoint.trim().is_empty()
        || request.region.trim().is_empty()
        || request.bucket.trim().is_empty()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_provider_config",
                "message": "endpoint, region i bucket są wymagane"
            })),
        )
            .into_response();
    }

    let result = async {
        db::upsert_provider_config(
            &state.pool,
            provider_name,
            request.endpoint.trim(),
            request.region.trim(),
            request.bucket.trim(),
            request.force_path_style.unwrap_or(false),
            request.enabled.unwrap_or(true),
            None,
            Some("CONFIG_SAVED"),
            None,
            None,
        )
        .await?;

        if let (Some(access_key_id), Some(secret_access_key)) = (
            request.access_key_id.as_deref(),
            request.secret_access_key.as_deref(),
        ) {
            let (sealed_access_key_id, sealed_secret_access_key) =
                seal_provider_secrets(access_key_id, secret_access_key).map_err(io_error)?;
            db::upsert_provider_secret(
                &state.pool,
                provider_name,
                &sealed_access_key_id,
                &sealed_secret_access_key,
            )
            .await?;
        }

        db::set_system_config_value(
            &state.pool,
            SYSTEM_CONFIG_ONBOARDING_STATE,
            OnboardingState::InProgress.as_str(),
        )
        .await?;
        db::set_system_config_value(
            &state.pool,
            SYSTEM_CONFIG_ONBOARDING_MODE,
            OnboardingMode::CloudEnabled.as_str(),
        )
        .await?;
        db::set_system_config_value(&state.pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP, "security")
            .await?;

        let has_secret = db::get_provider_secret(&state.pool, provider_name)
            .await?
            .is_some();
        let validation = validate_persisted_provider_connection(&state.pool, provider_name).await;
        let (last_test_status, last_test_error, last_test_at, validation_report) = match validation
        {
            Ok(report) => (
                Some(report.status.clone()),
                None,
                Some(report.last_run),
                report,
            ),
            Err(err) => {
                let report = ValidationReport {
                    status: "ERROR".to_string(),
                    message: err.to_string(),
                    last_run: unix_timestamp_millis(),
                    provider_name: provider_name.to_string(),
                    endpoint_reachable: !matches!(
                        &err,
                        crate::onboarding::ProviderError::EndpointUnreachable(_)
                    ),
                    authenticated: false,
                    list_objects_ok: false,
                    put_object_ok: false,
                    delete_object_ok: false,
                    error_kind: Some(provider_error_kind(&err).to_string()),
                };
                (
                    Some(report.status.clone()),
                    Some(report.message.clone()),
                    Some(report.last_run),
                    report,
                )
            }
        };
        db::upsert_provider_config(
            &state.pool,
            provider_name,
            request.endpoint.trim(),
            request.region.trim(),
            request.bucket.trim(),
            request.force_path_style.unwrap_or(false),
            request.enabled.unwrap_or(true),
            None,
            last_test_status.as_deref(),
            last_test_error.as_deref(),
            last_test_at,
        )
        .await?;

        Ok::<_, Box<dyn std::error::Error>>(SetupProviderResponse {
            provider_name: provider_name.to_string(),
            enabled: request.enabled.unwrap_or(true),
            access_key_status: if has_secret {
                "SET".to_string()
            } else {
                "MISSING".to_string()
            },
            secret_key_status: if has_secret {
                "SET".to_string()
            } else {
                "MISSING".to_string()
            },
            validation: validation_report,
        })
    }
    .await;

    match result {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => internal_server_error(io_error(err)),
    }
}

async fn post_complete_onboarding(State(state): State<ApiState>) -> impl IntoResponse {
    let result = async {
        let active_provider_configs = crate::onboarding::get_active_provider_configs(&state.pool)
            .await
            .map_err(io_error)?;
        let cloud_enabled = !active_provider_configs.is_empty();
        let onboarding_mode = if cloud_enabled {
            OnboardingMode::CloudEnabled
        } else {
            OnboardingMode::LocalOnly
        };

        db::set_system_config_value(
            &state.pool,
            SYSTEM_CONFIG_ONBOARDING_STATE,
            OnboardingState::Completed.as_str(),
        )
        .await?;
        db::set_system_config_value(
            &state.pool,
            SYSTEM_CONFIG_ONBOARDING_MODE,
            onboarding_mode.as_str(),
        )
        .await?;
        db::set_system_config_value(&state.pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP, "completed")
            .await?;
        db::set_system_config_value(
            &state.pool,
            SYSTEM_CONFIG_CLOUD_ENABLED,
            if cloud_enabled { "1" } else { "0" },
        )
        .await?;

        trigger_runtime_provider_reload(&state, cloud_enabled).await;

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(CompleteOnboardingResponse {
            onboarding_state: OnboardingState::Completed.as_str().to_string(),
            onboarding_mode: onboarding_mode.as_str().to_string(),
            cloud_enabled,
        })
    }
    .await;

    match result {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => internal_server_error(io_error(err)),
    }
}

async fn post_join_existing(
    State(state): State<ApiState>,
    Json(request): Json<JoinExistingRequest>,
) -> impl IntoResponse {
    let provider_id = request.provider_id.trim();
    if !KNOWN_PROVIDERS.contains(&provider_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "unsupported_provider",
                "provider_id": provider_id,
                "human_readable_reason": "Wybierz jednego ze skonfigurowanych dostawców OmniDrive przed dołączeniem do istniejącego Skarbca."
            })),
        )
            .into_response();
    }

    let runtime_paths = RuntimePaths::detect();
    let result = perform_vault_restore(
        &state.pool,
        &runtime_paths,
        &request.passphrase,
        provider_id,
    )
    .await;

    let restore = match result {
        Ok(report) => report,
        Err(err) => {
            let status = match err {
                crate::onboarding::RestoreError::IncorrectPassphrase(_) => StatusCode::BAD_REQUEST,
                crate::onboarding::RestoreError::MetadataNotFound(_) => StatusCode::NOT_FOUND,
                crate::onboarding::RestoreError::NetworkError(_) => StatusCode::BAD_GATEWAY,
                crate::onboarding::RestoreError::MissingProviderConfig(_) => {
                    StatusCode::BAD_REQUEST
                }
                crate::onboarding::RestoreError::SnapshotApply(_)
                | crate::onboarding::RestoreError::Runtime(_)
                | crate::onboarding::RestoreError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };
            return (
                status,
                Json(serde_json::json!({
                    "error": provider_restore_error_kind(&err),
                    "message": err.to_string(),
                    "human_readable_reason": err.human_readable_reason(),
                })),
            )
                .into_response();
        }
    };

    if let Err(err) = finalize_join_existing_runtime(&state, &runtime_paths, provider_id).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "runtime_activation_failed",
                "message": err.to_string(),
                "human_readable_reason": "Metadane Skarbca zostały odtworzone, ale OmniDrive nie mógł poprawnie przełączyć tego urządzenia do trybu sync-root."
            })),
        )
            .into_response();
    }

    let result = async {
        db::set_system_config_value(
            &state.pool,
            SYSTEM_CONFIG_ONBOARDING_STATE,
            OnboardingState::Completed.as_str(),
        )
        .await?;
        db::set_system_config_value(
            &state.pool,
            SYSTEM_CONFIG_ONBOARDING_MODE,
            OnboardingMode::JoinExisting.as_str(),
        )
        .await?;
        db::set_system_config_value(&state.pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP, "completed")
            .await?;
        db::set_system_config_value(&state.pool, SYSTEM_CONFIG_CLOUD_ENABLED, "1").await?;

        trigger_runtime_provider_reload(&state, true).await;

        Ok::<_, sqlx::Error>(JoinExistingResponse {
            onboarding_state: OnboardingState::Completed.as_str().to_string(),
            onboarding_mode: OnboardingMode::JoinExisting.as_str().to_string(),
            cloud_enabled: true,
            restore,
        })
    }
    .await;

    match result {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn trigger_runtime_provider_reload(state: &ApiState, trigger_reconcile: bool) {
    if let Some(tx) = &state.runtime_reload_tx {
        let next_generation = tx.borrow().wrapping_add(1);
        if tx.send(next_generation).is_ok() {
            info!("[RUNTIME] provider reload signal dispatched (generation={next_generation})");
        } else {
            warn!("[RUNTIME] provider reload signal dispatch failed: no active receivers");
        }
    }

    if let Some(downloader) = state.downloader.as_ref() {
        match downloader.reload_active_providers_from_db().await {
            Ok(provider_names) => {
                info!(
                    "[RUNTIME] downloader providers reloaded from DB: [{}]",
                    if provider_names.is_empty() {
                        "none".to_string()
                    } else {
                        provider_names.join(", ")
                    }
                );
            }
            Err(err) => {
                warn!("[RUNTIME] downloader provider reload failed: {err}");
            }
        }
    }

    if trigger_reconcile {
        match repair::RepairWorker::run_reconcile_batch_now(state.pool.clone()).await {
            Ok(report) => {
                info!(
                    "[RUNTIME] post-onboarding reconciliation completed (reconciled_packs={})",
                    report.reconciled_packs
                );
            }
            Err(err) => {
                warn!("[RUNTIME] post-onboarding reconciliation warning: {err}");
            }
        }
    }
}

async fn finalize_join_existing_runtime(
    state: &ApiState,
    runtime_paths: &RuntimePaths,
    provider_id: &str,
) -> Result<(), std::io::Error> {
    let downloader = match state.downloader.clone() {
        Some(existing) => {
            if let Err(err) = existing.reload_active_providers_from_db().await {
                warn!("[RESTORE] downloader runtime reload warning: {err}");
            }
            existing
        }
        None => {
            let provider_config = crate::onboarding::load_provider_config_from_onboarding_db(
                &state.pool,
                provider_id,
            )
            .await
            .map_err(io_error)?;
            Arc::new(
                Downloader::from_provider_configs(
                    state.pool.clone(),
                    state.vault_keys.clone(),
                    runtime_paths.download_spool_dir.clone(),
                    std::time::Duration::from_millis(120_000),
                    vec![provider_config],
                )
                .await
                .map_err(io_error)?,
            )
        }
    };

    smart_sync::install_hydration_runtime(state.pool.clone(), downloader).map_err(io_error)?;

    let repair_report = smart_sync::repair_sync_root(&state.pool, &runtime_paths.sync_root)
        .await
        .map_err(io_error)?;
    for action in &repair_report.actions {
        info!("[RESTORE] sync-root recovery: {}", action);
    }

    smart_sync::project_vault_to_sync_root(&state.pool, &runtime_paths.sync_root)
        .await
        .map_err(io_error)?;
    info!(
        "[RESTORE] Placeholder hydration projected into {}",
        runtime_paths.sync_root.display()
    );

    virtual_drive::hide_sync_root(&runtime_paths.sync_root).map_err(io_error)?;
    let preferred_drive_letter =
        env::var("OMNIDRIVE_DRIVE_LETTER").unwrap_or_else(|_| "O:".to_string());
    let _ = virtual_drive::unmount_virtual_drive(&preferred_drive_letter);
    let drive_letter = virtual_drive::select_mount_drive_letter(&preferred_drive_letter)
        .unwrap_or(preferred_drive_letter.clone());
    virtual_drive::mount_virtual_drive(&drive_letter, &runtime_paths.sync_root)
        .map_err(io_error)?;

    match shell_state::repair_explorer_integration() {
        Ok(report) => {
            for action in &report.actions {
                info!("[RESTORE] shell repair: {}", action);
            }
        }
        Err(err) => {
            error!("[RESTORE] shell repair warning: {}", err);
        }
    }

    Ok(())
}

async fn get_transfers(State(state): State<ApiState>) -> impl IntoResponse {
    match db::list_recent_upload_jobs(&state.pool, 50).await {
        Ok(jobs) => {
            let mut transfers = Vec::with_capacity(jobs.len());

            for job in jobs {
                let targets = match db::get_upload_targets_for_job(&state.pool, job.id).await {
                    Ok(targets) => targets,
                    Err(err) => return internal_server_error(err),
                };

                transfers.push(TransferResponse {
                    job_id: job.id,
                    pack_id: job.pack_id,
                    status: job.status,
                    attempts: job.attempts.unwrap_or(0),
                    providers: targets
                        .into_iter()
                        .map(|target| ProviderTransferResponse {
                            provider: target.provider,
                            status: target.status,
                            attempts: target.attempts.unwrap_or(0),
                            last_error: target.last_error,
                            bucket: target.bucket,
                            object_key: target.object_key,
                            etag: target.etag,
                            version_id: target.version_id,
                            last_attempt_at: target.last_attempt_at,
                            updated_at: target.updated_at,
                            completed_at: target.completed_at,
                        })
                        .collect(),
                });
            }

            (StatusCode::OK, Json(transfers)).into_response()
        }
        Err(err) => internal_server_error(err),
    }
}

async fn get_health(State(state): State<ApiState>) -> impl IntoResponse {
    let mut providers = Vec::with_capacity(KNOWN_PROVIDERS.len());
    let mut latest_by_provider = HashMap::with_capacity(KNOWN_PROVIDERS.len());

    for provider in KNOWN_PROVIDERS {
        match db::get_latest_upload_target_for_provider(&state.pool, provider).await {
            Ok(record) => {
                latest_by_provider.insert(provider, record);
            }
            Err(err) => return internal_server_error(err),
        }
    }

    for provider in KNOWN_PROVIDERS {
        let latest = latest_by_provider.remove(provider).flatten();
        let response = match latest {
            Some(target) => ProviderHealthResponse {
                provider: provider.to_string(),
                connection_status: provider_connection_status(
                    &target.status,
                    target.last_error.is_some(),
                ),
                last_attempt_status: Some(target.status),
                last_attempt_at: target.last_attempt_at.or(target.updated_at),
                last_success_at: target.completed_at,
                last_error: target.last_error,
            },
            None => ProviderHealthResponse {
                provider: provider.to_string(),
                connection_status: "UNKNOWN".to_string(),
                last_attempt_status: None,
                last_attempt_at: None,
                last_success_at: None,
                last_error: None,
            },
        };
        providers.push(response);
    }

    (StatusCode::OK, Json(providers)).into_response()
}

async fn get_diagnostics_health(State(state): State<ApiState>) -> impl IntoResponse {
    match build_diagnostics_health_response(&state).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn get_shell_state() -> impl IntoResponse {
    let response = build_shell_state_response();
    (StatusCode::OK, Json(response)).into_response()
}

async fn get_sync_root_state() -> impl IntoResponse {
    match build_sync_root_state_response() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "ERROR",
                "message": err.to_string(),
                "last_run": unix_timestamp_millis(),
            })),
        )
            .into_response(),
    }
}

async fn get_maintenance_status(State(state): State<ApiState>) -> impl IntoResponse {
    let health = match build_diagnostics_health_response(&state).await {
        Ok(response) => maintenance_overview_item(&response),
        Err(err) => maintenance_error_item(format!("Diagnostyka health nie powiodła się: {err}")),
    };
    let shell = maintenance_overview_item(&build_shell_state_response());
    let sync_root = match build_sync_root_state_response() {
        Ok(response) => maintenance_overview_item(&response),
        Err(err) => maintenance_error_item(format!("Diagnostyka sync-root nie powiodła się: {err}")),
    };
    let backup = match build_recovery_status_response(&state).await {
        Ok(response) => maintenance_overview_item(&response),
        Err(err) => maintenance_error_item(format!("Diagnostyka odzyskiwania nie powiodła się: {err}")),
    };

    (
        StatusCode::OK,
        Json(MaintenanceOverviewResponse {
            health,
            shell,
            sync_root,
            backup,
        }),
    )
        .into_response()
}

async fn get_maintenance_diagnostics(State(state): State<ApiState>) -> impl IntoResponse {
    let health = match build_diagnostics_health_response(&state).await {
        Ok(response) => serde_json::to_value(response).unwrap_or_default(),
        Err(err) => serde_json::json!({
            "status": "ERROR",
            "message": format!("Diagnostyka health nie powiodła się: {err}"),
            "last_run": unix_timestamp_millis(),
        }),
    };

    let shell = serde_json::to_value(build_shell_state_response()).unwrap_or_default();
    let sync_root = match build_sync_root_state_response() {
        Ok(response) => serde_json::to_value(response).unwrap_or_default(),
        Err(err) => serde_json::json!({
            "status": "ERROR",
            "message": format!("Diagnostyka sync-root nie powiodła się: {err}"),
            "last_run": unix_timestamp_millis(),
        }),
    };

    let backup = match build_recovery_status_response(&state).await {
        Ok(response) => serde_json::to_value(response).unwrap_or_default(),
        Err(err) => serde_json::json!({
            "status": "ERROR",
            "message": format!("Diagnostyka odzyskiwania nie powiodła się: {err}"),
            "last_run": unix_timestamp_millis(),
        }),
    };

    let cache = match db::get_cache_status_summary(&state.pool).await {
        Ok(summary) => {
            let config = AppConfig::from_env();
            let runtime = cache::cache_runtime_stats();
            serde_json::json!({
                "status": "OK",
                "message": "Telemetria cache została zebrana pomyślnie.",
                "last_run": unix_timestamp_millis(),
                "total_entries": summary.total_entries,
                "total_bytes": summary.total_bytes,
                "max_bytes": config.max_cache_bytes,
                "prefetched_entries": summary.prefetched_entries,
                "hit_count": runtime.hit_count,
                "miss_count": runtime.miss_count,
            })
        }
        Err(err) => serde_json::json!({
            "status": "ERROR",
            "message": format!("Diagnostyka cache nie powiodła się: {err}"),
            "last_run": unix_timestamp_millis(),
        }),
    };

    let scrub = match db::get_scrub_status_summary(&state.pool).await {
        Ok(summary) => serde_json::json!({
            "status": "OK",
            "message": "Telemetria scrub została zebrana pomyślnie.",
            "last_run": unix_timestamp_millis(),
            "total_shards": summary.total_shards,
            "verified_shards": summary.verified_shards,
            "healthy_shards": summary.healthy_shards,
            "corrupted_or_missing": summary.corrupted_or_missing,
            "verified_light_shards": summary.verified_light_shards,
            "verified_deep_shards": summary.verified_deep_shards,
            "last_scrub_timestamp": summary.last_scrub_timestamp,
        }),
        Err(err) => serde_json::json!({
            "status": "ERROR",
            "message": format!("Diagnostyka scrub nie powiodła się: {err}"),
            "last_run": unix_timestamp_millis(),
        }),
    };

    let vault_health = match db::get_vault_health_summary(&state.pool).await {
        Ok(summary) => serde_json::json!({
            "status": if summary.unreadable_packs > 0 {
                "ERROR"
            } else if summary.degraded_packs > 0 {
                "WARN"
            } else {
                "OK"
            },
            "message": if summary.unreadable_packs > 0 {
                format!("{} pack(s) are unreadable.", summary.unreadable_packs)
            } else if summary.degraded_packs > 0 {
                format!("{} pack(s) are degraded but recoverable.", summary.degraded_packs)
            } else {
                "Wszystkie aktywne pakiety są zdrowe.".to_string()
            },
            "last_run": unix_timestamp_millis(),
            "total_packs": summary.total_packs,
            "healthy_packs": summary.healthy_packs,
            "degraded_packs": summary.degraded_packs,
            "unreadable_packs": summary.unreadable_packs,
        }),
        Err(err) => serde_json::json!({
            "status": "ERROR",
            "message": format!("Diagnostyka kondycji Skarbca nie powiodła się: {err}"),
            "last_run": unix_timestamp_millis(),
        }),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "OK",
            "message": "Migawka diagnostyki utrzymaniowej została zebrana pomyślnie.",
            "last_run": unix_timestamp_millis(),
            "health": health,
            "shell": shell,
            "sync_root": sync_root,
            "backup": backup,
            "cache": cache,
            "scrub": scrub,
            "vault_health": vault_health,
        })),
    )
        .into_response()
}

async fn get_storage_cost(State(state): State<ApiState>) -> impl IntoResponse {
    match build_storage_cost_response(&state).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn get_multidevice_status(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(local_device) = (match db::get_local_device_identity(&state.pool).await {
        Ok(record) => record,
        Err(err) => return internal_server_error(err),
    }) else {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "WARN",
                "message": "Tożsamość lokalnego urządzenia nie została jeszcze zainicjalizowana.",
                "last_run": unix_timestamp_millis(),
                "trusted_peers": [],
                "recent_conflicts": [],
            })),
        )
            .into_response();
    };

    let vault_id = match db::get_vault_params(&state.pool).await {
        Ok(Some(record)) => record.vault_id,
        Ok(None) => "local-vault".to_string(),
        Err(err) => return internal_server_error(err),
    };
    let peer_port = AppConfig::from_env().peer_port;

    match peer::snapshot_multi_device(
        &state.pool,
        &crate::device_identity::LocalDeviceIdentity {
            device_id: local_device.device_id,
            device_name: local_device.device_name,
            peer_token: local_device.peer_token,
            created_at: local_device.created_at,
            updated_at: local_device.updated_at,
        },
        &vault_id,
        peer_port,
    )
    .await
    {
        Ok(snapshot) => (StatusCode::OK, Json(snapshot)).into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn get_vault_health(State(state): State<ApiState>) -> impl IntoResponse {
    match db::get_vault_health_summary(&state.pool).await {
        Ok(summary) => (
            StatusCode::OK,
            Json(VaultHealthResponse {
                total_packs: summary.total_packs,
                healthy_packs: summary.healthy_packs,
                degraded_packs: summary.degraded_packs,
                unreadable_packs: summary.unreadable_packs,
            }),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn post_unlock(
    State(state): State<ApiState>,
    Json(request): Json<UnlockRequest>,
) -> impl IntoResponse {
    match state
        .vault_keys
        .unlock(&state.pool, &request.passphrase)
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(UnlockResponse {
                status: "UNLOCKED".to_string(),
                initialized: result.initialized,
            }),
        )
            .into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "unlock_failed",
                "message": err.to_string()
            })),
        )
            .into_response(),
    }
}

async fn delete_file(
    State(state): State<ApiState>,
    Path(inode_id): Path<i64>,
) -> impl IntoResponse {
    let inode = match db::get_inode_by_id(&state.pool, inode_id).await {
        Ok(inode) => inode,
        Err(err) => return internal_server_error(err),
    };

    let Some(inode) = inode else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "inode_not_found",
                "inode_id": inode_id,
            })),
        )
            .into_response();
    };

    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "inode_not_file",
                "inode_id": inode_id,
                "kind": inode.kind,
            })),
        )
            .into_response();
    }

    if let Err(err) = db::delete_file_chunks(&state.pool, inode_id).await {
        return internal_server_error(err);
    }
    if let Err(err) = db::delete_inode_record(&state.pool, inode_id).await {
        return internal_server_error(err);
    }

    (
        StatusCode::OK,
        Json(DeleteFileResponse {
            inode_id,
            deleted: true,
        }),
    )
        .into_response()
}

async fn get_files(State(state): State<ApiState>) -> impl IntoResponse {
    match db::list_active_files(&state.pool).await {
        Ok(files) => (
            StatusCode::OK,
            Json(
                files
                    .into_iter()
                    .map(|file| FileEntryResponse {
                        inode_id: file.inode_id,
                        path: file.path,
                        size: file.size,
                        current_revision_id: file.current_revision_id,
                        current_revision_created_at: file.current_revision_created_at,
                        smart_sync_pin_state: file.smart_sync_pin_state,
                        smart_sync_hydration_state: file.smart_sync_hydration_state,
                    })
                    .collect::<Vec<_>>(),
            ),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn get_file_sync_status(
    State(state): State<ApiState>,
    Path(inode_id): Path<i64>,
) -> impl IntoResponse {
    match db::get_smart_sync_state(&state.pool, inode_id).await {
        Ok(Some(status)) => (
            StatusCode::OK,
            Json(SmartSyncStatusResponse {
                inode_id: status.inode_id,
                revision_id: status.revision_id,
                pin_state: status.pin_state,
                hydration_state: status.hydration_state,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "smart_sync_state_not_found",
                "inode_id": inode_id,
            })),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn pin_file(State(state): State<ApiState>, Path(inode_id): Path<i64>) -> impl IntoResponse {
    let inode = match db::get_inode_by_id(&state.pool, inode_id).await {
        Ok(inode) => inode,
        Err(err) => return internal_server_error(err),
    };
    let Some(inode) = inode else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "inode_not_found", "inode_id": inode_id })),
        )
            .into_response();
    };
    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "inode_not_file", "inode_id": inode_id })),
        )
            .into_response();
    }

    let sync_root = sync_root_path();
    if let Err(err) = db::set_pin_state(&state.pool, inode_id, 1).await {
        return internal_server_error(err);
    }
    if let Err(err) =
        smart_sync::sync_placeholder_pin_state(&state.pool, &sync_root, inode_id, false).await
    {
        return internal_server_error(err);
    }

    match db::get_smart_sync_state(&state.pool, inode_id).await {
        Ok(Some(status)) => (
            StatusCode::OK,
            Json(SmartSyncActionResponse {
                inode_id: status.inode_id,
                pin_state: status.pin_state,
                hydration_state: status.hydration_state,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": "smart_sync_state_not_found", "inode_id": inode_id }),
            ),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn unpin_file(State(state): State<ApiState>, Path(inode_id): Path<i64>) -> impl IntoResponse {
    let inode = match db::get_inode_by_id(&state.pool, inode_id).await {
        Ok(inode) => inode,
        Err(err) => return internal_server_error(err),
    };
    let Some(inode) = inode else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "inode_not_found", "inode_id": inode_id })),
        )
            .into_response();
    };
    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "inode_not_file", "inode_id": inode_id })),
        )
            .into_response();
    }

    let sync_root = sync_root_path();
    if let Err(err) = db::set_pin_state(&state.pool, inode_id, 0).await {
        return internal_server_error(err);
    }
    if let Err(err) =
        smart_sync::sync_placeholder_pin_state(&state.pool, &sync_root, inode_id, true).await
    {
        return internal_server_error(err);
    }

    match db::get_smart_sync_state(&state.pool, inode_id).await {
        Ok(Some(status)) => (
            StatusCode::OK,
            Json(SmartSyncActionResponse {
                inode_id: status.inode_id,
                pin_state: status.pin_state,
                hydration_state: status.hydration_state,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": "smart_sync_state_not_found", "inode_id": inode_id }),
            ),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn set_filesystem_policy(
    State(state): State<ApiState>,
    Json(request): Json<FilesystemPolicyRequest>,
) -> impl IntoResponse {
    let policy_type = match normalize_policy_type(&request.policy_type) {
        Some(policy_type) => policy_type,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_policy_type",
                    "policy_type": request.policy_type,
                })),
            )
                .into_response();
        }
    };

    let (inode_id, logical_path, inode) =
        match resolve_filesystem_request_target(&state.pool, &request.path).await {
            Ok(target) => target,
            Err(response) => return response,
        };

    if let Err(err) =
        db::set_sync_policy_type_for_path(&state.pool, &logical_path, policy_type).await
    {
        return internal_server_error(err);
    }

    if policy_type == "LOCAL" && inode.kind == "FILE" {
        let sync_root = sync_root_path();
        if let Err(err) = db::set_pin_state(&state.pool, inode_id, 1).await {
            return internal_server_error(err);
        }
        if let Err(err) =
            smart_sync::hydrate_placeholder_now(&state.pool, &sync_root, inode_id).await
        {
            return internal_server_error(err);
        }
    }

    (
        StatusCode::OK,
        Json(FilesystemPolicyResponse {
            inode_id,
            path: logical_path,
            policy_type: policy_type.to_string(),
            repair_reconciliation_scheduled: policy_type == "PARANOIA",
        }),
    )
        .into_response()
}

async fn pin_filesystem_path(
    State(state): State<ApiState>,
    Json(request): Json<FilesystemPathRequest>,
) -> impl IntoResponse {
    let (inode_id, _, inode) =
        match resolve_filesystem_request_target(&state.pool, &request.path).await {
            Ok(target) => target,
            Err(response) => return response,
        };
    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "inode_not_file",
                "inode_id": inode_id,
                "kind": inode.kind,
            })),
        )
            .into_response();
    }

    let sync_root = sync_root_path();
    if let Err(err) = db::set_pin_state(&state.pool, inode_id, 1).await {
        return internal_server_error(err);
    }
    if let Err(err) = smart_sync::hydrate_placeholder_now(&state.pool, &sync_root, inode_id).await {
        return internal_server_error(err);
    }

    match db::get_smart_sync_state(&state.pool, inode_id).await {
        Ok(Some(status)) => (
            StatusCode::OK,
            Json(SmartSyncActionResponse {
                inode_id: status.inode_id,
                pin_state: status.pin_state,
                hydration_state: status.hydration_state,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": "smart_sync_state_not_found", "inode_id": inode_id }),
            ),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn unpin_filesystem_path(
    State(state): State<ApiState>,
    Json(request): Json<FilesystemPathRequest>,
) -> impl IntoResponse {
    let (inode_id, _, inode) =
        match resolve_filesystem_request_target(&state.pool, &request.path).await {
            Ok(target) => target,
            Err(response) => return response,
        };
    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "inode_not_file",
                "inode_id": inode_id,
                "kind": inode.kind,
            })),
        )
            .into_response();
    }

    let sync_root = sync_root_path();
    if let Err(err) = db::set_pin_state(&state.pool, inode_id, 0).await {
        return internal_server_error(err);
    }
    if let Err(err) =
        smart_sync::sync_placeholder_pin_state(&state.pool, &sync_root, inode_id, true).await
    {
        return internal_server_error(err);
    }

    match db::get_smart_sync_state(&state.pool, inode_id).await {
        Ok(Some(status)) => (
            StatusCode::OK,
            Json(SmartSyncActionResponse {
                inode_id: status.inode_id,
                pin_state: status.pin_state,
                hydration_state: status.hydration_state,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": "smart_sync_state_not_found", "inode_id": inode_id }),
            ),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn get_file_revisions(
    State(state): State<ApiState>,
    Path(inode_id): Path<i64>,
) -> impl IntoResponse {
    let inode = match db::get_inode_by_id(&state.pool, inode_id).await {
        Ok(inode) => inode,
        Err(err) => return internal_server_error(err),
    };

    let Some(inode) = inode else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "inode_not_found",
                "inode_id": inode_id,
            })),
        )
            .into_response();
    };

    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "inode_not_file",
                "inode_id": inode_id,
                "kind": inode.kind,
            })),
        )
            .into_response();
    }

    match db::list_file_revisions(&state.pool, inode_id).await {
        Ok(revisions) => (
            StatusCode::OK,
            Json(
                revisions
                    .into_iter()
                    .map(|revision| FileRevisionResponse {
                        revision_id: revision.revision_id,
                        inode_id: revision.inode_id,
                        created_at: revision.created_at,
                        size: revision.size,
                        is_current: revision.is_current != 0,
                        immutable_until: revision.immutable_until,
                        device_id: revision.device_id,
                        parent_revision_id: revision.parent_revision_id,
                        origin: revision.origin,
                        conflict_reason: revision.conflict_reason,
                    })
                    .collect::<Vec<_>>(),
            ),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn restore_file_revision(
    State(state): State<ApiState>,
    Path((inode_id, revision_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    let inode = match db::get_inode_by_id(&state.pool, inode_id).await {
        Ok(inode) => inode,
        Err(err) => return internal_server_error(err),
    };

    let Some(inode) = inode else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "inode_not_found",
                "inode_id": inode_id,
            })),
        )
            .into_response();
    };

    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "inode_not_file",
                "inode_id": inode_id,
                "kind": inode.kind,
            })),
        )
            .into_response();
    }

    let revision = match db::get_file_revision(&state.pool, inode_id, revision_id).await {
        Ok(revision) => revision,
        Err(err) => return internal_server_error(err),
    };

    let Some(_) = revision else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "revision_not_found",
                "inode_id": inode_id,
                "revision_id": revision_id,
            })),
        )
            .into_response();
    };

    let current_revision = match db::get_current_file_revision(&state.pool, inode_id).await {
        Ok(revision) => revision,
        Err(err) => return internal_server_error(err),
    };

    let local_device = match db::get_local_device_identity(&state.pool).await {
        Ok(device) => device,
        Err(err) => return internal_server_error(err),
    };
    let conflict_device_id = local_device
        .as_ref()
        .map(|device| device.device_id.as_str());
    let conflict_device_name = local_device
        .as_ref()
        .map(|device| device.device_name.as_str())
        .unwrap_or("Unknown Device");

    let conflict_copy = match current_revision {
        Some(current) => {
            let lineage =
                match db::classify_revision_lineage(&state.pool, revision_id, current.revision_id)
                    .await
                {
                    Ok(lineage) => lineage,
                    Err(err) => return internal_server_error(err),
                };

            let conflict_reason = match lineage {
                db::RevisionLineageRelation::Same
                | db::RevisionLineageRelation::CandidateDescendsFromCurrent => None,
                db::RevisionLineageRelation::CurrentDescendsFromCandidate => Some("restore_rewind"),
                db::RevisionLineageRelation::Parallel => Some("parallel_restore"),
            };

            match conflict_reason {
                Some(reason) => match db::materialize_conflict_copy_from_revision(
                    &state.pool,
                    current.revision_id,
                    conflict_device_id,
                    conflict_device_name,
                    reason,
                )
                .await
                {
                    Ok((conflict_inode_id, conflict_revision_id, conflict_name, _conflict_id)) => {
                        Some((conflict_inode_id, conflict_revision_id, conflict_name))
                    }
                    Err(err) => return internal_server_error(err),
                },
                None => None,
            }
        }
        None => None,
    };

    match db::promote_revision_to_current(&state.pool, revision_id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(RestoreRevisionResponse {
                inode_id,
                revision_id,
                restored: true,
                conflict_copy_inode_id: conflict_copy.as_ref().map(|value| value.0),
                conflict_copy_revision_id: conflict_copy.as_ref().map(|value| value.1),
                conflict_copy_name: conflict_copy.map(|value| value.2),
            }),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn materialize_conflict_copy(
    State(state): State<ApiState>,
    Path((inode_id, revision_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    let inode = match db::get_inode_by_id(&state.pool, inode_id).await {
        Ok(inode) => inode,
        Err(err) => return internal_server_error(err),
    };

    let Some(inode) = inode else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "inode_not_found",
                "inode_id": inode_id,
            })),
        )
            .into_response();
    };

    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "inode_not_file",
                "inode_id": inode_id,
                "kind": inode.kind,
            })),
        )
            .into_response();
    }

    let revision = match db::get_file_revision(&state.pool, inode_id, revision_id).await {
        Ok(revision) => revision,
        Err(err) => return internal_server_error(err),
    };

    let Some(_) = revision else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "revision_not_found",
                "inode_id": inode_id,
                "revision_id": revision_id,
            })),
        )
            .into_response();
    };

    let local_device = match db::get_local_device_identity(&state.pool).await {
        Ok(device) => device,
        Err(err) => return internal_server_error(err),
    };
    let conflict_device_id = local_device
        .as_ref()
        .map(|device| device.device_id.as_str());
    let conflict_device_name = local_device
        .as_ref()
        .map(|device| device.device_name.as_str())
        .unwrap_or("Unknown Device");

    match db::materialize_conflict_copy_from_revision(
        &state.pool,
        revision_id,
        conflict_device_id,
        conflict_device_name,
        "manual_conflict_copy",
    )
    .await
    {
        Ok((conflict_inode_id, conflict_revision_id, conflict_name, conflict_id)) => (
            StatusCode::OK,
            Json(ConflictCopyResponse {
                inode_id,
                source_revision_id: revision_id,
                conflict_copy_inode_id: conflict_inode_id,
                conflict_copy_revision_id: conflict_revision_id,
                conflict_copy_name: conflict_name,
                conflict_id,
            }),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn get_quota(State(state): State<ApiState>) -> impl IntoResponse {
    let app_config = AppConfig::from_env();
    let mut providers = Vec::with_capacity(KNOWN_PROVIDERS.len());

    for provider in KNOWN_PROVIDERS {
        match db::get_physical_usage_for_provider(&state.pool, provider).await {
            Ok(used_physical_bytes) => providers.push(ProviderQuotaResponse {
                provider: provider.to_string(),
                used_physical_bytes,
            }),
            Err(err) => return internal_server_error(err),
        }
    }

    (
        StatusCode::OK,
        Json(QuotaResponse {
            max_physical_bytes_per_provider: app_config.max_physical_bytes_per_provider,
            providers,
        }),
    )
        .into_response()
}

async fn get_cache_status(State(state): State<ApiState>) -> impl IntoResponse {
    let config = AppConfig::from_env();
    let summary = match db::get_cache_status_summary(&state.pool).await {
        Ok(summary) => summary,
        Err(err) => return internal_server_error(err),
    };
    let runtime = cache::cache_runtime_stats();

    (
        StatusCode::OK,
        Json(CacheStatusResponse {
            total_entries: summary.total_entries,
            total_bytes: summary.total_bytes,
            max_bytes: config.max_cache_bytes,
            prefetched_entries: summary.prefetched_entries,
            hit_count: runtime.hit_count,
            miss_count: runtime.miss_count,
        }),
    )
        .into_response()
}

async fn get_recovery_status(State(state): State<ApiState>) -> impl IntoResponse {
    match build_recovery_status_response(&state).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn get_scrub_status(State(state): State<ApiState>) -> impl IntoResponse {
    match db::get_scrub_status_summary(&state.pool).await {
        Ok(summary) => (
            StatusCode::OK,
            Json(ScrubStatusResponse {
                total_shards: summary.total_shards,
                verified_shards: summary.verified_shards,
                healthy_shards: summary.healthy_shards,
                corrupted_or_missing: summary.corrupted_or_missing,
                verified_light_shards: summary.verified_light_shards,
                verified_deep_shards: summary.verified_deep_shards,
                last_scrub_timestamp: summary.last_scrub_timestamp,
            }),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn post_scrub_now(State(state): State<ApiState>) -> impl IntoResponse {
    match scrubber::run_scrub_batch_now(state.pool.clone()).await {
        Ok(processed_shards) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "OK",
                "message": format!("Light scrub completed. Processed {} shard(s).", processed_shards),
                "last_run": unix_timestamp_millis(),
                "processed_shards": processed_shards,
            })),
        )
        .into_response(),
        Err(scrubber::ScrubberError::MissingProviderConfig) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "WARN",
                "message": "Scrub jest bezczynny, ponieważ nie skonfigurowano zdalnych dostawców.",
                "last_run": unix_timestamp_millis(),
                "processed_shards": 0,
            })),
        )
        .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn post_repair_now(State(state): State<ApiState>) -> impl IntoResponse {
    match repair::RepairWorker::run_repair_batch_now(state.pool.clone()).await {
        Ok(report) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "OK",
                "message": if report.repaired_packs == 0 {
                    "Brak zdegradowanych pakietów wymagających natychmiastowej naprawy.".to_string()
                } else {
                    format!("Naprawa przetworzyła {} zdegradowanych pakietów.", report.repaired_packs)
                },
                "last_run": unix_timestamp_millis(),
                "processed_packs": report.processed_packs,
                "repaired_packs": report.repaired_packs,
                "reconciled_packs": report.reconciled_packs,
            })),
        )
            .into_response(),
        Err(RepairError::MissingProviderConfig) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "WARN",
                "message": "Naprawa jest bezczynna, ponieważ nie skonfigurowano zdalnych dostawców.",
                "last_run": unix_timestamp_millis(),
                "processed_packs": 0,
                "repaired_packs": 0,
                "reconciled_packs": 0,
            })),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn post_reconcile_now(State(state): State<ApiState>) -> impl IntoResponse {
    match repair::RepairWorker::run_reconcile_batch_now(state.pool.clone()).await {
        Ok(report) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "OK",
                "message": if report.reconciled_packs == 0 {
                    "No pack policy drift required reconciliation.".to_string()
                } else {
                    format!("Reconciliation processed {} pack(s).", report.reconciled_packs)
                },
                "last_run": unix_timestamp_millis(),
                "processed_packs": report.processed_packs,
                "repaired_packs": report.repaired_packs,
                "reconciled_packs": report.reconciled_packs,
            })),
        )
            .into_response(),
        Err(RepairError::MissingProviderConfig) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "WARN",
                "message": "Proces reconciliation jest bezczynny, ponieważ nie skonfigurowano zdalnych dostawców.",
                "last_run": unix_timestamp_millis(),
                "processed_packs": 0,
                "repaired_packs": 0,
                "reconciled_packs": 0,
            })),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn post_repair_shell() -> impl IntoResponse {
    let mut actions = Vec::new();
    let mut last_state = shell_state::audit_shell_state();

    match shell_state::repair_virtual_drive() {
        Ok(report) => {
            actions.extend(report.actions);
            last_state = report.shell_state;
        }
        Err(err) => {
            error!("shell repair virtual drive failed: {}", err);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "ERROR",
                    "step": "virtual_drive",
                    "message": err.to_string(),
                    "last_run": unix_timestamp_millis(),
                    "shell_state": last_state,
                })),
            )
                .into_response();
        }
    }

    match shell_state::repair_explorer_integration() {
        Ok(report) => {
            actions.extend(report.actions);
            last_state = report.shell_state;
        }
        Err(err) => {
            error!("shell repair explorer integration failed: {}", err);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "ERROR",
                    "step": "explorer_integration",
                    "message": err.to_string(),
                    "last_run": unix_timestamp_millis(),
                    "actions": actions,
                    "shell_state": last_state,
                })),
            )
                .into_response();
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "OK",
            "message": if actions.is_empty() {
                "Stan shell byl juz poprawny.".to_string()
            } else {
                format!("Naprawa shell zastosowala {} akcje.", actions.len())
            },
            "last_run": unix_timestamp_millis(),
            "actions": actions,
            "shell_state": last_state,
        })),
    )
        .into_response()
}

async fn post_repair_sync_root(State(state): State<ApiState>) -> impl IntoResponse {
    let runtime_paths = RuntimePaths::detect();
    match smart_sync::repair_sync_root(&state.pool, &runtime_paths.sync_root).await {
        Ok(report) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "OK",
                "message": if report.actions.is_empty() {
                    "Sync-root byl juz poprawny.".to_string()
                } else {
                    format!("Naprawa sync-root zastosowala {} akcje.", report.actions.len())
                },
                "last_run": unix_timestamp_millis(),
                "actions": report.actions,
                "sync_root_state": report.sync_root_state,
            })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "ERROR",
                "message": err.to_string(),
                "last_run": unix_timestamp_millis(),
            })),
        )
            .into_response(),
    }
}

async fn get_scrub_errors(State(state): State<ApiState>) -> impl IntoResponse {
    match db::list_scrub_errors(&state.pool, 100).await {
        Ok(errors) => (
            StatusCode::OK,
            Json(
                errors
                    .into_iter()
                    .map(|record| ScrubErrorResponse {
                        pack_id: record.pack_id,
                        provider: record.provider,
                        shard_index: record.shard_index,
                        last_verified_at: record.last_verified_at,
                        status: record.last_verification_status,
                    })
                    .collect::<Vec<_>>(),
            ),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn post_snapshot_local(
    State(state): State<ApiState>,
    Json(request): Json<SnapshotLocalRequest>,
) -> impl IntoResponse {
    let output_path = std::path::PathBuf::from(&request.output_path);
    let master_key = match state.vault_keys.require_master_key().await {
        Ok(key) => key,
        Err(VaultError::Locked) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "vault_locked",
                    "message": "odblokuj Skarbiec przed utworzeniem zaszyfrowanej migawki metadanych"
                })),
            )
                .into_response();
        }
        Err(err) => return internal_server_error(err),
    };

    match disaster_recovery::create_encrypted_metadata_snapshot(
        &state.pool,
        &output_path,
        &master_key,
    )
    .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(SnapshotLocalResponse {
                output_path: if output_path
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(".enc")
                {
                    output_path.display().to_string()
                } else {
                    format!("{}.enc", output_path.display())
                },
                created: true,
            }),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn post_backup_now(State(state): State<ApiState>) -> impl IntoResponse {
    let master_key = match state.vault_keys.require_master_key().await {
        Ok(key) => key,
        Err(VaultError::Locked) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "vault_locked",
                    "message": "odblokuj Skarbiec przed utworzeniem zaszyfrowanej kopii metadanych"
                })),
            )
                .into_response();
        }
        Err(err) => return internal_server_error(err),
    };

    let provider_manager = match disaster_recovery::MetadataBackupProviderManager::from_onboarding_db_all(&state.pool).await {
        Ok(manager) => manager,
        Err(err) => return internal_server_error(err),
    };

    match disaster_recovery::run_metadata_backup_now(&state.pool, &provider_manager, &master_key)
        .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "OK",
                "message": "Zaszyfrowana kopia metadanych została wysłana pomyślnie.",
                "last_run": unix_timestamp_millis(),
                "uploaded": true
            })),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn build_diagnostics_health_response(
    state: &ApiState,
) -> Result<MaintenanceStatus<DiagnosticsHealthResponse>, sqlx::Error> {
    let snapshot = state.diagnostics.snapshot();
    let pending_uploads_queue_size = db::get_pending_upload_queue_size(&state.pool).await?;
    let cache_summary = db::get_cache_status_summary(&state.pool).await?;
    let runtime_cache = cache::cache_runtime_stats();
    let last_upload_error = match &snapshot.last_upload_error {
        Some(value) => Some(value.clone()),
        None => db::get_latest_upload_error(&state.pool).await?,
    };

    let health = DiagnosticsHealthResponse {
        uptime_seconds: snapshot.uptime_seconds,
        pending_uploads_queue_size,
        last_upload_error: last_upload_error.clone(),
        cache_size_bytes: cache_summary.total_bytes,
        cache_hit_count: runtime_cache.hit_count,
        cache_miss_count: runtime_cache.miss_count,
        worker_statuses: DiagnosticsWorkerStatusesResponse {
            uploader: snapshot.uploader.as_str().to_string(),
            repair: snapshot.repair.as_str().to_string(),
            scrubber: snapshot.scrubber.as_str().to_string(),
            gc: snapshot.gc.as_str().to_string(),
            watcher: snapshot.watcher.as_str().to_string(),
            metadata_backup: snapshot.metadata_backup.as_str().to_string(),
            peer: snapshot.peer.as_str().to_string(),
            api: snapshot.api.as_str().to_string(),
        },
    };

    let (level, message) = if let Some(error) = last_upload_error {
        (
            MaintenanceLevel::Warn,
            format!(
                "Usługi w tle działają, ale ostatnia wysyłka zgłosiła błąd: {}",
                error
            ),
        )
    } else if pending_uploads_queue_size > 0 {
        (
            MaintenanceLevel::Warn,
            format!(
                "{} wysyłek nadal oczekuje na przetworzenie.",
                pending_uploads_queue_size
            ),
        )
    } else {
        (
            MaintenanceLevel::Ok,
            "Usługi w tle są zdrowe i brak zaległych wysyłek.".to_string(),
        )
    };

    Ok(MaintenanceStatus {
        status: level.as_str().to_string(),
        message,
        last_run: unix_timestamp_millis(),
        details: health,
    })
}

fn build_shell_state_response() -> MaintenanceStatus<shell_state::ShellStateSnapshot> {
    let snapshot = shell_state::audit_shell_state();
    let message = if snapshot.is_healthy() {
        format!(
            "Dysk {} jest zamontowany, a integracja z Eksploratorem jest poprawna.",
            snapshot.preferred_drive_letter
        )
    } else if !snapshot.drive_present {
        format!(
            "Dysk {} jest niedostępny i wymaga naprawy.",
            snapshot.preferred_drive_letter
        )
    } else if !snapshot.drive_target_matches {
        format!(
            "Dysk {} wskazuje nieoczekiwany cel i wymaga naprawy.",
            snapshot.preferred_drive_letter
        )
    } else if !snapshot.drive_browsable {
        format!(
            "Dysk {} istnieje, ale nie jest przeglądalny w Eksploratorze.",
            snapshot.preferred_drive_letter
        )
    } else {
        "Wykryto drift integracji z Eksploratorem; można go naprawić.".to_string()
    };

    MaintenanceStatus {
        status: if snapshot.is_healthy() {
            MaintenanceLevel::Ok
        } else {
            MaintenanceLevel::Warn
        }
        .as_str()
        .to_string(),
        message,
        last_run: unix_timestamp_millis(),
        details: snapshot,
    }
}

fn build_sync_root_state_response()
-> Result<MaintenanceStatus<smart_sync::SyncRootStateSnapshot>, smart_sync::SmartSyncError> {
    let runtime_paths = RuntimePaths::detect();
    let snapshot = smart_sync::audit_sync_root_state(&runtime_paths.sync_root)?;
    let shell_mode = shell_state::audit_shell_state().mode;

    let (level, message) = if shell_mode == "local_only" {
        (
            MaintenanceLevel::Ok,
            "Smart Sync jest celowo bezczynny do czasu skonfigurowania zdalnych dostawców.".to_string(),
        )
    } else if snapshot.registered && snapshot.connected && snapshot.registered_for_provider {
        (
            MaintenanceLevel::Ok,
            format!("Sync-root {} jest zarejestrowany i połączony.", snapshot.path),
        )
    } else if snapshot.path_exists {
        (
            MaintenanceLevel::Warn,
            format!(
                "Sync-root {} istnieje, ale rejestracja lub połączenie są niepełne.",
                snapshot.path
            ),
        )
    } else {
        (
            MaintenanceLevel::Error,
            format!(
                "Sync-root {} jest niedostępny i wymaga naprawy.",
                snapshot.path
            ),
        )
    };

    Ok(MaintenanceStatus {
        status: level.as_str().to_string(),
        message,
        last_run: unix_timestamp_millis(),
        details: snapshot,
    })
}

async fn build_recovery_status_response(
    state: &ApiState,
) -> Result<MaintenanceStatus<RecoveryStatusResponse>, sqlx::Error> {
    let last_successful_backup = db::get_last_successful_metadata_backup_at(&state.pool).await?;
    let recent_attempts: Vec<MetadataBackupAttemptResponse> =
        db::list_recent_metadata_backups(&state.pool, 10)
            .await?
            .into_iter()
            .map(|record| MetadataBackupAttemptResponse {
                backup_id: record.backup_id,
                created_at: record.created_at,
                snapshot_version: record.snapshot_version,
                object_key: record.object_key,
                provider: record.provider,
                encrypted_size: record.encrypted_size,
                status: record.status,
                last_error: record.last_error,
            })
            .collect();

    let last_run = recent_attempts
        .iter()
        .map(|attempt| attempt.created_at)
        .max()
        .or(last_successful_backup)
        .unwrap_or_else(unix_timestamp_millis);

    let latest_failed = recent_attempts
        .iter()
        .find(|attempt| attempt.status.eq_ignore_ascii_case("FAILED"));
    let (level, message) = if let Some(attempt) = latest_failed {
        (
            MaintenanceLevel::Warn,
            format!(
                "Ostatnia kopia metadanych do {} nie powiodła się{}",
                attempt.provider,
                attempt
                    .last_error
                    .as_deref()
                    .map(|err| format!(": {}", err))
                    .unwrap_or_default()
            ),
        )
    } else if let Some(timestamp) = last_successful_backup {
        (
            MaintenanceLevel::Ok,
            format!(
                "Kopia metadanych jest dostępna. Ostatni udany przebieg: {}.",
                timestamp
            ),
        )
    } else {
        (
            MaintenanceLevel::Warn,
            "Nie zarejestrowano jeszcze kopii metadanych.".to_string(),
        )
    };

    Ok(MaintenanceStatus {
        status: level.as_str().to_string(),
        message,
        last_run,
        details: RecoveryStatusResponse {
            last_successful_backup,
            recent_attempts,
        },
    })
}

async fn build_storage_cost_response(state: &ApiState) -> Result<StorageCostResponse, sqlx::Error> {
    let app_config = AppConfig::from_env();
    let guard_snapshot = cloud_guard::snapshot(&state.pool).await.ok();
    let mode_summaries = db::get_active_storage_mode_summaries(&state.pool).await?;
    let orphaned_summary = db::get_orphaned_pack_summary(&state.pool).await?;
    let active_packs = db::list_active_packs(&state.pool, 100_000).await?;

    let reconcile_backlog_packs = count_reconcile_backlog(&state.pool, &active_packs).await?;

    let mut logical_bytes = 0u64;
    let mut physical_bytes = 0u64;
    let mut estimated_paranoia_physical_bytes = 0u64;
    let mut estimated_provider_bytes_avoided = 0u64;
    let mut storage_modes = Vec::with_capacity(mode_summaries.len());

    for summary in mode_summaries {
        let summary_logical_bytes = u64::try_from(summary.logical_bytes).unwrap_or(0);
        let summary_physical_bytes = u64::try_from(summary.physical_bytes).unwrap_or(0);
        let total_shard_bytes = u64::try_from(summary.total_shard_bytes).unwrap_or(0);
        let estimated_paranoia_bytes = match summary.storage_mode.as_str() {
            "EC_2_1" => summary_physical_bytes,
            "SINGLE_REPLICA" | "LOCAL_ONLY" => total_shard_bytes.saturating_mul(3),
            _ => summary_physical_bytes,
        };
        let avoided_bytes = estimated_paranoia_bytes.saturating_sub(summary_physical_bytes);

        logical_bytes = logical_bytes.saturating_add(summary_logical_bytes);
        physical_bytes = physical_bytes.saturating_add(summary_physical_bytes);
        estimated_paranoia_physical_bytes =
            estimated_paranoia_physical_bytes.saturating_add(estimated_paranoia_bytes);
        estimated_provider_bytes_avoided =
            estimated_provider_bytes_avoided.saturating_add(avoided_bytes);

        storage_modes.push(StorageCostModeResponse {
            storage_mode: summary.storage_mode.clone(),
            active_packs: summary.active_packs,
            logical_bytes: summary_logical_bytes,
            physical_bytes: summary_physical_bytes,
            estimated_paranoia_physical_bytes: estimated_paranoia_bytes,
            estimated_provider_bytes_avoided: avoided_bytes,
            estimated_monthly_cost_usd: round_cost_estimate(
                bytes_to_gib(summary_physical_bytes)
                    * app_config.estimated_cost_per_gib_month_default,
            ),
        });
    }

    let mut providers = Vec::with_capacity(KNOWN_PROVIDERS.len());
    let mut estimated_monthly_cost_usd = 0.0f64;
    for provider in KNOWN_PROVIDERS {
        let used_physical_bytes =
            db::get_physical_usage_for_provider(&state.pool, provider).await?;
        let rate = provider_cost_rate(&app_config, provider);
        let provider_cost = round_cost_estimate(bytes_to_gib(used_physical_bytes) * rate);
        estimated_monthly_cost_usd += provider_cost;
        providers.push(StorageCostProviderResponse {
            provider: provider.to_string(),
            used_physical_bytes,
            usage_share_percent: percent_of(used_physical_bytes, physical_bytes),
            estimated_monthly_cost_usd: provider_cost,
            configured_cost_per_gib_month: rate,
        });
    }

    let message = if logical_bytes == 0 {
        "Brak aktywnych pakietow, dlatego dashboard storage pokazuje pusty slad Skarbca."
            .to_string()
    } else {
        format!(
            "Zajętość Skarbca: {:.2} GiB logicznie vs {:.2} GiB fizycznie, szacowany koszt zdalny ${:.2}/mies.",
            bytes_to_gib(logical_bytes),
            bytes_to_gib(physical_bytes),
            round_cost_estimate(estimated_monthly_cost_usd)
        )
    };

    Ok(StorageCostResponse {
        status: "OK".to_string(),
        message,
        last_run: unix_timestamp_millis(),
        logical_bytes,
        physical_bytes,
        physical_to_logical_ratio: ratio_of(physical_bytes, logical_bytes),
        estimated_monthly_cost_usd: round_cost_estimate(estimated_monthly_cost_usd),
        estimated_provider_bytes_avoided,
        estimated_paranoia_physical_bytes,
        reconcile_backlog_packs,
        orphaned_packs: orphaned_summary.pack_count,
        orphaned_physical_bytes: u64::try_from(orphaned_summary.physical_bytes).unwrap_or(0),
        gc_candidate_packs: orphaned_summary.pack_count,
        cloud_guard_status: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.status.clone())
            .unwrap_or_else(|| "WARN".to_string()),
        cloud_guard_message: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.message.clone())
            .unwrap_or_else(|| "Migawka Cloud Guard jest niedostępna.".to_string()),
        dry_run_active: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.dry_run_active)
            .unwrap_or(app_config.dry_run_active),
        cloud_suspended: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.cloud_suspended)
            .unwrap_or(false),
        cloud_suspend_reason: guard_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.cloud_suspend_reason.clone()),
        session_read_ops: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.session_read_ops)
            .unwrap_or(0),
        session_write_ops: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.session_write_ops)
            .unwrap_or(0),
        session_egress_bytes: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.session_egress_bytes)
            .unwrap_or(0),
        daily_read_ops: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.daily_read_ops)
            .unwrap_or(0),
        daily_write_ops: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.daily_write_ops)
            .unwrap_or(0),
        daily_egress_bytes: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.daily_egress_bytes)
            .unwrap_or(0),
        daily_read_ops_limit: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.daily_read_ops_limit)
            .unwrap_or(i64::try_from(app_config.cloud_daily_read_ops_limit).unwrap_or(i64::MAX)),
        daily_write_ops_limit: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.daily_write_ops_limit)
            .unwrap_or(i64::try_from(app_config.cloud_daily_write_ops_limit).unwrap_or(i64::MAX)),
        daily_egress_bytes_limit: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.daily_egress_bytes_limit)
            .unwrap_or(
                i64::try_from(app_config.cloud_daily_egress_bytes_limit).unwrap_or(i64::MAX),
            ),
        read_quota_percent: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.read_quota_percent)
            .unwrap_or(0.0),
        write_quota_percent: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.write_quota_percent)
            .unwrap_or(0.0),
        egress_quota_percent: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.egress_quota_percent)
            .unwrap_or(0.0),
        providers,
        storage_modes,
    })
}

fn maintenance_overview_item<T>(status: &MaintenanceStatus<T>) -> MaintenanceOverviewItem
where
    T: Serialize,
{
    MaintenanceOverviewItem {
        status: status.status.clone(),
        message: status.message.clone(),
        last_run: status.last_run,
    }
}

fn maintenance_error_item(message: String) -> MaintenanceOverviewItem {
    MaintenanceOverviewItem {
        status: MaintenanceLevel::Error.as_str().to_string(),
        message,
        last_run: unix_timestamp_millis(),
    }
}

fn unix_timestamp_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

async fn count_reconcile_backlog(
    pool: &SqlitePool,
    active_packs: &[db::PackRecord],
) -> Result<usize, sqlx::Error> {
    let mut count = 0usize;
    for pack in active_packs {
        let desired = db::get_desired_storage_mode_for_pack(pool, &pack.pack_id).await?;
        if db::StorageMode::from_str(&pack.storage_mode) != desired {
            count += 1;
        }
    }
    Ok(count)
}

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

fn ratio_of(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn percent_of(value: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (value as f64 / total as f64) * 100.0
    }
}

fn round_cost_estimate(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn provider_cost_rate(config: &AppConfig, provider: &str) -> f64 {
    match provider {
        "cloudflare-r2" => config.estimated_cost_per_gib_month_r2,
        "backblaze-b2" => config.estimated_cost_per_gib_month_b2,
        "scaleway" => config.estimated_cost_per_gib_month_scaleway,
        _ => config.estimated_cost_per_gib_month_default,
    }
}

fn provider_connection_status(target_status: &str, has_error: bool) -> String {
    match target_status {
        "COMPLETED" => "HEALTHY".to_string(),
        "IN_PROGRESS" if has_error => "DEGRADED".to_string(),
        "IN_PROGRESS" => "ATTEMPTING".to_string(),
        "PENDING" | "FAILED" if has_error => "DEGRADED".to_string(),
        "PENDING" => "PENDING".to_string(),
        other => other.to_string(),
    }
}

async fn build_onboarding_status_response(
    state: &ApiState,
) -> Result<MaintenanceStatus<OnboardingStatusResponse>, sqlx::Error> {
    let onboarding_state = db::get_system_config_value(&state.pool, SYSTEM_CONFIG_ONBOARDING_STATE)
        .await?
        .unwrap_or_else(|| OnboardingState::Initial.as_str().to_string());
    let onboarding_mode = db::get_system_config_value(&state.pool, SYSTEM_CONFIG_ONBOARDING_MODE)
        .await?
        .unwrap_or_else(|| OnboardingMode::LocalOnly.as_str().to_string());
    let current_step = db::get_system_config_value(&state.pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP)
        .await?
        .unwrap_or_else(|| "welcome".to_string());
    let draft_env_detected =
        db::get_system_config_value(&state.pool, SYSTEM_CONFIG_DRAFT_ENV_DETECTED)
            .await?
            .is_some_and(|value| value == "1");
    let cloud_enabled = db::get_system_config_value(&state.pool, SYSTEM_CONFIG_CLOUD_ENABLED)
        .await?
        .is_some_and(|value| value == "1");
    let local_device = db::get_local_device_identity(&state.pool).await?;
    let providers = db::list_provider_configs(&state.pool).await?;

    let mut provider_statuses = Vec::with_capacity(providers.len());
    for provider in providers {
        let has_secret = db::get_provider_secret(&state.pool, &provider.provider_name)
            .await?
            .is_some();
        provider_statuses.push(OnboardingProviderStatusResponse {
            provider_name: provider.provider_name,
            endpoint: provider.endpoint,
            region: provider.region,
            bucket: provider.bucket,
            force_path_style: provider.force_path_style != 0,
            enabled: provider.enabled != 0,
            draft_source: provider.draft_source,
            last_test_status: provider.last_test_status,
            last_test_error: provider.last_test_error,
            last_test_at: provider.last_test_at,
            access_key_status: if has_secret {
                "SET".to_string()
            } else {
                "MISSING".to_string()
            },
            secret_key_status: if has_secret {
                "SET".to_string()
            } else {
                "MISSING".to_string()
            },
        });
    }

    let level = if onboarding_state == OnboardingState::Completed.as_str() {
        MaintenanceLevel::Ok
    } else {
        MaintenanceLevel::Warn
    };

    let message = if onboarding_state == OnboardingState::Completed.as_str() {
        "Onboarding zakończony.".to_string()
    } else if draft_env_detected {
        "Onboarding nie jest zakończony; szkice dostawców zaimportowano z .env do przeglądu.".to_string()
    } else {
        "Onboarding nie został jeszcze zakończony.".to_string()
    };

    Ok(MaintenanceStatus {
        status: level.as_str().to_string(),
        message,
        last_run: unix_timestamp_millis(),
        details: OnboardingStatusResponse {
            onboarding_state,
            onboarding_mode,
            current_step,
            draft_env_detected,
            cloud_enabled,
            device_name: local_device
                .as_ref()
                .map(|device| device.device_name.clone()),
            device_id: local_device.as_ref().map(|device| device.device_id.clone()),
            providers: provider_statuses,
        },
    })
}

fn internal_server_error(err: impl std::error::Error) -> axum::response::Response {
    error!("api request failed: {err}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": "internal_server_error" })),
    )
        .into_response()
}

fn io_error(err: impl fmt::Display) -> std::io::Error {
    std::io::Error::other(err.to_string())
}

fn provider_error_kind(err: &crate::onboarding::ProviderError) -> &'static str {
    match err {
        crate::onboarding::ProviderError::InvalidCredentials(_) => "InvalidCredentials",
        crate::onboarding::ProviderError::BucketNotFound(_) => "BucketNotFound",
        crate::onboarding::ProviderError::AccessDenied(_) => "AccessDenied",
        crate::onboarding::ProviderError::EndpointUnreachable(_) => "EndpointUnreachable",
        crate::onboarding::ProviderError::ClockSkew(_) => "ClockSkew",
        crate::onboarding::ProviderError::MissingProviderConfig(_) => "MissingProviderConfig",
        crate::onboarding::ProviderError::MissingSecrets(_) => "MissingSecrets",
        crate::onboarding::ProviderError::Url(_) => "InvalidEndpoint",
        crate::onboarding::ProviderError::Io(_) => "Io",
        crate::onboarding::ProviderError::Aws(_) => "Aws",
    }
}

fn provider_restore_error_kind(err: &crate::onboarding::RestoreError) -> &'static str {
    match err {
        crate::onboarding::RestoreError::MissingProviderConfig(_) => "MissingProviderConfig",
        crate::onboarding::RestoreError::IncorrectPassphrase(_) => "IncorrectPassphrase",
        crate::onboarding::RestoreError::MetadataNotFound(_) => "MetadataNotFound",
        crate::onboarding::RestoreError::NetworkError(_) => "NetworkError",
        crate::onboarding::RestoreError::SnapshotApply(_) => "SnapshotApply",
        crate::onboarding::RestoreError::Runtime(_) => "Runtime",
        crate::onboarding::RestoreError::Io(_) => "Io",
    }
}

fn normalize_policy_type(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_uppercase().as_str() {
        "PARANOIA" => Some("PARANOIA"),
        "STANDARD" => Some("STANDARD"),
        "LOCAL" => Some("LOCAL"),
        _ => None,
    }
}

async fn resolve_filesystem_request_target(
    pool: &SqlitePool,
    raw_path: &str,
) -> Result<(i64, String, db::InodeRecord), axum::response::Response> {
    let logical_path = match normalize_filesystem_api_path(raw_path) {
        Some(path) => path,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_filesystem_path",
                    "path": raw_path,
                })),
            )
                .into_response());
        }
    };

    let inode_id = match db::resolve_path(pool, &logical_path).await {
        Ok(Some(inode_id)) => inode_id,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "inode_not_found",
                    "path": logical_path,
                })),
            )
                .into_response());
        }
        Err(err) => return Err(internal_server_error(err)),
    };

    let inode = match db::get_inode_by_id(pool, inode_id).await {
        Ok(Some(inode)) => inode,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "inode_not_found",
                    "inode_id": inode_id,
                })),
            )
                .into_response());
        }
        Err(err) => return Err(internal_server_error(err)),
    };

    let canonical_path = match db::get_inode_path(pool, inode_id).await {
        Ok(Some(path)) => path,
        Ok(None) => logical_path,
        Err(err) => return Err(internal_server_error(err)),
    };

    Ok((inode_id, canonical_path, inode))
}

fn normalize_filesystem_api_path(raw_path: &str) -> Option<String> {
    let trimmed = raw_path.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }

    let sync_root = sync_root_path();
    let drive_letter = env::var("OMNIDRIVE_DRIVE_LETTER").unwrap_or_else(|_| "O:".to_string());
    let drive_prefix = format!(
        "{}\\",
        drive_letter
            .trim()
            .trim_end_matches('\\')
            .trim_end_matches('/')
            .to_ascii_uppercase()
    );
    let candidate = trimmed.replace('/', "\\");
    let candidate_upper = candidate.to_ascii_uppercase();
    let sync_root_rendered = sync_root.to_string_lossy().replace('/', "\\");
    let sync_root_upper = sync_root_rendered.to_ascii_uppercase();

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

fn sync_root_path() -> std::path::PathBuf {
    crate::runtime_paths::RuntimePaths::detect().sync_root
}
