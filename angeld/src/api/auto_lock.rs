use super::ApiState;
use super::error::ApiError;
use crate::acl;
use crate::auto_lock::{AutoLockError, DEFAULT_IDLE_MIN, MONITOR, WARNING_THRESHOLD_SECS};
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct SetTimeoutRequest {
    idle_timeout_min: u32,
}

#[derive(Serialize)]
struct AutoLockStatusResponse {
    idle_timeout_min: u32,
    remaining_seconds: u64,
    state: &'static str,
}

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/auto-lock/timeout", post(post_timeout))
        .route("/api/auto-lock/status", get(get_status))
        .route("/api/auto-lock/touch", post(post_touch))
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

async fn get_status(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<AutoLockStatusResponse>, ApiError> {
    let _ = acl::require_session_no_touch(&state.pool, &headers).await?;
    let mon = MONITOR.get().ok_or(ApiError::Internal {
        message: "auto-lock monitor not initialized".into(),
    })?;
    let vault_locked = state.vault_keys.require_key().await.is_err();
    let rem = mon.remaining_secs();
    let state_str = if vault_locked {
        "locked"
    } else if rem == 0 {
        "expired"
    } else if rem <= WARNING_THRESHOLD_SECS {
        "warning"
    } else {
        "active"
    };
    let idle_min = u32::try_from(mon.idle_timeout_secs() / 60).unwrap_or(DEFAULT_IDLE_MIN);
    Ok(Json(AutoLockStatusResponse {
        idle_timeout_min: idle_min,
        remaining_seconds: rem,
        state: state_str,
    }))
}

async fn post_touch(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let _ = acl::require_session(&state.pool, &headers).await?;
    crate::auto_lock::touch(crate::auto_lock::TouchSource::ManualExtend);
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(feature = "test-helpers")]
pub(crate) fn test_routes() -> Router<ApiState> {
    Router::new().route(
        "/api/auto-lock/_test/simulate-session-lock",
        post(post_test_simulate),
    )
}

#[cfg(feature = "test-helpers")]
async fn post_test_simulate(State(state): State<ApiState>) -> Result<StatusCode, ApiError> {
    #[cfg(target_os = "windows")]
    let dispatched = crate::win_session::test_dispatcher_tx()
        .map(|tx| {
            let _ = tx.send(crate::win_session::SessionEvent::Lock);
        })
        .is_some();
    #[cfg(not(target_os = "windows"))]
    let dispatched = false;

    if !dispatched {
        crate::lock_flow::force_lock_and_dismount(
            &state.pool,
            &state.vault_keys,
            crate::lock_flow::LockReason::WinSessionLock,
            None,
        )
        .await;
    }
    Ok(StatusCode::NO_CONTENT)
}
