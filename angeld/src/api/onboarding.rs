// angeld/src/api/onboarding.rs — Onboarding API handlers (extracted from mod.rs)

use super::error::ApiError;
use super::{unix_timestamp_millis, ApiState, MaintenanceLevel, MaintenanceStatus};
use crate::config::AppConfig;
use crate::db;
use crate::device_identity::ensure_local_device_identity;
use crate::downloader::Downloader;
use crate::onboarding::{
    OnboardingMode, OnboardingState, ValidationReport, VaultRestoreReport,
    SYSTEM_CONFIG_CLOUD_ENABLED, SYSTEM_CONFIG_DRAFT_ENV_DETECTED,
    SYSTEM_CONFIG_LAST_ONBOARDING_STEP, SYSTEM_CONFIG_ONBOARDING_MODE,
    SYSTEM_CONFIG_ONBOARDING_STATE, cleanup_stale_uploads, perform_vault_restore,
    reset_onboarding, seal_provider_secrets, validate_persisted_provider_connection,
};
use crate::repair;
use crate::runtime_paths::RuntimePaths;
use crate::shell_state;
use crate::smart_sync;
use crate::uploader::KNOWN_PROVIDERS;
use crate::virtual_drive;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::env;
use std::sync::Arc;
use tracing::{error, info, warn};

// ── Route table ─────────────────────────────────────────────────────────

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/onboarding/status", get(get_onboarding_status))
        .route(
            "/api/onboarding/bootstrap-local",
            post(post_bootstrap_local),
        )
        .route("/api/onboarding/setup-identity", post(post_setup_identity))
        .route("/api/onboarding/setup-provider", post(post_setup_provider))
        .route("/api/onboarding/join-existing", post(post_join_existing))
        .route("/api/onboarding/complete", post(post_complete_onboarding))
        .route("/api/onboarding/reset", post(post_reset_onboarding))
}

// ── Request / Response types ────────────────────────────────────────────

#[derive(Serialize)]
pub(super) struct OnboardingProviderStatusResponse {
    provider_name: String,
    endpoint: String,
    region: String,
    bucket: String,
    force_path_style: bool,
    enabled: bool,
    draft_source: Option<String>,
    last_test_status: Option<String>,
    last_test_error: Option<String>,
    last_test_at: Option<i64>,
    access_key_status: String,
    secret_key_status: String,
}

#[derive(Serialize)]
pub(super) struct OnboardingStatusResponse {
    onboarding_state: String,
    onboarding_mode: String,
    current_step: String,
    draft_env_detected: bool,
    cloud_enabled: bool,
    device_name: Option<String>,
    device_id: Option<String>,
    providers: Vec<OnboardingProviderStatusResponse>,
}

#[derive(Deserialize)]
struct SetupIdentityRequest {
    device_name: String,
}

#[derive(Serialize)]
struct SetupIdentityResponse {
    device_id: String,
    device_name: String,
}

#[derive(Deserialize)]
struct SetupProviderRequest {
    provider_name: String,
    endpoint: String,
    region: String,
    bucket: String,
    force_path_style: Option<bool>,
    enabled: Option<bool>,
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
}

#[derive(Serialize)]
struct SetupProviderResponse {
    provider_name: String,
    enabled: bool,
    access_key_status: String,
    secret_key_status: String,
    validation: ValidationReport,
}

#[derive(Deserialize)]
struct JoinExistingRequest {
    passphrase: String,
    provider_id: String,
}

#[derive(Serialize)]
struct JoinExistingResponse {
    onboarding_state: String,
    onboarding_mode: String,
    cloud_enabled: bool,
    restore: VaultRestoreReport,
}

#[derive(Serialize)]
struct CompleteOnboardingResponse {
    onboarding_state: String,
    onboarding_mode: String,
    cloud_enabled: bool,
}

// ── Handlers ────────────────────────────────────────────────────────────

async fn get_onboarding_status(
    State(state): State<ApiState>,
) -> Result<Json<MaintenanceStatus<OnboardingStatusResponse>>, ApiError> {
    let response = build_onboarding_status_response(&state).await?;
    Ok(Json(response))
}

