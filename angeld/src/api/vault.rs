use crate::acl::{self, Role};
use crate::db;
use crate::identity;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

use super::{internal_server_error, io_error, ApiState};

// ── Request / Response structs ──────────────────────────────────────

#[derive(Serialize)]
struct VaultHealthResponse {
    total_packs: i64,
    healthy_packs: i64,
    degraded_packs: i64,
    unreadable_packs: i64,
}

#[derive(Deserialize)]
struct InviteRequest {
    role: Option<String>,
    max_uses: Option<i64>,
    expires_in_secs: Option<i64>,
}

#[derive(Serialize)]
struct InviteResponse {
    code: String,
    role: String,
    max_uses: i64,
    expires_at: Option<i64>,
}

#[derive(Deserialize)]
struct JoinRequest {
    invite_code: String,
    device_id: String,
    device_name: String,
    public_key: String, // base64
}

#[derive(Serialize)]
struct JoinResponse {
    user_id: String,
    device_id: String,
    role: String,
    status: String,
}

#[derive(Serialize)]
struct WrappedKeyResponse {
    wrapped_vault_key: Option<String>, // base64
    vault_key_generation: Option<i64>,
    owner_public_key: Option<String>, // base64, needed for ECDH unwrap
    status: String,
}

#[derive(Serialize)]
struct PendingDeviceInfo {
    device_id: String,
    device_name: String,
    user_id: String,
    public_key: String, // base64
    created_at: i64,
}

#[derive(Deserialize)]
struct AddDeviceRequest {
    user_id: String,
    device_id: String,
    device_name: String,
    public_key: String, // base64url, 32-byte X25519 public key
}

#[derive(Serialize)]
struct AddDeviceResponse {
    status: String,
    device_id: String,
    user_id: String,
    wrapped_vault_key: Option<String>,        // base64url
    vault_key_generation: Option<i64>,
    wrapping_device_public_key: Option<String>, // base64url — for ECDH unwrap on the new device
}

// ── Routes ──────────────────────────────────────────────────────────

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/health/vault", get(get_vault_health))
        .route("/api/vault/status", get(get_vault_status))
        .route("/api/vault/invite", post(post_vault_invite))
        .route("/api/vault/join", post(post_vault_join))
        .route(
            "/api/vault/accept-device/{device_id}",
            post(post_accept_device),
        )
        .route("/api/vault/my-wrapped-key", get(get_my_wrapped_key))
        .route("/api/vault/pending-devices", get(get_pending_devices))
        .route("/api/vault/add-device", post(post_add_device))
        .route("/api/devices/{device_id}/revoke", post(post_revoke_device))
        .route("/api/vault/rewrap-status", get(get_rewrap_status))
        .route(
            "/api/vault/members/{user_id}/remove",
            post(post_remove_member),
        )
}

// ── Handlers ────────────────────────────────────────────────────────

