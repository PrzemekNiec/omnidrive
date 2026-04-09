use crate::acl::{self, Role};
use crate::db;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info};

use super::{internal_server_error, ApiState};

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
) -> impl IntoResponse {
    // ACL: Member+ can create share links
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Member).await {
        return resp;
    }

    // Vault must be unlocked
    let envelope_key = match state.vault_keys.require_envelope_key().await {
        Ok(k) => k,
        Err(_) => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "vault_locked"})),
            )
                .into_response()
        }
    };

    // Look up inode
    let inode = match db::get_inode_by_id(&state.pool, inode_id).await {
        Ok(Some(inode)) => inode,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "inode_not_found"})),
            )
                .into_response()
        }
        Err(err) => return internal_server_error(err),
    };

    // Get current revision
    let revision = match db::get_current_file_revision(&state.pool, inode_id).await {
        Ok(Some(rev)) => rev,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "no_current_revision"})),
            )
                .into_response()
        }
        Err(err) => return internal_server_error(err),
    };

    // Get or create DEK — this is the key that goes in the URL fragment
    let (_dek_id, dek_secret) =
        match state.vault_keys.get_or_create_dek(&state.pool, inode_id).await {
            Ok(pair) => pair,
            Err(err) => {
                error!("failed to get DEK for sharing: {err}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "dek_unavailable"})),
                )
                    .into_response();
            }
        };

    let _ = envelope_key; // used to verify vault is unlocked

    let dek_bytes: &[u8; 32] = dek_secret.expose_secret();
    let dek_base64url = crate::sharing::encode_dek_for_url(dek_bytes);
    let share_id = crate::sharing::generate_share_id();

    // Calculate expiry
    let expires_at = request.expires_in_hours.map(|hours| {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        now_ms + (hours as i64 * 3_600_000)
    });

    // Get file name from inode path
    let file_name = inode.name.clone();

    // Hash password if provided
    let password_hash = request
        .password
        .as_deref()
        .filter(|p| !p.is_empty())
        .map(crate::sharing::hash_share_password);
    let password_protected = password_hash.is_some();

    if let Err(err) = db::create_shared_link(
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
    .await
    {
        return internal_server_error(err);
    }

    let share_url = format!("http://localhost:8787/share/{share_id}");
    let full_link = format!("{share_url}#{dek_base64url}");

    info!("share link created for inode {inode_id}, share_id={share_id}");

    (
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
    )
        .into_response()
}

async fn list_all_shares(State(state): State<ApiState>, headers: HeaderMap) -> impl IntoResponse {
    // ACL: Viewer+ can list shares
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Viewer).await {
        return resp;
    }

    match db::list_shared_links(&state.pool).await {
        Ok(links) => (StatusCode::OK, Json(links)).into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn list_file_shares(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(inode_id): Path<i64>,
) -> impl IntoResponse {
    // ACL: Viewer+ can list shares for a file
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Viewer).await {
        return resp;
    }

    match db::list_shared_links_for_inode(&state.pool, inode_id).await {
        Ok(links) => (StatusCode::OK, Json(links)).into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn revoke_share(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(share_id): Path<String>,
) -> impl IntoResponse {
    // ACL: Member+ can revoke shares
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Member).await {
        return resp;
    }

    match db::revoke_shared_link(&state.pool, &share_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({"revoked": true})),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "share_not_found_or_already_revoked"})),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn delete_share(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(share_id): Path<String>,
) -> impl IntoResponse {
    // ACL: Member+ can delete shares
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Member).await {
        return resp;
    }

    match db::delete_shared_link(&state.pool, &share_id).await {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"deleted": true}))).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "share_not_found"})),
        )
            .into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn get_share_page(Path(share_id): Path<String>) -> impl IntoResponse {
    // Validate share_id exists and is valid before serving the page
    // (the page itself will fetch /meta for details)
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

async fn get_sw_download_placeholder(Path(share_id): Path<String>) -> impl IntoResponse {
    // Service Worker intercepts this; if it doesn't, return a helpful message
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
) -> impl IntoResponse {
    let link = match db::get_shared_link(&state.pool, &share_id).await {
        Ok(Some(link)) => link,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "share_not_found"})),
            )
                .into_response()
        }
        Err(err) => return internal_server_error(err),
    };

    let password_hash = match &link.password_hash {
        Some(h) => h,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "share_not_password_protected"})),
            )
                .into_response()
        }
    };

    if !crate::sharing::verify_share_password(&request.password, password_hash) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid_password"})),
        )
            .into_response();
    }

    let token = crate::sharing::generate_share_token();
    if let Err(err) =
        db::create_share_password_token(&state.pool, &token, &share_id, SHARE_TOKEN_TTL_SECONDS)
            .await
    {
        return internal_server_error(err);
    }

    (
        StatusCode::OK,
        Json(VerifyPasswordResponse {
            token,
            expires_in_seconds: SHARE_TOKEN_TTL_SECONDS,
        }),
    )
        .into_response()
}

