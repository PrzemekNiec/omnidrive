use crate::acl::{self, Role};
use crate::cache;
use crate::config::AppConfig;
use crate::db;
use crate::disaster_recovery;
use crate::repair::{self, RepairError};
use crate::runtime_paths::RuntimePaths;
use crate::scrubber;
use crate::shell_state;
use crate::smart_sync;
use crate::vault::VaultError;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use tracing::{error, info};

use super::error::ApiError;
use super::{unix_timestamp_millis, ApiState, MaintenanceLevel, MaintenanceOverviewItem,
    MaintenanceStatus,
};

// ── Request / Response types (maintenance-only) ─────────────────────────

#[derive(Serialize)]
struct CacheStatusResponse {
    total_entries: i64,
    total_bytes: i64,
    max_bytes: u64,
    prefetched_entries: i64,
    hit_count: u64,
    miss_count: u64,
}

#[derive(serde::Deserialize)]
struct SnapshotLocalRequest {
    output_path: String,
}

#[derive(Serialize)]
struct SnapshotLocalResponse {
    output_path: String,
    created: bool,
}

#[derive(Serialize)]
struct RecoveryStatusResponse {
    last_successful_backup: Option<i64>,
    recent_attempts: Vec<MetadataBackupAttemptResponse>,
}

#[derive(Serialize)]
struct ScrubStatusResponse {
    total_shards: i64,
    verified_shards: i64,
    healthy_shards: i64,
    corrupted_or_missing: i64,
    verified_light_shards: i64,
    verified_deep_shards: i64,
    last_scrub_timestamp: Option<i64>,
}

#[derive(Serialize)]
struct ScrubErrorResponse {
    pack_id: String,
    provider: String,
    shard_index: i64,
    last_verified_at: Option<i64>,
    status: Option<String>,
}

#[derive(Serialize)]
struct MetadataBackupAttemptResponse {
    backup_id: String,
    created_at: i64,
    snapshot_version: i64,
    object_key: String,
    provider: String,
    encrypted_size: i64,
    status: String,
    last_error: Option<String>,
}

#[derive(Serialize)]
struct MaintenanceOverviewResponse {
    health: MaintenanceOverviewItem,
    shell: MaintenanceOverviewItem,
    sync_root: MaintenanceOverviewItem,
    backup: MaintenanceOverviewItem,
}

// ── Routes ──────────────────────────────────────────────────────────────

pub(super) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/maintenance/status", get(get_maintenance_status))
        .route(
            "/api/maintenance/diagnostics",
            get(get_maintenance_diagnostics),
        )
        .route("/api/cache/status", get(get_cache_status))
        .route("/api/maintenance/scrub-status", get(get_scrub_status))
        .route("/api/maintenance/scrub-errors", get(get_scrub_errors))
        .route("/api/maintenance/scrub-now", post(post_scrub_now))
        .route("/api/maintenance/repair-now", post(post_repair_now))
        .route("/api/maintenance/reconcile-now", post(post_reconcile_now))
        .route("/api/maintenance/repair-shell", post(post_repair_shell))
        .route(
            "/api/maintenance/repair-sync-root",
            post(post_repair_sync_root),
        )
        .route("/api/metadata-backup/status", get(get_recovery_status))
        .route("/api/metadata-backup/backup-now", post(post_backup_now))
        .route("/api/metadata-backup/snapshot-local", post(post_snapshot_local))
        .route("/api/ingest", get(get_ingest_jobs))
        .route("/api/ingest/{job_id}/retry", post(post_ingest_retry))
        .route("/api/ingest/{job_id}/cleanup", post(post_ingest_cleanup))
}

// ── Handlers ────────────────────────────────────────────────────────────

