use std::env;
use std::path::PathBuf;

pub const DEFAULT_MAX_PHYSICAL_BYTES_PER_PROVIDER: u64 = 80_530_636_800;
pub const DEFAULT_MAX_UPLOAD_BYTES_PER_SEC: u64 = 0;
pub const DEFAULT_CACHE_MAX_BYTES: u64 = 53_687_091_200;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppConfig {
    pub max_physical_bytes_per_provider: u64,
    pub max_upload_bytes_per_sec: u64,
    pub max_cache_bytes: u64,
    pub default_watch_dir: Option<PathBuf>,
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
            default_watch_dir: env::var("OMNIDRIVE_WATCH_DIR")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(PathBuf::from),
        }
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}
