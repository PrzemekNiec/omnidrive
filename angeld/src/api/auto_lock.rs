//! α.A.b.1 — REST endpoint for auto-lock configuration.
//!
//! Only `POST /api/auto-lock/timeout` is wired in α.A.b.1.
//! `GET /api/auto-lock/status` and `POST /api/auto-lock/touch` are added in α.A.b.2.

use super::ApiState;
use super::error::ApiError;
use crate::acl;
use crate::auto_lock::{AutoLockError, MONITOR};
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use serde::Deserialize;

#[derive(Deserialize)]
struct SetTimeoutRequest {
    idle_timeout_min: u32,
}

pub fn routes() -> Router<ApiState> {
    Router::new().route("/api/auto-lock/timeout", post(post_timeout))
    // GET /api/auto-lock/status + POST /api/auto-lock/touch are added in α.A.b.2 — do NOT add here.
}

async fn post_timeout(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(body): Json<SetTimeoutRequest>,
) -> Result<StatusCode, ApiError> {
    let _ = acl::require_session(&state.pool, &headers).await?;
    let mon = MONITOR.get().ok_or(ApiError::Internal {
        message: "auto-lock monitor not initialized".into(),
    })?;
    match mon.set_timeout_minutes(body.idle_timeout_min).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(AutoLockError::InvalidPreset(n)) => Err(ApiError::BadRequest {
            code: "invalid_preset",
            message: format!("idle_timeout_min={n} not in [5,15,30,60]"),
        }),
        Err(e) => Err(ApiError::Internal {
            message: e.to_string(),
        }),
    }
}
