use crate::db;
use crate::uploader::KNOWN_PROVIDERS;
use crate::vault::VaultKeyStore;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
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
            .route("/api/transfers", get(get_transfers))
            .route("/api/health", get(get_health))
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
