use crate::config::AppConfig;
use crate::db;
use crate::uploader::KNOWN_PROVIDERS;

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use sysinfo::System;
use tokio::sync::Mutex;

use super::error::ApiError;
use super::ApiState;

static SYSINFO: OnceLock<Mutex<System>> = OnceLock::new();

fn sysinfo() -> &'static Mutex<System> {
    SYSINFO.get_or_init(|| {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        Mutex::new(sys)
    })
}

// ── G.1: /api/stats/overview ───────────────────────────────────────

#[derive(Serialize)]
struct StatsOverviewResponse {
    files_count: i64,
    logical_size_bytes: i64,
    monthly_cost_usd: f64,
    devices_count: i64,
}

async fn get_stats_overview(
    State(state): State<ApiState>,
) -> Result<Json<StatsOverviewResponse>, ApiError> {
    let overview = db::get_stats_overview(&state.pool).await?;
    let devices_count = db::count_active_devices(&state.pool).await?;

    // Estimate monthly cost from physical usage across all providers
    let app_config = AppConfig::from_env();
    let mut total_cost = 0.0f64;
    for provider in KNOWN_PROVIDERS {
        let used = db::get_physical_usage_for_provider(&state.pool, provider).await?;
        let rate = app_config.provider_cost_per_gib_month(provider);
        total_cost += bytes_to_gib(used) * rate;
    }

    Ok(Json(StatsOverviewResponse {
        files_count: overview.files_count,
        logical_size_bytes: overview.logical_size_bytes,
        monthly_cost_usd: round2(total_cost),
        devices_count,
    }))
}

// ── G.2: /api/stats/traffic ────────────────────────────────────────

#[derive(Deserialize)]
struct TrafficQuery {
    hours: Option<u32>,
}

#[derive(Serialize)]
struct TrafficResponse {
    buckets: Vec<db::TrafficBucket>,
}

async fn get_stats_traffic(
    State(state): State<ApiState>,
    Query(query): Query<TrafficQuery>,
) -> Result<Json<TrafficResponse>, ApiError> {
    let hours = query.hours.unwrap_or(24).min(168); // max 7 days
    let buckets = db::get_traffic_buckets(&state.pool, hours).await?;
    Ok(Json(TrafficResponse { buckets }))
}

// ── G.3: /api/stats/system ─────────────────────────────────────────

#[derive(Serialize)]
struct StatsSystemResponse {
    nodes_count: i64,
    nodes_delta: i64,
    cpu_percent: f64,
    latency_ms: f64,
    latency_delta_ms: f64,
    integrity_percent: f64,
}

async fn get_stats_system(
    State(state): State<ApiState>,
) -> Result<Json<StatsSystemResponse>, ApiError> {
    // Nodes: count trusted peers (non-stale)
    let peers = db::list_trusted_peers(&state.pool).await?;
    let nodes_count = peers.len() as i64 + 1; // +1 for self

    // Integrity: derive from vault health
    let vault = db::get_vault_health_summary(&state.pool).await?;
    let integrity_percent = if vault.total_packs > 0 {
        round2((vault.healthy_packs as f64 / vault.total_packs as f64) * 100.0)
    } else {
        100.0
    };

    let cpu_percent = {
        let mut sys = sysinfo().lock().await;
        sys.refresh_cpu_usage();
        f64::from(sys.global_cpu_info().cpu_usage())
    };

    Ok(Json(StatsSystemResponse {
        nodes_count,
        nodes_delta: 0,
        cpu_percent,
        latency_ms: 0.0,
        latency_delta_ms: 0.0,
        integrity_percent,
    }))
}

// ── Routes ─────────────────────────────────────────────────────────

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/stats/overview", get(get_stats_overview))
        .route("/api/stats/traffic", get(get_stats_traffic))
        .route("/api/stats/system", get(get_stats_system))
}

// ── Helpers ────────────────────────────────────────────────────────

fn bytes_to_gib(b: u64) -> f64 {
    b as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