async fn get_maintenance_status(
    State(state): State<ApiState>,
) -> Json<MaintenanceOverviewResponse> {
    let health = match super::diagnostics::build_diagnostics_health_response(&state).await {
        Ok(response) => maintenance_overview_item(&response),
        Err(err) => maintenance_error_item(format!("Diagnostyka health nie powiodła się: {err}")),
    };
    let shell = maintenance_overview_item(&super::diagnostics::build_shell_state_response());
    let sync_root = match super::diagnostics::build_sync_root_state_response() {
        Ok(response) => maintenance_overview_item(&response),
        Err(err) => {
            maintenance_error_item(format!("Diagnostyka sync-root nie powiodła się: {err}"))
        }
    };
    let backup = match build_recovery_status_response(&state).await {
        Ok(response) => maintenance_overview_item(&response),
        Err(err) => maintenance_error_item(format!(
            "Diagnostyka odzyskiwania nie powiodła się: {err}"
        )),
    };

    Json(MaintenanceOverviewResponse {
        health,
        shell,
        sync_root,
        backup,
    })
}

async fn get_maintenance_diagnostics(
    State(state): State<ApiState>,
) -> Json<serde_json::Value> {
    let health = match super::diagnostics::build_diagnostics_health_response(&state).await {
        Ok(response) => serde_json::to_value(response).unwrap_or_default(),
        Err(err) => serde_json::json!({
            "status": "ERROR",
            "message": format!("Diagnostyka health nie powiodła się: {err}"),
            "last_run": unix_timestamp_millis(),
        }),
    };

    let shell =
        serde_json::to_value(super::diagnostics::build_shell_state_response()).unwrap_or_default();
    let sync_root = match super::diagnostics::build_sync_root_state_response() {
        Ok(response) => serde_json::to_value(response).unwrap_or_default(),
        Err(err) => serde_json::json!({
            "status": "ERROR",
            "message": format!("Diagnostyka sync-root nie powiodła się: {err}"),
            "last_run": unix_timestamp_millis(),
        }),
    };

    let backup = match build_recovery_status_response(&state).await {
        Ok(response) => serde_json::to_value(response).unwrap_or_default(),
        Err(err) => serde_json::json!({
            "status": "ERROR",
            "message": format!("Diagnostyka odzyskiwania nie powiodła się: {err}"),
            "last_run": unix_timestamp_millis(),
        }),
    };

    let cache = match db::get_cache_status_summary(&state.pool).await {
        Ok(summary) => {
            let config = AppConfig::from_env();
            let runtime = cache::cache_runtime_stats();
            serde_json::json!({
                "status": "OK",
                "message": "Telemetria cache została zebrana pomyślnie.",
                "last_run": unix_timestamp_millis(),
                "total_entries": summary.total_entries,
                "total_bytes": summary.total_bytes,
                "max_bytes": config.max_cache_bytes,
                "prefetched_entries": summary.prefetched_entries,
                "hit_count": runtime.hit_count,
                "miss_count": runtime.miss_count,
            })
        }
        Err(err) => serde_json::json!({
            "status": "ERROR",
            "message": format!("Diagnostyka cache nie powiodła się: {err}"),
            "last_run": unix_timestamp_millis(),
        }),
    };

    let scrub = match db::get_scrub_status_summary(&state.pool).await {
        Ok(summary) => serde_json::json!({
            "status": "OK",
            "message": "Telemetria scrub została zebrana pomyślnie.",
            "last_run": unix_timestamp_millis(),
            "total_shards": summary.total_shards,
            "verified_shards": summary.verified_shards,
            "healthy_shards": summary.healthy_shards,
            "corrupted_or_missing": summary.corrupted_or_missing,
            "verified_light_shards": summary.verified_light_shards,
            "verified_deep_shards": summary.verified_deep_shards,
            "last_scrub_timestamp": summary.last_scrub_timestamp,
        }),
        Err(err) => serde_json::json!({
            "status": "ERROR",
            "message": format!("Diagnostyka scrub nie powiodła się: {err}"),
            "last_run": unix_timestamp_millis(),
        }),
    };

    let vault_health = match db::get_vault_health_summary(&state.pool).await {
        Ok(summary) => serde_json::json!({
            "status": if summary.unreadable_packs > 0 {
                "ERROR"
            } else if summary.degraded_packs > 0 {
                "WARN"
            } else {
                "OK"
            },
            "message": if summary.unreadable_packs > 0 {
                format!("{} pack(s) are unreadable.", summary.unreadable_packs)
            } else if summary.degraded_packs > 0 {
                format!("{} pack(s) are degraded but recoverable.", summary.degraded_packs)
            } else {
                "Wszystkie aktywne pakiety są zdrowe.".to_string()
            },
            "last_run": unix_timestamp_millis(),
            "total_packs": summary.total_packs,
            "healthy_packs": summary.healthy_packs,
            "degraded_packs": summary.degraded_packs,
            "unreadable_packs": summary.unreadable_packs,
        }),
        Err(err) => serde_json::json!({
            "status": "ERROR",
            "message": format!("Diagnostyka kondycji Skarbca nie powiodła się: {err}"),
            "last_run": unix_timestamp_millis(),
        }),
    };

    Json(serde_json::json!({
        "status": "OK",
        "message": "Migawka diagnostyki utrzymaniowej została zebrana pomyślnie.",
        "last_run": unix_timestamp_millis(),
        "health": health,
        "shell": shell,
        "sync_root": sync_root,
        "backup": backup,
        "cache": cache,
        "scrub": scrub,
        "vault_health": vault_health,
    }))
}

