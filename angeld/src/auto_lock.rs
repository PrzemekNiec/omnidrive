//! α.A.b — central monitor for idle-timeout auto-lock.
//!
//! Owns wait-free activity state (AtomicU64), the tick loop, and the
//! REST-facing config setter. Touch hooks live in `acl.rs` and
//! `smart_sync.rs`; the lock teardown lives in `lock_flow.rs`.
//!
//! Several items in this module are forward-declared for α.A.b.2–α.A.b.4
//! and are not yet called from production code; `dead_code` is suppressed
//! module-wide for the duration of the skeleton phase.
#![allow(dead_code)]

use crate::vault::VaultKeyStore;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;

pub const DEFAULT_IDLE_MIN: u32 = 15;
pub const ALLOWED_PRESETS: [u32; 4] = [5, 15, 30, 60];
pub const WARNING_THRESHOLD_SECS: u64 = 60;
pub const SYSTEM_CONFIG_KEY: &str = "vault.auto_lock_idle_min";
pub const TICK_INTERVAL_SECS: u64 = 10;

#[derive(Copy, Clone, Debug)]
pub enum TouchSource {
    AuthApi,
    CfApi,
    ManualExtend,
}

#[derive(Debug, thiserror::Error)]
pub enum AutoLockError {
    #[error("auto-lock init failed: {0}")]
    Init(#[from] sqlx::Error),
    #[error("invalid preset: {0} (allowed: 5,15,30,60)")]
    InvalidPreset(u32),
    #[error("monitor already initialized")]
    DoubleInit,
}

pub struct AutoLockMonitor {
    pub(crate) last_activity: AtomicU64,
    pub(crate) idle_timeout_secs: AtomicU64,
    pub(crate) daemon_start: tokio::time::Instant,
    pub(crate) pool: SqlitePool,
    /// Used by `force_lock` in α.A.b.2 — kept in the skeleton for API completeness.
    #[allow(dead_code)]
    pub(crate) vault_keys: VaultKeyStore,
}

pub static MONITOR: OnceLock<Arc<AutoLockMonitor>> = OnceLock::new();

// ── Accessors + init ────────────────────────────────────────────────

use std::sync::atomic::Ordering;
use tracing::{info, warn};

/// Wait-free top-level touch — no-op when `MONITOR` is not yet initialised
/// (testing, startup before `init`, cfapi callback firing before run).
pub fn touch(source: TouchSource) {
    if let Some(mon) = MONITOR.get() {
        mon.touch(source);
    }
}

/// Returns a clone of the global `AutoLockMonitor` handle once initialised.
pub fn monitor() -> Option<Arc<AutoLockMonitor>> {
    MONITOR.get().cloned()
}

impl AutoLockMonitor {
    pub fn now_secs(&self) -> u64 {
        self.daemon_start.elapsed().as_secs()
    }

    pub fn idle_timeout_secs(&self) -> u64 {
        self.idle_timeout_secs.load(Ordering::Relaxed)
    }

    pub fn remaining_secs(&self) -> u64 {
        let last = self.last_activity.load(Ordering::Relaxed);
        let timeout = self.idle_timeout_secs();
        if last == 0 {
            // No touch recorded yet — countdown has not started.
            return timeout;
        }
        timeout.saturating_sub(self.now_secs().saturating_sub(last))
    }

    pub async fn init(
        pool: SqlitePool,
        vault_keys: VaultKeyStore,
    ) -> Result<Arc<Self>, AutoLockError> {
        // Debug-only override for SMOKE H2 (spec §5.5). When the env var
        // parses to a valid positive u32 we use it as the active timeout
        // WITHOUT touching the DB — DB remains the source of truth for the
        // user's persisted preference.
        let env_override_min: Option<u32> = if cfg!(debug_assertions) {
            std::env::var("OMNIDRIVE_AUTO_LOCK_TEST_MIN")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|m| *m > 0)
        } else {
            None
        };

