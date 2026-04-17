use crate::acl::{self, Role};
use crate::db;
use crate::identity;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

use super::error::ApiError;
use super::ApiState;

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
        // ── G.7: lista urządzeń Skarbca ──
        .route("/api/vault/devices", get(get_vault_devices))
}

// ── Handlers ────────────────────────────────────────────────────────

async fn get_vault_health(
    State(state): State<ApiState>,
) -> Result<Json<VaultHealthResponse>, ApiError> {
    let summary = db::get_vault_health_summary(&state.pool).await?;
    Ok(Json(VaultHealthResponse {
        total_packs: summary.total_packs,
        healthy_packs: summary.healthy_packs,
        degraded_packs: summary.degraded_packs,
        unreadable_packs: summary.unreadable_packs,
    }))
}

async fn get_vault_status(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let unlocked = state.vault_keys.require_key().await.is_ok();
    Json(serde_json::json!({ "unlocked": unlocked }))
}

// ── Epic 34.1b: Invite flow endpoints ───────────────────────────────

async fn post_vault_invite(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<InviteRequest>,
) -> Result<Json<InviteResponse>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Admin).await?;
    let vault_id = caller.vault_id;
    let caller_device_id = caller.device_id;
    let owner_user_id = caller.user_id;

    let role = req.role.unwrap_or_else(|| "member".to_string());
    let max_uses = req.max_uses.unwrap_or(1);

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

    db::create_invite_code(&state.pool, &code, &vault_id, &owner_user_id, &role, max_uses, expires_at)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?;

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

    Ok(Json(InviteResponse {
        code,
        role,
        max_uses,
        expires_at,
    }))
}

async fn post_vault_join(
    State(state): State<ApiState>,
    Json(req): Json<JoinRequest>,
) -> Result<Json<JoinResponse>, ApiError> {
    let invite = db::get_invite_code(&state.pool, &req.invite_code)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?
        .ok_or(ApiError::BadRequest {
            code: "invalid_invite_code",
            message: "invite code not found".to_string(),
        })?;

    if !db::is_invite_code_valid(&invite) {
        return Err(ApiError::BadRequest {
            code: "invite_expired_or_exhausted",
            message: "invite code is expired or exhausted".to_string(),
        });
    }

    let public_key = base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        &req.public_key,
    )
    .ok()
    .filter(|pk| pk.len() == 32)
    .ok_or(ApiError::BadRequest {
        code: "invalid_public_key",
        message: "expected 32-byte X25519 public key (base64url)".to_string(),
    })?;

    db::consume_invite_code(&state.pool, &req.invite_code)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?;

    let user_id = format!("user-{}", &req.device_id);
    if let Err(e) = db::create_user(&state.pool, &user_id, &req.device_name, None, "local", None).await {
        warn!("create_user during join: {e}");
    }

    if let Err(e) = db::create_device(&state.pool, &req.device_id, &user_id, &req.device_name, &public_key).await {
        warn!("create_device during join: {e}");
    }

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

    Ok(Json(JoinResponse {
        user_id,
        device_id: req.device_id,
        role: invite.role,
        status: "pending_acceptance".to_string(),
    }))
}

async fn post_accept_device(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(target_device_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Admin).await?;
    let vault_id = caller.vault_id;

    let master_key = state.vault_keys.require_master_key().await.map_err(|_| {
        ApiError::Locked {
            message: "vault must be unlocked to accept devices".to_string(),
        }
    })?;

    let envelope_key = state.vault_keys.require_envelope_key().await.map_err(|_| {
        ApiError::BadRequest {
            code: "no_envelope_key",
            message: "vault key not available".to_string(),
        }
    })?;

    let owner_private =
        identity::get_device_private_key(&state.pool, &master_key)
            .await
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
            })?;

    let target_device = db::get_device(&state.pool, &target_device_id)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?
        .ok_or(ApiError::NotFound {
            resource: "device",
            id: target_device_id.clone(),
        })?;

    if target_device.wrapped_vault_key.is_some() {
        return Err(ApiError::Conflict {
            message: "device already has a wrapped vault key".to_string(),
        });
    }

    if target_device.revoked_at.is_some() {
        return Err(ApiError::Forbidden {
            message: "device is revoked".to_string(),
        });
    }

    if target_device.public_key.len() != 32 {
        return Err(ApiError::BadRequest {
            code: "invalid_device_public_key",
            message: "device public key must be 32 bytes".to_string(),
        });
    }
    let mut member_pubkey = [0u8; 32];
    member_pubkey.copy_from_slice(&target_device.public_key);

    let wrapped = identity::wrap_vault_key_for_device(&owner_private, &member_pubkey, &envelope_key)
        .map_err(|e| ApiError::Internal {
            message: format!("key wrap failed: {e}"),
        })?;

    db::set_device_wrapped_vault_key(&state.pool, &target_device_id, &wrapped, 1)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?;

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

    Ok(Json(serde_json::json!({
        "status": "accepted",
        "device_id": target_device_id,
        "user_id": target_device.user_id,
    })))
}

