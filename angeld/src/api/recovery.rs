//! Epic 34.6a: Recovery Keys HTTP API.
//!
//! Routes:
//!   POST /api/recovery/generate  (Owner, vault unlocked)  → 24-word mnemonic
//!   POST /api/recovery/restore   (no auth)                → set new passphrase
//!   POST /api/recovery/revoke    (Owner)                  → invalidate all keys
//!
//! Restore does NOT rotate the envelope Vault Key, so DEKs are untouched.  It
//! derives a fresh Argon2 salt, re-wraps the existing Vault Key with a KEK
//! derived from the new passphrase, and rewrites `vault_state`.

use crate::acl::{self, Role};
use crate::db;
use crate::recovery;
use crate::vault::VaultError;

use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use omnidrive_core::crypto::{KeyBytes, RootKdfParams, WRAPPED_KEY_LEN, derive_root_keys, wrap_key};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use super::error::ApiError;
use super::ApiState;

pub(super) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/recovery/generate", post(generate_recovery_key))
        .route("/api/recovery/restore", post(restore_from_recovery_key))
        .route("/api/recovery/revoke", post(revoke_recovery_keys))
        .route("/api/recovery/status", get(recovery_status))
}

// ── GET /api/recovery/status ───────────────────────────────────────────

#[derive(Serialize)]
struct StatusResponse {
    active_count: usize,
    last_created_at: Option<i64>,
    vk_generation: i64,
    word_count: usize,
}

async fn recovery_status(
    State(state): State<ApiState>,
) -> Result<Json<StatusResponse>, ApiError> {
    let vault = db::get_vault_params(&state.pool)
        .await?
        .ok_or(ApiError::BadRequest {
            code: "vault_not_initialized",
            message: "vault not initialized".to_string(),
        })?;
    let active = db::list_active_recovery_keys(&state.pool, &vault.vault_id).await?;
    let last_created_at = active.iter().map(|r| r.created_at).max();
    Ok(Json(StatusResponse {
        active_count: active.len(),
        last_created_at,
        vk_generation: vault.vault_key_generation.unwrap_or(1),
        word_count: recovery::RECOVERY_WORD_COUNT,
    }))
}

// ── POST /api/recovery/generate ─────────────────────────────────────────

#[derive(Serialize)]
struct GenerateResponse {
    mnemonic: String,
    word_count: usize,
    vk_generation: i64,
    recovery_key_id: i64,
}

async fn generate_recovery_key(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<GenerateResponse>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Owner).await?;

    let envelope_key = state.vault_keys.require_envelope_key().await.map_err(|err| match err {
        VaultError::Locked => ApiError::BadRequest {
            code: "vault_locked",
            message: "odblokuj Skarbiec przed wygenerowaniem klucza odzyskiwania".to_string(),
        },
        other => ApiError::Internal { message: other.to_string() },
    })?;

    let vault = db::get_vault_params(&state.pool)
        .await?
        .ok_or(ApiError::BadRequest {
            code: "vault_not_initialized",
            message: "vault not initialized".to_string(),
        })?;
    let vk_generation = vault.vault_key_generation.unwrap_or(1);

    let mnemonic = recovery::generate_mnemonic();
    let recovery_key = recovery::derive_recovery_key(&mnemonic);
    let wrapped = recovery::wrap_vault_key(&recovery_key, &envelope_key)
        .map_err(|err: recovery::RecoveryError| ApiError::Internal { message: err.to_string() })?;

    let recovery_key_id = db::insert_recovery_key(
        &state.pool,
        &caller.vault_id,
        &wrapped,
        vk_generation,
        Some(&caller.user_id),
    )
    .await?;

    let _ = db::insert_audit_log(
        &state.pool,
        &caller.vault_id,
        "recovery_key_generate",
        Some(&caller.user_id),
        Some(&caller.device_id),
        None,
        None,
        Some(&format!(
            "{{\"recovery_key_id\":{},\"vk_generation\":{}}}",
            recovery_key_id, vk_generation
        )),
    )
    .await;

    Ok(Json(GenerateResponse {
        mnemonic: mnemonic.to_string(),
        word_count: recovery::RECOVERY_WORD_COUNT,
        vk_generation,
        recovery_key_id,
    }))
}