async fn get_cache_status(
    State(state): State<ApiState>,
) -> Result<Json<CacheStatusResponse>, ApiError> {
    let config = AppConfig::from_env();
    let summary = db::get_cache_status_summary(&state.pool).await?;
    let runtime = cache::cache_runtime_stats();

    Ok(Json(CacheStatusResponse {
        total_entries: summary.total_entries,
        total_bytes: summary.total_bytes,
        max_bytes: config.max_cache_bytes,
        prefetched_entries: summary.prefetched_entries,
        hit_count: runtime.hit_count,
        miss_count: runtime.miss_count,
    }))
}

async fn get_recovery_status(
    State(state): State<ApiState>,
) -> Result<Json<MaintenanceStatus<RecoveryStatusResponse>>, ApiError> {
    let response = build_recovery_status_response(&state).await?;
    Ok(Json(response))
}

async fn get_scrub_status(
    State(state): State<ApiState>,
) -> Result<Json<ScrubStatusResponse>, ApiError> {
    let summary = db::get_scrub_status_summary(&state.pool).await?;
    Ok(Json(ScrubStatusResponse {
        total_shards: summary.total_shards,
        verified_shards: summary.verified_shards,
        healthy_shards: summary.healthy_shards,
        corrupted_or_missing: summary.corrupted_or_missing,
        verified_light_shards: summary.verified_light_shards,
        verified_deep_shards: summary.verified_deep_shards,
        last_scrub_timestamp: summary.last_scrub_timestamp,
    }))
}

async fn get_scrub_errors(
    State(state): State<ApiState>,
) -> Result<Json<Vec<ScrubErrorResponse>>, ApiError> {
    let errors = db::list_scrub_errors(&state.pool, 100).await?;
    Ok(Json(
        errors
            .into_iter()
            .map(|record| ScrubErrorResponse {
                pack_id: record.pack_id,
                provider: record.provider,
                shard_index: record.shard_index,
                last_verified_at: record.last_verified_at,
                status: record.last_verification_status,
            })
            .collect(),
    ))
}