async fn get_vault_health(State(state): State<ApiState>) -> impl IntoResponse {
    match db::get_vault_health_summary(&state.pool).await {
        Ok(summary) => (
            StatusCode::OK,
            Json(VaultHealthResponse {
                total_packs: summary.total_packs,
                healthy_packs: summary.healthy_packs,
                degraded_packs: summary.degraded_packs,
                unreadable_packs: summary.unreadable_packs,
            }),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn get_vault_status(State(state): State<ApiState>) -> impl IntoResponse {
    let unlocked = state.vault_keys.require_key().await.is_ok();
    (
        StatusCode::OK,
        Json(serde_json::json!({ "unlocked": unlocked })),
    )
        .into_response()
}

// ── Epic 34.1b: Invite flow endpoints ───────────────────────────────

async fn post_vault_invite(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<InviteRequest>,
) -> impl IntoResponse {
    // ACL: Owner or Admin can create invites
    let caller = match acl::require_role(&state.pool, &headers, Role::Admin).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let vault_id = caller.vault_id;
    let caller_device_id = caller.device_id;
    let owner_user_id = caller.user_id;

    let role = req.role.unwrap_or_else(|| "member".to_string());
    let max_uses = req.max_uses.unwrap_or(1);

    // Generate 128-bit random invite code (base64url)
    let mut code_bytes = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut code_bytes);
    let code = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        code_bytes,
    );

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let expires_at = req.expires_in_secs.map(|secs| now + secs);

    if let Err(e) =
        db::create_invite_code(&state.pool, &code, &vault_id, &owner_user_id, &role, max_uses, expires_at).await
    {
        return internal_server_error(io_error(e));
    }

    let _ = db::insert_audit_log(
        &state.pool,
        &vault_id,
        "create_invite",
        Some(&owner_user_id),
        Some(&caller_device_id),
        None,
        None,
        Some(&format!(r#"{{"role":"{role}","max_uses":{max_uses}}}"#)),
    )
    .await;

    (
        StatusCode::OK,
        Json(InviteResponse {
            code,
            role,
            max_uses,
            expires_at,
        }),
    )
        .into_response()
}

async fn post_vault_join(
    State(state): State<ApiState>,
    Json(req): Json<JoinRequest>,
) -> impl IntoResponse {
    // Validate invite code
    let invite = match db::get_invite_code(&state.pool, &req.invite_code).await {
        Ok(Some(inv)) => inv,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid_invite_code" })),
            )
                .into_response()
        }
        Err(e) => return internal_server_error(io_error(e)),
    };

    if !db::is_invite_code_valid(&invite) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invite_expired_or_exhausted" })),
        )
            .into_response();
    }

    // Decode public key
    let public_key = match base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        &req.public_key,
    ) {
        Ok(pk) if pk.len() == 32 => pk,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid_public_key", "message": "expected 32-byte X25519 public key (base64url)" })),
            )
                .into_response()
        }
    };

    // Consume invite
    if let Err(e) = db::consume_invite_code(&state.pool, &req.invite_code).await {
        return internal_server_error(io_error(e));
    }

    // Create user
    let user_id = format!("user-{}", &req.device_id);
    if let Err(e) = db::create_user(&state.pool, &user_id, &req.device_name, None, "local", None).await {
        // May already exist if re-joining
        warn!("create_user during join: {e}");
    }

    // Create device (with public key, but NO wrapped_vault_key yet — pending owner acceptance)
    if let Err(e) = db::create_device(&state.pool, &req.device_id, &user_id, &req.device_name, &public_key).await
    {
        warn!("create_device during join: {e}");
    }

    // Add vault membership
    if let Err(e) = db::add_vault_member(
        &state.pool,
        &user_id,
        &invite.vault_id,
        &invite.role,
        Some(&invite.created_by),
    )
    .await
    {
        warn!("add_vault_member during join: {e}");
    }

    let _ = db::insert_audit_log(
        &state.pool,
        &invite.vault_id,
        "join",
        Some(&user_id),
        Some(&req.device_id),
        None,
        None,
        Some(&format!(r#"{{"invite_code":"[REDACTED]","role":"{}"}}"#, invite.role)),
    )
    .await;

    (
        StatusCode::OK,
        Json(JoinResponse {
            user_id,
            device_id: req.device_id,
            role: invite.role,
            status: "pending_acceptance".to_string(),
        }),
    )
        .into_response()
}

async fn post_accept_device(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(target_device_id): Path<String>,
) -> impl IntoResponse {
    // ACL: Owner or Admin can accept devices
    let caller = match acl::require_role(&state.pool, &headers, Role::Admin).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let vault_id = caller.vault_id;

    // Get owner's master key (vault must be unlocked)
    let master_key = match state.vault_keys.require_master_key().await {
        Ok(mk) => mk,
        Err(_) => {
            return (
                StatusCode::LOCKED,
                Json(serde_json::json!({ "error": "vault_locked", "message": "vault must be unlocked to accept devices" })),
            )
                .into_response()
        }
    };

    // Get envelope vault key (the key we're distributing)
    let envelope_key = match state.vault_keys.require_envelope_key().await {
        Ok(ek) => ek,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "no_envelope_key", "message": "vault key not available" })),
            )
                .into_response()
        }
    };

    // Get owner's private key
    let owner_private = match identity::get_device_private_key(&state.pool, &master_key).await {
        Ok(pk) => pk,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "owner_key_error", "message": e.to_string() })),
            )
                .into_response()
        }
    };

    // Get target device's public key
    let target_device = match db::get_device(&state.pool, &target_device_id).await {
        Ok(Some(d)) => d,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "device_not_found" })),
            )
                .into_response()
        }
        Err(e) => return internal_server_error(io_error(e)),
    };

    if target_device.wrapped_vault_key.is_some() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "already_accepted", "message": "device already has a wrapped vault key" })),
        )
            .into_response();
    }

    if target_device.revoked_at.is_some() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "device_revoked" })),
        )
            .into_response();
    }

    let mut member_pubkey = [0u8; 32];
    if target_device.public_key.len() != 32 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid_device_public_key" })),
        )
            .into_response();
    }
    member_pubkey.copy_from_slice(&target_device.public_key);

    // ECDH wrap vault key
    let wrapped = match identity::wrap_vault_key_for_device(&owner_private, &member_pubkey, &envelope_key) {
        Ok(w) => w,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "key_wrap_failed", "message": e.to_string() })),
            )
                .into_response()
        }
    };

    // Store wrapped VK for device
    if let Err(e) = db::set_device_wrapped_vault_key(&state.pool, &target_device_id, &wrapped, 1).await {
        return internal_server_error(io_error(e));
    }

    let _ = db::insert_audit_log(
        &state.pool,
        &vault_id,
        "accept_device",
        None,
        None,
        Some(&target_device.user_id),
        Some(&target_device_id),
        None,
    )
    .await;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "accepted",
            "device_id": target_device_id,
            "user_id": target_device.user_id,
        })),
    )
        .into_response()
}

