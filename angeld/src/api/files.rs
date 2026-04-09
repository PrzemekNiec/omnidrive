use crate::acl::{self, Role};
use crate::config::AppConfig;
use crate::db;
use crate::smart_sync;
use crate::uploader::KNOWN_PROVIDERS;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::env;

use super::ApiState;

// ── Response / Request structs ──────────────────────────────────────────

#[derive(Serialize)]
struct DeleteFileResponse {
    inode_id: i64,
    deleted: bool,
}

#[derive(Serialize)]
struct FileEntryResponse {
    inode_id: i64,
    path: String,
    size: i64,
    current_revision_id: Option<i64>,
    current_revision_created_at: Option<i64>,
    smart_sync_pin_state: Option<i64>,
    smart_sync_hydration_state: Option<i64>,
}

#[derive(Serialize)]
struct FileRevisionResponse {
    revision_id: i64,
    inode_id: i64,
    created_at: i64,
    size: i64,
    is_current: bool,
    immutable_until: Option<i64>,
    device_id: Option<String>,
    parent_revision_id: Option<i64>,
    origin: String,
    conflict_reason: Option<String>,
}

#[derive(Serialize)]
struct RestoreRevisionResponse {
    inode_id: i64,
    revision_id: i64,
    restored: bool,
    conflict_copy_inode_id: Option<i64>,
    conflict_copy_revision_id: Option<i64>,
    conflict_copy_name: Option<String>,
}

#[derive(Serialize)]
struct ConflictCopyResponse {
    inode_id: i64,
    source_revision_id: i64,
    conflict_copy_inode_id: i64,
    conflict_copy_revision_id: i64,
    conflict_copy_name: String,
    conflict_id: i64,
}

#[derive(Serialize)]
struct QuotaResponse {
    max_physical_bytes_per_provider: u64,
    providers: Vec<ProviderQuotaResponse>,
}

#[derive(Serialize)]
struct ProviderQuotaResponse {
    provider: String,
    used_physical_bytes: u64,
}

#[derive(Serialize)]
struct SmartSyncStatusResponse {
    inode_id: i64,
    revision_id: i64,
    pin_state: i64,
    hydration_state: i64,
}

#[derive(Serialize)]
struct SmartSyncActionResponse {
    inode_id: i64,
    pin_state: i64,
    hydration_state: i64,
}

#[derive(Deserialize)]
struct FilesystemPolicyRequest {
    path: String,
    policy_type: String,
}

#[derive(Deserialize)]
struct FilesystemPathRequest {
    path: String,
}

#[derive(Serialize)]
struct FilesystemPolicyResponse {
    inode_id: i64,
    path: String,
    policy_type: String,
    repair_reconciliation_scheduled: bool,
}

// ── Routes ──────────────────────────────────────────────────────────────

pub(super) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/files", get(get_files))
        .route("/api/files/{inode_id}", delete(delete_file))
        .route(
            "/api/files/{inode_id}/sync_status",
            get(get_file_sync_status),
        )
        .route("/api/files/{inode_id}/pin", post(pin_file))
        .route("/api/files/{inode_id}/unpin", post(unpin_file))
        .route("/api/filesystem/set-policy", post(set_filesystem_policy))
        .route("/api/filesystem/pin", post(pin_filesystem_path))
        .route("/api/filesystem/unpin", post(unpin_filesystem_path))
        .route("/api/files/{inode_id}/revisions", get(get_file_revisions))
        .route(
            "/api/files/{inode_id}/revisions/{revision_id}/restore",
            post(restore_file_revision),
        )
        .route(
            "/api/files/{inode_id}/revisions/{revision_id}/materialize-conflict-copy",
            post(materialize_conflict_copy),
        )
        .route("/api/quota", get(get_quota))
}

// ── Handlers ────────────────────────────────────────────────────────────

