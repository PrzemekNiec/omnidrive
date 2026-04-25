use super::error::ApiError;
use super::ApiState;
use crate::db;
use crate::runtime_paths::RuntimePaths;
use crate::smart_sync;
use crate::windows_hello;

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
        .route("/api/unlock/windows-hello", post(post_windows_hello_unlock))
        .route("/api/unlock/hello-available", get(get_hello_available))
        .route("/api/auth/session", get(get_auth_session))
        .route("/api/auth/logout", post(post_auth_logout))
        .route("/api/auth/renew", post(post_auth_renew))
        .route("/api/change-password", post(post_change_password))
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

    // Silently store passphrase in Windows Credential Manager (DPAPI-encrypted) so
    // that subsequent unlocks can use Windows Hello without retyping the passphrase.
    if let Err(err) = windows_hello::store_passphrase(&request.passphrase) {
        warn!("[UNLOCK] windows_hello store failed (non-fatal): {err}");
    }

    // Lazy mount: register CF sync root, project placeholders, hide OmniSync dir,
    // mount virtual drive.  Happens only after a successful passphrase unlock.
    let pool = state.pool.clone();
    tokio::spawn(async move {
        let paths = RuntimePaths::detect();

        // 1. CF register + connect + project placeholders.
        if let Err(err) = smart_sync::mount_after_unlock(&pool, &paths.sync_root).await {
            tracing::warn!("[UNLOCK] CF mount failed: {err}");
            return;
        }

        // 2. Hide OmniSync dir and mount virtual drive.
        let preferred = std::env::var("OMNIDRIVE_DRIVE_LETTER").unwrap_or_else(|_| "O:".to_string());
        if let Err(err) = crate::virtual_drive::hide_sync_root(&paths.sync_root) {
            tracing::warn!("[UNLOCK] hide_sync_root failed: {err}");
        }
        let letter = crate::virtual_drive::select_mount_drive_letter(&preferred)
            .unwrap_or_else(|_| preferred.clone());
        if let Err(err) = crate::virtual_drive::mount_virtual_drive(&letter, &paths.sync_root) {
            tracing::warn!("[UNLOCK] virtual drive mount failed: {err}");
        } else {
            tracing::info!("[UNLOCK] vault mounted at {letter}");
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

    if let Ok(Some(vault)) = db::get_vault_params(&state.pool).await {
        let actor_device = db::get_local_device_identity(&state.pool)
            .await
            .ok()
            .flatten()
            .map(|d| d.device_id);
        let _ = db::insert_audit_log(
            &state.pool,
            &vault.vault_id,
            "vault_unlock",
            None,
            actor_device.as_deref(),
            None,
            None,
            Some(r#"{"result":"success"}"#),
        )
        .await;
    }

    Ok(Json(UnlockResponse {
        status: "UNLOCKED".to_string(),
        initialized: result.initialized,
        session_token,
        expires_at,
    }))
}

/// Look up local device identity -> find user_id -> create session.
pub(super) async fn create_session_for_local_device(
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
pub(super) async fn extract_session(
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
        Some(session) => {
            let user = db::get_user(&state.pool, &session.user_id).await.ok().flatten();
            Json(serde_json::json!({
                "valid": true,
                "user_id": session.user_id,
                "device_id": session.device_id,
                "expires_at": session.expires_at,
                "email": user.as_ref().and_then(|u| u.email.as_deref()),
                "display_name": user.as_ref().map(|u| u.display_name.as_str()),
            }))
        },
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

    // Capture session identity before deleting so we can emit an audit event
    let session_before = db::validate_user_session(&state.pool, token).await.ok().flatten();

    let deleted = db::delete_user_session(&state.pool, token).await?;
    if deleted {
        if let (Some(session), Ok(Some(vault))) =
            (session_before, db::get_vault_params(&state.pool).await)
        {
            let _ = db::insert_audit_log(
                &state.pool,
                &vault.vault_id,
                "logout",
                Some(&session.user_id),
                Some(&session.device_id),
                None,
                None,
                None,
            )
            .await;
        }
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

// -- Change password ----------------------------------------------------------

#[derive(Deserialize)]
struct ChangePasswordRequest {
    old_passphrase: String,
    new_passphrase: String,
}

#[derive(Serialize)]
struct ChangePasswordResponse {
    status: String,
    deks_rewrapped: u64,
    new_generation: i64,
}

/// POST /api/change-password — re-derives KEK with a new passphrase, re-wraps the
/// Vault Key and all DEKs in one atomic batch. Requires a valid session token and
/// the current passphrase as confirmation.
async fn post_change_password(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<Json<ChangePasswordResponse>, ApiError> {
    extract_session(&state.pool, &headers).await.ok_or(ApiError::Unauthorized {
        message: "valid session required to change password".to_string(),
    })?;

    if request.new_passphrase.is_empty() {
        return Err(ApiError::BadRequest {
            code: "empty_passphrase",
            message: "new passphrase must not be empty".to_string(),
        });
    }

    let valid = state
        .vault_keys
        .verify_passphrase(&state.pool, &request.old_passphrase)
        .await
        .map_err(|e| ApiError::Internal { message: e.to_string() })?;
    if !valid {
        return Err(ApiError::BadRequest {
            code: "wrong_passphrase",
            message: "current passphrase is incorrect".to_string(),
        });
    }

    let result = state
        .vault_keys
        .rotate_vault_key(&state.pool, &request.new_passphrase)
        .await
        .map_err(|e| ApiError::Internal { message: e.to_string() })?;

    // Update Windows Hello credential so subsequent Hello unlocks use the new passphrase.
    if let Err(err) = windows_hello::store_passphrase(&request.new_passphrase) {
        warn!("[CHANGE_PASSWORD] windows_hello update failed (non-fatal): {err}");
    }

    if let Ok(Some(vault)) = db::get_vault_params(&state.pool).await {
        let actor_device = db::get_local_device_identity(&state.pool)
            .await
            .ok()
            .flatten()
            .map(|d| d.device_id);
        let details = format!(
            r#"{{"deks_rewrapped":{},"new_generation":{}}}"#,
            result.deks_rewrapped, result.new_generation
        );
        let _ = db::insert_audit_log(
            &state.pool,
            &vault.vault_id,
            "password_changed",
            None,
            actor_device.as_deref(),
            None,
            None,
            Some(&details),
        )
        .await;
    }

    Ok(Json(ChangePasswordResponse {
        status: "ok".to_string(),
        deks_rewrapped: result.deks_rewrapped,
        new_generation: result.new_generation,
    }))
}

// -- Windows Hello endpoints --------------------------------------------------

/// GET /api/unlock/hello-available — returns whether a DPAPI credential is stored.
async fn get_hello_available() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "available": windows_hello::has_stored_credential() }))
}

/// POST /api/unlock/windows-hello — unlock vault using stored DPAPI credential.
/// The OS has already authenticated the user (fingerprint/PIN at Windows login);
/// DPAPI decryption succeeds transparently within the active session.
async fn post_windows_hello_unlock(
    State(state): State<ApiState>,
) -> Result<Json<UnlockResponse>, ApiError> {
    let passphrase = windows_hello::retrieve_passphrase()
        .map_err(|e| ApiError::Internal { message: e })?
        .ok_or(ApiError::NotFound {
            resource: "windows_hello_credential",
            id: "vault".to_string(),
        })?;

    let result = state
        .vault_keys
        .unlock(&state.pool, &passphrase)
        .await
        .map_err(|e| ApiError::BadRequest {
            code: "unlock_failed",
            message: e.to_string(),
        })?;

    // Lazy mount (same as passphrase unlock)
    let pool2 = state.pool.clone();
    tokio::spawn(async move {
        let paths = RuntimePaths::detect();
        if let Err(err) = smart_sync::mount_after_unlock(&pool2, &paths.sync_root).await {
            tracing::warn!("[WH_UNLOCK] CF mount failed: {err}");
            return;
        }
        let preferred = std::env::var("OMNIDRIVE_DRIVE_LETTER").unwrap_or_else(|_| "O:".to_string());
        if let Err(err) = crate::virtual_drive::hide_sync_root(&paths.sync_root) {
            tracing::warn!("[WH_UNLOCK] hide_sync_root failed: {err}");
        }
        let letter = crate::virtual_drive::select_mount_drive_letter(&preferred)
            .unwrap_or_else(|_| preferred.clone());
        if let Err(err) = crate::virtual_drive::mount_virtual_drive(&letter, &paths.sync_root) {
            tracing::warn!("[WH_UNLOCK] virtual drive mount failed: {err}");
        } else {
            tracing::info!("[WH_UNLOCK] vault mounted at {letter}");
        }
    });

    let (session_token, expires_at) = match create_session_for_local_device(&state.pool).await {
        Ok(session) => (Some(session.token), Some(session.expires_at)),
        Err(err) => {
            warn!("[WH_UNLOCK] session token creation failed: {err}");
            (None, None)
        }
    };

    if let Ok(Some(vault)) = db::get_vault_params(&state.pool).await {
        let actor_device = db::get_local_device_identity(&state.pool)
            .await
            .ok()
            .flatten()
            .map(|d| d.device_id);
        let _ = db::insert_audit_log(
            &state.pool,
            &vault.vault_id,
            "vault_unlock",
            None,
            actor_device.as_deref(),
            None,
            None,
            Some(r#"{"result":"success","method":"windows_hello"}"#),
        )
        .await;
    }

    Ok(Json(UnlockResponse {
        status: "UNLOCKED".to_string(),
        initialized: result.initialized,
        session_token,
        expires_at,
    }))
}
