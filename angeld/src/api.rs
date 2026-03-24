use crate::cache;
use crate::config::AppConfig;
use crate::db;
use crate::diagnostics::{DaemonDiagnostics, WorkerKind, WorkerStatus};
use crate::disaster_recovery;
use crate::scrubber;
use crate::smart_sync;
use crate::uploader::KNOWN_PROVIDERS;
use crate::vault::{VaultError, VaultKeyStore};
use axum::extract::{Path, State};
use axum::http::StatusCode;
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
use tracing::{error, info};

#[derive(Clone)]
struct ApiState {
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
    diagnostics: Arc<DaemonDiagnostics>,
}

pub struct ApiServer {
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
    diagnostics: Arc<DaemonDiagnostics>,
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
}

#[derive(Serialize)]
struct RestoreRevisionResponse {
    inode_id: i64,
    revision_id: i64,
    restored: bool,
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
struct BackupNowResponse {
    uploaded: bool,
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
struct ScrubNowResponse {
    processed_shards: usize,
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
    api: String,
}

impl ApiServer {
    pub fn from_env(
        pool: SqlitePool,
        vault_keys: VaultKeyStore,
        diagnostics: Arc<DaemonDiagnostics>,
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
            bind_addr,
        })
    }

    pub async fn run(self) -> Result<(), ApiError> {
        let diagnostics = self.diagnostics.clone();
        let state = ApiState {
            pool: self.pool,
            vault_keys: self.vault_keys,
            diagnostics: diagnostics.clone(),
        };
        let app = Router::new()
            .route("/", get(get_index))
            .route("/api/transfers", get(get_transfers))
            .route("/api/health", get(get_health))
            .route("/api/diagnostics/health", get(get_diagnostics_health))
            .route("/api/health/vault", get(get_vault_health))
            .route("/api/files", get(get_files))
            .route("/api/files/{inode_id}", delete(delete_file))
            .route("/api/files/{inode_id}/sync_status", get(get_file_sync_status))
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
            .route("/api/quota", get(get_quota))
            .route("/api/cache/status", get(get_cache_status))
            .route("/api/maintenance/scrub-status", get(get_scrub_status))
            .route("/api/maintenance/scrub-errors", get(get_scrub_errors))
            .route("/api/maintenance/scrub-now", post(post_scrub_now))
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
    let snapshot = state.diagnostics.snapshot();
    let pending_uploads_queue_size = match db::get_pending_upload_queue_size(&state.pool).await {
        Ok(value) => value,
        Err(err) => return internal_server_error(err),
    };
    let cache_summary = match db::get_cache_status_summary(&state.pool).await {
        Ok(summary) => summary,
        Err(err) => return internal_server_error(err),
    };
    let runtime_cache = cache::cache_runtime_stats();
    let last_upload_error = match &snapshot.last_upload_error {
        Some(value) => Some(value.clone()),
        None => match db::get_latest_upload_error(&state.pool).await {
            Ok(value) => value,
            Err(err) => return internal_server_error(err),
        },
    };

    (
        StatusCode::OK,
        Json(DiagnosticsHealthResponse {
            uptime_seconds: snapshot.uptime_seconds,
            pending_uploads_queue_size,
            last_upload_error,
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
                api: snapshot.api.as_str().to_string(),
            },
        }),
    )
        .into_response()
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

async fn pin_file(
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
            Json(serde_json::json!({ "error": "smart_sync_state_not_found", "inode_id": inode_id })),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn unpin_file(
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
            Json(serde_json::json!({ "error": "smart_sync_state_not_found", "inode_id": inode_id })),
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
                .into_response()
        }
    };

    let (inode_id, logical_path, inode) =
        match resolve_filesystem_request_target(&state.pool, &request.path).await {
            Ok(target) => target,
            Err(response) => return response,
        };

    if let Err(err) = db::set_sync_policy_type_for_path(&state.pool, &logical_path, policy_type).await
    {
        return internal_server_error(err);
    }

    if policy_type == "LOCAL" && inode.kind == "FILE" {
        let sync_root = sync_root_path();
        if let Err(err) = db::set_pin_state(&state.pool, inode_id, 1).await {
            return internal_server_error(err);
        }
        if let Err(err) = smart_sync::hydrate_placeholder_now(&state.pool, &sync_root, inode_id).await
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
    let (inode_id, _, inode) = match resolve_filesystem_request_target(&state.pool, &request.path).await
    {
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
            Json(serde_json::json!({ "error": "smart_sync_state_not_found", "inode_id": inode_id })),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn unpin_filesystem_path(
    State(state): State<ApiState>,
    Json(request): Json<FilesystemPathRequest>,
) -> impl IntoResponse {
    let (inode_id, _, inode) = match resolve_filesystem_request_target(&state.pool, &request.path).await
    {
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
    if let Err(err) = smart_sync::sync_placeholder_pin_state(&state.pool, &sync_root, inode_id, true).await
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
            Json(serde_json::json!({ "error": "smart_sync_state_not_found", "inode_id": inode_id })),
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

    match db::promote_revision_to_current(&state.pool, revision_id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(RestoreRevisionResponse {
                inode_id,
                revision_id,
                restored: true,
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
    let last_successful_backup = match db::get_last_successful_metadata_backup_at(&state.pool).await
    {
        Ok(value) => value,
        Err(err) => return internal_server_error(err),
    };

    let recent_attempts = match db::list_recent_metadata_backups(&state.pool, 10).await {
        Ok(records) => records
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
            .collect(),
        Err(err) => return internal_server_error(err),
    };

    (
        StatusCode::OK,
        Json(RecoveryStatusResponse {
            last_successful_backup,
            recent_attempts,
        }),
    )
        .into_response()
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
            Json(ScrubNowResponse { processed_shards }),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
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
                    "message": "unlock the vault before creating an encrypted metadata snapshot"
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
                    "message": "unlock the vault before creating an encrypted metadata backup"
                })),
            )
                .into_response();
        }
        Err(err) => return internal_server_error(err),
    };

    let provider_manager = match disaster_recovery::MetadataBackupProviderManager::from_env().await
    {
        Ok(manager) => manager,
        Err(err) => return internal_server_error(err),
    };

    match disaster_recovery::run_metadata_backup_now(
        &state.pool,
        &provider_manager,
        &master_key,
    )
    .await
    {
        Ok(()) => (StatusCode::OK, Json(BackupNowResponse { uploaded: true })).into_response(),
        Err(err) => internal_server_error(err),
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

fn internal_server_error(err: impl std::error::Error) -> axum::response::Response {
    error!("api request failed: {err}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": "internal_server_error" })),
    )
        .into_response()
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
                .into_response())
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
                .into_response())
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
                .into_response())
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
    env::var("OMNIDRIVE_SYNC_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            env::var("LOCALAPPDATA")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| {
                    env::var("USERPROFILE")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|_| std::path::PathBuf::from(r"C:\Users\Default"))
                })
                .join("OmniDrive")
                .join("SyncRoot")
        })
}
