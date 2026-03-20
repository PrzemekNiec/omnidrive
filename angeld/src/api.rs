use crate::config::AppConfig;
use crate::db;
use crate::uploader::KNOWN_PROVIDERS;
use crate::vault::VaultKeyStore;
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

#[derive(Clone)]
struct ApiState {
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
}

pub struct ApiServer {
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
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
struct ProviderQuotaResponse {
    provider: String,
    used_physical_bytes: u64,
}

impl ApiServer {
    pub fn from_env(pool: SqlitePool, vault_keys: VaultKeyStore) -> Result<Self, ApiError> {
        let _ = dotenvy::dotenv();

        let bind_addr = env::var("OMNIDRIVE_API_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8787".to_string())
            .parse::<SocketAddr>()
            .map_err(|_| ApiError::InvalidBindAddress("OMNIDRIVE_API_BIND".to_string()))?;

        Ok(Self {
            pool,
            vault_keys,
            bind_addr,
        })
    }

    pub async fn run(self) -> Result<(), ApiError> {
        let state = ApiState {
            pool: self.pool,
            vault_keys: self.vault_keys,
        };
        let app = Router::new()
            .route("/", get(get_index))
            .route("/api/transfers", get(get_transfers))
            .route("/api/health", get(get_health))
            .route("/api/health/vault", get(get_vault_health))
            .route("/api/files", get(get_files))
            .route("/api/files/{inode_id}", delete(delete_file))
            .route("/api/files/{inode_id}/revisions", get(get_file_revisions))
            .route(
                "/api/files/{inode_id}/revisions/{revision_id}/restore",
                post(restore_file_revision),
            )
            .route("/api/quota", get(get_quota))
            .route("/api/unlock", post(post_unlock))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(self.bind_addr)
            .await
            .map_err(ApiError::Io)?;
        println!("api server listening on http://{}", self.bind_addr);

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
                    })
                    .collect::<Vec<_>>(),
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
    eprintln!("api request failed: {err}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": "internal_server_error" })),
    )
        .into_response()
}
