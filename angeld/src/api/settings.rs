use crate::autostart;
use crate::runtime_paths::RuntimePaths;

use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::error::ApiError;
use super::ApiState;

#[derive(Serialize)]
struct SettingsPathsResponse {
    log_dir: String,
    spool_dir: String,
    cache_dir: String,
}

#[derive(Deserialize)]
struct AutostartRequest {
    enabled: bool,
}

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/settings/paths", get(get_paths))
        .route("/api/settings/autostart", post(post_autostart))
        .route("/api/settings/restart-daemon", post(post_restart_daemon))
}

async fn get_paths() -> Json<SettingsPathsResponse> {
    let paths = RuntimePaths::detect();
    Json(SettingsPathsResponse {
        log_dir: paths.log_dir.to_string_lossy().into_owned(),
        spool_dir: paths.spool_dir.to_string_lossy().into_owned(),
        cache_dir: paths.cache_dir.to_string_lossy().into_owned(),
    })
}

async fn post_autostart(
    Json(req): Json<AutostartRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if req.enabled {
        let cmd = autostart::default_current_user_autostart_command()
            .map_err(|e: crate::autostart::AutostartError| ApiError::Internal { message: e.to_string() })?;
        autostart::register_current_user_autostart(&cmd)
            .map_err(|e: crate::autostart::AutostartError| ApiError::Internal { message: e.to_string() })?;
    } else {
        autostart::unregister_current_user_autostart()
            .map_err(|e: crate::autostart::AutostartError| ApiError::Internal { message: e.to_string() })?;
    }
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

async fn post_restart_daemon() -> impl IntoResponse {
    tokio::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        std::process::exit(0);
    });
    Json(serde_json::json!({ "status": "restarting" }))
}
