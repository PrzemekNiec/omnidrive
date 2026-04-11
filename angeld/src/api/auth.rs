use super::error::ApiError;
use super::ApiState;
use crate::db;
use crate::runtime_paths::RuntimePaths;
use crate::smart_sync;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tracing::warn;

#[derive(Deserialize)]
struct UnlockRequest {
    passphrase: String,
}

#[derive(Serialize)]
struct UnlockResponse {
    status: String,
    initialized: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<i64>,
}

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/unlock", post(post_unlock))
        .route("/api/auth/session", get(get_auth_session))
        .route("/api/auth/logout", post(post_auth_logout))
        .route("/api/auth/renew", post(post_auth_renew))
}

async fn post_unlock(
    State(state): State<ApiState>,
    Json(request): Json<UnlockRequest>,
) -> Result<Json<UnlockResponse>, ApiError> {
    let result = state
        .vault_keys
        .unlock(&state.pool, &request.passphrase)
        .await
        .map_err(|e| ApiError::BadRequest {
            code: "unlock_failed",
            message: e.to_string(),
        })?;

    // Delete stale placeholder files and re-create them so Windows
    // issues fresh FETCH_DATA callbacks now that the vault is unlocked.
    let pool = state.pool.clone();
    tokio::spawn(async move {
        let paths = RuntimePaths::detect();
        if let Err(err) =
            smart_sync::reset_placeholders_after_unlock(&pool, &paths.sync_root).await
        {
            tracing::warn!("[UNLOCK] placeholder reset failed: {err}");
        }
    });

    // Epic 34.3a: Issue a session token for the local device/user
    let (session_token, expires_at) = match create_session_for_local_device(&state.pool).await {
        Ok(session) => (Some(session.token), Some(session.expires_at)),
        Err(err) => {
            warn!("[UNLOCK] session token creation failed: {err}");
            (None, None)
        }
    };

    Ok(Json(UnlockResponse {
        status: "UNLOCKED".to_string(),
        initialized: result.initialized,
        session_token,
        expires_at,
    }))
}

/// Look up local device identity -> find user_id -> create session.
async fn create_session_for_local_device(
    pool: &sqlx::SqlitePool,
) -> Result<db::UserSession, String> {
    let device = db::get_local_device_identity(pool)
        .await
        .map_err(|e| format!("db error: {e}"))?
        .ok_or_else(|| "no local device identity".to_string())?;

    // Find which user owns this device
    let device_rec = db::get_device(pool, &device.device_id)
        .await
        .map_err(|e| format!("db error: {e}"))?
        .ok_or_else(|| "device not in multi-user tables".to_string())?;

    let token = db::generate_session_token();
    db::create_user_session(
        pool,
        &token,
        &device_rec.user_id,
        &device.device_id,
        db::SESSION_TTL_SECONDS,
    )
    .await
    .map_err(|e| format!("session insert error: {e}"))
}

/// Extract and validate a session token from the request `Authorization: Bearer <token>` header.
/// Returns the valid session or None if missing/expired.
async fn extract_session(
    pool: &sqlx::SqlitePool,
    headers: &axum::http::HeaderMap,
) -> Option<db::UserSession> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    db::validate_user_session(pool, token).await.ok().flatten()
}

// -- Epic 34.3a: Session endpoints -------------------------------------------

/// GET /api/auth/session -- check current session validity
async fn get_auth_session(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    match extract_session(&state.pool, &headers).await {
        Some(session) => Json(serde_json::json!({
            "valid": true,
            "user_id": session.user_id,
            "device_id": session.device_id,
            "expires_at": session.expires_at,
        })),
        None => Json(serde_json::json!({
            "valid": false,
            "error": "invalid_or_expired_session",
        })),
    }
}

/// POST /api/auth/logout -- invalidate current session
async fn post_auth_logout(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(ApiError::BadRequest {
            code: "missing_authorization_header",
            message: "missing Authorization header".to_string(),
        })?;

    let deleted = db::delete_user_session(&state.pool, token).await?;
    if deleted {
        Ok(Json(serde_json::json!({ "status": "logged_out" })))
    } else {
        Err(ApiError::NotFound {
            resource: "session",
            id: "current".to_string(),
        })
    }
}

/// POST /api/auth/renew -- extend session TTL by 24h
async fn post_auth_renew(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(ApiError::BadRequest {
            code: "missing_authorization_header",
            message: "missing Authorization header".to_string(),
        })?;

    let renewed = db::renew_user_session(&state.pool, token, db::SESSION_TTL_SECONDS).await?;
    if renewed {
        let new_expires = db::epoch_secs() + db::SESSION_TTL_SECONDS;
        Ok(Json(serde_json::json!({
            "status": "renewed",
            "expires_at": new_expires,
        })))
    } else {
        Err(ApiError::Unauthorized {
            message: "invalid or expired session".to_string(),
        })
    }
}