async fn delete_file(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(inode_id): Path<i64>,
) -> impl IntoResponse {
    // ACL: Member+ can delete files
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Member).await {
        return resp;
    }

    let inode = match db::get_inode_by_id(&state.pool, inode_id).await {
        Ok(inode) => inode,
        Err(err) => return super::internal_server_error(err),
    };

    let Some(inode) = inode else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "inode_not_found",
                "inode_id": inode_id,
            })),
        )
            .into_response();
    };

    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "inode_not_file",
                "inode_id": inode_id,
                "kind": inode.kind,
            })),
        )
            .into_response();
    }

    if let Err(err) = db::delete_file_chunks(&state.pool, inode_id).await {
        return super::internal_server_error(err);
    }
    if let Err(err) = db::delete_inode_record(&state.pool, inode_id).await {
        return super::internal_server_error(err);
    }

    (
        StatusCode::OK,
        Json(DeleteFileResponse {
            inode_id,
            deleted: true,
        }),
    )
        .into_response()
}

async fn get_files(State(state): State<ApiState>, headers: HeaderMap) -> impl IntoResponse {
    // ACL: Viewer+ can list files
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Viewer).await {
        return resp;
    }

    match db::list_active_files(&state.pool).await {
        Ok(files) => (
            StatusCode::OK,
            Json(
                files
                    .into_iter()
                    .map(|file| FileEntryResponse {
                        inode_id: file.inode_id,
                        path: file.path,
                        size: file.size,
                        current_revision_id: file.current_revision_id,
                        current_revision_created_at: file.current_revision_created_at,
                        smart_sync_pin_state: file.smart_sync_pin_state,
                        smart_sync_hydration_state: file.smart_sync_hydration_state,
                    })
                    .collect::<Vec<_>>(),
            ),
        )
            .into_response(),
        Err(err) => super::internal_server_error(err),
    }
}

async fn get_file_sync_status(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(inode_id): Path<i64>,
) -> impl IntoResponse {
    // ACL: Viewer+ can check sync status
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Viewer).await {
        return resp;
    }

    match db::get_smart_sync_state(&state.pool, inode_id).await {
        Ok(Some(status)) => (
            StatusCode::OK,
            Json(SmartSyncStatusResponse {
                inode_id: status.inode_id,
                revision_id: status.revision_id,
                pin_state: status.pin_state,
                hydration_state: status.hydration_state,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "smart_sync_state_not_found",
                "inode_id": inode_id,
            })),
        )
            .into_response(),
        Err(err) => super::internal_server_error(err),
    }
}

async fn pin_file(State(state): State<ApiState>, headers: HeaderMap, Path(inode_id): Path<i64>) -> impl IntoResponse {
    // ACL: Member+ can pin files
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Member).await {
        return resp;
    }

    let inode = match db::get_inode_by_id(&state.pool, inode_id).await {
        Ok(inode) => inode,
        Err(err) => return super::internal_server_error(err),
    };
    let Some(inode) = inode else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "inode_not_found", "inode_id": inode_id })),
        )
            .into_response();
    };
    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "inode_not_file", "inode_id": inode_id })),
        )
            .into_response();
    }

    let sync_root = sync_root_path();
    if let Err(err) = db::set_pin_state(&state.pool, inode_id, 1).await {
        return super::internal_server_error(err);
    }
    if let Err(err) =
        smart_sync::sync_placeholder_pin_state(&state.pool, &sync_root, inode_id, false).await
    {
        return super::internal_server_error(err);
    }

    match db::get_smart_sync_state(&state.pool, inode_id).await {
        Ok(Some(status)) => (
            StatusCode::OK,
            Json(SmartSyncActionResponse {
                inode_id: status.inode_id,
                pin_state: status.pin_state,
                hydration_state: status.hydration_state,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": "smart_sync_state_not_found", "inode_id": inode_id }),
            ),
        )
            .into_response(),
        Err(err) => super::internal_server_error(err),
    }
}