async fn post_scrub_now(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Admin).await?;

    match scrubber::run_scrub_batch_now(state.pool.clone()).await {
        Ok(processed_shards) => {
            let _ = db::insert_audit_log(
                &state.pool,
                &caller.vault_id,
                "scrub_start",
                Some(&caller.user_id),
                Some(&caller.device_id),
                None,
                None,
                Some(&format!(r#"{{"processed_shards":{processed_shards}}}"#)),
            )
            .await;
            Ok(Json(serde_json::json!({
            "status": "OK",
            "message": format!("Light scrub completed. Processed {} shard(s).", processed_shards),
            "last_run": unix_timestamp_millis(),
            "processed_shards": processed_shards,
        })))
        }
        Err(scrubber::ScrubberError::MissingProviderConfig) => Ok(Json(serde_json::json!({
            "status": "WARN",
            "message": "Scrub jest bezczynny, ponieważ nie skonfigurowano zdalnych dostawców.",
            "last_run": unix_timestamp_millis(),
            "processed_shards": 0,
        }))),
        Err(err) => {
            error!("api request failed: {err}");
            Err(ApiError::Internal {
                message: err.to_string(),
            })
        }
    }
}

async fn post_repair_now(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Admin).await?;

    match repair::RepairWorker::run_repair_batch_now(state.pool.clone()).await {
        Ok(report) => {
            let _ = db::insert_audit_log(
                &state.pool,
                &caller.vault_id,
                "repair_start",
                Some(&caller.user_id),
                Some(&caller.device_id),
                None,
                None,
                Some(&format!(
                    r#"{{"processed":{},"repaired":{},"reconciled":{}}}"#,
                    report.processed_packs, report.repaired_packs, report.reconciled_packs
                )),
            )
            .await;
            Ok(Json(serde_json::json!({
            "status": "OK",
            "message": if report.repaired_packs == 0 {
                "Brak zdegradowanych pakietów wymagających natychmiastowej naprawy.".to_string()
            } else {
                format!("Naprawa przetworzyła {} zdegradowanych pakietów.", report.repaired_packs)
            },
            "last_run": unix_timestamp_millis(),
            "processed_packs": report.processed_packs,
            "repaired_packs": report.repaired_packs,
            "reconciled_packs": report.reconciled_packs,
        })))
        }
        Err(RepairError::MissingProviderConfig) => Ok(Json(serde_json::json!({
            "status": "WARN",
            "message": "Naprawa jest bezczynna, ponieważ nie skonfigurowano zdalnych dostawców.",
            "last_run": unix_timestamp_millis(),
            "processed_packs": 0,
            "repaired_packs": 0,
            "reconciled_packs": 0,
        }))),
        Err(err) => {
            error!("api request failed: {err}");
            Err(ApiError::Internal {
                message: err.to_string(),
            })
        }
    }
}

async fn post_reconcile_now(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Admin).await?;

    match repair::RepairWorker::run_reconcile_batch_now(state.pool.clone()).await {
        Ok(report) => {
            let _ = db::insert_audit_log(
                &state.pool,
                &caller.vault_id,
                "reconcile_start",
                Some(&caller.user_id),
                Some(&caller.device_id),
                None,
                None,
                Some(&format!(
                    r#"{{"processed":{},"repaired":{},"reconciled":{}}}"#,
                    report.processed_packs, report.repaired_packs, report.reconciled_packs
                )),
            )
            .await;
            Ok(Json(serde_json::json!({
            "status": "OK",
            "message": if report.reconciled_packs == 0 {
                "No pack policy drift required reconciliation.".to_string()
            } else {
                format!("Reconciliation processed {} pack(s).", report.reconciled_packs)
            },
            "last_run": unix_timestamp_millis(),
            "processed_packs": report.processed_packs,
            "repaired_packs": report.repaired_packs,
            "reconciled_packs": report.reconciled_packs,
        })))
        }
        Err(RepairError::MissingProviderConfig) => Ok(Json(serde_json::json!({
            "status": "WARN",
            "message": "Proces reconciliation jest bezczynny, ponieważ nie skonfigurowano zdalnych dostawców.",
            "last_run": unix_timestamp_millis(),
            "processed_packs": 0,
            "repaired_packs": 0,
            "reconciled_packs": 0,
        }))),
        Err(err) => {
            error!("api request failed: {err}");
            Err(ApiError::Internal {
                message: err.to_string(),
            })
        }
    }
}

async fn post_repair_shell() -> Result<Json<serde_json::Value>, ApiError> {
    let mut actions = Vec::new();

    let drive_report = shell_state::repair_virtual_drive().map_err(|err| {
        error!("shell repair virtual drive failed: {}", err);
        ApiError::Internal {
            message: format!("shell repair failed at virtual_drive: {err}"),
        }
    })?;
    actions.extend(drive_report.actions);

    let explorer_report = shell_state::repair_explorer_integration().map_err(|err| {
        error!("shell repair explorer integration failed: {}", err);
        ApiError::Internal {
            message: format!("shell repair failed at explorer_integration: {err}"),
        }
    })?;
    actions.extend(explorer_report.actions);
    let last_state = explorer_report.shell_state;

    Ok(Json(serde_json::json!({
        "status": "OK",
        "message": if actions.is_empty() {
            "Stan shell byl juz poprawny.".to_string()
        } else {
            format!("Naprawa shell zastosowala {} akcje.", actions.len())
        },
        "last_run": unix_timestamp_millis(),
        "actions": actions,
        "shell_state": last_state,
    })))
}