async fn post_bootstrap_local(
    State(state): State<ApiState>,
) -> Result<Json<MaintenanceStatus<OnboardingStatusResponse>>, ApiError> {
    shell_state::set_cloud_mode_hint(false);

    db::set_system_config_value(
        &state.pool,
        SYSTEM_CONFIG_ONBOARDING_STATE,
        OnboardingState::InProgress.as_str(),
    )
    .await?;
    db::set_system_config_value(
        &state.pool,
        SYSTEM_CONFIG_ONBOARDING_MODE,
        OnboardingMode::LocalOnly.as_str(),
    )
    .await?;
    db::set_system_config_value(&state.pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP, "identity")
        .await?;
    db::set_system_config_value(&state.pool, SYSTEM_CONFIG_CLOUD_ENABLED, "0").await?;

    match cleanup_stale_uploads(&state.pool).await {
        Ok(actions) => {
            for action in &actions {
                info!("[ONBOARDING] complete multipart cleanup: {}", action);
            }
        }
        Err(err) => {
            error!("[ONBOARDING] complete multipart cleanup failed: {}", err);
        }
    }

    let response = build_onboarding_status_response(&state).await?;
    Ok(Json(response))
}

async fn post_setup_identity(
    State(state): State<ApiState>,
    Json(request): Json<SetupIdentityRequest>,
) -> Result<Json<SetupIdentityResponse>, ApiError> {
    let device_name = request.device_name.trim();
    if device_name.is_empty() {
        return Err(ApiError::BadRequest {
            code: "invalid_device_name",
            message: "device_name cannot be empty".to_string(),
        });
    }

    let mut app_config = AppConfig::from_env();
    app_config.device_name_override = Some(device_name.to_string());
    let local_device =
        ensure_local_device_identity(&state.pool, &app_config)
            .await
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
            })?;
    db::update_local_device_name(&state.pool, device_name).await?;
    db::set_system_config_value(
        &state.pool,
        SYSTEM_CONFIG_ONBOARDING_STATE,
        OnboardingState::InProgress.as_str(),
    )
    .await?;
    db::set_system_config_value(&state.pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP, "providers")
        .await?;

    Ok(Json(SetupIdentityResponse {
        device_id: local_device.device_id,
        device_name: device_name.to_string(),
    }))
}

