use crate::cache;
use crate::cloud_guard;
use crate::config::AppConfig;
use crate::db;
use crate::peer;
use crate::runtime_paths::RuntimePaths;
use crate::shell_state;
use crate::smart_sync;
use crate::uploader::KNOWN_PROVIDERS;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashMap;
use super::{internal_server_error, unix_timestamp_millis, ApiState, MaintenanceLevel, MaintenanceStatus};

// ── Response structs ────────────────────────────────────────────────

#[derive(Serialize)]
struct TransferResponse {
    job_id: i64,
    pack_id: String,
    status: String,
    attempts: i64,
    providers: Vec<ProviderTransferResponse>,
}

#[derive(Serialize)]
struct ProviderTransferResponse {
    provider: String,
    status: String,
    attempts: i64,
    last_error: Option<String>,
    bucket: Option<String>,
    object_key: Option<String>,
    etag: Option<String>,
    version_id: Option<String>,
    last_attempt_at: Option<i64>,
    updated_at: Option<i64>,
    completed_at: Option<i64>,
}

#[derive(Serialize)]
struct ProviderHealthResponse {
    provider: String,
    connection_status: String,
    last_attempt_status: Option<String>,
    last_attempt_at: Option<i64>,
    last_success_at: Option<i64>,
    last_error: Option<String>,
}

#[derive(Serialize)]
pub(super) struct DiagnosticsHealthResponse {
    uptime_seconds: u64,
    pending_uploads_queue_size: i64,
    last_upload_error: Option<String>,
    cache_size_bytes: i64,
    cache_hit_count: u64,
    cache_miss_count: u64,
    worker_statuses: DiagnosticsWorkerStatusesResponse,
}

#[derive(Serialize)]
struct DiagnosticsWorkerStatusesResponse {
    uploader: String,
    repair: String,
    scrubber: String,
    gc: String,
    watcher: String,
    metadata_backup: String,
    peer: String,
    api: String,
    ingest: String,
}

#[derive(Serialize)]
struct StorageCostResponse {
    status: String,
    message: String,
    last_run: i64,
    logical_bytes: u64,
    physical_bytes: u64,
    physical_to_logical_ratio: f64,
    estimated_monthly_cost_usd: f64,
    estimated_provider_bytes_avoided: u64,
    estimated_paranoia_physical_bytes: u64,
    reconcile_backlog_packs: usize,
    orphaned_packs: i64,
    orphaned_physical_bytes: u64,
    gc_candidate_packs: i64,
    cloud_guard_status: String,
    cloud_guard_message: String,
    dry_run_active: bool,
    cloud_suspended: bool,
    cloud_suspend_reason: Option<String>,
    session_read_ops: i64,
    session_write_ops: i64,
    session_egress_bytes: i64,
    daily_read_ops: i64,
    daily_write_ops: i64,
    daily_egress_bytes: i64,
    daily_read_ops_limit: i64,
    daily_write_ops_limit: i64,
    daily_egress_bytes_limit: i64,
    read_quota_percent: f64,
    write_quota_percent: f64,
    egress_quota_percent: f64,
    providers: Vec<StorageCostProviderResponse>,
    storage_modes: Vec<StorageCostModeResponse>,
}

#[derive(Serialize)]
struct StorageCostProviderResponse {
    provider: String,
    used_physical_bytes: u64,
    usage_share_percent: f64,
    estimated_monthly_cost_usd: f64,
    configured_cost_per_gib_month: f64,
}

#[derive(Serialize)]
struct StorageCostModeResponse {
    storage_mode: String,
    active_packs: i64,
    logical_bytes: u64,
    physical_bytes: u64,
    estimated_paranoia_physical_bytes: u64,
    estimated_provider_bytes_avoided: u64,
    estimated_monthly_cost_usd: f64,
}

// ── Routes ──────────────────────────────────────────────────────────

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/transfers", get(get_transfers))
        .route("/api/health", get(get_health))
        .route("/api/diagnostics/health", get(get_diagnostics_health))
        .route("/api/diagnostics/shell", get(get_shell_state))
        .route("/api/diagnostics/sync-root", get(get_sync_root_state))
        .route("/api/storage/cost", get(get_storage_cost))
        .route("/api/multidevice/status", get(get_multidevice_status))
}

// ── Handlers ────────────────────────────────────────────────────────

