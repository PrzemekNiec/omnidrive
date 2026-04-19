use crate::acl::{self, Role};
use crate::db;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info};

use super::error::ApiError;
use super::ApiState;

// ── Request / Response structs ──────────────────────────────────────────

#[derive(Deserialize)]
struct CreateShareRequest {
    expires_in_hours: Option<u64>,
    max_downloads: Option<i64>,
    password: Option<String>,
}

#[derive(Serialize)]
struct CreateShareResponse {
    share_id: String,
    share_url: String,
    dek_base64url: String,
    full_link: String,
    expires_at: Option<i64>,
    max_downloads: Option<i64>,
    password_protected: bool,
}

#[derive(Deserialize)]
struct VerifyPasswordRequest {
    password: String,
}

#[derive(Serialize)]
struct VerifyPasswordResponse {
    token: String,
    expires_in_seconds: i64,
}

#[derive(Deserialize)]
struct ShareTokenQuery {
    token: Option<String>,
}

#[derive(Serialize)]
struct ShareMetaResponse {
    file_name: String,
    file_size: i64,
    chunk_count: usize,
    chunks: Vec<ShareChunkMeta>,
}

#[derive(Serialize)]
struct ShareChunkMeta {
    index: i64,
    file_offset: i64,
    plain_size: i64,
    encrypted_size: i64,
}

/// Token TTL for password-verified share access (10 minutes).
const SHARE_TOKEN_TTL_SECONDS: i64 = 600;

// ── Routes ──────────────────────────────────────────────────────────────

pub(super) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/share/{share_id}", get(get_share_page))
        .route("/api/share/{share_id}/meta", get(get_share_meta))
        .route(
            "/api/share/{share_id}/chunks/{chunk_index}",
            get(get_share_chunk),
        )
        .route("/api/files/{inode_id}/share", post(create_share_link))
        .route("/api/files/{inode_id}/shares", get(list_file_shares))
        .route("/api/shares", get(list_all_shares))
        .route("/api/shares/{share_id}/revoke", post(revoke_share))
        .route("/api/shares/{share_id}", delete(delete_share))
        .route(
            "/api/share/{share_id}/verify-password",
            post(verify_share_password),
        )
        .route("/share-sw.js", get(get_share_sw_js))
        .route("/sw-download/{share_id}", get(get_sw_download_placeholder))
}

// ── Handlers ────────────────────────────────────────────────────────────

async fn create_share_link(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(inode_id): Path<i64>,
    Json(request): Json<CreateShareRequest>,
) -> Result<(StatusCode, Json<CreateShareResponse>), ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Member).await?;

    let envelope_key = state.vault_keys.require_envelope_key().await.map_err(|_| {
        ApiError::Forbidden {
            message: "vault is locked".to_string(),
        }
    })?;

    let inode = db::get_inode_by_id(&state.pool, inode_id)
        .await?
        .ok_or(ApiError::NotFound {
            resource: "inode",
            id: inode_id.to_string(),
        })?;

    let revision = db::get_current_file_revision(&state.pool, inode_id)
        .await?
        .ok_or(ApiError::NotFound {
            resource: "current_revision",
            id: inode_id.to_string(),
        })?;

    let (_dek_id, dek_secret) = state
        .vault_keys
        .get_or_create_dek(&state.pool, inode_id)
        .await
        .map_err(|err| {
            error!("failed to get DEK for sharing: {err}");
            ApiError::Internal {
                message: "dek_unavailable".to_string(),
            }
        })?;

    let _ = envelope_key; // used to verify vault is unlocked

    let dek_bytes: &[u8; 32] = dek_secret.expose_secret();
    let dek_base64url = crate::sharing::encode_dek_for_url(dek_bytes);
    let share_id = crate::sharing::generate_share_id();

    let expires_at = request.expires_in_hours.map(|hours| {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        now_ms + (hours as i64 * 3_600_000)
    });

    let file_name = inode.name.clone();

    let password_hash = request
        .password
        .as_deref()
        .filter(|p| !p.is_empty())
        .map(crate::sharing::hash_share_password);
    let password_protected = password_hash.is_some();

    db::create_shared_link(
        &state.pool,
        &share_id,
        inode_id,
        revision.revision_id,
        &file_name,
        revision.size,
        expires_at,
        request.max_downloads,
        password_hash.as_deref(),
    )
    .await?;

    let base = share_base_url(&headers);
    let share_url = format!("{base}/share/{share_id}");
    let full_link = format!("{share_url}#{dek_base64url}");

    info!("share link created for inode {inode_id}, share_id={share_id}");

    let _ = db::insert_audit_log(
        &state.pool,
        &caller.vault_id,
        "share_create",
        Some(&caller.user_id),
        Some(&caller.device_id),
        None,
        None,
        Some(&format!(
            r#"{{"share_id":"{share_id}","inode_id":{inode_id},"password_protected":{password_protected},"max_downloads":{}}}"#,
            request
                .max_downloads
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".to_string())
        )),
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(CreateShareResponse {
            share_id,
            share_url,
            dek_base64url,
            full_link,
            expires_at,
            max_downloads: request.max_downloads,
            password_protected,
        }),
    ))
}