async fn post_setup_provider(
    State(state): State<ApiState>,
    Json(request): Json<SetupProviderRequest>,
) -> Result<Json<SetupProviderResponse>, ApiError> {
    let provider_name = request.provider_name.trim();
    if !KNOWN_PROVIDERS.contains(&provider_name) {
        return Err(ApiError::BadRequest {
            code: "unsupported_provider",
            message: format!("unsupported provider: {provider_name}"),
        });
    }

    if request.endpoint.trim().is_empty()
        || request.region.trim().is_empty()
        || request.bucket.trim().is_empty()
    {
        return Err(ApiError::BadRequest {
            code: "invalid_provider_config",
            message: "endpoint, region i bucket są wymagane".to_string(),
        });
    }

    db::upsert_provider_config(
        &state.pool,
        provider_name,
        request.endpoint.trim(),
        request.region.trim(),
        request.bucket.trim(),
        request.force_path_style.unwrap_or(false),
        request.enabled.unwrap_or(true),
        None,
        Some("CONFIG_SAVED"),
        None,
        None,
    )
    .await?;

    if let (Some(access_key_id), Some(secret_access_key)) = (
        request.access_key_id.as_deref(),
        request.secret_access_key.as_deref(),
    ) {
        let (sealed_access_key_id, sealed_secret_access_key) =
            seal_provider_secrets(access_key_id, secret_access_key).map_err(|e| {
                ApiError::Internal {
                    message: e.to_string(),
                }
            })?;
        db::upsert_provider_secret(
            &state.pool,
            provider_name,
            &sealed_access_key_id,
            &sealed_secret_access_key,
        )
        .await?;
    }

    db::set_system_config_value(
        &state.pool,
        SYSTEM_CONFIG_ONBOARDING_STATE,
        OnboardingState::InProgress.as_str(),
    )
    .await?;
    db::set_system_config_value(
        &state.pool,
        SYSTEM_CONFIG_ONBOARDING_MODE,
        OnboardingMode::CloudEnabled.as_str(),
    )
    .await?;
    db::set_system_config_value(&state.pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP, "security")
        .await?;

    let has_secret = db::get_provider_secret(&state.pool, provider_name)
        .await?
        .is_some();
    let validation = validate_persisted_provider_connection(&state.pool, provider_name).await;
    let (last_test_status, last_test_error, last_test_at, validation_report) = match validation {
        Ok(report) => (
            Some(report.status.clone()),
            None,
            Some(report.last_run),
            report,
        ),
        Err(err) => {
            let report = ValidationReport {
                status: "ERROR".to_string(),
                message: err.to_string(),
                last_run: unix_timestamp_millis(),
                provider_name: provider_name.to_string(),
                endpoint_reachable: !matches!(
                    &err,
                    crate::onboarding::ProviderError::EndpointUnreachable(_)
                ),
                authenticated: false,
                list_objects_ok: false,
                put_object_ok: false,
                delete_object_ok: false,
                error_kind: Some(provider_error_kind(&err).to_string()),
            };
            (
                Some(report.status.clone()),
                Some(report.message.clone()),
                Some(report.last_run),
                report,
            )
        }
    };
    db::upsert_provider_config(
        &state.pool,
        provider_name,
        request.endpoint.trim(),
        request.region.trim(),
        request.bucket.trim(),
        request.force_path_style.unwrap_or(false),
        request.enabled.unwrap_or(true),
        None,
        last_test_status.as_deref(),
        last_test_error.as_deref(),
        last_test_at,
    )
    .await?;

    if let Some(ref downloader) = state.downloader {
        if let Err(err) = downloader.reload_active_providers_from_db().await {
            tracing::warn!("[PROVIDER] hot-reload after setup-provider failed: {err}");
        } else {
            tracing::info!("[PROVIDER] hot-reloaded providers after setup-provider for {provider_name}");
        }
    }

    Ok(Json(SetupProviderResponse {
        provider_name: provider_name.to_string(),
        enabled: request.enabled.unwrap_or(true),
        access_key_status: if has_secret {
            "SET".to_string()
        } else {
            "MISSING".to_string()
        },
        secret_key_status: if has_secret {
            "SET".to_string()
        } else {
            "MISSING".to_string()
        },
        validation: validation_report,
    }))
}

async fn post_complete_onboarding(
    State(state): State<ApiState>,
) -> Result<Json<CompleteOnboardingResponse>, ApiError> {
    let active_provider_configs = crate::onboarding::get_active_provider_configs(&state.pool)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?;
    let cloud_enabled = !active_provider_configs.is_empty();
    shell_state::set_cloud_mode_hint(cloud_enabled);
    let onboarding_mode = if cloud_enabled {
        OnboardingMode::CloudEnabled
    } else {
        OnboardingMode::LocalOnly
    };

    db::set_system_config_value(
        &state.pool,
        SYSTEM_CONFIG_ONBOARDING_STATE,
        OnboardingState::Completed.as_str(),
    )
    .await?;
    db::set_system_config_value(
        &state.pool,
        SYSTEM_CONFIG_ONBOARDING_MODE,
        onboarding_mode.as_str(),
    )
    .await?;
    db::set_system_config_value(&state.pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP, "completed")
        .await?;
    db::set_system_config_value(
        &state.pool,
        SYSTEM_CONFIG_CLOUD_ENABLED,
        if cloud_enabled { "1" } else { "0" },
    )
    .await?;

    trigger_runtime_provider_reload(&state, cloud_enabled).await;

    Ok(Json(CompleteOnboardingResponse {
        onboarding_state: OnboardingState::Completed.as_str().to_string(),
        onboarding_mode: onboarding_mode.as_str().to_string(),
        cloud_enabled,
    }))
}