async fn get_transfers(State(state): State<ApiState>) -> impl IntoResponse {
    match db::list_recent_upload_jobs(&state.pool, 50).await {
        Ok(jobs) => {
            let mut transfers = Vec::with_capacity(jobs.len());

            for job in jobs {
                let targets = match db::get_upload_targets_for_job(&state.pool, job.id).await {
                    Ok(targets) => targets,
                    Err(err) => return internal_server_error(err),
                };

                transfers.push(TransferResponse {
                    job_id: job.id,
                    pack_id: job.pack_id,
                    status: job.status,
                    attempts: job.attempts.unwrap_or(0),
                    providers: targets
                        .into_iter()
                        .map(|target| ProviderTransferResponse {
                            provider: target.provider,
                            status: target.status,
                            attempts: target.attempts.unwrap_or(0),
                            last_error: target.last_error,
                            bucket: target.bucket,
                            object_key: target.object_key,
                            etag: target.etag,
                            version_id: target.version_id,
                            last_attempt_at: target.last_attempt_at,
                            updated_at: target.updated_at,
                            completed_at: target.completed_at,
                        })
                        .collect(),
                });
            }

            (StatusCode::OK, Json(transfers)).into_response()
        }
        Err(err) => internal_server_error(err),
    }
}

async fn get_health(State(state): State<ApiState>) -> impl IntoResponse {
    let mut providers = Vec::with_capacity(KNOWN_PROVIDERS.len());
    let mut latest_by_provider = HashMap::with_capacity(KNOWN_PROVIDERS.len());

    for provider in KNOWN_PROVIDERS {
        match db::get_latest_upload_target_for_provider(&state.pool, provider).await {
            Ok(record) => {
                latest_by_provider.insert(provider, record);
            }
            Err(err) => return internal_server_error(err),
        }
    }

    for provider in KNOWN_PROVIDERS {
        let latest = latest_by_provider.remove(provider).flatten();
        let response = match latest {
            Some(target) => ProviderHealthResponse {
                provider: provider.to_string(),
                connection_status: provider_connection_status(
                    &target.status,
                    target.last_error.is_some(),
                ),
                last_attempt_status: Some(target.status),
                last_attempt_at: target.last_attempt_at.or(target.updated_at),
                last_success_at: target.completed_at,
                last_error: target.last_error,
            },
            None => ProviderHealthResponse {
                provider: provider.to_string(),
                connection_status: "UNKNOWN".to_string(),
                last_attempt_status: None,
                last_attempt_at: None,
                last_success_at: None,
                last_error: None,
            },
        };
        providers.push(response);
    }

    (StatusCode::OK, Json(providers)).into_response()
}

async fn get_diagnostics_health(State(state): State<ApiState>) -> impl IntoResponse {
    match build_diagnostics_health_response(&state).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn get_shell_state() -> impl IntoResponse {
    let response = build_shell_state_response();
    (StatusCode::OK, Json(response)).into_response()
}

async fn get_sync_root_state() -> impl IntoResponse {
    match build_sync_root_state_response() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "ERROR",
                "message": err.to_string(),
                "last_run": unix_timestamp_millis(),
            })),
        )
            .into_response(),
    }
}

async fn get_storage_cost(State(state): State<ApiState>) -> impl IntoResponse {
    match build_storage_cost_response(&state).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => internal_server_error(err),
    }
}

async fn get_multidevice_status(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(local_device) = (match db::get_local_device_identity(&state.pool).await {
        Ok(record) => record,
        Err(err) => return internal_server_error(err),
    }) else {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "WARN",
                "message": "Tożsamość lokalnego urządzenia nie została jeszcze zainicjalizowana.",
                "last_run": unix_timestamp_millis(),
                "trusted_peers": [],
                "recent_conflicts": [],
            })),
        )
            .into_response();
    };

    let vault_id = match db::get_vault_params(&state.pool).await {
        Ok(Some(record)) => record.vault_id,
        Ok(None) => "local-vault".to_string(),
        Err(err) => return internal_server_error(err),
    };
    let peer_port = AppConfig::from_env().peer_port;

    match peer::snapshot_multi_device(
        &state.pool,
        &crate::device_identity::LocalDeviceIdentity {
            device_id: local_device.device_id,
            device_name: local_device.device_name,
            peer_token: local_device.peer_token,
            created_at: local_device.created_at,
            updated_at: local_device.updated_at,
        },
        &vault_id,
        peer_port,
    )
    .await
    {
        Ok(snapshot) => (StatusCode::OK, Json(snapshot)).into_response(),
        Err(err) => internal_server_error(err),
    }
}