async fn get_my_wrapped_key(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    // ACL: Viewer+ (any authenticated member) can check their wrapped key
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Viewer).await {
        return resp;
    }

    let device_id = match params.get("device_id") {
        Some(id) => id.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "missing_device_id" })),
            )
                .into_response()
        }
    };

    let device = match db::get_device(&state.pool, &device_id).await {
        Ok(Some(d)) => d,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "device_not_found" })),
            )
                .into_response()
        }
        Err(e) => return internal_server_error(io_error(e)),
    };

    if device.revoked_at.is_some() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "device_revoked" })),
        )
            .into_response();
    }

    let (wrapped_vk, owner_pub) = match &device.wrapped_vault_key {
        Some(wvk) => {
            // Find owner's public key for ECDH unwrap on the member side
            let owner_pub = find_owner_public_key(&state.pool).await;
            (
                Some(base64::Engine::encode(
                    &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                    wvk,
                )),
                owner_pub,
            )
        }
        None => (None, None),
    };

    let status = if device.wrapped_vault_key.is_some() {
        "ready"
    } else {
        "pending_acceptance"
    };

    (
        StatusCode::OK,
        Json(WrappedKeyResponse {
            wrapped_vault_key: wrapped_vk,
            vault_key_generation: device.vault_key_generation,
            owner_public_key: owner_pub,
            status: status.to_string(),
        }),
    )
        .into_response()
}

async fn get_pending_devices(State(state): State<ApiState>, headers: HeaderMap) -> impl IntoResponse {
    // ACL: Admin+ can view pending devices
    let caller = match acl::require_role(&state.pool, &headers, Role::Admin).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let vault_id = caller.vault_id;

    let members = match db::list_vault_members(&state.pool, &vault_id).await {
        Ok(m) => m,
        Err(e) => return internal_server_error(io_error(e)),
    };

    let mut pending = Vec::new();
    for member in &members {
        let devices = match db::list_devices_for_user(&state.pool, &member.user_id).await {
            Ok(d) => d,
            Err(_) => continue,
        };
        for dev in devices {
            if dev.wrapped_vault_key.is_none() && dev.revoked_at.is_none() {
                // Skip placeholder pubkeys (all zeros = owner pre-34.1a)
                if dev.public_key == vec![0u8; 32] {
                    continue;
                }
                pending.push(PendingDeviceInfo {
                    device_id: dev.device_id,
                    device_name: dev.device_name,
                    user_id: dev.user_id,
                    public_key: base64::Engine::encode(
                        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                        &dev.public_key,
                    ),
                    created_at: dev.created_at,
                });
            }
        }
    }

    (StatusCode::OK, Json(serde_json::json!({ "pending_devices": pending }))).into_response()
}