async fn post_join_existing(
    State(state): State<ApiState>,
    Json(request): Json<JoinExistingRequest>,
) -> Result<Json<JoinExistingResponse>, ApiError> {
    let provider_id = request.provider_id.trim();
    if !KNOWN_PROVIDERS.contains(&provider_id) {
        return Err(ApiError::BadRequest {
            code: "unsupported_provider",
            message: "Wybierz jednego ze skonfigurowanych dostawców OmniDrive przed dołączeniem do istniejącego Skarbca.".to_string(),
        });
    }

    let runtime_paths = RuntimePaths::detect();
    let restore = perform_vault_restore(
        &state.pool,
        &runtime_paths,
        &request.passphrase,
        provider_id,
    )
    .await
    .map_err(|err| {
        let (code, message) = (provider_restore_error_kind(&err), err.to_string());
        match err {
            crate::onboarding::RestoreError::IncorrectPassphrase(_) => ApiError::BadRequest {
                code,
                message,
            },
            crate::onboarding::RestoreError::MetadataNotFound(_) => ApiError::NotFound {
                resource: "metadata",
                id: provider_id.to_string(),
            },
            crate::onboarding::RestoreError::NetworkError(_) => ApiError::BadGateway { message },
            crate::onboarding::RestoreError::MissingProviderConfig(_) => ApiError::BadRequest {
                code,
                message,
            },
            crate::onboarding::RestoreError::SnapshotApply(_)
            | crate::onboarding::RestoreError::Runtime(_)
            | crate::onboarding::RestoreError::Io(_) => ApiError::Internal { message },
        }
    })?;

    finalize_join_existing_runtime(&state, &runtime_paths, provider_id)
        .await
        .map_err(|err| ApiError::Internal {
            message: format!(
                "Metadane Skarbca zostały odtworzone, ale OmniDrive nie mógł poprawnie przełączyć tego urządzenia do trybu sync-root: {}",
                err
            ),
        })?;

    shell_state::set_cloud_mode_hint(true);
    db::set_system_config_value(
        &state.pool,
        SYSTEM_CONFIG_ONBOARDING_STATE,
        OnboardingState::Completed.as_str(),
    )
    .await?;
    db::set_system_config_value(
        &state.pool,
        SYSTEM_CONFIG_ONBOARDING_MODE,
        OnboardingMode::JoinExisting.as_str(),
    )
    .await?;
    db::set_system_config_value(&state.pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP, "completed")
        .await?;
    db::set_system_config_value(&state.pool, SYSTEM_CONFIG_CLOUD_ENABLED, "1").await?;

    trigger_runtime_provider_reload(&state, true).await;

    Ok(Json(JoinExistingResponse {
        onboarding_state: OnboardingState::Completed.as_str().to_string(),
        onboarding_mode: OnboardingMode::JoinExisting.as_str().to_string(),
        cloud_enabled: true,
        restore,
    }))
}

async fn post_reset_onboarding(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    reset_onboarding(&state.pool).await?;
    shell_state::set_cloud_mode_hint(false);
    Ok(Json(serde_json::json!({
        "status": "OK",
        "message": "Onboarding state has been reset. Reload the dashboard to see the wizard."
    })))
}

// ── Helpers ─────────────────────────────────────────────────────────────

async fn trigger_runtime_provider_reload(state: &ApiState, trigger_reconcile: bool) {
    if let Some(tx) = &state.runtime_reload_tx {
        let next_generation = tx.borrow().wrapping_add(1);
        if tx.send(next_generation).is_ok() {
            info!("[RUNTIME] provider reload signal dispatched (generation={next_generation})");
        } else {
            warn!("[RUNTIME] provider reload signal dispatch failed: no active receivers");
        }
    }

    if let Some(downloader) = state.downloader.as_ref() {
        match downloader.reload_active_providers_from_db().await {
            Ok(provider_names) => {
                info!(
                    "[RUNTIME] downloader providers reloaded from DB: [{}]",
                    if provider_names.is_empty() {
                        "none".to_string()
                    } else {
                        provider_names.join(", ")
                    }
                );
            }
            Err(err) => {
                warn!("[RUNTIME] downloader provider reload failed: {err}");
            }
        }
    }

    if trigger_reconcile {
        match repair::RepairWorker::run_reconcile_batch_now(state.pool.clone()).await {
            Ok(report) => {
                info!(
                    "[RUNTIME] post-onboarding reconciliation completed (reconciled_packs={})",
                    report.reconciled_packs
                );
            }
            Err(err) => {
                warn!("[RUNTIME] post-onboarding reconciliation warning: {err}");
            }
        }
    }
}

