//! Epic 34.5: Audit Trail — read-only API for listing audit events.
//!
//! The write path is the collection of `db::insert_audit_log` call sites in
//! the various API handlers (unlock, logout, share create/revoke/delete,
//! provider configure, onboarding complete/join/reset, scrub/repair/reconcile/
//! backup, device accept/revoke, member invite/join/remove).  This module
//! exposes a read endpoint that any vault member can use to inspect recent
//! activity.

use crate::acl::{self, Role};
use crate::db;

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::error::ApiError;
use super::ApiState;

const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 500;

pub(super) fn routes() -> Router<ApiState> {
    Router::new().route("/api/audit", get(list_audit_events))
}

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
}

#[derive(Serialize)]
struct AuditEvent {
    id: i64,
    timestamp: i64,
    action: String,
    actor_user_id: Option<String>,
    actor_device_id: Option<String>,
    target_user_id: Option<String>,
    target_device_id: Option<String>,
    details: Option<String>,
}

#[derive(Serialize)]
struct AuditListResponse {
    vault_id: String,
    events: Vec<AuditEvent>,
}

async fn list_audit_events(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AuditListResponse>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Admin).await?;
    let limit = query
        .limit
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);

    let records = db::list_audit_logs(&state.pool, &caller.vault_id, limit).await?;
    let events = records
        .into_iter()
        .map(|r| AuditEvent {
            id: r.id,
            timestamp: r.timestamp,
            action: r.action,
            actor_user_id: r.actor_user_id,
            actor_device_id: r.actor_device_id,
            target_user_id: r.target_user_id,
            target_device_id: r.target_device_id,
            details: r.details,
        })
        .collect();

    Ok(Json(AuditListResponse {
        vault_id: caller.vault_id,
        events,
    }))
}