async fn unpin_file(State(state): State<ApiState>, headers: HeaderMap, Path(inode_id): Path<i64>) -> impl IntoResponse {
    // ACL: Member+ can unpin files
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Member).await {
        return resp;
    }

    let inode = match db::get_inode_by_id(&state.pool, inode_id).await {
        Ok(inode) => inode,
        Err(err) => return super::internal_server_error(err),
    };
    let Some(inode) = inode else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "inode_not_found", "inode_id": inode_id })),
        )
            .into_response();
    };
    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "inode_not_file", "inode_id": inode_id })),
        )
            .into_response();
    }

    let sync_root = sync_root_path();
    if let Err(err) = db::set_pin_state(&state.pool, inode_id, 0).await {
        return super::internal_server_error(err);
    }
    if let Err(err) =
        smart_sync::sync_placeholder_pin_state(&state.pool, &sync_root, inode_id, true).await
    {
        return super::internal_server_error(err);
    }

    match db::get_smart_sync_state(&state.pool, inode_id).await {
        Ok(Some(status)) => (
            StatusCode::OK,
            Json(SmartSyncActionResponse {
                inode_id: status.inode_id,
                pin_state: status.pin_state,
                hydration_state: status.hydration_state,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": "smart_sync_state_not_found", "inode_id": inode_id }),
            ),
        )
            .into_response(),
        Err(err) => super::internal_server_error(err),
    }
}

async fn set_filesystem_policy(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<FilesystemPolicyRequest>,
) -> impl IntoResponse {
    // ACL: Member+ can set filesystem policies
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Member).await {
        return resp;
    }

    let policy_type = match normalize_policy_type(&request.policy_type) {
        Some(policy_type) => policy_type,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_policy_type",
                    "policy_type": request.policy_type,
                })),
            )
                .into_response();
        }
    };

    let (inode_id, logical_path, inode) =
        match resolve_filesystem_request_target(&state.pool, &request.path).await {
            Ok(target) => target,
            Err(response) => return response,
        };

    if let Err(err) =
        db::set_sync_policy_type_for_path(&state.pool, &logical_path, policy_type).await
    {
        return super::internal_server_error(err);
    }

    if policy_type == "LOCAL" && inode.kind == "FILE" {
        let sync_root = sync_root_path();
        if let Err(err) = db::set_pin_state(&state.pool, inode_id, 1).await {
            return super::internal_server_error(err);
        }
        if let Err(err) =
            smart_sync::hydrate_placeholder_now(&state.pool, &sync_root, inode_id).await
        {
            return super::internal_server_error(err);
        }
    }

    (
        StatusCode::OK,
        Json(FilesystemPolicyResponse {
            inode_id,
            path: logical_path,
            policy_type: policy_type.to_string(),
            repair_reconciliation_scheduled: policy_type == "PARANOIA",
        }),
    )
        .into_response()
}

async fn pin_filesystem_path(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<FilesystemPathRequest>,
) -> impl IntoResponse {
    // ACL: Member+ can pin filesystem paths
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Member).await {
        return resp;
    }

    let (inode_id, _, inode) =
        match resolve_filesystem_request_target(&state.pool, &request.path).await {
            Ok(target) => target,
            Err(response) => return response,
        };
    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "inode_not_file",
                "inode_id": inode_id,
                "kind": inode.kind,
            })),
        )
            .into_response();
    }

    let sync_root = sync_root_path();
    if let Err(err) = db::set_pin_state(&state.pool, inode_id, 1).await {
        return super::internal_server_error(err);
    }
    if let Err(err) = smart_sync::hydrate_placeholder_now(&state.pool, &sync_root, inode_id).await {
        return super::internal_server_error(err);
    }

    match db::get_smart_sync_state(&state.pool, inode_id).await {
        Ok(Some(status)) => (
            StatusCode::OK,
            Json(SmartSyncActionResponse {
                inode_id: status.inode_id,
                pin_state: status.pin_state,
                hydration_state: status.hydration_state,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": "smart_sync_state_not_found", "inode_id": inode_id }),
            ),
        )
            .into_response(),
        Err(err) => super::internal_server_error(err),
    }
}