// ── Helper: find owner public key ───────────────────────────────────

async fn find_owner_public_key(pool: &SqlitePool) -> Option<String> {
    let device = db::get_local_device_identity(pool).await.ok()??;
    let pubkey = device.public_key?;
    if pubkey.len() == 32 && pubkey != vec![0u8; 32] {
        Some(base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            &pubkey,
        ))
    } else {
        None
    }
}

// ── Epic 34.1c: Multi-device key distribution ──────────────────────

/// Registers a new device for an existing vault member and auto-accepts
/// (wraps VK) when the user already has >=1 active device and the vault
/// is currently unlocked on this daemon.
async fn post_add_device(
    State(state): State<ApiState>,
    Json(req): Json<AddDeviceRequest>,
) -> impl IntoResponse {
    let vault_id = match db::get_vault_params(&state.pool).await {
        Ok(Some(v)) => v.vault_id,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "vault_not_initialized" })),
            )
                .into_response()
        }
        Err(e) => return internal_server_error(io_error(e)),
    };

    // Verify user is an existing vault member
    match db::get_vault_member(&state.pool, &req.user_id, &vault_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": "not_a_member", "message": "user is not a vault member — use /api/vault/join with an invite code" })),
            )
                .into_response()
        }
        Err(e) => return internal_server_error(io_error(e)),
    }

    // Decode public key
    let public_key = match base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        &req.public_key,
    ) {
        Ok(pk) if pk.len() == 32 => pk,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid_public_key", "message": "expected 32-byte X25519 public key (base64url)" })),
            )
                .into_response()
        }
    };

    // Check if device already exists
    if let Ok(Some(existing)) = db::get_device(&state.pool, &req.device_id).await {
        if existing.revoked_at.is_some() {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": "device_revoked" })),
            )
                .into_response();
        }
        if existing.wrapped_vault_key.is_some() {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "device_already_active", "message": "device already has a wrapped vault key" })),
            )
                .into_response();
        }
    }

    // Create device entry
    if let Err(e) = db::create_device(&state.pool, &req.device_id, &req.user_id, &req.device_name, &public_key).await {
        // May already exist from a prior attempt — not fatal
        warn!("create_device during add-device: {e}");
    }

    // Auto-accept: if user has >=1 active device AND vault is unlocked -> wrap VK immediately
    let has_active_device = match db::get_active_devices_for_user(&state.pool, &req.user_id).await {
        Ok(devs) => !devs.is_empty(),
        Err(_) => false,
    };

    let auto_accepted = if has_active_device {
        try_auto_wrap_vault_key(&state, &req.device_id, &public_key, &vault_id).await
    } else {
        // First device for this user — also auto-accept if vault is unlocked
        // (user just entered passphrase on this new machine)
        try_auto_wrap_vault_key(&state, &req.device_id, &public_key, &vault_id).await
    };

    let (wrapped_vk_b64, vk_gen, wrapping_pub_b64) = match auto_accepted {
        Some((wrapped, generation, pub_key)) => (Some(wrapped), Some(generation), Some(pub_key)),
        None => (None, None, None),
    };

    let status = if wrapped_vk_b64.is_some() {
        "accepted"
    } else {
        "pending_acceptance"
    };

    let _ = db::insert_audit_log(
        &state.pool,
        &vault_id,
        "add_device",
        Some(&req.user_id),
        Some(&req.device_id),
        None,
        None,
        Some(&format!(r#"{{"auto_accepted":{},"device_name":"{}"}}"#, wrapped_vk_b64.is_some(), req.device_name)),
    )
    .await;

    (
        StatusCode::OK,
        Json(AddDeviceResponse {
            status: status.to_string(),
            device_id: req.device_id,
            user_id: req.user_id,
            wrapped_vault_key: wrapped_vk_b64,
            vault_key_generation: vk_gen,
            wrapping_device_public_key: wrapping_pub_b64,
        }),
    )
        .into_response()
}

/// Attempts to wrap the vault key for a new device using the local device's
/// private key. Returns (wrapped_vk_base64, vault_key_generation, wrapping_device_pubkey_base64)
/// or None if the vault is locked or keys are unavailable.
async fn try_auto_wrap_vault_key(
    state: &ApiState,
    target_device_id: &str,
    target_public_key: &[u8],
    vault_id: &str,
) -> Option<(String, i64, String)> {
    let master_key = state.vault_keys.require_master_key().await.ok()?;
    let envelope_key = state.vault_keys.require_envelope_key().await.ok()?;
    let owner_private = identity::get_device_private_key(&state.pool, &master_key).await.ok()?;

    let mut target_pub = [0u8; 32];
    if target_public_key.len() != 32 {
        return None;
    }
    target_pub.copy_from_slice(target_public_key);

    let wrapped = identity::wrap_vault_key_for_device(&owner_private, &target_pub, &envelope_key).ok()?;

    // Get current vault_key_generation
    let vk_gen = match db::get_vault_params(&state.pool).await {
        Ok(Some(v)) => v.vault_key_generation.unwrap_or(1),
        _ => 1,
    };

    // Store wrapped VK
    db::set_device_wrapped_vault_key(&state.pool, target_device_id, &wrapped, vk_gen)
        .await
        .ok()?;

    // Get local device's public key for ECDH unwrap on the receiving side
    let local_device = db::get_local_device_identity(&state.pool).await.ok()??;
    let pub_key_bytes = local_device.public_key?;
    if pub_key_bytes.len() != 32 || pub_key_bytes == vec![0u8; 32] {
        return None;
    }

    let wrapped_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        wrapped,
    );
    let pub_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        &pub_key_bytes,
    );

    let _ = db::insert_audit_log(
        &state.pool,
        vault_id,
        "auto_accept_device",
        None,
        Some(target_device_id),
        None,
        None,
        Some(r#"{"reason":"existing_member_auto_accept"}"#),
    )
    .await;

    Some((wrapped_b64, vk_gen, pub_b64))
}