async fn finalize_join_existing_runtime(
    state: &ApiState,
    runtime_paths: &RuntimePaths,
    provider_id: &str,
) -> Result<(), std::io::Error> {
    fn io_error(e: impl std::fmt::Display) -> std::io::Error {
        std::io::Error::other(e.to_string())
    }

    let downloader = match state.downloader.clone() {
        Some(existing) => {
            if let Err(err) = existing.reload_active_providers_from_db().await {
                warn!("[RESTORE] downloader runtime reload warning: {err}");
            }
            existing
        }
        None => {
            let provider_config = crate::onboarding::load_provider_config_from_onboarding_db(
                &state.pool,
                provider_id,
            )
            .await
            .map_err(&io_error)?;
            Arc::new(
                Downloader::from_provider_configs(
                    state.pool.clone(),
                    state.vault_keys.clone(),
                    runtime_paths.download_spool_dir.clone(),
                    std::time::Duration::from_millis(120_000),
                    vec![provider_config],
                )
                .await
                .map_err(&io_error)?,
            )
        }
    };

    smart_sync::install_hydration_runtime(state.pool.clone(), downloader).map_err(&io_error)?;

    let repair_report = smart_sync::repair_sync_root(&state.pool, &runtime_paths.sync_root)
        .await
        .map_err(&io_error)?;
    for action in &repair_report.actions {
        info!("[RESTORE] sync-root recovery: {}", action);
    }

    smart_sync::project_vault_to_sync_root(&state.pool, &runtime_paths.sync_root)
        .await
        .map_err(&io_error)?;
    info!(
        "[RESTORE] Placeholder hydration projected into {}",
        runtime_paths.sync_root.display()
    );

    virtual_drive::hide_sync_root(&runtime_paths.sync_root).map_err(&io_error)?;
    let preferred_drive_letter =
        env::var("OMNIDRIVE_DRIVE_LETTER").unwrap_or_else(|_| "O:".to_string());
    let _ = virtual_drive::unmount_virtual_drive(&preferred_drive_letter);
    let drive_letter = virtual_drive::select_mount_drive_letter(&preferred_drive_letter)
        .unwrap_or(preferred_drive_letter.clone());
    virtual_drive::mount_virtual_drive(&drive_letter, &runtime_paths.sync_root)
        .map_err(&io_error)?;

    match shell_state::repair_explorer_integration() {
        Ok(report) => {
            for action in &report.actions {
                info!("[RESTORE] shell repair: {}", action);
            }
        }
        Err(err) => {
            error!("[RESTORE] shell repair warning: {}", err);
        }
    }

    Ok(())
}