async fn list_all_shares(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<db::SharedLinkRecord>>, ApiError> {
    acl::require_role(&state.pool, &headers, Role::Viewer).await?;
    let links = db::list_shared_links(&state.pool).await?;
    Ok(Json(links))
}

async fn list_file_shares(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(inode_id): Path<i64>,
) -> Result<Json<Vec<db::SharedLinkRecord>>, ApiError> {
    acl::require_role(&state.pool, &headers, Role::Viewer).await?;
    let links = db::list_shared_links_for_inode(&state.pool, inode_id).await?;
    Ok(Json(links))
}

async fn revoke_share(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(share_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Member).await?;

    let revoked = db::revoke_shared_link(&state.pool, &share_id).await?;
    if revoked {
        let _ = db::insert_audit_log(
            &state.pool,
            &caller.vault_id,
            "share_revoke",
            Some(&caller.user_id),
            Some(&caller.device_id),
            None,
            None,
            Some(&format!(r#"{{"share_id":"{share_id}"}}"#)),
        )
        .await;
        Ok(Json(serde_json::json!({"revoked": true})))
    } else {
        Err(ApiError::NotFound {
            resource: "share",
            id: share_id,
        })
    }
}

async fn delete_share(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(share_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Member).await?;

    let deleted = db::delete_shared_link(&state.pool, &share_id).await?;
    if deleted {
        let _ = db::insert_audit_log(
            &state.pool,
            &caller.vault_id,
            "share_delete",
            Some(&caller.user_id),
            Some(&caller.device_id),
            None,
            None,
            Some(&format!(r#"{{"share_id":"{share_id}"}}"#)),
        )
        .await;
        Ok(Json(serde_json::json!({"deleted": true})))
    } else {
        Err(ApiError::NotFound {
            resource: "share",
            id: share_id,
        })
    }
}

async fn get_share_page(Path(share_id): Path<String>) -> Html<&'static str> {
    let _ = share_id;
    Html(include_str!("../../static/share.html"))
}

async fn get_share_sw_js() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/javascript")],
        include_str!("../../static/share-sw.js"),
    )
}

async fn get_sw_download_placeholder(Path(share_id): Path<String>) -> (StatusCode, &'static str) {
    let _ = share_id;
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Service Worker nie jest aktywny. Odswierz strone i sprobuj ponownie.",
    )
}

async fn verify_share_password(
    State(state): State<ApiState>,
    Path(share_id): Path<String>,
    Json(request): Json<VerifyPasswordRequest>,
) -> Result<Json<VerifyPasswordResponse>, ApiError> {
    let link = db::get_shared_link(&state.pool, &share_id)
        .await?
        .ok_or(ApiError::NotFound {
            resource: "share",
            id: share_id.clone(),
        })?;

    let password_hash = link.password_hash.as_deref().ok_or(ApiError::BadRequest {
        code: "share_not_password_protected",
        message: "this share is not password protected".to_string(),
    })?;

    if !crate::sharing::verify_share_password(&request.password, password_hash) {
        return Err(ApiError::Unauthorized {
            message: "invalid password".to_string(),
        });
    }

    let token = crate::sharing::generate_share_token();
    db::create_share_password_token(&state.pool, &token, &share_id, SHARE_TOKEN_TTL_SECONDS)
        .await?;

    Ok(Json(VerifyPasswordResponse {
        token,
        expires_in_seconds: SHARE_TOKEN_TTL_SECONDS,
    }))
}

/// Check if password-protected share access is authorized.
/// Returns Ok(()) if access granted, or an ApiError if denied.
async fn check_share_password_access(
    pool: &sqlx::SqlitePool,
    link: &db::SharedLinkRecord,
    token: &Option<String>,
) -> Result<(), ApiError> {
    if link.password_hash.is_none() {
        return Ok(());
    }
    match token {
        Some(t) if !t.is_empty() => {
            let valid = db::validate_share_password_token(pool, t, &link.share_id).await?;
            if valid {
                Ok(())
            } else {
                Err(ApiError::Unauthorized {
                    message: "invalid or expired share token".to_string(),
                })
            }
        }
        _ => Err(ApiError::Unauthorized {
            message: "password required".to_string(),
        }),
    }
}

async fn get_share_meta(
    State(state): State<ApiState>,
    Path(share_id): Path<String>,
    Query(query): Query<ShareTokenQuery>,
) -> Result<Json<ShareMetaResponse>, ApiError> {
    let link = db::get_shared_link(&state.pool, &share_id)
        .await?
        .ok_or(ApiError::NotFound {
            resource: "share",
            id: share_id.clone(),
        })?;

    if !db::is_shared_link_valid(&link) {
        let reason = if link.revoked != 0 {
            "revoked"
        } else if link.max_downloads.is_some()
            && link.download_count >= link.max_downloads.unwrap_or(i64::MAX)
        {
            "download_limit_reached"
        } else {
            "expired"
        };
        return Err(ApiError::Gone {
            message: format!("share is invalid: {reason}"),
        });
    }

    check_share_password_access(&state.pool, &link, &query.token).await?;

    let chunks = db::get_chunk_locations_for_revision(&state.pool, link.revision_id).await?;

    let chunk_meta: Vec<ShareChunkMeta> = chunks
        .iter()
        .map(|c| ShareChunkMeta {
            index: c.chunk_index,
            file_offset: c.file_offset,
            plain_size: c.size,
            encrypted_size: c.encrypted_size,
        })
        .collect();

    Ok(Json(ShareMetaResponse {
        file_name: link.file_name,
        file_size: link.file_size,
        chunk_count: chunk_meta.len(),
        chunks: chunk_meta,
    }))
}

async fn get_share_chunk(
    State(state): State<ApiState>,
    Path((share_id, chunk_index)): Path<(String, i64)>,
    Query(query): Query<ShareTokenQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let link = db::get_shared_link(&state.pool, &share_id)
        .await?
        .ok_or(ApiError::NotFound {
            resource: "share",
            id: share_id.clone(),
        })?;

    if !db::is_shared_link_valid(&link) {
        return Err(ApiError::Gone {
            message: "share is invalid".to_string(),
        });
    }

    check_share_password_access(&state.pool, &link, &query.token).await?;

    let downloader = state.downloader.as_ref().ok_or(ApiError::ServiceUnavailable {
        message: "downloader unavailable".to_string(),
    })?;

    let chunks = db::get_chunk_locations_for_revision(&state.pool, link.revision_id).await?;

    let chunk = chunks.iter().find(|c| c.chunk_index == chunk_index).ok_or(
        ApiError::NotFound {
            resource: "chunk",
            id: chunk_index.to_string(),
        },
    )?;

    let encrypted = downloader.get_encrypted_chunk_bytes(chunk).await.map_err(|err| {
        error!("failed to get encrypted chunk for share {share_id}: {err}");
        ApiError::Internal {
            message: "chunk fetch failed".to_string(),
        }
    })?;

    // If this is the last chunk, increment download count
    let is_last_chunk = chunk_index == (chunks.len() as i64 - 1);
    if is_last_chunk {
        let _ = db::increment_shared_link_download_count(&state.pool, &share_id).await;
    }

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/octet-stream")],
        encrypted.to_bytes(),
    ))
}

/// Determine the base URL for generated share links.
///
/// Priority:
/// 1. `OMNIDRIVE_SHARE_HOST` env var — allows explicit override, e.g.
///    `OMNIDRIVE_SHARE_HOST=http://192.168.1.10:8787` for LAN sharing.
///    If the value already contains `://` it is used as-is; otherwise
///    `http://` is prepended.
/// 2. `Host` header of the incoming request — works naturally for both
///    loopback (`127.0.0.1:8787`) and LAN (`192.168.x.x:8787`) clients.
/// 3. Fallback: `http://127.0.0.1:8787`.
fn share_base_url(headers: &HeaderMap) -> String {
    if let Ok(override_host) = env::var("OMNIDRIVE_SHARE_HOST") {
        let trimmed = override_host.trim().to_string();
        if !trimmed.is_empty() {
            return if trimmed.contains("://") {
                trimmed
            } else {
                format!("http://{trimmed}")
            };
        }
    }

    if let Some(host) = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
    {
        return format!("http://{host}");
    }

    "http://127.0.0.1:8787".to_string()
}