        let stored = crate::db::get_system_config_value(&pool, SYSTEM_CONFIG_KEY).await?;
        let db_minutes = resolve_minutes_from_db(stored.as_deref());
        if stored.as_deref() != Some(db_minutes.to_string().as_str()) {
            crate::db::set_system_config_value(&pool, SYSTEM_CONFIG_KEY, &db_minutes.to_string())
                .await?;
        }
        let minutes = env_override_min.unwrap_or(db_minutes);
        if env_override_min.is_some() {
            info!(
                "[AUTO-LOCK] OMNIDRIVE_AUTO_LOCK_TEST_MIN active: {}min (DB persisted: {}min)",
                minutes, db_minutes
            );
        } else {
            info!("[AUTO-LOCK] init: idle_timeout_min={}", minutes);
        }
        let mon = Arc::new(Self {
            last_activity: AtomicU64::new(0),
            idle_timeout_secs: AtomicU64::new(u64::from(minutes) * 60),
            daemon_start: tokio::time::Instant::now(),
            pool,
            vault_keys,
        });
        Ok(mon)
    }

    pub async fn set_timeout_minutes(&self, m: u32) -> Result<(), AutoLockError> {
        if !ALLOWED_PRESETS.contains(&m) {
            return Err(AutoLockError::InvalidPreset(m));
        }
        crate::db::set_system_config_value(&self.pool, SYSTEM_CONFIG_KEY, &m.to_string()).await?;
        self.idle_timeout_secs
            .store(u64::from(m) * 60, Ordering::Relaxed);
        info!("[AUTO-LOCK] timeout updated to {}min", m);
        Ok(())
    }

    /// Wait-free activity stamp. Hot-path safe: single relaxed store on an
    /// AtomicU64 (seconds since `daemon_start`). `_source` is reserved for
    /// telemetry in α.A.b.4 and intentionally unused here.
    pub fn touch(&self, _source: TouchSource) {
        // TODO(α.A.b.4): forward TouchSource to telemetry counter
        self.last_activity.store(self.now_secs(), Ordering::Relaxed);
    }

    pub async fn run_tick_loop(self: Arc<Self>) {
        use futures::FutureExt;
        use std::panic::AssertUnwindSafe;

        let tick = std::time::Duration::from_secs(TICK_INTERVAL_SECS);
        loop {
            let me = Arc::clone(&self);
            let _ = AssertUnwindSafe(async move {
                tokio::time::sleep(tick).await;
                let now = me.now_secs();
                let last = me.last_activity.load(Ordering::Relaxed);
                let timeout = me.idle_timeout_secs.load(Ordering::Relaxed);
                let elapsed = now.saturating_sub(last);
                if elapsed < timeout {
                    return;
                }
                if me.vault_keys.require_key().await.is_err() {
                    return;
                }
                info!(
                    "[AUTO-LOCK] idle exceeded ({}s >= {}s) — forcing lock",
                    elapsed, timeout
                );
                crate::lock_flow::force_lock_and_dismount(
                    &me.pool,
                    &me.vault_keys,
                    crate::lock_flow::LockReason::IdleTimeout,
                    None,
                )
                .await;
            })
            .catch_unwind()
            .await;
        }
    }
}

fn resolve_minutes_from_db(value: Option<&str>) -> u32 {
    match value.and_then(|v| v.parse::<u32>().ok()) {
        Some(n) if ALLOWED_PRESETS.contains(&n) => n,
        Some(n) => {
            warn!("[AUTO-LOCK] db value {} not a preset, clamping", n);
            clamp_to_preset(n)
        }
        None => {
            if let Some(raw) = value {
                warn!(
                    "[AUTO-LOCK] db value {:?} unparseable, defaulting to 15",
                    raw
                );
            }
            DEFAULT_IDLE_MIN
        }
    }
}