pub(super) async fn build_onboarding_status_response(
    state: &ApiState,
) -> Result<MaintenanceStatus<OnboardingStatusResponse>, sqlx::Error> {
    let onboarding_state = db::get_system_config_value(&state.pool, SYSTEM_CONFIG_ONBOARDING_STATE)
        .await?
        .unwrap_or_else(|| OnboardingState::Initial.as_str().to_string());
    let onboarding_mode = db::get_system_config_value(&state.pool, SYSTEM_CONFIG_ONBOARDING_MODE)
        .await?
        .unwrap_or_else(|| OnboardingMode::LocalOnly.as_str().to_string());
    let current_step = db::get_system_config_value(&state.pool, SYSTEM_CONFIG_LAST_ONBOARDING_STEP)
        .await?
        .unwrap_or_else(|| "welcome".to_string());
    let draft_env_detected =
        db::get_system_config_value(&state.pool, SYSTEM_CONFIG_DRAFT_ENV_DETECTED)
            .await?
            .is_some_and(|value| value == "1");
    let cloud_enabled = db::get_system_config_value(&state.pool, SYSTEM_CONFIG_CLOUD_ENABLED)
        .await?
        .is_some_and(|value| value == "1");
    let local_device = db::get_local_device_identity(&state.pool).await?;
    let providers = db::list_provider_configs(&state.pool).await?;

    let mut provider_statuses = Vec::with_capacity(providers.len());
    for provider in providers {
        let has_secret = db::get_provider_secret(&state.pool, &provider.provider_name)
            .await?
            .is_some();
        provider_statuses.push(OnboardingProviderStatusResponse {
            provider_name: provider.provider_name,
            endpoint: provider.endpoint,
            region: provider.region,
            bucket: provider.bucket,
            force_path_style: provider.force_path_style != 0,
            enabled: provider.enabled != 0,
            draft_source: provider.draft_source,
            last_test_status: provider.last_test_status,
            last_test_error: provider.last_test_error,
            last_test_at: provider.last_test_at,
            access_key_status: if has_secret {
                "SET".to_string()
            } else {
                "MISSING".to_string()
            },
            secret_key_status: if has_secret {
                "SET".to_string()
            } else {
                "MISSING".to_string()
            },
        });
    }

    let level = if onboarding_state == OnboardingState::Completed.as_str() {
        MaintenanceLevel::Ok
    } else {
        MaintenanceLevel::Warn
    };

    let message = if onboarding_state == OnboardingState::Completed.as_str() {
        "Onboarding zakończony.".to_string()
    } else if draft_env_detected {
        "Onboarding nie jest zakończony; szkice dostawców zaimportowano z .env do przeglądu.".to_string()
    } else {
        "Onboarding nie został jeszcze zakończony.".to_string()
    };

    Ok(MaintenanceStatus {
        status: level.as_str().to_string(),
        message,
        last_run: unix_timestamp_millis(),
        details: OnboardingStatusResponse {
            onboarding_state,
            onboarding_mode,
            current_step,
            draft_env_detected,
            cloud_enabled,
            device_name: local_device
                .as_ref()
                .map(|device| device.device_name.clone()),
            device_id: local_device.as_ref().map(|device| device.device_id.clone()),
            providers: provider_statuses,
        },
    })
}

fn provider_error_kind(err: &crate::onboarding::ProviderError) -> &'static str {
    match err {
        crate::onboarding::ProviderError::InvalidCredentials(_) => "InvalidCredentials",
        crate::onboarding::ProviderError::BucketNotFound(_) => "BucketNotFound",
        crate::onboarding::ProviderError::AccessDenied(_) => "AccessDenied",
        crate::onboarding::ProviderError::EndpointUnreachable(_) => "EndpointUnreachable",
        crate::onboarding::ProviderError::ClockSkew(_) => "ClockSkew",
        crate::onboarding::ProviderError::MissingProviderConfig(_) => "MissingProviderConfig",
        crate::onboarding::ProviderError::MissingSecrets(_) => "MissingSecrets",
        crate::onboarding::ProviderError::Url(_) => "InvalidEndpoint",
        crate::onboarding::ProviderError::Io(_) => "Io",
        crate::onboarding::ProviderError::Aws(_) => "Aws",
    }
}

fn provider_restore_error_kind(err: &crate::onboarding::RestoreError) -> &'static str {
    match err {
        crate::onboarding::RestoreError::MissingProviderConfig(_) => "MissingProviderConfig",
        crate::onboarding::RestoreError::IncorrectPassphrase(_) => "IncorrectPassphrase",
        crate::onboarding::RestoreError::MetadataNotFound(_) => "MetadataNotFound",
        crate::onboarding::RestoreError::NetworkError(_) => "NetworkError",
        crate::onboarding::RestoreError::SnapshotApply(_) => "SnapshotApply",
        crate::onboarding::RestoreError::Runtime(_) => "Runtime",
        crate::onboarding::RestoreError::Io(_) => "Io",
    }
}