// ── POST /api/recovery/restore ──────────────────────────────────────────

#[derive(Deserialize)]
struct RestoreRequest {
    mnemonic: String,
    new_passphrase: String,
}

#[derive(Serialize)]
struct RestoreResponse {
    restored: bool,
    vk_generation: i64,
}

async fn restore_from_recovery_key(
    State(state): State<ApiState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<RestoreRequest>,
) -> Result<Json<RestoreResponse>, ApiError> {
    let ip = addr.ip();
    let ua = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .chars()
        .take(200)
        .collect::<String>();

    // ── Rate-limit check ──────────────────────────────────────────────
    if let Err(retry_after) = state.recovery_limiter.check(ip) {
        return Err(ApiError::TooManyRequests {
            retry_after_secs: retry_after,
            message: format!("too many recovery attempts — retry after {retry_after}s"),
        });
    }

    if request.new_passphrase.is_empty() {
        return Err(ApiError::BadRequest {
            code: "empty_passphrase",
            message: "new passphrase cannot be empty".to_string(),
        });
    }

    let vault = db::get_vault_params(&state.pool)
        .await?
        .ok_or(ApiError::BadRequest {
            code: "vault_not_initialized",
            message: "vault not initialized".to_string(),
        })?;

    // ── State-guard ────────────────────────────────────────────────────
    // Block if vault is active and there was a restore attempt in the last 24h,
    // unless recovery_unlocked_until in system_config extends past now.
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let unlocked_until: i64 = db::get_system_config_value(&state.pool, "recovery_unlocked_until")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);

    if unlocked_until <= now_secs {
        let window_secs = now_secs - 86_400;
        let last_attempt: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(timestamp) FROM audit_logs \
             WHERE vault_id = ? AND action IN ('recovery_key_restore', 'recovery_restore_failure') \
             AND timestamp >= ?",
        )
        .bind(&vault.vault_id)
        .bind(window_secs)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(None);

        if last_attempt.is_some() {
            let _ = db::insert_audit_log(
                &state.pool,
                &vault.vault_id,
                "recovery_restore_blocked",
                None,
                None,
                None,
                None,
                Some(&format!(r#"{{"ip":"{ip}","ua":{ua:?}}}"#)),
            )
            .await;
            return Err(ApiError::Forbidden {
                message: "recovery blocked: a restore attempt was recorded within the last 24h on this active vault".to_string(),
            });
        }
    }

    // ── Audit: record this attempt (no mnemonic) ───────────────────────
    let _ = db::insert_audit_log(
        &state.pool,
        &vault.vault_id,
        "recovery_restore_attempt",
        None,
        None,
        None,
        None,
        Some(&format!(r#"{{"ip":"{ip}","ua":{ua:?}}}"#)),
    )
    .await;

    // ── Mnemonic validation ────────────────────────────────────────────
    let mnemonic = match recovery::parse_mnemonic(request.mnemonic.trim()) {
        Ok(m) => m,
        Err(err) => {
            state.recovery_limiter.record_failure(ip);
            let _ = db::insert_audit_log(
                &state.pool,
                &vault.vault_id,
                "recovery_restore_failure",
                None,
                None,
                None,
                None,
                Some(&format!(r#"{{"ip":"{ip}","reason":"invalid_mnemonic","ua":{ua:?}}}"#)),
            )
            .await;
            return Err(ApiError::BadRequest {
                code: "invalid_mnemonic",
                message: err.to_string(),
            });
        }
    };
    let recovery_key = recovery::derive_recovery_key(&mnemonic);

    let records = db::list_active_recovery_keys(&state.pool, &vault.vault_id).await?;
    if records.is_empty() {
        return Err(ApiError::NotFound {
            resource: "recovery_key",
            id: vault.vault_id.clone(),
        });
    }

    // Try each active recovery key — AES-KW's integrity check makes a wrong
    // mnemonic immediately fail.
    let envelope_key: KeyBytes = match records.iter().find_map(|r| {
        let wrapped: [u8; WRAPPED_KEY_LEN] = r.wrapped_vault_key.as_slice().try_into().ok()?;
        recovery::unwrap_vault_key(&recovery_key, &wrapped).ok()
    }) {
        Some(k) => k,
        None => {
            state.recovery_limiter.record_failure(ip);
            let _ = db::insert_audit_log(
                &state.pool,
                &vault.vault_id,
                "recovery_restore_failure",
                None,
                None,
                None,
                None,
                Some(&format!(r#"{{"ip":"{ip}","reason":"mnemonic_mismatch","ua":{ua:?}}}"#)),
            )
            .await;
            return Err(ApiError::Unauthorized {
                message: "recovery mnemonic did not match any active key".to_string(),
            });
        }
    };

    // Derive new root keys with a fresh salt, reusing the existing Argon2
    // cost parameters.
    let cfg = db::get_vault_config(&state.pool)
        .await?
        .ok_or(ApiError::Internal {
            message: "missing vault_config".to_string(),
        })?;
    let new_salt = RootKdfParams::random_salt();
    let params = RootKdfParams::new(
        u32::try_from(cfg.parameter_set_version).map_err(|_| ApiError::Internal {
            message: "invalid parameter_set_version".to_string(),
        })?,
        new_salt.to_vec(),
        u32::try_from(cfg.memory_cost_kib).map_err(|_| ApiError::Internal {
            message: "invalid memory_cost_kib".to_string(),
        })?,
        u32::try_from(cfg.time_cost).map_err(|_| ApiError::Internal {
            message: "invalid time_cost".to_string(),
        })?,
        u32::try_from(cfg.lanes).map_err(|_| ApiError::Internal {
            message: "invalid lanes".to_string(),
        })?,
    );
    let new_root_keys = derive_root_keys(request.new_passphrase.as_bytes(), &params).map_err(
        |err| ApiError::Internal {
            message: err.to_string(),
        },
    )?;

    let new_wrapped = wrap_key(&new_root_keys.kek, &envelope_key).map_err(|err| {
        ApiError::Internal {
            message: err.to_string(),
        }
    })?;

    // Keep the same VK generation — the envelope key hasn't changed, so DEKs
    // still point at the right VK.
    let generation = vault.vault_key_generation.unwrap_or(1);
    let argon2_params_json = format!(
        r#"{{"mode":"LOCAL_VAULT","parameter_set_version":{},"memory_cost_kib":{},"time_cost":{},"lanes":{}}}"#,
        params.parameter_set_version,
        params.memory_cost_kib,
        params.time_cost,
        params.lanes
    );
    db::rotate_vault_state(
        &state.pool,
        &new_salt,
        &argon2_params_json,
        &new_wrapped,
        generation,
    )
    .await?;
    db::set_vault_config(
        &state.pool,
        &new_salt,
        i64::from(params.parameter_set_version),
        i64::from(params.memory_cost_kib),
        i64::from(params.time_cost),
        i64::from(params.lanes),
    )
    .await?;

    state.recovery_limiter.record_success(ip);
    let _ = db::insert_audit_log(
        &state.pool,
        &vault.vault_id,
        "recovery_key_restore",
        None,
        None,
        None,
        None,
        Some(&format!(r#"{{"vk_generation":{generation},"ip":"{ip}","ua":{ua:?}}}"#)),
    )
    .await;

    Ok(Json(RestoreResponse {
        restored: true,
        vk_generation: generation,
    }))
}

// ── POST /api/recovery/revoke ───────────────────────────────────────────

#[derive(Serialize)]
struct RevokeResponse {
    revoked_count: u64,
}

async fn revoke_recovery_keys(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<RevokeResponse>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Owner).await?;

    let revoked_count = db::revoke_all_recovery_keys(&state.pool, &caller.vault_id).await?;

    let _ = db::insert_audit_log(
        &state.pool,
        &caller.vault_id,
        "recovery_key_revoke",
        Some(&caller.user_id),
        Some(&caller.device_id),
        None,
        None,
        Some(&format!("{{\"revoked_count\":{}}}", revoked_count)),
    )
    .await;

    Ok(Json(RevokeResponse { revoked_count }))
}