// ── Builder functions (pub(super) so mod.rs maintenance handlers can use them) ──

pub(super) async fn build_diagnostics_health_response(
    state: &ApiState,
) -> Result<MaintenanceStatus<DiagnosticsHealthResponse>, sqlx::Error> {
    let snapshot = state.diagnostics.snapshot();
    let pending_uploads_queue_size = db::get_pending_upload_queue_size(&state.pool).await?;
    let cache_summary = db::get_cache_status_summary(&state.pool).await?;
    let runtime_cache = cache::cache_runtime_stats();
    let last_upload_error = match &snapshot.last_upload_error {
        Some(value) => Some(value.clone()),
        None => db::get_latest_upload_error(&state.pool).await?,
    };

    let health = DiagnosticsHealthResponse {
        uptime_seconds: snapshot.uptime_seconds,
        pending_uploads_queue_size,
        last_upload_error: last_upload_error.clone(),
        cache_size_bytes: cache_summary.total_bytes,
        cache_hit_count: runtime_cache.hit_count,
        cache_miss_count: runtime_cache.miss_count,
        worker_statuses: DiagnosticsWorkerStatusesResponse {
            uploader: snapshot.uploader.as_str().to_string(),
            repair: snapshot.repair.as_str().to_string(),
            scrubber: snapshot.scrubber.as_str().to_string(),
            gc: snapshot.gc.as_str().to_string(),
            watcher: snapshot.watcher.as_str().to_string(),
            metadata_backup: snapshot.metadata_backup.as_str().to_string(),
            peer: snapshot.peer.as_str().to_string(),
            api: snapshot.api.as_str().to_string(),
            ingest: snapshot.ingest.as_str().to_string(),
        },
    };

    let (level, message) = if let Some(error) = last_upload_error {
        (
            MaintenanceLevel::Warn,
            format!(
                "Usługi w tle działają, ale ostatnia wysyłka zgłosiła błąd: {}",
                error
            ),
        )
    } else if pending_uploads_queue_size > 0 {
        (
            MaintenanceLevel::Warn,
            format!(
                "{} wysyłek nadal oczekuje na przetworzenie.",
                pending_uploads_queue_size
            ),
        )
    } else {
        (
            MaintenanceLevel::Ok,
            "Usługi w tle są zdrowe i brak zaległych wysyłek.".to_string(),
        )
    };

    Ok(MaintenanceStatus {
        status: level.as_str().to_string(),
        message,
        last_run: unix_timestamp_millis(),
        details: health,
    })
}

pub(super) fn build_shell_state_response() -> MaintenanceStatus<shell_state::ShellStateSnapshot> {
    let snapshot = shell_state::audit_shell_state();
    let message = if snapshot.is_healthy() {
        format!(
            "Dysk {} jest zamontowany, a integracja z Eksploratorem jest poprawna.",
            snapshot.preferred_drive_letter
        )
    } else if !snapshot.drive_present {
        format!(
            "Dysk {} jest niedostępny i wymaga naprawy.",
            snapshot.preferred_drive_letter
        )
    } else if !snapshot.drive_target_matches {
        format!(
            "Dysk {} wskazuje nieoczekiwany cel i wymaga naprawy.",
            snapshot.preferred_drive_letter
        )
    } else if !snapshot.drive_browsable {
        format!(
            "Dysk {} istnieje, ale nie jest przeglądalny w Eksploratorze.",
            snapshot.preferred_drive_letter
        )
    } else {
        "Wykryto drift integracji z Eksploratorem; można go naprawić.".to_string()
    };

    MaintenanceStatus {
        status: if snapshot.is_healthy() {
            MaintenanceLevel::Ok
        } else {
            MaintenanceLevel::Warn
        }
        .as_str()
        .to_string(),
        message,
        last_run: unix_timestamp_millis(),
        details: snapshot,
    }
}