async fn post_repair_sync_root(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Admin).await?;

    let runtime_paths = RuntimePaths::detect();
    let report = smart_sync::repair_sync_root(&state.pool, &runtime_paths.sync_root)
        .await
        .map_err(|err| ApiError::Internal {
            message: err.to_string(),
        })?;

    let _ = db::insert_audit_log(
        &state.pool,
        &caller.vault_id,
        "repair_sync_root",
        Some(&caller.user_id),
        Some(&caller.device_id),
        None,
        None,
        Some(&format!(r#"{{"actions_count":{}}}"#, report.actions.len())),
    )
    .await;

    Ok(Json(serde_json::json!({
        "status": "OK",
        "message": if report.actions.is_empty() {
            "Sync-root byl juz poprawny.".to_string()
        } else {
            format!("Naprawa sync-root zastosowala {} akcje.", report.actions.len())
        },
        "last_run": unix_timestamp_millis(),
        "actions": report.actions,
        "sync_root_state": report.sync_root_state,
    })))
}

async fn post_snapshot_local(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<SnapshotLocalRequest>,
) -> Result<Json<SnapshotLocalResponse>, ApiError> {
    acl::require_role(&state.pool, &headers, Role::Admin).await?;

    let output_path = std::path::PathBuf::from(&request.output_path);
    let master_key = state.vault_keys.require_master_key().await.map_err(|err| {
        match err {
            VaultError::Locked => ApiError::BadRequest {
                code: "vault_locked",
                message: "odblokuj Skarbiec przed utworzeniem zaszyfrowanej migawki metadanych".to_string(),
            },
            other => ApiError::Internal {
                message: other.to_string(),
            },
        }
    })?;

    disaster_recovery::create_encrypted_metadata_snapshot(
        &state.pool,
        &output_path,
        &master_key,
    )
    .await
    .map_err(|err| ApiError::Internal {
        message: err.to_string(),
    })?;

    Ok(Json(SnapshotLocalResponse {
        output_path: if output_path
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(".enc")
        {
            output_path.display().to_string()
        } else {
            format!("{}.enc", output_path.display())
        },
        created: true,
    }))
}

async fn post_backup_now(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Admin).await?;

    let master_key = state.vault_keys.require_master_key().await.map_err(|err| {
        match err {
            VaultError::Locked => ApiError::BadRequest {
                code: "vault_locked",
                message: "odblokuj Skarbiec przed utworzeniem zaszyfrowanej kopii metadanych".to_string(),
            },
            other => ApiError::Internal {
                message: other.to_string(),
            },
        }
    })?;

    let provider_manager =
        disaster_recovery::MetadataBackupProviderManager::from_onboarding_db_all(&state.pool)
            .await
            .map_err(|err| ApiError::Internal {
                message: err.to_string(),
            })?;

    disaster_recovery::run_metadata_backup_now(&state.pool, &provider_manager, &master_key)
        .await
        .map_err(|err| ApiError::Internal {
            message: err.to_string(),
        })?;

    let _ = db::insert_audit_log(
        &state.pool,
        &caller.vault_id,
        "backup_start",
        Some(&caller.user_id),
        Some(&caller.device_id),
        None,
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({
        "status": "OK",
        "message": "Zaszyfrowana kopia metadanych została wysłana pomyślnie.",
        "last_run": unix_timestamp_millis(),
        "uploaded": true
    })))
}

// ── Ingest API (Epic 35.1e) ──────────────────────────────────────────

