use crate::config::AppConfig;
use crate::db;
use serde::Serialize;
use sqlx::SqlitePool;
use std::fmt;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SYSTEM_CONFIG_CLOUD_SUSPENDED: &str = "cloud_suspended";
pub const SYSTEM_CONFIG_CLOUD_SUSPEND_REASON: &str = "cloud_suspend_reason";
pub const SYSTEM_CONFIG_DRY_RUN_ACTIVE: &str = "dry_run_active";

#[derive(Clone, Copy, Debug)]
pub enum GuardOperation {
    Read {
        count: i64,
        estimated_egress_bytes: i64,
    },
    Write {
        count: i64,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct CloudGuardSnapshot {
    pub status: String,
    pub message: String,
    pub dry_run_active: bool,
    pub cloud_suspended: bool,
    pub cloud_suspend_reason: Option<String>,
    pub day_epoch: i64,
    pub session_read_ops: i64,
    pub session_write_ops: i64,
    pub session_egress_bytes: i64,
    pub daily_read_ops: i64,
    pub daily_write_ops: i64,
    pub daily_egress_bytes: i64,
    pub daily_read_ops_limit: i64,
    pub daily_write_ops_limit: i64,
    pub daily_egress_bytes_limit: i64,
    pub read_quota_percent: f64,
    pub write_quota_percent: f64,
    pub egress_quota_percent: f64,
}

#[derive(Debug)]
pub enum GuardDecision {
    Allowed,
    DryRun { message: String },
    Suspended { reason: String },
    QuotaExceeded { reason: String },
}

#[derive(Debug)]
pub enum CloudGuardError {
    Db(sqlx::Error),
}

impl fmt::Display for CloudGuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Db(err) => write!(f, "cloud guard sqlite error: {err}"),
        }
    }
}

impl std::error::Error for CloudGuardError {}

impl From<sqlx::Error> for CloudGuardError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

#[derive(Default)]
struct SessionUsage {
    read_ops: i64,
    write_ops: i64,
    egress_bytes: i64,
}

static SESSION_USAGE: OnceLock<Mutex<SessionUsage>> = OnceLock::new();

fn session_usage() -> &'static Mutex<SessionUsage> {
    SESSION_USAGE.get_or_init(|| Mutex::new(SessionUsage::default()))
}

pub async fn sync_runtime_flags(
    pool: &SqlitePool,
    dry_run_active: bool,
) -> Result<(), CloudGuardError> {
    db::set_system_config_value(
        pool,
        SYSTEM_CONFIG_DRY_RUN_ACTIVE,
        if dry_run_active { "1" } else { "0" },
    )
    .await?;
    if !dry_run_active {
        clear_cloud_suspension(pool).await?;
    }
    Ok(())
}

pub async fn clear_cloud_suspension(pool: &SqlitePool) -> Result<(), CloudGuardError> {
    db::set_system_config_value(pool, SYSTEM_CONFIG_CLOUD_SUSPENDED, "0").await?;
    db::set_system_config_value(pool, SYSTEM_CONFIG_CLOUD_SUSPEND_REASON, "").await?;
    Ok(())
}

pub async fn set_cloud_suspension(pool: &SqlitePool, reason: &str) -> Result<(), CloudGuardError> {
    db::set_system_config_value(pool, SYSTEM_CONFIG_CLOUD_SUSPENDED, "1").await?;
    db::set_system_config_value(pool, SYSTEM_CONFIG_CLOUD_SUSPEND_REASON, reason).await?;
    Ok(())
}

pub async fn is_cloud_suspended(pool: &SqlitePool) -> Result<bool, CloudGuardError> {
    Ok(
        db::get_system_config_value(pool, SYSTEM_CONFIG_CLOUD_SUSPENDED)
            .await?
            .is_some_and(|v| v == "1"),
    )
}