// ── Epic 34.2a: Device revocation ──────────────────────────────────

/// Revokes a device: clears its wrapped vault key, sets revoked_at,
/// and logs an audit event. Only owner/admin can revoke.
/// Self-revocation (revoking the local daemon's own device) is blocked.
async fn post_revoke_device(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(target_device_id): Path<String>,
) -> impl IntoResponse {
    // ACL: Owner or Admin can revoke devices
    let caller = match acl::require_role(&state.pool, &headers, Role::Admin).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let vault_id = caller.vault_id;
    let caller_device_id = caller.device_id;
    let caller_user_id = caller.user_id;

    // Block self-revocation — revoking the local device would brick this daemon
    if caller_device_id == target_device_id {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "cannot_revoke_self", "message": "cannot revoke the local device — use another device to revoke this one" })),
        )
            .into_response();
    }

    // Verify target device exists
    let target_device = match db::get_device(&state.pool, &target_device_id).await {
        Ok(Some(d)) => d,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "device_not_found" })),
            )
                .into_response()
        }
        Err(e) => return internal_server_error(io_error(e)),
    };

    if target_device.revoked_at.is_some() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "already_revoked", "message": "device is already revoked" })),
        )
            .into_response();
    }

    // Revoke: sets revoked_at + clears wrapped_vault_key
    if let Err(e) = db::revoke_device(&state.pool, &target_device_id).await {
        return internal_server_error(io_error(e));
    }

    let _ = db::insert_audit_log(
        &state.pool,
        &vault_id,
        "revoke_device",
        Some(&caller_user_id),
        Some(&caller_device_id),
        Some(&target_device.user_id),
        Some(&target_device_id),
        Some(&format!(
            r#"{{"device_name":"{}","user_id":"{}"}}"#,
            target_device.device_name, target_device.user_id
        )),
    )
    .await;

    // Count remaining active devices for the affected user
    let remaining = db::get_active_devices_for_user(&state.pool, &target_device.user_id)
        .await
        .map(|d| d.len())
        .unwrap_or(0);

    // Trigger VK rotation (immediate phase) — re-wraps VK for active devices,
    // enqueues DEKs for lazy background re-wrap.
    let rotation = match state.vault_keys.rotate_for_revocation(&state.pool).await {
        Ok(r) => Some(r),
        Err(e) => {
            warn!("VK rotation after revocation failed: {e}");
            None
        }
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "revoked",
            "device_id": target_device_id,
            "user_id": target_device.user_id,
            "remaining_active_devices": remaining,
            "vk_rotation": rotation.as_ref().map(|r| serde_json::json!({
                "new_generation": r.new_generation,
                "devices_rewrapped": r.devices_rewrapped,
                "deks_enqueued": r.deks_enqueued,
            })),
        })),
    )
        .into_response()
}