async fn get_ingest_jobs(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = db::list_ingest_jobs(&state.pool).await?;
    let jobs: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "id": row.id,
                "file_path": row.file_path,
                "file_size": row.file_size,
                "state": row.state,
                "bytes_processed": row.bytes_processed,
                "attempt_count": row.attempt_count,
                "error_message": row.error_message,
                "created_at": row.created_at,
                "updated_at": row.updated_at,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "jobs": jobs })))
}

async fn post_ingest_retry(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(job_id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acl::require_role(&state.pool, &headers, Role::Member).await?;

    let retried = db::retry_ingest_job(&state.pool, job_id).await?;
    if retried {
        info!("api: ingest job {} requeued for retry", job_id);
        Ok(Json(serde_json::json!({ "ok": true, "job_id": job_id })))
    } else {
        Err(ApiError::NotFound {
            resource: "ingest_job",
            id: job_id.to_string(),
        })
    }
}

async fn post_ingest_cleanup(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(job_id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acl::require_role(&state.pool, &headers, Role::Member).await?;

    let spool_dir = std::env::var("OMNIDRIVE_SPOOL_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(".omnidrive/spool"));

    match crate::ingest::cleanup_failed_ingest(&state.pool, &spool_dir, job_id).await {
        Ok(true) => {
            info!("api: ingest job {} cleaned up", job_id);
            Ok(Json(serde_json::json!({ "ok": true, "job_id": job_id })))
        }
        Ok(false) => Err(ApiError::NotFound {
            resource: "ingest_job",
            id: job_id.to_string(),
        }),
        Err(err) => Err(ApiError::BadRequest {
            code: "cleanup_failed",
            message: err.to_string(),
        }),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

async fn build_recovery_status_response(
    state: &ApiState,
) -> Result<MaintenanceStatus<RecoveryStatusResponse>, sqlx::Error> {
    let last_successful_backup = db::get_last_successful_metadata_backup_at(&state.pool).await?;
    let recent_attempts: Vec<MetadataBackupAttemptResponse> =
        db::list_recent_metadata_backups(&state.pool, 10)
            .await?
            .into_iter()
            .map(|record| MetadataBackupAttemptResponse {
                backup_id: record.backup_id,
                created_at: record.created_at,
                snapshot_version: record.snapshot_version,
                object_key: record.object_key,
                provider: record.provider,
                encrypted_size: record.encrypted_size,
                status: record.status,
                last_error: record.last_error,
            })
            .collect();

    let last_run = recent_attempts
        .iter()
        .map(|attempt| attempt.created_at)
        .max()
        .or(last_successful_backup)
        .unwrap_or_else(unix_timestamp_millis);

    let latest_failed = recent_attempts
        .iter()
        .find(|attempt| attempt.status.eq_ignore_ascii_case("FAILED"));
    let (level, message) = if let Some(attempt) = latest_failed {
        (
            MaintenanceLevel::Warn,
            format!(
                "Ostatnia kopia metadanych do {} nie powiodła się{}",
                attempt.provider,
                attempt
                    .last_error
                    .as_deref()
                    .map(|err| format!(": {}", err))
                    .unwrap_or_default()
            ),
        )
    } else if let Some(timestamp) = last_successful_backup {
        (
            MaintenanceLevel::Ok,
            format!(
                "Kopia metadanych jest dostępna. Ostatni udany przebieg: {}.",
                timestamp
            ),
        )
    } else {
        (
            MaintenanceLevel::Warn,
            "Nie zarejestrowano jeszcze kopii metadanych.".to_string(),
        )
    };

    Ok(MaintenanceStatus {
        status: level.as_str().to_string(),
        message,
        last_run,
        details: RecoveryStatusResponse {
            last_successful_backup,
            recent_attempts,
        },
    })
}

fn maintenance_overview_item<T>(status: &MaintenanceStatus<T>) -> MaintenanceOverviewItem
where
    T: Serialize,
{
    MaintenanceOverviewItem {
        status: status.status.clone(),
        message: status.message.clone(),
        last_run: status.last_run,
    }
}

fn maintenance_error_item(message: String) -> MaintenanceOverviewItem {
    MaintenanceOverviewItem {
        status: MaintenanceLevel::Error.as_str().to_string(),
        message,
        last_run: unix_timestamp_millis(),
    }
}