async fn unpin_filesystem_path(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<FilesystemPathRequest>,
) -> impl IntoResponse {
    // ACL: Member+ can unpin filesystem paths
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Member).await {
        return resp;
    }

    let (inode_id, _, inode) =
        match resolve_filesystem_request_target(&state.pool, &request.path).await {
            Ok(target) => target,
            Err(response) => return response,
        };
    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "inode_not_file",
                "inode_id": inode_id,
                "kind": inode.kind,
            })),
        )
            .into_response();
    }

    let sync_root = sync_root_path();
    if let Err(err) = db::set_pin_state(&state.pool, inode_id, 0).await {
        return super::internal_server_error(err);
    }
    if let Err(err) =
        smart_sync::sync_placeholder_pin_state(&state.pool, &sync_root, inode_id, true).await
    {
        return super::internal_server_error(err);
    }

    match db::get_smart_sync_state(&state.pool, inode_id).await {
        Ok(Some(status)) => (
            StatusCode::OK,
            Json(SmartSyncActionResponse {
                inode_id: status.inode_id,
                pin_state: status.pin_state,
                hydration_state: status.hydration_state,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": "smart_sync_state_not_found", "inode_id": inode_id }),
            ),
        )
            .into_response(),
        Err(err) => super::internal_server_error(err),
    }
}

async fn get_file_revisions(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(inode_id): Path<i64>,
) -> impl IntoResponse {
    // ACL: Viewer+ can view file revisions
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Viewer).await {
        return resp;
    }

    let inode = match db::get_inode_by_id(&state.pool, inode_id).await {
        Ok(inode) => inode,
        Err(err) => return super::internal_server_error(err),
    };

    let Some(inode) = inode else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "inode_not_found",
                "inode_id": inode_id,
            })),
        )
            .into_response();
    };

    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "inode_not_file",
                "inode_id": inode_id,
                "kind": inode.kind,
            })),
        )
            .into_response();
    }

    match db::list_file_revisions(&state.pool, inode_id).await {
        Ok(revisions) => (
            StatusCode::OK,
            Json(
                revisions
                    .into_iter()
                    .map(|revision| FileRevisionResponse {
                        revision_id: revision.revision_id,
                        inode_id: revision.inode_id,
                        created_at: revision.created_at,
                        size: revision.size,
                        is_current: revision.is_current != 0,
                        immutable_until: revision.immutable_until,
                        device_id: revision.device_id,
                        parent_revision_id: revision.parent_revision_id,
                        origin: revision.origin,
                        conflict_reason: revision.conflict_reason,
                    })
                    .collect::<Vec<_>>(),
            ),
        )
            .into_response(),
        Err(err) => super::internal_server_error(err),
    }
}