pub async fn current_decision(
    pool: &SqlitePool,
    operation: GuardOperation,
) -> Result<GuardDecision, CloudGuardError> {
    let config = AppConfig::from_env();
    let dry_run_active = config.dry_run_active
        || db::get_system_config_value(pool, SYSTEM_CONFIG_DRY_RUN_ACTIVE)
            .await?
            .is_some_and(|value| value == "1");
    if dry_run_active {
        return Ok(GuardDecision::DryRun {
            message: "DRY-RUN MODE ACTIVE - NO CLOUD COSTS".to_string(),
        });
    }
    if is_cloud_suspended(pool).await? {
        let reason = db::get_system_config_value(pool, SYSTEM_CONFIG_CLOUD_SUSPEND_REASON)
            .await?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "cloud operations are suspended by circuit breaker".to_string());
        return Ok(GuardDecision::Suspended { reason });
    }

    let day_epoch = current_day_epoch();
    let delta = match operation {
        GuardOperation::Read {
            count,
            estimated_egress_bytes,
        } => db::CloudUsageDelta {
            read_ops: count.max(0),
            write_ops: 0,
            egress_bytes: estimated_egress_bytes.max(0),
        },
        GuardOperation::Write { count } => db::CloudUsageDelta {
            read_ops: 0,
            write_ops: count.max(0),
            egress_bytes: 0,
        },
    };

    let limits = (
        i64::try_from(config.cloud_daily_read_ops_limit).unwrap_or(i64::MAX),
        i64::try_from(config.cloud_daily_write_ops_limit).unwrap_or(i64::MAX),
        i64::try_from(config.cloud_daily_egress_bytes_limit).unwrap_or(i64::MAX),
    );
    let result = db::apply_cloud_usage_delta_with_limits(
        pool, day_epoch, delta, limits.0, limits.1, limits.2,
    )
    .await?;

    if !result.allowed {
        let reason = format!(
            "cloud circuit breaker tripped (daily read_ops={}/{} write_ops={}/{} egress_bytes={}/{})",
            result.read_ops, limits.0, result.write_ops, limits.1, result.egress_bytes, limits.2
        );
        set_cloud_suspension(pool, &reason).await?;
        return Ok(GuardDecision::QuotaExceeded { reason });
    }

    {
        let mut usage = session_usage()
            .lock()
            .expect("session usage mutex poisoned");
        usage.read_ops = usage.read_ops.saturating_add(delta.read_ops);
        usage.write_ops = usage.write_ops.saturating_add(delta.write_ops);
        usage.egress_bytes = usage.egress_bytes.saturating_add(delta.egress_bytes);
    }

    Ok(GuardDecision::Allowed)
}

pub fn enforce_single_upload_size_limit(file_size: u64) -> Result<(), String> {
    let config = AppConfig::from_env();
    if file_size > config.cloud_max_single_upload_bytes {
        return Err(format!(
            "file size guard exceeded: {} bytes is above the configured limit {} bytes",
            file_size, config.cloud_max_single_upload_bytes
        ));
    }
    Ok(())
}

pub async fn snapshot(pool: &SqlitePool) -> Result<CloudGuardSnapshot, CloudGuardError> {
    let config = AppConfig::from_env();
    let day_epoch = current_day_epoch();
    let daily = db::get_cloud_usage_for_day(pool, day_epoch).await?;
    let suspended = db::get_system_config_value(pool, SYSTEM_CONFIG_CLOUD_SUSPENDED)
        .await?
        .is_some_and(|v| v == "1");
    let suspend_reason = db::get_system_config_value(pool, SYSTEM_CONFIG_CLOUD_SUSPEND_REASON)
        .await?
        .filter(|value| !value.trim().is_empty());
    let dry_run_active = config.dry_run_active
        || db::get_system_config_value(pool, SYSTEM_CONFIG_DRY_RUN_ACTIVE)
            .await?
            .is_some_and(|value| value == "1");
    let (session_read_ops, session_write_ops, session_egress_bytes) = {
        let session = session_usage()
            .lock()
            .expect("session usage mutex poisoned");
        (session.read_ops, session.write_ops, session.egress_bytes)
    };

    let daily_read_ops = daily.as_ref().map(|d| d.read_ops).unwrap_or(0);
    let daily_write_ops = daily.as_ref().map(|d| d.write_ops).unwrap_or(0);
    let daily_egress_bytes = daily.as_ref().map(|d| d.egress_bytes).unwrap_or(0);
    let read_limit = i64::try_from(config.cloud_daily_read_ops_limit).unwrap_or(i64::MAX);
    let write_limit = i64::try_from(config.cloud_daily_write_ops_limit).unwrap_or(i64::MAX);
    let egress_limit = i64::try_from(config.cloud_daily_egress_bytes_limit).unwrap_or(i64::MAX);

    let status = if dry_run_active {
        "WARN".to_string()
    } else if suspended {
        "ERROR".to_string()
    } else {
        "OK".to_string()
    };
    let message = if dry_run_active {
        "DRY-RUN mode is active; cloud operations are simulated with no external side effects."
            .to_string()
    } else if suspended {
        suspend_reason
            .clone()
            .unwrap_or_else(|| "cloud operations are suspended by circuit breaker".to_string())
    } else {
        "Cloud guard is active and within configured daily limits.".to_string()
    };

    Ok(CloudGuardSnapshot {
        status,
        message,
        dry_run_active,
        cloud_suspended: suspended,
        cloud_suspend_reason: suspend_reason,
        day_epoch,
        session_read_ops,
        session_write_ops,
        session_egress_bytes,
        daily_read_ops,
        daily_write_ops,
        daily_egress_bytes,
        daily_read_ops_limit: read_limit,
        daily_write_ops_limit: write_limit,
        daily_egress_bytes_limit: egress_limit,
        read_quota_percent: percent(daily_read_ops, read_limit),
        write_quota_percent: percent(daily_write_ops, write_limit),
        egress_quota_percent: percent(daily_egress_bytes, egress_limit),
    })
}

fn current_day_epoch() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i128)
        .unwrap_or_default();
    (millis / 86_400_000) as i64
}

fn percent(value: i64, limit: i64) -> f64 {
    if limit <= 0 {
        return 0.0;
    }
    ((value as f64 / limit as f64) * 100.0).clamp(0.0, 100.0)
}