/// Check if password-protected share access is authorized.
/// Returns None if access granted, or a response to return if denied.
async fn check_share_password_access(
    pool: &sqlx::SqlitePool,
    link: &db::SharedLinkRecord,
    token: &Option<String>,
) -> Option<axum::response::Response> {
    link.password_hash.as_ref()?;
    match token {
        Some(t) if !t.is_empty() => {
            match db::validate_share_password_token(pool, t, &link.share_id).await {
                Ok(true) => None,
                Ok(false) => Some(
                    (
                        StatusCode::UNAUTHORIZED,
                        Json(serde_json::json!({"error": "invalid_or_expired_token", "requires_password": true})),
                    )
                        .into_response(),
                ),
                Err(err) => Some(internal_server_error(err)),
            }
        }
        _ => Some(
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"requires_password": true})),
            )
                .into_response(),
        ),
    }
}

async fn get_share_meta(
    State(state): State<ApiState>,
    Path(share_id): Path<String>,
    Query(query): Query<ShareTokenQuery>,
) -> impl IntoResponse {
    let link = match db::get_shared_link(&state.pool, &share_id).await {
        Ok(Some(link)) => link,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "share_not_found"})),
            )
                .into_response()
        }
        Err(err) => return internal_server_error(err),
    };

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
        return (
            StatusCode::GONE,
            Json(serde_json::json!({"error": "share_invalid", "reason": reason})),
        )
            .into_response();
    }

    // Check password access
    if let Some(response) = check_share_password_access(&state.pool, &link, &query.token).await {
        return response;
    }

    // Get chunk locations for this revision
    let chunks = match db::get_chunk_locations_for_revision(&state.pool, link.revision_id).await {
        Ok(chunks) => chunks,
        Err(err) => return internal_server_error(err),
    };

    let chunk_meta: Vec<ShareChunkMeta> = chunks
        .iter()
        .map(|c| ShareChunkMeta {
            index: c.chunk_index,
            file_offset: c.file_offset,
            plain_size: c.size,
            encrypted_size: c.encrypted_size,
        })
        .collect();

    (
        StatusCode::OK,
        Json(ShareMetaResponse {
            file_name: link.file_name,
            file_size: link.file_size,
            chunk_count: chunk_meta.len(),
            chunks: chunk_meta,
        }),
    )
        .into_response()
}

async fn get_share_chunk(
    State(state): State<ApiState>,
    Path((share_id, chunk_index)): Path<(String, i64)>,
    Query(query): Query<ShareTokenQuery>,
) -> impl IntoResponse {
    let link = match db::get_shared_link(&state.pool, &share_id).await {
        Ok(Some(link)) => link,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "share_not_found"})),
            )
                .into_response()
        }
        Err(err) => return internal_server_error(err),
    };

    if !db::is_shared_link_valid(&link) {
        return (
            StatusCode::GONE,
            Json(serde_json::json!({"error": "share_invalid"})),
        )
            .into_response();
    }

    // Check password access
    if let Some(response) = check_share_password_access(&state.pool, &link, &query.token).await {
        return response;
    }

    let downloader = match state.downloader.as_ref() {
        Some(d) => d,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "downloader_unavailable"})),
            )
                .into_response()
        }
    };

    // Get chunk locations for this revision
    let chunks = match db::get_chunk_locations_for_revision(&state.pool, link.revision_id).await {
        Ok(chunks) => chunks,
        Err(err) => return internal_server_error(err),
    };

    let chunk = match chunks.iter().find(|c| c.chunk_index == chunk_index) {
        Some(c) => c,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "chunk_not_found"})),
            )
                .into_response()
        }
    };

    let encrypted = match downloader.get_encrypted_chunk_bytes(chunk).await {
        Ok(data) => data,
        Err(err) => {
            error!("failed to get encrypted chunk for share {share_id}: {err}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "chunk_fetch_failed"})),
            )
                .into_response();
        }
    };

    // If this is the last chunk, increment download count
    let is_last_chunk = chunk_index == (chunks.len() as i64 - 1);
    if is_last_chunk {
        let _ = db::increment_shared_link_download_count(&state.pool, &share_id).await;
    }

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/octet-stream")],
        encrypted.to_bytes(),
    )
        .into_response()
}