async fn restore_file_revision(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((inode_id, revision_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    // ACL: Member+ can restore file revisions
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Member).await {
        return resp;
    }

    let inode = match db::get_inode_by_id(&state.pool, inode_id).await {
        Ok(inode) => inode,
        Err(err) => return super::internal_server_error(err),
    };

    let Some(inode) = inode else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "inode_not_found",
                "inode_id": inode_id,
            })),
        )
            .into_response();
    };

    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "inode_not_file",
                "inode_id": inode_id,
                "kind": inode.kind,
            })),
        )
            .into_response();
    }

    let revision = match db::get_file_revision(&state.pool, inode_id, revision_id).await {
        Ok(revision) => revision,
        Err(err) => return super::internal_server_error(err),
    };

    let Some(_) = revision else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "revision_not_found",
                "inode_id": inode_id,
                "revision_id": revision_id,
            })),
        )
            .into_response();
    };

    let current_revision = match db::get_current_file_revision(&state.pool, inode_id).await {
        Ok(revision) => revision,
        Err(err) => return super::internal_server_error(err),
    };

    let local_device = match db::get_local_device_identity(&state.pool).await {
        Ok(device) => device,
        Err(err) => return super::internal_server_error(err),
    };
    let conflict_device_id = local_device
        .as_ref()
        .map(|device| device.device_id.as_str());
    let conflict_device_name = local_device
        .as_ref()
        .map(|device| device.device_name.as_str())
        .unwrap_or("Unknown Device");

    let conflict_copy = match current_revision {
        Some(current) => {
            let lineage =
                match db::classify_revision_lineage(&state.pool, revision_id, current.revision_id)
                    .await
                {
                    Ok(lineage) => lineage,
                    Err(err) => return super::internal_server_error(err),
                };

            let conflict_reason = match lineage {
                db::RevisionLineageRelation::Same
                | db::RevisionLineageRelation::CandidateDescendsFromCurrent => None,
                db::RevisionLineageRelation::CurrentDescendsFromCandidate => Some("restore_rewind"),
                db::RevisionLineageRelation::Parallel => Some("parallel_restore"),
            };

            match conflict_reason {
                Some(reason) => match db::materialize_conflict_copy_from_revision(
                    &state.pool,
                    current.revision_id,
                    conflict_device_id,
                    conflict_device_name,
                    reason,
                )
                .await
                {
                    Ok((conflict_inode_id, conflict_revision_id, conflict_name, _conflict_id)) => {
                        Some((conflict_inode_id, conflict_revision_id, conflict_name))
                    }
                    Err(err) => return super::internal_server_error(err),
                },
                None => None,
            }
        }
        None => None,
    };

    match db::promote_revision_to_current(&state.pool, revision_id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(RestoreRevisionResponse {
                inode_id,
                revision_id,
                restored: true,
                conflict_copy_inode_id: conflict_copy.as_ref().map(|value| value.0),
                conflict_copy_revision_id: conflict_copy.as_ref().map(|value| value.1),
                conflict_copy_name: conflict_copy.map(|value| value.2),
            }),
        )
            .into_response(),
        Err(err) => super::internal_server_error(err),
    }
}

async fn materialize_conflict_copy(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((inode_id, revision_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    // ACL: Member+ can materialize conflict copies
    if let Err(resp) = acl::require_role(&state.pool, &headers, Role::Member).await {
        return resp;
    }

    let inode = match db::get_inode_by_id(&state.pool, inode_id).await {
        Ok(inode) => inode,
        Err(err) => return super::internal_server_error(err),
    };

    let Some(inode) = inode else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "inode_not_found",
                "inode_id": inode_id,
            })),
        )
            .into_response();
    };

    if inode.kind != "FILE" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "inode_not_file",
                "inode_id": inode_id,
                "kind": inode.kind,
            })),
        )
            .into_response();
    }

    let revision = match db::get_file_revision(&state.pool, inode_id, revision_id).await {
        Ok(revision) => revision,
        Err(err) => return super::internal_server_error(err),
    };

    let Some(_) = revision else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "revision_not_found",
                "inode_id": inode_id,
                "revision_id": revision_id,
            })),
        )
            .into_response();
    };

    let local_device = match db::get_local_device_identity(&state.pool).await {
        Ok(device) => device,
        Err(err) => return super::internal_server_error(err),
    };
    let conflict_device_id = local_device
        .as_ref()
        .map(|device| device.device_id.as_str());
    let conflict_device_name = local_device
        .as_ref()
        .map(|device| device.device_name.as_str())
        .unwrap_or("Unknown Device");

    match db::materialize_conflict_copy_from_revision(
        &state.pool,
        revision_id,
        conflict_device_id,
        conflict_device_name,
        "manual_conflict_copy",
    )
    .await
    {
        Ok((conflict_inode_id, conflict_revision_id, conflict_name, conflict_id)) => (
            StatusCode::OK,
            Json(ConflictCopyResponse {
                inode_id,
                source_revision_id: revision_id,
                conflict_copy_inode_id: conflict_inode_id,
                conflict_copy_revision_id: conflict_revision_id,
                conflict_copy_name: conflict_name,
                conflict_id,
            }),
        )
            .into_response(),
        Err(err) => super::internal_server_error(err),
    }
}