pub(super) fn build_sync_root_state_response()
-> Result<MaintenanceStatus<smart_sync::SyncRootStateSnapshot>, smart_sync::SmartSyncError> {
    let runtime_paths = RuntimePaths::detect();
    let snapshot = smart_sync::audit_sync_root_state(&runtime_paths.sync_root)?;
    let shell_mode = shell_state::audit_shell_state().mode;

    let (level, message) = if shell_mode == "local_only" {
        (
            MaintenanceLevel::Ok,
            "Smart Sync jest celowo bezczynny do czasu skonfigurowania zdalnych dostawców.".to_string(),
        )
    } else if snapshot.registered && snapshot.connected && snapshot.registered_for_provider {
        (
            MaintenanceLevel::Ok,
            format!("Sync-root {} jest zarejestrowany i połączony.", snapshot.path),
        )
    } else if snapshot.path_exists {
        (
            MaintenanceLevel::Warn,
            format!(
                "Sync-root {} istnieje, ale rejestracja lub połączenie są niepełne.",
                snapshot.path
            ),
        )
    } else {
        (
            MaintenanceLevel::Error,
            format!(
                "Sync-root {} jest niedostępny i wymaga naprawy.",
                snapshot.path
            ),
        )
    };

    Ok(MaintenanceStatus {
        status: level.as_str().to_string(),
        message,
        last_run: unix_timestamp_millis(),
        details: snapshot,
    })
}

// ── Utility functions (private to diagnostics) ──────────────────────

async fn build_storage_cost_response(state: &ApiState) -> Result<StorageCostResponse, sqlx::Error> {
    let app_config = AppConfig::from_env();
    let guard_snapshot = cloud_guard::snapshot(&state.pool).await.ok();
    let mode_summaries = db::get_active_storage_mode_summaries(&state.pool).await?;
    let orphaned_summary = db::get_orphaned_pack_summary(&state.pool).await?;
    let active_packs = db::list_active_packs(&state.pool, 100_000).await?;

    let reconcile_backlog_packs = count_reconcile_backlog(&state.pool, &active_packs).await?;

    let mut logical_bytes = 0u64;
    let mut physical_bytes = 0u64;
    let mut estimated_paranoia_physical_bytes = 0u64;
    let mut estimated_provider_bytes_avoided = 0u64;
    let mut storage_modes = Vec::with_capacity(mode_summaries.len());

    for summary in mode_summaries {
        let summary_logical_bytes = u64::try_from(summary.logical_bytes).unwrap_or(0);
        let summary_physical_bytes = u64::try_from(summary.physical_bytes).unwrap_or(0);
        let total_shard_bytes = u64::try_from(summary.total_shard_bytes).unwrap_or(0);
        let estimated_paranoia_bytes = match summary.storage_mode.as_str() {
            "EC_2_1" => summary_physical_bytes,
            "SINGLE_REPLICA" | "LOCAL_ONLY" => total_shard_bytes.saturating_mul(3),
            _ => summary_physical_bytes,
        };
        let avoided_bytes = estimated_paranoia_bytes.saturating_sub(summary_physical_bytes);

        logical_bytes = logical_bytes.saturating_add(summary_logical_bytes);
        physical_bytes = physical_bytes.saturating_add(summary_physical_bytes);
        estimated_paranoia_physical_bytes =
            estimated_paranoia_physical_bytes.saturating_add(estimated_paranoia_bytes);
        estimated_provider_bytes_avoided =
            estimated_provider_bytes_avoided.saturating_add(avoided_bytes);

        storage_modes.push(StorageCostModeResponse {
            storage_mode: summary.storage_mode.clone(),
            active_packs: summary.active_packs,
            logical_bytes: summary_logical_bytes,
            physical_bytes: summary_physical_bytes,
            estimated_paranoia_physical_bytes: estimated_paranoia_bytes,
            estimated_provider_bytes_avoided: avoided_bytes,
            estimated_monthly_cost_usd: round_cost_estimate(
                bytes_to_gib(summary_physical_bytes)
                    * app_config.estimated_cost_per_gib_month_default,
            ),
        });
    }

    let mut providers = Vec::with_capacity(KNOWN_PROVIDERS.len());
    let mut estimated_monthly_cost_usd = 0.0f64;
    for provider in KNOWN_PROVIDERS {
        let used_physical_bytes =
            db::get_physical_usage_for_provider(&state.pool, provider).await?;
        let rate = provider_cost_rate(&app_config, provider);
        let provider_cost = round_cost_estimate(bytes_to_gib(used_physical_bytes) * rate);
        estimated_monthly_cost_usd += provider_cost;
        providers.push(StorageCostProviderResponse {
            provider: provider.to_string(),
            used_physical_bytes,
            usage_share_percent: percent_of(used_physical_bytes, physical_bytes),
            estimated_monthly_cost_usd: provider_cost,
            configured_cost_per_gib_month: rate,
        });
    }

    let message = if logical_bytes == 0 {
        "Brak aktywnych pakietow, dlatego dashboard storage pokazuje pusty slad Skarbca."
            .to_string()
    } else {
        format!(
            "Zajętość Skarbca: {:.2} GiB logicznie vs {:.2} GiB fizycznie, szacowany koszt zdalny ${:.2}/mies.",
            bytes_to_gib(logical_bytes),
            bytes_to_gib(physical_bytes),
            round_cost_estimate(estimated_monthly_cost_usd)
        )
    };

    Ok(StorageCostResponse {
        status: "OK".to_string(),
        message,
        last_run: unix_timestamp_millis(),
        logical_bytes,
        physical_bytes,
        physical_to_logical_ratio: ratio_of(physical_bytes, logical_bytes),
        estimated_monthly_cost_usd: round_cost_estimate(estimated_monthly_cost_usd),
        estimated_provider_bytes_avoided,
        estimated_paranoia_physical_bytes,
        reconcile_backlog_packs,
        orphaned_packs: orphaned_summary.pack_count,
        orphaned_physical_bytes: u64::try_from(orphaned_summary.physical_bytes).unwrap_or(0),
        gc_candidate_packs: orphaned_summary.pack_count,
        cloud_guard_status: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.status.clone())
            .unwrap_or_else(|| "WARN".to_string()),
        cloud_guard_message: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.message.clone())
            .unwrap_or_else(|| "Migawka Cloud Guard jest niedostępna.".to_string()),
        dry_run_active: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.dry_run_active)
            .unwrap_or(app_config.dry_run_active),
        cloud_suspended: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.cloud_suspended)
            .unwrap_or(false),
        cloud_suspend_reason: guard_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.cloud_suspend_reason.clone()),
        session_read_ops: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.session_read_ops)
            .unwrap_or(0),
        session_write_ops: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.session_write_ops)
            .unwrap_or(0),
        session_egress_bytes: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.session_egress_bytes)
            .unwrap_or(0),
        daily_read_ops: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.daily_read_ops)
            .unwrap_or(0),
        daily_write_ops: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.daily_write_ops)
            .unwrap_or(0),
        daily_egress_bytes: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.daily_egress_bytes)
            .unwrap_or(0),
        daily_read_ops_limit: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.daily_read_ops_limit)
            .unwrap_or(i64::try_from(app_config.cloud_daily_read_ops_limit).unwrap_or(i64::MAX)),
        daily_write_ops_limit: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.daily_write_ops_limit)
            .unwrap_or(i64::try_from(app_config.cloud_daily_write_ops_limit).unwrap_or(i64::MAX)),
        daily_egress_bytes_limit: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.daily_egress_bytes_limit)
            .unwrap_or(
                i64::try_from(app_config.cloud_daily_egress_bytes_limit).unwrap_or(i64::MAX),
            ),
        read_quota_percent: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.read_quota_percent)
            .unwrap_or(0.0),
        write_quota_percent: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.write_quota_percent)
            .unwrap_or(0.0),
        egress_quota_percent: guard_snapshot
            .as_ref()
            .map(|snapshot| snapshot.egress_quota_percent)
            .unwrap_or(0.0),
        providers,
        storage_modes,
    })
}

