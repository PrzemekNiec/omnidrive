mod audit;
mod auth;
mod diagnostics;
pub mod error;
mod files;
mod maintenance;
mod oauth;
mod onboarding;
mod recovery;
mod settings;
mod sharing;
mod stats;
mod vault;

use crate::diagnostics::{DaemonDiagnostics, WorkerKind, WorkerStatus};
use crate::downloader::Downloader;
use crate::vault::VaultKeyStore;
use axum::http::{Method, header, HeaderName};
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
    daemon_shutdown_tx: Arc<watch::Sender<bool>>,
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
        let (daemon_shutdown_tx, daemon_shutdown_rx) = watch::channel(false);
        let state = ApiState {
            pool: self.pool,
            vault_keys: self.vault_keys,
            diagnostics: diagnostics.clone(),
            downloader: self.downloader,
            runtime_reload_tx: self.runtime_reload_tx,
            daemon_shutdown_tx: Arc::new(daemon_shutdown_tx),
        };
        let app = Router::new()
            .route("/", get(get_index))
            .route("/legacy", get(get_legacy))
            .route("/wizard", get(get_wizard))
            .route("/wizard.js", get(get_wizard_js))
            .route("/qrcode.min.js", get(get_qrcode_js))
            .merge(onboarding::routes())
            .merge(diagnostics::routes())
            .merge(maintenance::routes())
            .merge(vault::routes())
            .merge(files::routes())
            .merge(auth::routes())
            // ── Sharing (Epic 33) — CORS layer scoped only to share routes ──
            .merge(sharing::routes().layer(share_cors_layer()))
            // ── Audit trail (Epic 34.5) ──
            .merge(audit::routes())
            // ── Recovery keys (Epic 34.6a) ──
            .merge(recovery::routes())
            // ── Stats (Epic 36 G-BE) ──
            .merge(stats::routes())
            // ── Settings (Epic 36 G.10) ──
            .merge(settings::routes())
            // ── Google OAuth2 (Sesja C) ──
            .merge(oauth::routes())
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(self.bind_addr)
            .await
            .map_err(ApiServerError::Io)?;
        diagnostics.set_worker_status(WorkerKind::Api, WorkerStatus::Idle);
        info!("api server listening on http://{}", self.bind_addr);

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let mut rx = daemon_shutdown_rx;
                rx.changed().await.ok();
            })
            .await
            .map_err(ApiServerError::Io)
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

async fn get_wizard() -> impl IntoResponse {
    (
        [
            (header::CACHE_CONTROL, "no-store"),
            (HeaderName::from_static("x-frame-options"), "DENY"),
            (HeaderName::from_static("referrer-policy"), "no-referrer"),
            (HeaderName::from_static("content-security-policy"),
             "default-src 'self'; script-src 'self' https://cdn.tailwindcss.com 'unsafe-inline'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'"),
        ],
        Html(include_str!("../../static/wizard.html")),
    )
}

async fn get_wizard_js() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        include_str!("../../static/wizard.js"),
    )
}

async fn get_qrcode_js() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../../static/qrcode.min.js"),
    )
}

pub(super) fn unix_timestamp_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// CORS layer for share API endpoints.
///
/// Loopback + private-LAN origins only. Public domains (skarbiec.app, github.io)
/// must NEVER be allowlisted — daemon stays deaf to the external internet.
/// Rationale:
/// - Tryb A (LAN Share): decryptor served from this same daemon → same-origin, no CORS needed
/// - Tryb B (Public Share): decryptor on GH Pages reads directly from B2/R2, daemon not involved
/// - Adding public origins = attack surface (XSS on GH Pages → fetch to LAN daemon with stolen session)
fn share_cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            is_allowed_origin(origin.as_bytes())
        }))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            header::CONTENT_TYPE,
            header::HeaderName::from_static("x-share-token"),
        ])
}

/// Extract the bare hostname from an `http://host` or `http://host:port` origin.
fn host_from_http_origin(origin: &[u8]) -> Option<&[u8]> {
    let rest = origin.strip_prefix(b"http://")?;
    Some(match rest.iter().position(|&b| b == b':') {
        Some(i) => &rest[..i],
        None => rest,
    })
}

/// True for loopback and RFC-1918 private-LAN origins only.
/// Parses host exactly — prevents prefix-bypass attacks like `localhost.evil.com`.
fn is_allowed_origin(origin: &[u8]) -> bool {
    let Some(host) = host_from_http_origin(origin) else {
        return false;
    };
    if host == b"localhost" || host == b"127.0.0.1" {
        return true;
    }
    let Ok(s) = std::str::from_utf8(host) else { return false; };
    let Ok(ip) = s.parse::<std::net::IpAddr>() else { return false; };
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 10
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
        }
        std::net::IpAddr::V6(_) => false,
    }
}


