use std::env;
use std::path::PathBuf;
use crate::runtime_paths::RuntimePaths;

pub const DEFAULT_MAX_PHYSICAL_BYTES_PER_PROVIDER: u64 = 80_530_636_800;
pub const DEFAULT_MAX_UPLOAD_BYTES_PER_SEC: u64 = 0;
pub const DEFAULT_CACHE_MAX_BYTES: u64 = 53_687_091_200;
pub const DEFAULT_ESTIMATED_COST_PER_GIB_MONTH: f64 = 0.01;
pub const DEFAULT_PEER_PORT: u16 = 8788;
pub const DEFAULT_PEER_DISCOVERY_PORT: u16 = 8789;
pub const DEFAULT_PEER_DISCOVERY_INTERVAL_MS: u64 = 5_000;
pub const DEFAULT_PEER_STALE_AFTER_MS: u64 = 60_000;
pub const DEFAULT_PEER_ERROR_BACKOFF_MS: u64 = 15_000;

#[derive(Clone, Debug, PartialEq)]
pub struct AppConfig {
    pub max_physical_bytes_per_provider: u64,
    pub max_upload_bytes_per_sec: u64,
    pub max_cache_bytes: u64,
    pub default_watch_dir: Option<PathBuf>,
    pub estimated_cost_per_gib_month_default: f64,
    pub estimated_cost_per_gib_month_r2: f64,
    pub estimated_cost_per_gib_month_b2: f64,
    pub estimated_cost_per_gib_month_scaleway: f64,
    pub peer_port: u16,
    pub peer_discovery_port: u16,
    pub peer_discovery_interval_ms: u64,
    pub peer_stale_after_ms: u64,
    pub peer_error_backoff_ms: u64,
    pub device_name_override: Option<String>,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            max_physical_bytes_per_provider: env_u64(
                "OMNIDRIVE_MAX_PHYSICAL_BYTES_PER_PROVIDER",
                DEFAULT_MAX_PHYSICAL_BYTES_PER_PROVIDER,
            ),
            max_upload_bytes_per_sec: env_u64(
                "OMNIDRIVE_MAX_UPLOAD_BYTES_PER_SEC",
                DEFAULT_MAX_UPLOAD_BYTES_PER_SEC,
            ),
            max_cache_bytes: env_u64("OMNIDRIVE_CACHE_MAX_BYTES", DEFAULT_CACHE_MAX_BYTES),
            estimated_cost_per_gib_month_default: env_f64(
                "OMNIDRIVE_ESTIMATED_COST_PER_GIB_MONTH",
                DEFAULT_ESTIMATED_COST_PER_GIB_MONTH,
            ),
            estimated_cost_per_gib_month_r2: env_f64(
                "OMNIDRIVE_R2_COST_PER_GIB_MONTH",
                env_f64(
                    "OMNIDRIVE_ESTIMATED_COST_PER_GIB_MONTH",
                    DEFAULT_ESTIMATED_COST_PER_GIB_MONTH,
                ),
            ),
            estimated_cost_per_gib_month_b2: env_f64(
                "OMNIDRIVE_B2_COST_PER_GIB_MONTH",
                env_f64(
                    "OMNIDRIVE_ESTIMATED_COST_PER_GIB_MONTH",
                    DEFAULT_ESTIMATED_COST_PER_GIB_MONTH,
                ),
            ),
            estimated_cost_per_gib_month_scaleway: env_f64(
                "OMNIDRIVE_SCALEWAY_COST_PER_GIB_MONTH",
                env_f64(
                    "OMNIDRIVE_ESTIMATED_COST_PER_GIB_MONTH",
                    DEFAULT_ESTIMATED_COST_PER_GIB_MONTH,
                ),
            ),
            default_watch_dir: env::var("OMNIDRIVE_WATCH_DIR")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(PathBuf::from)
                .or_else(|| RuntimePaths::detect().default_watch_dir),
            peer_port: env_u16("OMNIDRIVE_PEER_PORT", DEFAULT_PEER_PORT),
            peer_discovery_port: env_u16(
                "OMNIDRIVE_PEER_DISCOVERY_PORT",
                DEFAULT_PEER_DISCOVERY_PORT,
            ),
            peer_discovery_interval_ms: env_u64(
                "OMNIDRIVE_PEER_DISCOVERY_INTERVAL_MS",
                DEFAULT_PEER_DISCOVERY_INTERVAL_MS,
            ),
            peer_stale_after_ms: env_u64(
                "OMNIDRIVE_PEER_STALE_AFTER_MS",
                DEFAULT_PEER_STALE_AFTER_MS,
            ),
            peer_error_backoff_ms: env_u64(
                "OMNIDRIVE_PEER_ERROR_BACKOFF_MS",
                DEFAULT_PEER_ERROR_BACKOFF_MS,
            ),
            device_name_override: env::var("OMNIDRIVE_DEVICE_NAME")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        }
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_f64(key: &str, default: f64) -> f64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(default)
}

fn env_u16(key: &str, default: u16) -> u16 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(default)
}