async fn get_quota(State(state): State<ApiState>) -> impl IntoResponse {
    let app_config = AppConfig::from_env();
    let mut providers = Vec::with_capacity(KNOWN_PROVIDERS.len());

    for provider in KNOWN_PROVIDERS {
        match db::get_physical_usage_for_provider(&state.pool, provider).await {
            Ok(used_physical_bytes) => providers.push(ProviderQuotaResponse {
                provider: provider.to_string(),
                used_physical_bytes,
            }),
            Err(err) => return super::internal_server_error(err),
        }
    }

    (
        StatusCode::OK,
        Json(QuotaResponse {
            max_physical_bytes_per_provider: app_config.max_physical_bytes_per_provider,
            providers,
        }),
    )
        .into_response()
}

// ── Helper functions ────────────────────────────────────────────────────

fn normalize_policy_type(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_uppercase().as_str() {
        "PARANOIA" => Some("PARANOIA"),
        "STANDARD" => Some("STANDARD"),
        "LOCAL" => Some("LOCAL"),
        _ => None,
    }
}

async fn resolve_filesystem_request_target(
    pool: &SqlitePool,
    raw_path: &str,
) -> Result<(i64, String, db::InodeRecord), axum::response::Response> {
    let logical_path = match normalize_filesystem_api_path(raw_path) {
        Some(path) => path,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_filesystem_path",
                    "path": raw_path,
                })),
            )
                .into_response());
        }
    };

    let inode_id = match db::resolve_path(pool, &logical_path).await {
        Ok(Some(inode_id)) => inode_id,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "inode_not_found",
                    "path": logical_path,
                })),
            )
                .into_response());
        }
        Err(err) => return Err(super::internal_server_error(err)),
    };

    let inode = match db::get_inode_by_id(pool, inode_id).await {
        Ok(Some(inode)) => inode,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "inode_not_found",
                    "inode_id": inode_id,
                })),
            )
                .into_response());
        }
        Err(err) => return Err(super::internal_server_error(err)),
    };

    let canonical_path = match db::get_inode_path(pool, inode_id).await {
        Ok(Some(path)) => path,
        Ok(None) => logical_path,
        Err(err) => return Err(super::internal_server_error(err)),
    };

    Ok((inode_id, canonical_path, inode))
}

fn normalize_filesystem_api_path(raw_path: &str) -> Option<String> {
    let trimmed = raw_path.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }

    let sync_root = sync_root_path();
    let drive_letter = env::var("OMNIDRIVE_DRIVE_LETTER").unwrap_or_else(|_| "O:".to_string());
    let drive_prefix = format!(
        "{}\\",
        drive_letter
            .trim()
            .trim_end_matches('\\')
            .trim_end_matches('/')
            .to_ascii_uppercase()
    );
    let candidate = trimmed.replace('/', "\\");
    let candidate_upper = candidate.to_ascii_uppercase();
    let sync_root_rendered = sync_root.to_string_lossy().replace('/', "\\");
    let sync_root_upper = sync_root_rendered.to_ascii_uppercase();

    let relative = if candidate_upper.starts_with(&drive_prefix) {
        candidate[drive_prefix.len()..].to_string()
    } else if candidate_upper.starts_with(&(sync_root_upper.clone() + "\\")) {
        candidate[(sync_root_rendered.len() + 1)..].to_string()
    } else {
        candidate
    };

    let normalized = relative
        .trim_start_matches('\\')
        .trim_start_matches('/')
        .replace('\\', "/");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn sync_root_path() -> std::path::PathBuf {
    crate::runtime_paths::RuntimePaths::detect().sync_root
}