async fn count_reconcile_backlog(
    pool: &SqlitePool,
    active_packs: &[db::PackRecord],
) -> Result<usize, sqlx::Error> {
    let mut count = 0usize;
    for pack in active_packs {
        let desired = db::get_desired_storage_mode_for_pack(pool, &pack.pack_id).await?;
        if db::StorageMode::from_str(&pack.storage_mode) != desired {
            count += 1;
        }
    }
    Ok(count)
}

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

fn ratio_of(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn percent_of(value: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (value as f64 / total as f64) * 100.0
    }
}

fn round_cost_estimate(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn provider_cost_rate(config: &AppConfig, provider: &str) -> f64 {
    match provider {
        "cloudflare-r2" => config.estimated_cost_per_gib_month_r2,
        "backblaze-b2" => config.estimated_cost_per_gib_month_b2,
        "scaleway" => config.estimated_cost_per_gib_month_scaleway,
        _ => config.estimated_cost_per_gib_month_default,
    }
}

fn provider_connection_status(target_status: &str, has_error: bool) -> String {
    match target_status {
        "COMPLETED" => "HEALTHY".to_string(),
        "IN_PROGRESS" if has_error => "DEGRADED".to_string(),
        "IN_PROGRESS" => "ATTEMPTING".to_string(),
        "PENDING" | "FAILED" if has_error => "DEGRADED".to_string(),
        "PENDING" => "PENDING".to_string(),
        other => other.to_string(),
    }
}