async fn get_my_wrapped_key(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<WrappedKeyResponse>, ApiError> {
    acl::require_role(&state.pool, &headers, Role::Viewer).await?;

    let device_id = params.get("device_id").ok_or(ApiError::BadRequest {
        code: "missing_device_id",
        message: "device_id query parameter is required".to_string(),
    })?;

    let device = db::get_device(&state.pool, device_id)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?
        .ok_or(ApiError::NotFound {
            resource: "device",
            id: device_id.clone(),
        })?;

    if device.revoked_at.is_some() {
        return Err(ApiError::Forbidden {
            message: "device is revoked".to_string(),
        });
    }

    let (wrapped_vk, owner_pub) = match &device.wrapped_vault_key {
        Some(wvk) => {
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

    Ok(Json(WrappedKeyResponse {
        wrapped_vault_key: wrapped_vk,
        vault_key_generation: device.vault_key_generation,
        owner_public_key: owner_pub,
        status: status.to_string(),
    }))
}

async fn get_pending_devices(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Admin).await?;
    let vault_id = caller.vault_id;

    let members = db::list_vault_members(&state.pool, &vault_id)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?;

    let mut pending = Vec::new();
    for member in &members {
        let devices = match db::list_devices_for_user(&state.pool, &member.user_id).await {
            Ok(d) => d,
            Err(_) => continue,
        };
        for dev in devices {
            if dev.wrapped_vault_key.is_none() && dev.revoked_at.is_none() {
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

    Ok(Json(serde_json::json!({ "pending_devices": pending })))
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

async fn post_add_device(
    State(state): State<ApiState>,
    Json(req): Json<AddDeviceRequest>,
) -> Result<Json<AddDeviceResponse>, ApiError> {
    let vault_id = db::get_vault_params(&state.pool)
        .await?
        .ok_or(ApiError::BadRequest {
            code: "vault_not_initialized",
            message: "vault not initialized".to_string(),
        })?
        .vault_id;

    // Verify user is an existing vault member
    db::get_vault_member(&state.pool, &req.user_id, &vault_id)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?
        .ok_or(ApiError::Forbidden {
            message: "user is not a vault member — use /api/vault/join with an invite code".to_string(),
        })?;

    let public_key = base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        &req.public_key,
    )
    .ok()
    .filter(|pk| pk.len() == 32)
    .ok_or(ApiError::BadRequest {
        code: "invalid_public_key",
        message: "expected 32-byte X25519 public key (base64url)".to_string(),
    })?;

    // Check if device already exists
    if let Ok(Some(existing)) = db::get_device(&state.pool, &req.device_id).await {
        if existing.revoked_at.is_some() {
            return Err(ApiError::Forbidden {
                message: "device is revoked".to_string(),
            });
        }
        if existing.wrapped_vault_key.is_some() {
            return Err(ApiError::Conflict {
                message: "device already has a wrapped vault key".to_string(),
            });
        }
    }

    if let Err(e) = db::create_device(&state.pool, &req.device_id, &req.user_id, &req.device_name, &public_key).await {
        warn!("create_device during add-device: {e}");
    }

    let auto_accepted = try_auto_wrap_vault_key(&state, &req.device_id, &public_key, &vault_id).await;

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

    Ok(Json(AddDeviceResponse {
        status: status.to_string(),
        device_id: req.device_id,
        user_id: req.user_id,
        wrapped_vault_key: wrapped_vk_b64,
        vault_key_generation: vk_gen,
        wrapping_device_public_key: wrapping_pub_b64,
    }))
}

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

    let vk_gen = match db::get_vault_params(&state.pool).await {
        Ok(Some(v)) => v.vault_key_generation.unwrap_or(1),
        _ => 1,
    };

    db::set_device_wrapped_vault_key(&state.pool, target_device_id, &wrapped, vk_gen)
        .await
        .ok()?;

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

async fn post_revoke_device(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(target_device_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Admin).await?;
    let vault_id = caller.vault_id;
    let caller_device_id = caller.device_id;
    let caller_user_id = caller.user_id;

    if caller_device_id == target_device_id {
        return Err(ApiError::BadRequest {
            code: "cannot_revoke_self",
            message: "cannot revoke the local device — use another device to revoke this one".to_string(),
        });
    }

    let target_device = db::get_device(&state.pool, &target_device_id)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?
        .ok_or(ApiError::NotFound {
            resource: "device",
            id: target_device_id.clone(),
        })?;

    if target_device.revoked_at.is_some() {
        return Err(ApiError::Conflict {
            message: "device is already revoked".to_string(),
        });
    }

    db::revoke_device(&state.pool, &target_device_id)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?;

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

    let remaining = db::get_active_devices_for_user(&state.pool, &target_device.user_id)
        .await
        .map(|d| d.len())
        .unwrap_or(0);

    let rotation = match state.vault_keys.rotate_for_revocation(&state.pool).await {
        Ok(r) => Some(r),
        Err(e) => {
            warn!("VK rotation after revocation failed: {e}");
            None
        }
    };

    Ok(Json(serde_json::json!({
        "status": "revoked",
        "device_id": target_device_id,
        "user_id": target_device.user_id,
        "remaining_active_devices": remaining,
        "vk_rotation": rotation.as_ref().map(|r| serde_json::json!({
            "new_generation": r.new_generation,
            "devices_rewrapped": r.devices_rewrapped,
            "deks_enqueued": r.deks_enqueued,
        })),
    })))
}

// ── Epic 34.2c: User removal ───────────────────────────────────────

async fn post_remove_member(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(target_user_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Admin).await?;
    let vault_id = caller.vault_id;
    let caller_device_id = caller.device_id;
    let caller_user_id = caller.user_id;

    if caller_user_id == target_user_id {
        return Err(ApiError::BadRequest {
            code: "cannot_remove_self",
            message: "cannot remove yourself — transfer ownership first".to_string(),
        });
    }

    match db::get_vault_member(&state.pool, &target_user_id, &vault_id)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })? {
        Some(m) if m.role == "owner" => {
            return Err(ApiError::Forbidden {
                message: "cannot remove the vault owner".to_string(),
            })
        }
        Some(_) => {}
        None => {
            return Err(ApiError::NotFound {
                resource: "member",
                id: target_user_id.clone(),
            })
        }
    }

    let devices = db::list_devices_for_user(&state.pool, &target_user_id)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?;

    let mut devices_revoked = 0u64;
    for dev in &devices {
        if dev.revoked_at.is_none()
            && let Ok(true) = db::revoke_device(&state.pool, &dev.device_id).await {
                devices_revoked += 1;
            }
    }

    db::remove_vault_member(&state.pool, &target_user_id, &vault_id)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?;

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

    let rotation = match state.vault_keys.rotate_for_revocation(&state.pool).await {
        Ok(r) => Some(r),
        Err(e) => {
            warn!("VK rotation after member removal failed: {e}");
            None
        }
    };

    Ok(Json(serde_json::json!({
        "status": "removed",
        "user_id": target_user_id,
        "devices_revoked": devices_revoked,
        "vk_rotation": rotation.as_ref().map(|r| serde_json::json!({
            "new_generation": r.new_generation,
            "devices_rewrapped": r.devices_rewrapped,
            "deks_enqueued": r.deks_enqueued,
        })),
    })))
}

// ── G.7: GET /api/vault/devices ─────────────────────────────────────
#[derive(Serialize)]
struct DeviceListItem {
    device_id: String,
    device_name: String,
    user_id: String,
    last_seen_at: Option<i64>,
    created_at: i64,
    revoked: bool,
    has_vault_key: bool,
    /// First 8 hex chars of device_id — lightweight visual fingerprint.
    fingerprint: String,
}

async fn get_vault_devices(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Viewer).await?;

    let members = db::list_vault_members(&state.pool, &caller.vault_id).await?;
    let mut items: Vec<DeviceListItem> = Vec::new();
    for member in &members {
        let devices = db::list_devices_for_user(&state.pool, &member.user_id)
            .await
            .unwrap_or_default();
        for dev in devices {
            let fingerprint = dev.device_id
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .take(8)
                .collect::<String>()
                .to_uppercase();
            items.push(DeviceListItem {
                device_id: dev.device_id,
                device_name: dev.device_name,
                user_id: dev.user_id,
                last_seen_at: dev.last_seen_at,
                created_at: dev.created_at,
                revoked: dev.revoked_at.is_some(),
                has_vault_key: dev.wrapped_vault_key.is_some(),
                fingerprint,
            });
        }
    }

    Ok(Json(serde_json::json!({ "devices": items })))
}

async fn get_rewrap_status(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    acl::require_role(&state.pool, &headers, Role::Viewer).await?;

    let (total, pending, failed) = db::get_rewrap_status(&state.pool)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?;

    let done = total - pending - failed;
    Ok(Json(serde_json::json!({
        "total": total,
        "done": done,
        "pending": pending,
        "failed": failed,
        "complete": pending == 0 && failed == 0,
    })))
}