// ── Epic 34.2c: User removal ───────────────────────────────────────

/// Removes a user from the vault: revokes ALL their devices, deletes
/// their vault membership, triggers VK rotation. Only owner/admin.
async fn post_remove_member(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(target_user_id): Path<String>,
) -> impl IntoResponse {
    // ACL: Owner or Admin can remove members
    let caller = match acl::require_role(&state.pool, &headers, Role::Admin).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let vault_id = caller.vault_id;
    let caller_device_id = caller.device_id;
    let caller_user_id = caller.user_id;

    // Block self-removal
    if caller_user_id == target_user_id {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "cannot_remove_self", "message": "cannot remove yourself — transfer ownership first" })),
        )
            .into_response();
    }

    // Verify target is a member
    match db::get_vault_member(&state.pool, &target_user_id, &vault_id).await {
        Ok(Some(m)) if m.role == "owner" => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": "cannot_remove_owner", "message": "cannot remove the vault owner" })),
            )
                .into_response()
        }
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "member_not_found" })),
            )
                .into_response()
        }
        Err(e) => return internal_server_error(io_error(e)),
    }

    // Step 1: Revoke ALL devices for this user
    let devices = match db::list_devices_for_user(&state.pool, &target_user_id).await {
        Ok(d) => d,
        Err(e) => return internal_server_error(io_error(e)),
    };
    let mut devices_revoked = 0u64;
    for dev in &devices {
        if dev.revoked_at.is_none()
            && let Ok(true) = db::revoke_device(&state.pool, &dev.device_id).await {
                devices_revoked += 1;
            }
    }

    // Step 2: Delete vault membership
    if let Err(e) = db::remove_vault_member(&state.pool, &target_user_id, &vault_id).await {
        return internal_server_error(io_error(e));
    }

    // Step 3: Audit log
    let _ = db::insert_audit_log(
        &state.pool,
        &vault_id,
        "remove_member",
        Some(&caller_user_id),
        Some(&caller_device_id),
        Some(&target_user_id),
        None,
        Some(&format!(r#"{{"devices_revoked":{devices_revoked}}}"#)),
    )
    .await;

    // Step 4: Trigger VK rotation
    let rotation = match state.vault_keys.rotate_for_revocation(&state.pool).await {
        Ok(r) => Some(r),
        Err(e) => {
            warn!("VK rotation after member removal failed: {e}");
            None
        }
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "removed",
            "user_id": target_user_id,
            "devices_revoked": devices_revoked,
            "vk_rotation": rotation.as_ref().map(|r| serde_json::json!({
                "new_generation": r.new_generation,
                "devices_rewrapped": r.devices_rewrapped,
                "deks_enqueued": r.deks_enqueued,
            })),
        })),
    )
        .into_response()
}

/// Returns the status of the background DEK re-wrap queue.
async fn get_rewrap_status(State(state): State<ApiState>, headers: HeaderMap) -> impl IntoResponse {
    // ACL: Viewer+ can check rewrap status
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Viewer).await {
        return resp;
    }

    match db::get_rewrap_status(&state.pool).await {
        Ok((total, pending, failed)) => {
            let done = total - pending - failed;
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "total": total,
                    "done": done,
                    "pending": pending,
                    "failed": failed,
                    "complete": pending == 0 && failed == 0,
                })),
            )
                .into_response()
        }
        Err(e) => internal_server_error(io_error(e)),
    }
}