fn clamp_to_preset(n: u32) -> u32 {
    *ALLOWED_PRESETS
        .iter()
        .min_by_key(|p| (i64::from(**p) - i64::from(n)).abs())
        .expect("ALLOWED_PRESETS is non-empty")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Serialize tests that read/write the process-global env-var
    // OMNIDRIVE_AUTO_LOCK_TEST_MIN.  cargo runs unit tests in parallel
    // by default, so any test that observes or sets this var must hold
    // this lock for its entire duration.
    // tokio::sync::Mutex is used because the guard is held across .await points.
    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    // ── Task 1.1: type existence tests ──────────────────────────────

    #[test]
    fn touch_source_variants_exist() {
        let _ = TouchSource::AuthApi;
        let _ = TouchSource::CfApi;
        let _ = TouchSource::ManualExtend;
    }

    #[test]
    fn auto_lock_error_invalid_preset_renders() {
        let err = AutoLockError::InvalidPreset(7);
        assert!(format!("{err}").contains("7"));
        assert!(format!("{err}").contains("5,15,30,60"));
    }

    // ── Task 1.2: init tests ─────────────────────────────────────────

    async fn fresh_pool() -> SqlitePool {
        crate::db::init_db("sqlite::memory:").await.unwrap()
    }

    #[tokio::test]
    async fn init_uses_default_15_when_db_empty() {
        // Acquire ENV_LOCK so we don't race with the debug env-override tests
        // that temporarily set OMNIDRIVE_AUTO_LOCK_TEST_MIN.
        let _guard = ENV_LOCK.lock().await;

        let pool = fresh_pool().await;
        let mon = AutoLockMonitor::init(pool.clone(), VaultKeyStore::default())
            .await
            .unwrap();
        assert_eq!(mon.idle_timeout_secs(), 15 * 60);
        let stored = crate::db::get_system_config_value(&pool, SYSTEM_CONFIG_KEY)
            .await
            .unwrap();
        assert_eq!(stored.as_deref(), Some("15"));
    }

    #[tokio::test]
    async fn init_loads_stored_preset() {
        let _guard = ENV_LOCK.lock().await;

        let pool = fresh_pool().await;
        crate::db::set_system_config_value(&pool, SYSTEM_CONFIG_KEY, "30")
            .await
            .unwrap();
        let mon = AutoLockMonitor::init(pool, VaultKeyStore::default())
            .await
            .unwrap();
        assert_eq!(mon.idle_timeout_secs(), 30 * 60);
    }

    #[tokio::test]
    async fn init_clamps_invalid_value_to_nearest_preset() {
        let _guard = ENV_LOCK.lock().await;

        let pool = fresh_pool().await;
        crate::db::set_system_config_value(&pool, SYSTEM_CONFIG_KEY, "7")
            .await
            .unwrap();
        let mon = AutoLockMonitor::init(pool.clone(), VaultKeyStore::default())
            .await
            .unwrap();
        assert_eq!(mon.idle_timeout_secs(), 5 * 60);
        let stored = crate::db::get_system_config_value(&pool, SYSTEM_CONFIG_KEY)
            .await
            .unwrap();
        assert_eq!(stored.as_deref(), Some("5"));
    }

    #[tokio::test]
    async fn init_falls_back_to_default_on_unparseable_value() {
        let _guard = ENV_LOCK.lock().await;

        let pool = fresh_pool().await;
        crate::db::set_system_config_value(&pool, SYSTEM_CONFIG_KEY, "abc")
            .await
            .unwrap();
        let mon = AutoLockMonitor::init(pool.clone(), VaultKeyStore::default())
            .await
            .unwrap();
        assert_eq!(mon.idle_timeout_secs(), 15 * 60);
        let stored = crate::db::get_system_config_value(&pool, SYSTEM_CONFIG_KEY)
            .await
            .unwrap();
        assert_eq!(stored.as_deref(), Some("15"));
    }

    // `OMNIDRIVE_AUTO_LOCK_TEST_MIN` debug-only override — required by SMOKE H2 (spec §5.5).
    // Honored only when `cfg(debug_assertions)`; release builds ignore it.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn init_env_override_takes_precedence_over_db() {
        // Serialize env-var manipulation: cargo runs tests in parallel by default,
        // but env::set_var is process-global.  Use the module-level ENV_LOCK.
        let _guard = ENV_LOCK.lock().await;

        let pool = fresh_pool().await;
        crate::db::set_system_config_value(&pool, SYSTEM_CONFIG_KEY, "30")
            .await
            .unwrap();
        // SAFETY: process-wide env in tests — guarded by ENV_LOCK above.
        unsafe { std::env::set_var("OMNIDRIVE_AUTO_LOCK_TEST_MIN", "1") };
        let mon = AutoLockMonitor::init(pool.clone(), VaultKeyStore::default())
            .await
            .unwrap();
        unsafe { std::env::remove_var("OMNIDRIVE_AUTO_LOCK_TEST_MIN") };

        // Env override wins (1min), DB value untouched (still 30).
        assert_eq!(mon.idle_timeout_secs(), 60);
        let stored = crate::db::get_system_config_value(&pool, SYSTEM_CONFIG_KEY)
            .await
            .unwrap();
        assert_eq!(stored.as_deref(), Some("30"));
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn init_env_override_ignored_when_unparseable() {
        let _guard = ENV_LOCK.lock().await;

        let pool = fresh_pool().await;
        unsafe { std::env::set_var("OMNIDRIVE_AUTO_LOCK_TEST_MIN", "junk") };
        let mon = AutoLockMonitor::init(pool, VaultKeyStore::default())
            .await
            .unwrap();
        unsafe { std::env::remove_var("OMNIDRIVE_AUTO_LOCK_TEST_MIN") };

        assert_eq!(mon.idle_timeout_secs(), 15 * 60); // falls back to default path
    }

    // ── Task 1.3: set_timeout_minutes tests ─────────────────────────

    #[tokio::test]
    async fn set_timeout_accepts_each_preset() {
        let _guard = ENV_LOCK.lock().await;

        let pool = fresh_pool().await;
        let mon = AutoLockMonitor::init(pool, VaultKeyStore::default())
            .await
            .unwrap();
        for &m in &ALLOWED_PRESETS {
            mon.set_timeout_minutes(m).await.unwrap();
            assert_eq!(mon.idle_timeout_secs(), u64::from(m) * 60);
        }
    }

    #[tokio::test]
    async fn set_timeout_rejects_non_preset() {
        let _guard = ENV_LOCK.lock().await;

        let pool = fresh_pool().await;
        let mon = AutoLockMonitor::init(pool, VaultKeyStore::default())
            .await
            .unwrap();
        assert!(matches!(
            mon.set_timeout_minutes(7).await,
            Err(AutoLockError::InvalidPreset(7))
        ));
        assert_eq!(mon.idle_timeout_secs(), 15 * 60); // unchanged
    }

    #[tokio::test]
    async fn set_timeout_persists_to_db() {
        let _guard = ENV_LOCK.lock().await;

        let pool = fresh_pool().await;
        let mon = AutoLockMonitor::init(pool.clone(), VaultKeyStore::default())
            .await
            .unwrap();
        mon.set_timeout_minutes(60).await.unwrap();
        let stored = crate::db::get_system_config_value(&pool, SYSTEM_CONFIG_KEY)
            .await
            .unwrap();
        assert_eq!(stored.as_deref(), Some("60"));
    }

    // ── Fix 1: remaining_secs sentinel for fresh-start state ─────────

    #[tokio::test]
    async fn remaining_secs_full_timeout_when_no_touch_yet() {
        let _guard = ENV_LOCK.lock().await;

        let pool = fresh_pool().await;
        let mon = AutoLockMonitor::init(pool, VaultKeyStore::default())
            .await
            .unwrap();
        // Sleep so now_secs() advances past 0.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(
            mon.remaining_secs(),
            15 * 60,
            "no-touch state should report full timeout"
        );
    }

    // ── Task α.A.b.2.1: touch() + top-level helpers ─────────────────

    #[tokio::test]
    async fn touch_updates_last_activity_to_now_secs() {
        let _guard = ENV_LOCK.lock().await;

        let pool = fresh_pool().await;
        let mon = AutoLockMonitor::init(pool, VaultKeyStore::default())
            .await
            .unwrap();
        // Pause AFTER pool + monitor construction so sqlx's internal timers
        // run against a real clock. `daemon_start` is captured during init,
        // so advancing the paused clock 42s makes `now_secs()` deterministic.
        tokio::time::pause();
        tokio::time::advance(std::time::Duration::from_secs(42)).await;
        mon.touch(TouchSource::AuthApi);
        assert_eq!(mon.last_activity.load(Ordering::Relaxed), 42);
    }

    #[test]
    fn top_level_touch_never_panics() {
        // MONITOR is OnceLock — if other tests set it, the call still must not panic.
        // The key contract: top-level touch() never panics regardless of MONITOR state.
        crate::auto_lock::touch(TouchSource::AuthApi);
    }

    // ── Task α.A.b.2.4: run_tick_loop + catch_unwind ────────────────

    async fn setup_unlocked() -> (sqlx::SqlitePool, VaultKeyStore) {
        let pool = fresh_pool().await;
        crate::db::set_vault_params(&pool, b"saltsaltsaltsalt", "argon2id", "vault-test")
            .await
            .unwrap();
        let keys = VaultKeyStore::default();
        keys.unlock(&pool, "passphrase-for-test").await.unwrap();
        (pool, keys)
    }

    #[tokio::test]
    async fn tick_loop_locks_vault_after_timeout() {
        let _guard = ENV_LOCK.lock().await;

        let (pool, keys) = setup_unlocked().await;
        let mon = AutoLockMonitor::init(pool, keys.clone()).await.unwrap();
        mon.set_timeout_minutes(5).await.unwrap();

        tokio::time::pause();
        let task = tokio::spawn(Arc::clone(&mon).run_tick_loop());
        // Yield so the spawned task is polled and registers its sleep BEFORE
        // we advance the clock — otherwise `advance()` moves time past a sleep
        // that hasn't started, and the sleep's deadline (now+tick) lands in
        // the future relative to the advanced clock.
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_secs(
            5 * 60 + TICK_INTERVAL_SECS + 1,
        ))
        .await;
        // Resume real time so the spawned task's sqlx queries inside
        // `force_lock_and_dismount` can complete — sqlx's internal pool
        // timers do not fire while tokio::time is paused.
        tokio::time::resume();

        let mut locked = false;
        for _ in 0..200 {
            if keys.require_key().await.is_err() {
                locked = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        task.abort();
        assert!(locked, "vault should be locked after idle timeout");
    }

    #[tokio::test]
    async fn tick_loop_touch_resets_countdown() {
        let _guard = ENV_LOCK.lock().await;

        let (pool, keys) = setup_unlocked().await;
        let mon = AutoLockMonitor::init(pool, keys.clone()).await.unwrap();
        mon.set_timeout_minutes(5).await.unwrap();

        tokio::time::pause();
        let task = tokio::spawn(Arc::clone(&mon).run_tick_loop());
        // See `tick_loop_locks_vault_after_timeout` for why this yield is
        // required: the spawned task must register its sleep before `advance`.
        tokio::task::yield_now().await;

        tokio::time::advance(std::time::Duration::from_secs(290)).await;
        mon.touch(TouchSource::AuthApi);
        tokio::time::advance(std::time::Duration::from_secs(290)).await;
        tokio::task::yield_now().await;

        assert!(
            keys.require_key().await.is_ok(),
            "touch must reset the idle countdown"
        );
        task.abort();
    }

    #[tokio::test]
    async fn tick_loop_skips_when_vault_already_locked() {
        let _guard = ENV_LOCK.lock().await;

        let (pool, keys) = setup_unlocked().await;
        keys.lock().await;
        let mon = AutoLockMonitor::init(pool, keys.clone()).await.unwrap();
        mon.set_timeout_minutes(5).await.unwrap();

        tokio::time::pause();
        let task = tokio::spawn(Arc::clone(&mon).run_tick_loop());
        // See `tick_loop_locks_vault_after_timeout` for why this yield is
        // required: the spawned task must register its sleep before `advance`.
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_secs(10 * 60)).await;
        tokio::task::yield_now().await;

        let (cnt,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM audit_logs WHERE action = 'auto_lock'")
                .fetch_one(&mon.pool)
                .await
                .unwrap();
        assert_eq!(
            cnt, 0,
            "idempotent skip: no auto_lock audit when already locked"
        );
        task.abort();
    }
}
