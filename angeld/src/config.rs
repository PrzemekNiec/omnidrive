use crate::runtime_paths::RuntimePaths;
use std::env;
use std::path::PathBuf;

pub const DEFAULT_MAX_PHYSICAL_BYTES_PER_PROVIDER: u64 = 80_530_636_800;
pub const DEFAULT_MAX_UPLOAD_BYTES_PER_SEC: u64 = 0;
pub const DEFAULT_CACHE_MAX_BYTES: u64 = 53_687_091_200;
pub const DEFAULT_ESTIMATED_COST_PER_GIB_MONTH: f64 = 0.01;
pub const DEFAULT_CLOUD_DAILY_WRITE_OPS_LIMIT: u64 = 1_000;
pub const DEFAULT_CLOUD_DAILY_READ_OPS_LIMIT: u64 = 5_000;
pub const DEFAULT_CLOUD_DAILY_EGRESS_BYTES_LIMIT: u64 = 500 * 1024 * 1024;
pub const DEFAULT_CLOUD_MAX_SINGLE_UPLOAD_BYTES: u64 = 100 * 1024 * 1024;
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
    pub cloud_daily_write_ops_limit: u64,
    pub cloud_daily_read_ops_limit: u64,
    pub cloud_daily_egress_bytes_limit: u64,
    pub cloud_max_single_upload_bytes: u64,
    pub dry_run_active: bool,
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
    // Google OAuth2 (Sesja C)
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    pub oauth_redirect_url: String,
    pub oauth_google_auth_url: String,
    pub oauth_google_token_url: String,
    pub oauth_google_userinfo_url: String,
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
            cloud_daily_write_ops_limit: env_u64(
                "OMNIDRIVE_CLOUD_DAILY_WRITE_OPS_LIMIT",
                DEFAULT_CLOUD_DAILY_WRITE_OPS_LIMIT,
            ),
            cloud_daily_read_ops_limit: env_u64(
                "OMNIDRIVE_CLOUD_DAILY_READ_OPS_LIMIT",
                DEFAULT_CLOUD_DAILY_READ_OPS_LIMIT,
            ),
            cloud_daily_egress_bytes_limit: env_u64(
                "OMNIDRIVE_CLOUD_DAILY_EGRESS_BYTES_LIMIT",
                DEFAULT_CLOUD_DAILY_EGRESS_BYTES_LIMIT,
            ),
            cloud_max_single_upload_bytes: env_u64(
                "OMNIDRIVE_CLOUD_MAX_SINGLE_UPLOAD_BYTES",
                DEFAULT_CLOUD_MAX_SINGLE_UPLOAD_BYTES,
            ),
            dry_run_active: env_flag("OMNIDRIVE_DRY_RUN"),
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
            google_client_id: env::var("GOOGLE_CLIENT_ID").ok().filter(|s| !s.is_empty()),
            google_client_secret: env::var("GOOGLE_CLIENT_SECRET").ok().filter(|s| !s.is_empty()),
            oauth_redirect_url: env::var("OAUTH_REDIRECT_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8787/api/auth/google/callback".to_string()),
            oauth_google_auth_url: env::var("OMNIDRIVE_OAUTH_GOOGLE_AUTH_URL")
                .unwrap_or_else(|_| "https://accounts.google.com/o/oauth2/v2/auth".to_string()),
            oauth_google_token_url: env::var("OMNIDRIVE_OAUTH_GOOGLE_TOKEN_URL")
                .unwrap_or_else(|_| "https://oauth2.googleapis.com/token".to_string()),
            oauth_google_userinfo_url: env::var("OMNIDRIVE_OAUTH_GOOGLE_USERINFO_URL")
                .unwrap_or_else(|_| "https://www.googleapis.com/oauth2/v3/userinfo".to_string()),
        }
    }

    pub fn provider_cost_per_gib_month(&self, provider: &str) -> f64 {
        match provider {
            "cloudflare-r2" => self.estimated_cost_per_gib_month_r2,
            "backblaze-b2" => self.estimated_cost_per_gib_month_b2,
            "scaleway" => self.estimated_cost_per_gib_month_scaleway,
            _ => self.estimated_cost_per_gib_month_default,
        }
    }

    /// Enforce RFC 8252 loopback for the OAuth redirect URL.
    ///
    /// OmniDrive is Local-First: the daemon must never register a public OAuth
    /// redirect, because the authorization code could then be delivered to an
    /// external host the user does not control. Accepted hosts: `127.0.0.1`,
    /// `localhost`, `[::1]` — all over plain HTTP (browsers allow loopback
    /// exempt from the https-only rule).
    pub fn validate_oauth_redirect_loopback_only(&self) -> Result<(), String> {
        const ALLOWED_PREFIXES: &[&str] = &[
            "http://127.0.0.1:",
            "http://localhost:",
            "http://[::1]:",
        ];
        if ALLOWED_PREFIXES
            .iter()
            .any(|p| self.oauth_redirect_url.starts_with(p))
        {
            Ok(())
        } else {
            Err(format!(
                "OAUTH_REDIRECT_URL must be a loopback URL (http://127.0.0.1:…, http://localhost:…, or http://[::1]:…), got {}",
                self.oauth_redirect_url
            ))
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

fn env_flag(key: &str) -> bool {
    env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_redirect(url: &str) -> AppConfig {
        let mut cfg = AppConfig::from_env();
        cfg.oauth_redirect_url = url.to_string();
        cfg
    }

    #[test]
    fn oauth_redirect_loopback_ipv4_is_accepted() {
        let cfg = cfg_with_redirect("http://127.0.0.1:8787/api/auth/google/callback");
        assert!(cfg.validate_oauth_redirect_loopback_only().is_ok());
    }

    #[test]
    fn oauth_redirect_localhost_is_accepted() {
        let cfg = cfg_with_redirect("http://localhost:8787/api/auth/google/callback");
        assert!(cfg.validate_oauth_redirect_loopback_only().is_ok());
    }

    #[test]
    fn oauth_redirect_loopback_ipv6_is_accepted() {
        let cfg = cfg_with_redirect("http://[::1]:8787/api/auth/google/callback");
        assert!(cfg.validate_oauth_redirect_loopback_only().is_ok());
    }

    #[test]
    fn oauth_redirect_public_https_is_rejected() {
        let cfg = cfg_with_redirect("https://skarbiec.app/api/auth/google/callback");
        assert!(cfg.validate_oauth_redirect_loopback_only().is_err());
    }

    #[test]
    fn oauth_redirect_arbitrary_http_is_rejected() {
        let cfg = cfg_with_redirect("http://example.com/oauth/callback");
        assert!(cfg.validate_oauth_redirect_loopback_only().is_err());
    }
}
