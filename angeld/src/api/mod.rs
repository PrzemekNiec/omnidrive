mod audit;
mod auth;
mod diagnostics;
pub mod error;
mod files;
mod maintenance;
mod onboarding;
mod recovery;
mod settings;
mod sharing;
mod stats;
mod vault;

use crate::diagnostics::{DaemonDiagnostics, WorkerKind, WorkerStatus};
use crate::downloader::Downloader;
use crate::vault::VaultKeyStore;
use axum::http::{Method, header};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use serde::Serialize;
use tower_http::cors::{AllowOrigin, CorsLayer};
use sqlx::SqlitePool;
use std::env;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use tracing::info;

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
pub enum ApiServerError {
    InvalidBindAddress(String),
    Io(std::io::Error),
}


#[derive(Clone, Copy)]
pub(super) enum MaintenanceLevel {
    Ok,
    Warn,
    Error,
}

impl MaintenanceLevel {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Serialize)]
pub(super) struct MaintenanceStatus<T>
where
    T: Serialize,
{
    pub(super) status: String,
    pub(super) message: String,
    pub(super) last_run: i64,
    #[serde(flatten)]
    pub(super) details: T,
}

#[derive(Serialize)]
pub(super) struct MaintenanceOverviewItem {
    pub(super) status: String,
    pub(super) message: String,
    pub(super) last_run: i64,
}

impl ApiServer {
    pub fn from_env(
        pool: SqlitePool,
        vault_keys: VaultKeyStore,
        diagnostics: Arc<DaemonDiagnostics>,
        downloader: Option<Arc<Downloader>>,
        runtime_reload_tx: Option<watch::Sender<u64>>,
    ) -> Result<Self, ApiServerError> {
        let _ = dotenvy::dotenv();

        let bind_addr = env::var("OMNIDRIVE_API_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8787".to_string())
            .parse::<SocketAddr>()
            .map_err(|_| ApiServerError::InvalidBindAddress("OMNIDRIVE_API_BIND".to_string()))?;

        Ok(Self {
            pool,
            vault_keys,
            diagnostics,
            downloader,
            runtime_reload_tx,
            bind_addr,
        })
    }

    pub async fn run(self) -> Result<(), ApiServerError> {
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
            .route("/legacy", get(get_legacy))
            .route("/wizard.js", get(get_wizard_js))
            .merge(onboarding::routes())
            .merge(diagnostics::routes())
            .merge(maintenance::routes())
            .merge(vault::routes())
            .merge(files::routes())
            .merge(auth::routes())
            // ── Sharing (Epic 33) ──
            .merge(sharing::routes())
            // ── Audit trail (Epic 34.5) ──
            .merge(audit::routes())
            // ── Recovery keys (Epic 34.6a) ──
            .merge(recovery::routes())
            // ── Stats (Epic 36 G-BE) ──
            .merge(stats::routes())
            // ── Settings (Epic 36 G.10) ──
            .merge(settings::routes())
            .with_state(state)
            .layer(share_cors_layer());

        let listener = tokio::net::TcpListener::bind(self.bind_addr)
            .await
            .map_err(ApiServerError::Io)?;
        diagnostics.set_worker_status(WorkerKind::Api, WorkerStatus::Idle);
        info!("api server listening on http://{}", self.bind_addr);

        axum::serve(listener, app).await.map_err(ApiServerError::Io)
    }
}

impl fmt::Display for ApiServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBindAddress(key) => {
                write!(f, "invalid bind address in environment variable {key}")
            }
            Self::Io(err) => write!(f, "api server i/o error: {err}"),
        }
    }
}

impl std::error::Error for ApiServerError {}

async fn get_index() -> Html<&'static str> {
    Html(include_str!("../../static/index.html"))
}

async fn get_legacy() -> Html<&'static str> {
    Html(include_str!("../../static/legacy.html"))
}

async fn get_wizard_js() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../../static/wizard.js"),
    )
}

pub(super) fn unix_timestamp_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// CORS layer for public share API endpoints.
/// Allows cross-origin access from skarbiec.app and localhost (dev).
fn share_cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            let origin = origin.as_bytes();
            origin == b"https://skarbiec.app"
                || origin.starts_with(b"http://localhost")
                || origin.starts_with(b"http://127.0.0.1")
        }))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            header::CONTENT_TYPE,
            header::HeaderName::from_static("x-share-token"),
        ])
}


