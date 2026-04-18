use crate::autostart;
use crate::runtime_paths::RuntimePaths;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::auth::extract_session;
use super::error::ApiError;
use super::ApiState;

#[derive(Serialize)]
struct SettingsPathsResponse {
    log_dir: String,
    spool_dir: String,
}

#[derive(Serialize)]
struct AutostartResponse {
    status: &'static str,
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

async fn get_paths(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<SettingsPathsResponse>, ApiError> {
    extract_session(&state.pool, &headers)
        .await
        .ok_or(ApiError::Unauthorized { message: "session required".into() })?;
    let paths = RuntimePaths::detect();
    Ok(Json(SettingsPathsResponse {
        log_dir: paths.log_dir.to_string_lossy().into_owned(),
        spool_dir: paths.spool_dir.to_string_lossy().into_owned(),
    }))
}

async fn post_autostart(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<AutostartRequest>,
) -> Result<Json<AutostartResponse>, ApiError> {
    extract_session(&state.pool, &headers)
        .await
        .ok_or(ApiError::Unauthorized { message: "session required".into() })?;

    if req.enabled {
        let cmd = autostart::default_current_user_autostart_command()
            .map_err(ApiError::from)?;
        autostart::register_current_user_autostart(&cmd)
            .map_err(ApiError::from)?;
        info!("autostart enabled for current user");
    } else {
        autostart::unregister_current_user_autostart()
            .map_err(ApiError::from)?;
        info!("autostart disabled for current user");
    }
    Ok(Json(AutostartResponse { status: "ok" }))
}

async fn post_restart_daemon(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    extract_session(&state.pool, &headers)
        .await
        .ok_or(ApiError::Unauthorized { message: "session required".into() })?;

    // Signal the API server's graceful-shutdown future; main.rs select! then
    // runs the normal cleanup path (SyncRoot, virtual drive, pool close).
    let _ = state.daemon_shutdown_tx.send(true);
    Ok(Json(serde_json::json!({ "status": "restarting" })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_error::ApiError;

    #[test]
    fn autostart_error_missing_exe_maps_to_bad_request() {
        use std::path::PathBuf;
        let err = autostart::AutostartError::MissingExecutable(PathBuf::from("angeld.exe"));
        let api_err = ApiError::from(err);
        assert!(matches!(api_err, ApiError::BadRequest { code: "autostart_target_missing", .. }));
    }

    #[test]
    fn autostart_error_platform_maps_to_internal() {
        let err = autostart::AutostartError::Platform("unsupported".into());
        let api_err = ApiError::from(err);
        assert!(matches!(api_err, ApiError::Internal { .. }));
    }

    #[test]
    fn autostart_error_io_maps_to_internal() {
        let err = autostart::AutostartError::Io(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"));
        let api_err = ApiError::from(err);
        assert!(matches!(api_err, ApiError::Internal { .. }));
    }

    #[test]
    fn settings_paths_response_fields_are_non_empty_on_detect() {
        let paths = RuntimePaths::detect();
        assert!(!paths.log_dir.as_os_str().is_empty());
        assert!(!paths.spool_dir.as_os_str().is_empty());
    }
}
