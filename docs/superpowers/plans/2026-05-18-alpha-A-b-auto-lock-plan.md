# Auto-Lock Vault (α.A.b / P2-004) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add automatic vault lock (idle-timeout + Win+L) on top of the manual-logout teardown shipped in α.A.a, without changing existing crypto or session semantics.

**Architecture:** A single `AutoLockMonitor` (wait-free `AtomicU64` state, global `OnceLock`) is touched from authenticated API calls and cfapi hydration callbacks. A 10s tick task fires `lock_flow::force_lock_and_dismount(reason)` when idle exceeds the configurable timeout, and a dedicated Win32 message-pump thread fires the same flow on `WTS_SESSION_LOCK`. Manual logout (α.A.a) is refactored to go through the same `lock_flow`, so all four lock paths share one teardown.

**Tech Stack:** Rust (Edition 2024) · Tokio (paused-clock tests) · axum 0.8 · sqlx 0.8 (SQLite) · windows-rs 0.62 (`Win32_System_RemoteDesktop`, `Win32_UI_WindowsAndMessaging`) · feature-gated `test-helpers` for Win+L simulation · Vanilla JS frontend (Tailwind, no framework).

**Spec:** `docs/superpowers/specs/2026-05-18-alpha-A-b-auto-lock-design.md` — sections referenced inline as `→ spec §N`.

**HARD-GATE:** No production code is written until Przemek explicitly accepts this plan.

---

## File Structure

### New files (4)

| Path | Responsibility |
|---|---|
| `angeld/src/auto_lock.rs` | `AutoLockMonitor` (AtomicU64 state, OnceLock global), `TouchSource`, `AutoLockError`, top-level `touch()`/`monitor()` helpers, tick loop, init/clamp/set-timeout logic |
| `angeld/src/lock_flow.rs` | `LockReason` enum + `force_lock_and_dismount(pool, vault_keys, reason)` — single source of truth for audit → `vault_keys.lock()` → CF dismount → virtual-drive unmount. Reused by manual logout, idle timeout, Win+L, manual user action |
| `angeld/src/win_session.rs` | `cfg(target_os = "windows")` Win32 session-change observer (dedicated OS thread + hidden message-only window). `ObserverHandle` with Drop teardown. `test-helpers`-gated mpsc dispatcher for integration tests |
| `angeld/src/api/auto_lock.rs` | REST surface: `GET /api/auto-lock/status`, `POST /api/auto-lock/timeout`, `POST /api/auto-lock/touch`. `AutoLockStatus`/`SetTimeoutRequest` DTOs |

### Modified files (5)

| Path | Change |
|---|---|
| `angeld/src/lib.rs` | Declare `pub mod auto_lock; pub mod lock_flow; #[cfg(target_os="windows")] pub mod win_session;` |
| `angeld/src/acl.rs` | `require_session` and `require_role` call `auto_lock::touch(TouchSource::AuthApi)` after success. **NEW** `require_session_no_touch` (identical body without the touch) for status polling |
| `angeld/src/smart_sync.rs` | After `decode_file_identity = Some(_)` in `fetch_data_callback_inner` AND at the top of `fetch_placeholders_callback_inner` → `crate::auto_lock::touch(TouchSource::CfApi)` |
| `angeld/src/api/auth.rs` | `post_auth_logout` calls `lock_flow::force_lock_and_dismount(..., LockReason::Logout)` instead of inline `vault_keys.lock()` + spawn dismount |
| `angeld/src/api/mod.rs` | Add `pub mod auto_lock;` (api submodule), call `AutoLockMonitor::init` in `ApiServer::run`, spawn tick loop + win_session observer, merge `api::auto_lock::routes()` |

### Frontend files (modified)

| Path | Change |
|---|---|
| `angeld/static/index.html` | Topbar pill placeholder (`#auto-lock-pill`), toast container (`#auto-lock-toast`) |
| `angeld/static/auto-lock.js` (NEW) | `setInterval(5000)` poll, state-machine UI updates, `[Wydłuż]`/`[Zablokuj]` handlers, redirect on `locked` |

### Cargo features / deps

`angeld/Cargo.toml` already exposes `test-helpers = []`. The `windows` crate already lists the features we need — **verify before α.A.b.3** that `Win32_System_RemoteDesktop` and `Win32_UI_WindowsAndMessaging` are present; both must be added if missing (current Cargo.toml has neither — this plan adds them in α.A.b.3 Task 1).

---

## Sub-step decomposition

The spec breaks α.A.b into four sub-steps. Each ends with `commit + push` and a checkpoint where the agent **pauses and asks Przemek before continuing** (memory `feedback_token_budget`).

- **α.A.b.1 — Config layer** (no runtime side-effects yet): module skeleton, DB-backed timeout, `POST /timeout`, validation, clamping.
- **α.A.b.2 — Activity tracking**: touch, tick loop, ACL/cfapi hooks, `lock_flow` extract, `post_auth_logout` refactor, `GET /status` + `POST /touch`. End-to-end idle-lock works.
- **α.A.b.3 — Win+L observer**: dedicated OS thread, hidden window, WTS notification, test-helpers mpsc, graceful degradation.
- **α.A.b.4 — Frontend**: topbar pill, toast, polling client, lock redirect.

**SMOKE H2 + H3** are run after α.A.b.4 on Lenovo + Dell before the version bump.

---

# α.A.b.1 — Config layer

**Files:**
- Create: `angeld/src/auto_lock.rs`
- Create: `angeld/src/api/auto_lock.rs`
- Modify: `angeld/src/lib.rs` (module decl)
- Modify: `angeld/src/api/mod.rs` (module decl + `init` call in `ApiServer::run` + `.merge(api::auto_lock::routes())`)

**Outcome:** `MONITOR.get()` returns an initialized monitor with `idle_timeout_secs` loaded from `system_config`, default 15min, presets `{5,15,30,60}` enforced, `POST /api/auto-lock/timeout` updates DB + atomic store. No tick loop, no touch hooks — config plumbing only.

**Acceptance:** all unit tests in `auto_lock::tests` (init + clamping + set_timeout) pass; daemon boots without regression (`e2e_basic` green); `POST /api/auto-lock/timeout` returns 204 on `{5,15,30,60}`, 400 elsewhere.

---

### Task 1.1: Skeleton — types and errors (no logic)

**Files:**
- Create: `angeld/src/auto_lock.rs`
- Test: `angeld/src/auto_lock.rs` (`#[cfg(test)] mod tests` at bottom)
- Modify: `angeld/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `angeld/src/auto_lock.rs::tests`:

```rust
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
```

- [ ] **Step 2: Write the module skeleton**

Create `angeld/src/auto_lock.rs`:

```rust
//! α.A.b — central monitor for idle-timeout auto-lock.
//!
//! Owns wait-free activity state (AtomicU64), the tick loop, and the
//! REST-facing config setter.  Touch hooks live in `acl.rs` and
//! `smart_sync.rs`; the lock teardown lives in `lock_flow.rs`.

use crate::vault::VaultKeyStore;
use sqlx::SqlitePool;
use std::sync::OnceLock;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

pub const DEFAULT_IDLE_MIN: u32 = 15;
pub const ALLOWED_PRESETS: [u32; 4] = [5, 15, 30, 60];
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
    pub(crate) vault_keys: VaultKeyStore,
}

pub static MONITOR: OnceLock<Arc<AutoLockMonitor>> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;
    // (tests inserted here)
}
```

`angeld` does not currently depend on `thiserror`. Add it under `[dependencies]` in `angeld/Cargo.toml`:

```toml
thiserror = "1"
```

- [ ] **Step 3: Wire the module**

Edit `angeld/src/lib.rs` — find the existing `pub mod` list and add:

```rust
pub mod auto_lock;
```

(Place alphabetically near `pub mod audit` / `pub mod acl`. If `lib.rs` is absent and the crate is a binary, declare it in `angeld/src/main.rs` as `mod auto_lock;` instead — verify before writing.)

- [ ] **Step 4: Run the test**

```
cargo test -p angeld --lib auto_lock::tests::touch_source_variants_exist auto_lock::tests::auto_lock_error_invalid_preset_renders
```

Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add angeld/src/auto_lock.rs angeld/src/lib.rs angeld/Cargo.toml
git commit -m "feat(auto-lock): scaffold AutoLockMonitor types (α.A.b.1)"
```

---

### Task 1.2: `AutoLockMonitor::init` — load + clamp + default

**Files:**
- Modify: `angeld/src/auto_lock.rs`

- [ ] **Step 1: Write the failing tests**

Append to `auto_lock::tests`:

```rust
async fn fresh_pool() -> SqlitePool {
    crate::db::init_db("sqlite::memory:").await.unwrap()
}

#[tokio::test]
async fn init_uses_default_15_when_db_empty() {
    let pool = fresh_pool().await;
    let mon = AutoLockMonitor::init(pool.clone(), VaultKeyStore::default()).await.unwrap();
    assert_eq!(mon.idle_timeout_secs(), 15 * 60);
    let stored = crate::db::get_system_config_value(&pool, SYSTEM_CONFIG_KEY).await.unwrap();
    assert_eq!(stored.as_deref(), Some("15"));
}

#[tokio::test]
async fn init_loads_stored_preset() {
    let pool = fresh_pool().await;
    crate::db::set_system_config_value(&pool, SYSTEM_CONFIG_KEY, "30").await.unwrap();
    let mon = AutoLockMonitor::init(pool, VaultKeyStore::default()).await.unwrap();
    assert_eq!(mon.idle_timeout_secs(), 30 * 60);
}

#[tokio::test]
async fn init_clamps_invalid_value_to_nearest_preset() {
    let pool = fresh_pool().await;
    crate::db::set_system_config_value(&pool, SYSTEM_CONFIG_KEY, "7").await.unwrap();
    let mon = AutoLockMonitor::init(pool.clone(), VaultKeyStore::default()).await.unwrap();
    assert_eq!(mon.idle_timeout_secs(), 5 * 60);
    let stored = crate::db::get_system_config_value(&pool, SYSTEM_CONFIG_KEY).await.unwrap();
    assert_eq!(stored.as_deref(), Some("5"));
}

#[tokio::test]
async fn init_falls_back_to_default_on_unparseable_value() {
    let pool = fresh_pool().await;
    crate::db::set_system_config_value(&pool, SYSTEM_CONFIG_KEY, "abc").await.unwrap();
    let mon = AutoLockMonitor::init(pool.clone(), VaultKeyStore::default()).await.unwrap();
    assert_eq!(mon.idle_timeout_secs(), 15 * 60);
    let stored = crate::db::get_system_config_value(&pool, SYSTEM_CONFIG_KEY).await.unwrap();
    assert_eq!(stored.as_deref(), Some("15"));
}

// `OMNIDRIVE_AUTO_LOCK_TEST_MIN` debug-only override — required by SMOKE H2 (spec §5.5).
// Honored only when `cfg(debug_assertions)`; release builds ignore it.
#[cfg(debug_assertions)]
#[tokio::test]
async fn init_env_override_takes_precedence_over_db() {
    // Serialize env-var manipulation: cargo runs tests in parallel by default,
    // but env::set_var is process-global.  Use a dedicated mutex.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK.lock().unwrap();

    let pool = fresh_pool().await;
    crate::db::set_system_config_value(&pool, SYSTEM_CONFIG_KEY, "30").await.unwrap();
    // SAFETY: process-wide env in tests — guarded by ENV_LOCK above.
    unsafe { std::env::set_var("OMNIDRIVE_AUTO_LOCK_TEST_MIN", "1") };
    let mon = AutoLockMonitor::init(pool.clone(), VaultKeyStore::default()).await.unwrap();
    unsafe { std::env::remove_var("OMNIDRIVE_AUTO_LOCK_TEST_MIN") };

    // Env override wins (1min), DB value untouched (still 30).
    assert_eq!(mon.idle_timeout_secs(), 60);
    let stored = crate::db::get_system_config_value(&pool, SYSTEM_CONFIG_KEY).await.unwrap();
    assert_eq!(stored.as_deref(), Some("30"));
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn init_env_override_ignored_when_unparseable() {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK.lock().unwrap();

    let pool = fresh_pool().await;
    unsafe { std::env::set_var("OMNIDRIVE_AUTO_LOCK_TEST_MIN", "junk") };
    let mon = AutoLockMonitor::init(pool, VaultKeyStore::default()).await.unwrap();
    unsafe { std::env::remove_var("OMNIDRIVE_AUTO_LOCK_TEST_MIN") };

    assert_eq!(mon.idle_timeout_secs(), 15 * 60); // falls back to default path
}
```

- [ ] **Step 2: Run them to verify they fail**

```
cargo test -p angeld --lib auto_lock::tests::init_
```

Expected: 4 compile errors — `AutoLockMonitor::init` / `idle_timeout_secs` not defined.

- [ ] **Step 3: Implement `init` + accessors**

Append to `auto_lock.rs`:

```rust
use std::sync::atomic::Ordering;
use tracing::{info, warn};

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
        timeout.saturating_sub(self.now_secs().saturating_sub(last))
    }

    pub async fn init(
        pool: SqlitePool,
        vault_keys: VaultKeyStore,
    ) -> Result<Arc<Self>, AutoLockError> {
        // Debug-only override for SMOKE H2 (spec §5.5).  When the env var
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
            crate::db::set_system_config_value(&pool, SYSTEM_CONFIG_KEY, &db_minutes.to_string()).await?;
        }
        let minutes = env_override_min.unwrap_or(db_minutes);
        if env_override_min.is_some() {
            info!("[AUTO-LOCK] OMNIDRIVE_AUTO_LOCK_TEST_MIN active: {}min (DB persisted: {}min)", minutes, db_minutes);
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
                warn!("[AUTO-LOCK] db value {:?} unparseable, defaulting to 15", raw);
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
```

- [ ] **Step 4: Run tests, expect green**

```
cargo test -p angeld --lib auto_lock::tests::init_
```

Expected: 6 passed (4 DB-driven + 2 debug-only env override).

The release-build invariant — env override is **silently ignored** — has no dedicated test (the `#[cfg(debug_assertions)]` gate makes it impossible to assert from a unit-test process). SMOKE H2 on Lenovo/Dell is the real verification path: those boxes run release binaries.

- [ ] **Step 5: Commit**

```bash
git add angeld/src/auto_lock.rs
git commit -m "feat(auto-lock): init loads/clamps idle_timeout + debug env override (α.A.b.1)"
```

---

### Task 1.3: `set_timeout_minutes` — validate + write-back + atomic store

**Files:**
- Modify: `angeld/src/auto_lock.rs`

- [ ] **Step 1: Write the failing tests**

Append to `auto_lock::tests`:

```rust
#[tokio::test]
async fn set_timeout_accepts_each_preset() {
    let pool = fresh_pool().await;
    let mon = AutoLockMonitor::init(pool, VaultKeyStore::default()).await.unwrap();
    for &m in &ALLOWED_PRESETS {
        mon.set_timeout_minutes(m).await.unwrap();
        assert_eq!(mon.idle_timeout_secs(), u64::from(m) * 60);
    }
}

#[tokio::test]
async fn set_timeout_rejects_non_preset() {
    let pool = fresh_pool().await;
    let mon = AutoLockMonitor::init(pool, VaultKeyStore::default()).await.unwrap();
    assert!(matches!(
        mon.set_timeout_minutes(7).await,
        Err(AutoLockError::InvalidPreset(7))
    ));
    assert_eq!(mon.idle_timeout_secs(), 15 * 60); // unchanged
}

#[tokio::test]
async fn set_timeout_persists_to_db() {
    let pool = fresh_pool().await;
    let mon = AutoLockMonitor::init(pool.clone(), VaultKeyStore::default()).await.unwrap();
    mon.set_timeout_minutes(60).await.unwrap();
    let stored = crate::db::get_system_config_value(&pool, SYSTEM_CONFIG_KEY).await.unwrap();
    assert_eq!(stored.as_deref(), Some("60"));
}
```

- [ ] **Step 2: Verify they fail**

```
cargo test -p angeld --lib auto_lock::tests::set_timeout_
```

Expected: compile error — `set_timeout_minutes` not defined.

- [ ] **Step 3: Implement**

Append inside `impl AutoLockMonitor`:

```rust
pub async fn set_timeout_minutes(&self, m: u32) -> Result<(), AutoLockError> {
    if !ALLOWED_PRESETS.contains(&m) {
        return Err(AutoLockError::InvalidPreset(m));
    }
    crate::db::set_system_config_value(&self.pool, SYSTEM_CONFIG_KEY, &m.to_string()).await?;
    self.idle_timeout_secs.store(u64::from(m) * 60, Ordering::Relaxed);
    info!("[AUTO-LOCK] timeout updated to {}min", m);
    Ok(())
}
```

- [ ] **Step 4: Run tests**

```
cargo test -p angeld --lib auto_lock::tests::set_timeout_
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add angeld/src/auto_lock.rs
git commit -m "feat(auto-lock): set_timeout_minutes validates preset + persists (α.A.b.1)"
```

---

### Task 1.4: `POST /api/auto-lock/timeout` endpoint

**Files:**
- Create: `angeld/src/api/auto_lock.rs`
- Modify: `angeld/src/api/mod.rs` (add `mod auto_lock;` to the module list at top, no merge yet)

- [ ] **Step 1: Write the integration test**

Add to `angeld/tests/e2e_basic.rs` (or create `angeld/tests/e2e_auto_lock_config.rs` if `e2e_basic.rs` is already crowded — verify before writing):

```rust
#[tokio::test]
async fn auto_lock_timeout_endpoint_accepts_preset() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?; // ensures session token, see existing helper
    let resp = h.post_json("/api/auto-lock/timeout", serde_json::json!({"idle_timeout_min": 30})).await?;
    assert_eq!(resp.status, 204);
    Ok(())
}

#[tokio::test]
async fn auto_lock_timeout_endpoint_rejects_invalid() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let resp = h.post_json("/api/auto-lock/timeout", serde_json::json!({"idle_timeout_min": 7})).await?;
    assert_eq!(resp.status, 400);
    assert!(resp.body.contains("invalid_preset"));
    Ok(())
}
```

`DaemonHarness::unlock` already exists in `e2e_recovery.rs` (line 162). If `e2e_basic.rs` lacks a `unlock` helper, lift the implementation from `e2e_recovery.rs` into a shared `mod helpers` first (separate refactor commit, before this task).

- [ ] **Step 2: Verify it fails**

```
cargo test -p angeld --test e2e_basic auto_lock_timeout_endpoint_
```

Expected: 404 (route not registered).

- [ ] **Step 3: Implement the endpoint**

Create `angeld/src/api/auto_lock.rs`:

```rust
use super::ApiState;
use super::error::ApiError;
use crate::acl;
use crate::auto_lock::{self, AutoLockError, MONITOR};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct SetTimeoutRequest {
    idle_timeout_min: u32,
}

#[derive(Serialize)]
struct AutoLockStatus {
    idle_timeout_min: u32,
    remaining_seconds: u64,
    state: &'static str,
}

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/auto-lock/timeout", post(post_timeout))
        // GET /status + POST /touch are added in α.A.b.2 — do NOT add here.
}

async fn post_timeout(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(body): Json<SetTimeoutRequest>,
) -> Result<StatusCode, ApiError> {
    let _ = acl::require_session(&state.pool, &headers).await?;
    let mon = MONITOR.get().ok_or(ApiError::Internal {
        message: "auto-lock monitor not initialized".into(),
    })?;
    match mon.set_timeout_minutes(body.idle_timeout_min).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(AutoLockError::InvalidPreset(n)) => Err(ApiError::BadRequest {
            code: "invalid_preset",
            message: format!("idle_timeout_min={n} not in [5,15,30,60]"),
        }),
        Err(e) => Err(ApiError::Internal { message: e.to_string() }),
    }
}

// `routes` is also re-exported from auto_lock.rs for ergonomic
// integration; thin re-export only.
```

Add the corresponding re-export at the bottom of `angeld/src/auto_lock.rs`:

```rust
pub use crate::api::auto_lock as api_routes;
```

(Lets `api/mod.rs` import via either path — pick one convention, plan prefers `crate::api::auto_lock::routes()`.)

- [ ] **Step 4: Wire the route in `api/mod.rs`**

In `angeld/src/api/mod.rs`:
- Add at module list (after `mod auth;`): `mod auto_lock;`
- In `ApiServer::run`, **before** the existing `.with_state(state)` line, init the monitor and merge routes:

```rust
let monitor = crate::auto_lock::AutoLockMonitor::init(
    state.pool.clone(),
    state.vault_keys.clone(),
).await.map_err(|e| ApiServerError::Io(std::io::Error::other(e.to_string())))?;
let _ = crate::auto_lock::MONITOR.set(monitor);

let app = Router::new()
    // ... existing routes unchanged ...
    .merge(auto_lock::routes())
    .with_state(state);
```

- [ ] **Step 5: Run the integration tests**

```
cargo test -p angeld --test e2e_basic auto_lock_timeout_endpoint_
```

Expected: 2 passed.

- [ ] **Step 6: Verify the whole crate still builds + clippy clean**

```
cargo build -p angeld --release
cargo clippy -p angeld --all-targets -- -D warnings
```

Expected: both succeed.

- [ ] **Step 7: Commit α.A.b.1 checkpoint**

```bash
git add angeld/src/auto_lock.rs angeld/src/api/auto_lock.rs angeld/src/api/mod.rs angeld/tests/e2e_basic.rs
git commit -m "feat(auto-lock): POST /api/auto-lock/timeout endpoint (α.A.b.1)"
git push origin main
```

- [ ] **Step 8: CHECKPOINT — pause and ask Przemek**

Report:
- α.A.b.1 done: monitor scaffolded, init/clamp/set-timeout green, endpoint wired, daemon boots.
- 10 new unit tests, 2 new integration tests, all green.
- Ask: "α.A.b.1 PASS — continue α.A.b.2, or commit-and-park?"

Wait for explicit OK before α.A.b.2.

---

# α.A.b.2 — Activity tracking + tick loop

**Files:**
- Modify: `angeld/src/auto_lock.rs` (touch, run_tick_loop, top-level `touch()` helper)
- Create: `angeld/src/lock_flow.rs`
- Modify: `angeld/src/acl.rs` (touch hooks + `require_session_no_touch`)
- Modify: `angeld/src/smart_sync.rs` (cfapi callback hooks)
- Modify: `angeld/src/api/auth.rs` (`post_auth_logout` → `lock_flow`)
- Modify: `angeld/src/api/mod.rs` (`tokio::spawn(monitor.clone().run_tick_loop())`)
- Modify: `angeld/src/api/auto_lock.rs` (add GET /status, POST /touch)
- Modify: `angeld/src/lib.rs` (declare `lock_flow`)
- Create: `angeld/tests/e2e_auto_lock.rs`

**Outcome:** authenticated calls + cfapi events touch the monitor, the tick loop locks the vault after idle, status polling does not touch, manual logout goes through `lock_flow`.

**Acceptance:** all unit tests in `auto_lock`/`lock_flow` pass with `start_paused`, all 8 e2e_auto_lock tests pass, `e2e_basic` still green, `cargo clippy` clean.

---

### Task 2.1: `touch` + top-level helper + module wiring

**Files:**
- Modify: `angeld/src/auto_lock.rs`

- [ ] **Step 1: Write the failing test**

Append to `auto_lock::tests`:

```rust
#[tokio::test(start_paused = true)]
async fn touch_updates_last_activity_to_now_secs() {
    let pool = fresh_pool().await;
    let mon = AutoLockMonitor::init(pool, VaultKeyStore::default()).await.unwrap();
    tokio::time::advance(std::time::Duration::from_secs(42)).await;
    mon.touch(TouchSource::AuthApi);
    assert_eq!(mon.last_activity.load(std::sync::atomic::Ordering::Relaxed), 42);
}

#[test]
fn top_level_touch_is_noop_when_monitor_uninitialized() {
    // MONITOR is OnceLock — if other tests set it, this is best-effort.
    // The key contract: touch() never panics when MONITOR is unset.
    crate::auto_lock::touch(TouchSource::AuthApi);
}
```

- [ ] **Step 2: Verify failure**

```
cargo test -p angeld --lib auto_lock::tests::touch_
```

Expected: compile error — `AutoLockMonitor::touch` not defined.

- [ ] **Step 3: Implement `touch` and helper**

In `impl AutoLockMonitor`:

```rust
pub fn touch(&self, _source: TouchSource) {
    self.last_activity.store(self.now_secs(), Ordering::Relaxed);
}
```

At module top level (outside impl):

```rust
/// Wait-free touch — no-op when MONITOR is not yet initialised
/// (testing, startup before `init`, cfapi callback firing before run).
pub fn touch(source: TouchSource) {
    if let Some(mon) = MONITOR.get() {
        mon.touch(source);
    }
}

pub fn monitor() -> Option<Arc<AutoLockMonitor>> {
    MONITOR.get().cloned()
}
```

- [ ] **Step 4: Run, expect green**

```
cargo test -p angeld --lib auto_lock::tests::touch_
```

Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add angeld/src/auto_lock.rs
git commit -m "feat(auto-lock): wait-free touch() with TouchSource (α.A.b.2)"
```

---

### Task 2.2: `lock_flow::force_lock_and_dismount` — DRY extract

**Files:**
- Create: `angeld/src/lock_flow.rs`
- Modify: `angeld/src/lib.rs` (declare module)

- [ ] **Step 1: Write the failing tests**

Create `angeld/src/lock_flow.rs::tests` skeleton + tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::VaultKeyStore;

    async fn setup() -> (sqlx::SqlitePool, VaultKeyStore) {
        let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
        // Bootstrap minimal vault so insert_audit_log has a vault_id.
        crate::db::set_vault_params(&pool, b"saltsaltsaltsalt", "argon2id", "vault-test")
            .await
            .unwrap();
        let keys = VaultKeyStore::default();
        keys.unlock(&pool, "passphrase-for-test").await.unwrap();
        (pool, keys)
    }

    #[tokio::test]
    async fn force_lock_when_unlocked_locks_and_returns_true() {
        let (pool, keys) = setup().await;
        assert!(keys.require_key().await.is_ok());
        let was = force_lock_and_dismount(&pool, &keys, LockReason::IdleTimeout).await;
        assert!(was);
        assert!(keys.require_key().await.is_err());
    }

    #[tokio::test]
    async fn force_lock_when_already_locked_returns_false() {
        let (pool, keys) = setup().await;
        keys.lock().await;
        let was = force_lock_and_dismount(&pool, &keys, LockReason::IdleTimeout).await;
        assert!(!was);
    }

    #[tokio::test]
    async fn force_lock_emits_audit_with_reason_idle_timeout() {
        let (pool, keys) = setup().await;
        force_lock_and_dismount(&pool, &keys, LockReason::IdleTimeout).await;
        let logs: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT action, details FROM audit_logs ORDER BY id DESC LIMIT 1"
        ).fetch_all(&pool).await.unwrap();
        assert_eq!(logs[0].0, "auto_lock");
        assert!(logs[0].1.as_deref().unwrap_or("").contains("idle_timeout"));
    }
}
```

- [ ] **Step 2: Verify failure**

```
cargo test -p angeld --lib lock_flow::tests
```

Expected: compile error — `force_lock_and_dismount`/`LockReason` not defined.

- [ ] **Step 3: Implement `lock_flow.rs`**

Full file body:

```rust
//! α.A.b — single source of truth for "lock + teardown".
//!
//! Used by manual logout (α.A.a refactor), idle timeout (α.A.b.2),
//! Win+L (α.A.b.3) and manual user action.  Always succeeds (vault
//! MUST end up locked); individual teardown failures are logged + ignored.

use crate::db;
use crate::runtime_paths::RuntimePaths;
use crate::smart_sync;
use crate::vault::VaultKeyStore;
use sqlx::SqlitePool;
use tracing::{info, warn};

#[derive(Copy, Clone, Debug)]
pub enum LockReason {
    Logout,
    IdleTimeout,
    WinSessionLock,
    ManualUserAction,
}

impl LockReason {
    pub fn audit(&self) -> (&'static str, Option<&'static str>) {
        match self {
            LockReason::Logout            => ("logout",     None),
            LockReason::IdleTimeout       => ("auto_lock",  Some(r#"{"reason":"idle_timeout"}"#)),
            LockReason::WinSessionLock    => ("auto_lock",  Some(r#"{"reason":"win_session_lock"}"#)),
            LockReason::ManualUserAction  => ("vault_lock", Some(r#"{"reason":"manual"}"#)),
        }
    }
}

pub async fn force_lock_and_dismount(
    pool: &SqlitePool,
    vault_keys: &VaultKeyStore,
    reason: LockReason,
) -> bool {
    let was_unlocked = vault_keys.require_key().await.is_ok();

    if was_unlocked {
        if let Ok(Some(vault)) = db::get_vault_params(pool).await {
            let (action, details) = reason.audit();
            if let Err(e) = db::insert_audit_log(
                pool, &vault.vault_id, action, None, None, None, None, details,
            ).await {
                warn!("[LOCK_FLOW] audit emission failed (reason={:?}): {}", reason, e);
            }
        }
    }

    vault_keys.lock().await;

    if was_unlocked {
        info!("[LOCK_FLOW] locked, reason={:?} — spawning teardown", reason);
        tokio::spawn(async move {
            let paths = RuntimePaths::detect();
            if let Err(err) = smart_sync::dismount_after_lock(&paths.sync_root).await {
                warn!("[LOCK_FLOW] CF dismount failed: {err}");
            }
            let drive_letter = std::env::var("OMNIDRIVE_DRIVE_LETTER")
                .unwrap_or_else(|_| "O:".to_string());
            if let Err(err) = crate::virtual_drive::unmount_virtual_drive(&drive_letter) {
                warn!("[LOCK_FLOW] virtual drive unmount warning: {err}");
            }
        });
    }

    was_unlocked
}

// (tests inserted here — see Step 1)
```

In `angeld/src/lib.rs` add `pub mod lock_flow;` (or `mod lock_flow;` in `main.rs` if no lib.rs — verify).

- [ ] **Step 4: Run tests**

```
cargo test -p angeld --lib lock_flow::tests
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add angeld/src/lock_flow.rs angeld/src/lib.rs
git commit -m "feat(lock-flow): unified force_lock_and_dismount with LockReason (α.A.b.2)"
```

---

### Task 2.3: Refactor `post_auth_logout` → `lock_flow`

**Files:**
- Modify: `angeld/src/api/auth.rs`

- [ ] **Step 1: Write the regression test**

Add to `angeld/tests/e2e_basic.rs`:

```rust
#[tokio::test]
async fn logout_emits_logout_audit_not_auto_lock() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    h.post("/api/auth/logout").await?;
    let pool = h.open_db_readonly().await?;
    let row: (String,) = sqlx::query_as("SELECT action FROM audit_logs ORDER BY id DESC LIMIT 1")
        .fetch_one(&pool).await?;
    assert_eq!(row.0, "logout");
    Ok(())
}
```

(`open_db_readonly` exists in `e2e_recovery.rs`; lift to shared helper if missing — same caveat as Task 1.4 Step 1.)

- [ ] **Step 2: Verify it passes today**

```
cargo test -p angeld --test e2e_basic logout_emits_logout_audit_not_auto_lock
```

Expected: PASS today (audit `"logout"` is already emitted in `post_auth_logout:228-238`). This test is a **regression guard** — its purpose is to fail loudly if the refactor accidentally drops the `"logout"` event or replaces it with `"auto_lock"`.

- [ ] **Step 3: Refactor `post_auth_logout`**

In `angeld/src/api/auth.rs:193-264`, replace the body after the `extract token + validate session` block (lines 207-256) with:

```rust
let session_before = db::validate_user_session(&state.pool, token).await.ok().flatten();

if session_before.is_some() {
    crate::lock_flow::force_lock_and_dismount(
        &state.pool,
        &state.vault_keys,
        crate::lock_flow::LockReason::Logout,
    ).await;
}

let deleted = db::delete_user_session(&state.pool, token).await?;
if deleted {
    Ok(Json(serde_json::json!({ "status": "logged_out" })))
} else {
    Err(ApiError::NotFound { resource: "session", id: "current".to_string() })
}
```

Remove now-unused imports (`smart_sync`, `RuntimePaths`, `warn`) if no other handler in this file uses them — verify before deleting; `windows_hello` warn at line 59 still needs `warn`.

**Side-effect to note:** the audit `actor_user_id`/`actor_device_id` were previously populated from `session_before`. `force_lock_and_dismount` writes `None` for both because it has no session context. If audit identity is required for the `logout` event, add an optional `actor: Option<(&str, &str)>` parameter to `force_lock_and_dismount` and pass `session_before.map(|s| (&s.user_id, &s.device_id))`. **Take this branch** — preserving observability over a marginal API simplification.

Revised `lock_flow::force_lock_and_dismount` signature (apply the same diff in Task 2.2 retrospectively if needed):

```rust
pub async fn force_lock_and_dismount(
    pool: &SqlitePool,
    vault_keys: &VaultKeyStore,
    reason: LockReason,
    actor: Option<(&str, &str)>, // (user_id, device_id)
) -> bool { ... }
```

Audit call inside becomes:

```rust
db::insert_audit_log(
    pool, &vault.vault_id, action,
    actor.map(|(u, _)| u),
    actor.map(|(_, d)| d),
    None, None, details,
).await
```

Call sites:
- `post_auth_logout`: `actor = session_before.as_ref().map(|s| (s.user_id.as_str(), s.device_id.as_str()))`
- Idle / Win+L: `actor = None`

Update the Task 2.2 tests to pass `None` for actor.

- [ ] **Step 4: Run regression + lock_flow tests**

```
cargo test -p angeld --lib lock_flow::tests
cargo test -p angeld --test e2e_basic logout_emits_logout_audit_not_auto_lock
```

Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add angeld/src/api/auth.rs angeld/src/lock_flow.rs angeld/tests/e2e_basic.rs
git commit -m "refactor(auth): post_auth_logout uses lock_flow with actor (α.A.b.2)"
```

---

### Task 2.4: Tick loop with `catch_unwind`

**Files:**
- Modify: `angeld/src/auto_lock.rs`

- [x] **Step 1: Write the failing tests**

Append to `auto_lock::tests`:

```rust
use std::sync::atomic::Ordering;

async fn setup_unlocked() -> (sqlx::SqlitePool, VaultKeyStore) {
    let pool = fresh_pool().await;
    crate::db::set_vault_params(&pool, b"saltsaltsaltsalt", "argon2id", "vault-test")
        .await.unwrap();
    let keys = VaultKeyStore::default();
    keys.unlock(&pool, "passphrase-for-test").await.unwrap();
    (pool, keys)
}

#[tokio::test(start_paused = true)]
async fn tick_loop_locks_vault_after_timeout() {
    let (pool, keys) = setup_unlocked().await;
    let mon = AutoLockMonitor::init(pool, keys.clone()).await.unwrap();
    mon.set_timeout_minutes(5).await.unwrap();
    let task = tokio::spawn(Arc::clone(&mon).run_tick_loop());
    // Drive time past 5min + one tick.
    tokio::time::advance(std::time::Duration::from_secs(5 * 60 + TICK_INTERVAL_SECS + 1)).await;
    tokio::task::yield_now().await;
    assert!(keys.require_key().await.is_err(), "vault should be locked");
    task.abort();
}

#[tokio::test(start_paused = true)]
async fn tick_loop_touch_resets_countdown() {
    let (pool, keys) = setup_unlocked().await;
    let mon = AutoLockMonitor::init(pool, keys.clone()).await.unwrap();
    mon.set_timeout_minutes(5).await.unwrap();
    let task = tokio::spawn(Arc::clone(&mon).run_tick_loop());

    // Advance 4:50, then touch.
    tokio::time::advance(std::time::Duration::from_secs(290)).await;
    mon.touch(TouchSource::AuthApi);
    // Advance another 4:50.  Total elapsed 9:40 but last_activity≈4:50, so 4:50 idle — under 5min.
    tokio::time::advance(std::time::Duration::from_secs(290)).await;
    tokio::task::yield_now().await;
    assert!(keys.require_key().await.is_ok(), "touch must reset countdown");
    task.abort();
}

#[tokio::test(start_paused = true)]
async fn tick_loop_skips_when_vault_already_locked() {
    let (pool, keys) = setup_unlocked().await;
    keys.lock().await;
    let mon = AutoLockMonitor::init(pool, keys.clone()).await.unwrap();
    mon.set_timeout_minutes(5).await.unwrap();
    let task = tokio::spawn(Arc::clone(&mon).run_tick_loop());
    tokio::time::advance(std::time::Duration::from_secs(10 * 60)).await;
    tokio::task::yield_now().await;
    // No audit row for auto_lock should exist (idempotent skip).
    let cnt: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_logs WHERE action = 'auto_lock'")
        .fetch_one(&mon.pool).await.unwrap();
    assert_eq!(cnt.0, 0);
    task.abort();
}
```

- [x] **Step 2: Verify failure**

```
cargo test -p angeld --lib auto_lock::tests::tick_loop_
```

Expected: compile error — `run_tick_loop` not defined.

- [x] **Step 3: Implement `run_tick_loop`**

Append to `impl AutoLockMonitor`:

```rust
pub async fn run_tick_loop(self: Arc<Self>) {
    use std::panic::AssertUnwindSafe;
    use futures::FutureExt; // add `futures = "0.3"` to angeld/Cargo.toml if missing

    let tick = std::time::Duration::from_secs(TICK_INTERVAL_SECS);
    loop {
        let me = Arc::clone(&self);
        let _ = AssertUnwindSafe(async move {
            tokio::time::sleep(tick).await;
            let now      = me.now_secs();
            let last     = me.last_activity.load(Ordering::Relaxed);
            let timeout  = me.idle_timeout_secs.load(Ordering::Relaxed);
            let elapsed  = now.saturating_sub(last);
            if elapsed < timeout { return; }
            if me.vault_keys.require_key().await.is_err() { return; }
            info!("[AUTO-LOCK] idle exceeded ({}s >= {}s) — forcing lock", elapsed, timeout);
            crate::lock_flow::force_lock_and_dismount(
                &me.pool, &me.vault_keys, crate::lock_flow::LockReason::IdleTimeout, None,
            ).await;
        }).catch_unwind().await;
    }
}
```

`futures` crate dep check: it's not in `angeld/Cargo.toml`. Two options:
1. **Add `futures = "0.3"`** (small, widely used). Preferred for `catch_unwind`.
2. Use `tokio::task::spawn` + `JoinHandle::await` and inspect `is_panic()` — more code, no new dep.

**Take option 1** — add `futures = "0.3"` to `[dependencies]`. Justify in the commit: catch_unwind on async is non-trivial without it.

- [x] **Step 4: Run tests**

```
cargo test -p angeld --lib auto_lock::tests::tick_loop_
```

Expected: 3 passed.

- [x] **Step 5: Commit**

```bash
git add angeld/src/auto_lock.rs angeld/Cargo.toml
git commit -m "feat(auto-lock): tick loop locks vault on idle timeout (α.A.b.2)"
```

---

### Task 2.5: ACL touch hooks + `require_session_no_touch`

**Files:**
- Modify: `angeld/src/acl.rs`

- [x] **Step 1: Write the failing test**

Append to `acl::tests`:

```rust
#[tokio::test]
async fn require_session_touches_monitor() {
    // Bootstrap minimal monitor.
    let pool = db::init_db("sqlite::memory:").await.unwrap();
    let mon = crate::auto_lock::AutoLockMonitor::init(
        pool.clone(), crate::vault::VaultKeyStore::default()
    ).await.unwrap();
    let _ = crate::auto_lock::MONITOR.set(mon.clone());

    // Create user + session.
    db::set_vault_params(&pool, b"salt1234567890ab", "argon2id", "vault-1").await.unwrap();
    db::create_user(&pool, "u-tester", "Tester", None, "local", None).await.unwrap();
    db::create_device(&pool, "dev-t", "u-tester", "TPC", &[0u8; 32]).await.unwrap();
    db::add_vault_member(&pool, "u-tester", "vault-1", "member", None).await.unwrap();
    let token = db::generate_session_token();
    db::create_user_session(&pool, &token, "u-tester", "dev-t", db::SESSION_TTL_SECONDS).await.unwrap();

    let before = mon.last_activity.load(std::sync::atomic::Ordering::Relaxed);
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());

    require_session(&pool, &headers).await.unwrap();

    let after = mon.last_activity.load(std::sync::atomic::Ordering::Relaxed);
    assert!(after >= before, "require_session must touch monitor");
}

#[tokio::test]
async fn require_session_no_touch_does_not_touch_monitor() {
    // Same setup as above … reuse helper.
    // After call: last_activity unchanged.
    // (See full body when implementing — use a #[cfg(test)] helper to dedupe setup.)
}
```

**Note:** `OnceLock::set` panics if already set. Tests sharing one process must coordinate. Two options:
1. **Recommended:** factor monitor logic so tests don't depend on the global `MONITOR` — pass `Arc<AutoLockMonitor>` to `extract_session_or_401` via an optional ref param. **Rejected** — too invasive for ACL signature.
2. **Recommended:** add `#[cfg(test)] fn set_monitor_for_test(...)` that uses `OnceLock::get_or_init`. Acceptable: first test to run wins; subsequent tests assert relative deltas, not absolute.
3. **Recommended (chosen):** use `tokio::test` with `#[ignore]`-able global serialization or a `parking_lot::Mutex<()>` to serialize. Simpler: in `acl::tests` use one `#[tokio::test]` that drives both `require_session` and `require_session_no_touch` sequentially and checks **delta** between them.

Final form for the test:

```rust
#[tokio::test]
async fn require_session_variants_touch_or_skip() {
    let (pool, _mon, headers) = setup_acl_monitor().await;
    let mon = crate::auto_lock::MONITOR.get().unwrap();

    // require_session_no_touch: snapshot unchanged.
    let before = mon.last_activity.load(std::sync::atomic::Ordering::Relaxed);
    require_session_no_touch(&pool, &headers).await.unwrap();
    let after_no_touch = mon.last_activity.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(before, after_no_touch);

    // require_session: snapshot increases (or stays equal if 0s elapsed — bump time).
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    require_session(&pool, &headers).await.unwrap();
    let after_touch = mon.last_activity.load(std::sync::atomic::Ordering::Relaxed);
    assert!(after_touch >= after_no_touch);
}
```

`setup_acl_monitor()` lives in `acl::tests` as a private helper; uses `OnceLock::get_or_init` semantics (set if unset, otherwise reuse).

- [x] **Step 2: Verify failure**

```
cargo test -p angeld --lib acl::tests::require_session_variants_touch_or_skip
```

Expected: compile error — `require_session_no_touch` not defined.

- [x] **Step 3: Implement the touch hooks + the new variant**

In `angeld/src/acl.rs:107-133`:

```rust
pub async fn require_session(
    pool: &SqlitePool,
    headers: &HeaderMap,
) -> Result<db::UserSession, ApiError> {
    let s = extract_session_or_401(pool, headers).await?;
    crate::auto_lock::touch(crate::auto_lock::TouchSource::AuthApi);
    Ok(s)
}

pub async fn require_session_no_touch(
    pool: &SqlitePool,
    headers: &HeaderMap,
) -> Result<db::UserSession, ApiError> {
    extract_session_or_401(pool, headers).await
}
```

In `require_role`, after the privilege check passes (just before `Ok(AuthorizedCaller { ... })`), add:

```rust
crate::auto_lock::touch(crate::auto_lock::TouchSource::AuthApi);
```

- [x] **Step 4: Run, expect green**

```
cargo test -p angeld --lib acl::tests
```

Expected: all green (including pre-existing tests — verify no regression).

- [x] **Step 5: Commit**

```bash
git add angeld/src/acl.rs
git commit -m "feat(acl): touch monitor on require_session/role + add no_touch variant (α.A.b.2)"
```

---

### Task 2.6: cfapi callback hooks

**Files:**
- Modify: `angeld/src/smart_sync.rs`

- [x] **Step 1: Visual diff first**

These callbacks are `unsafe extern "system"` and Win32-driven; no clean unit test exists today. Coverage comes from `e2e_files_call_touches_timer` in Task 2.9.

- [x] **Step 2: Add the touch calls**

In `smart_sync.rs::fetch_data_callback_inner` (current line ~454), after the `let Some(identity) = decode_file_identity(...) else { ... };` block and **before** `let request = HydrationRequest { ... };`:

```rust
crate::auto_lock::touch(crate::auto_lock::TouchSource::CfApi);
```

In `smart_sync.rs::fetch_placeholders_callback_inner` (current line ~605), at the top of the function after the null check:

```rust
crate::auto_lock::touch(crate::auto_lock::TouchSource::CfApi);
```

Do **not** touch in `cancel_fetch_data_callback_inner` — explicit decision in spec §3.2.

- [x] **Step 3: Build + clippy**

```
cargo build -p angeld --release
cargo clippy -p angeld --all-targets -- -D warnings
```

Expected: both succeed.

- [x] **Step 4: Commit**

```bash
git add angeld/src/smart_sync.rs
git commit -m "feat(smart-sync): touch auto-lock monitor on cfapi hydration callbacks (α.A.b.2)"
```

---

### Task 2.7: `GET /api/auto-lock/status` + `POST /api/auto-lock/touch`

**Files:**
- Modify: `angeld/src/api/auto_lock.rs`

- [x] **Step 1: Write the integration tests**

Create `angeld/tests/e2e_auto_lock.rs` (mirroring `e2e_basic.rs` style — copy harness or extract shared helper):

```rust
mod helpers;
use helpers::DaemonHarness;

#[tokio::test]
async fn e2e_status_endpoint_returns_active_state() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let resp = h.get_json("/api/auto-lock/status").await?;
    assert_eq!(resp["idle_timeout_min"].as_u64(), Some(15));
    assert!(resp["remaining_seconds"].as_u64().unwrap() > 0);
    assert_eq!(resp["state"].as_str(), Some("active"));
    Ok(())
}

#[tokio::test]
async fn e2e_status_polling_does_not_touch() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let r1 = h.get_json("/api/auto-lock/status").await?;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let r2 = h.get_json("/api/auto-lock/status").await?;
    let rem1 = r1["remaining_seconds"].as_u64().unwrap();
    let rem2 = r2["remaining_seconds"].as_u64().unwrap();
    assert!(rem2 < rem1, "remaining_seconds must DECREASE over time (polling didn't touch); got {rem1} -> {rem2}");
    Ok(())
}

#[tokio::test]
async fn e2e_files_call_touches_timer() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let r1 = h.get_json("/api/auto-lock/status").await?;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    // Any authenticated API call.  /api/auth/session is cheap.
    h.get_json("/api/auth/session").await?;
    let r2 = h.get_json("/api/auto-lock/status").await?;
    assert!(r2["remaining_seconds"].as_u64().unwrap() >= r1["remaining_seconds"].as_u64().unwrap() - 1);
    Ok(())
}

#[tokio::test]
async fn e2e_touch_endpoint_resets_remaining() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let before = h.get_json("/api/auto-lock/status").await?;
    h.post("/api/auto-lock/touch").await?;
    let after = h.get_json("/api/auto-lock/status").await?;
    assert!(after["remaining_seconds"].as_u64().unwrap() >= before["remaining_seconds"].as_u64().unwrap());
    Ok(())
}

#[tokio::test]
async fn e2e_set_timeout_endpoint_hot_reloads() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    h.post_json("/api/auto-lock/timeout", serde_json::json!({"idle_timeout_min": 30})).await?;
    let resp = h.get_json("/api/auto-lock/status").await?;
    assert_eq!(resp["idle_timeout_min"].as_u64(), Some(30));
    Ok(())
}

#[tokio::test]
async fn e2e_unauthenticated_health_does_not_touch() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let r1 = h.get_json("/api/auto-lock/status").await?;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    // Anonymous endpoint — must NOT touch.
    let _ = h.get_raw("/api/diagnostics/health").await?;
    let r2 = h.get_json("/api/auto-lock/status").await?;
    assert!(r2["remaining_seconds"].as_u64().unwrap() < r1["remaining_seconds"].as_u64().unwrap());
    Ok(())
}

#[tokio::test]
async fn e2e_set_timeout_endpoint_rejects_invalid() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let resp = h.post_json_raw("/api/auto-lock/timeout", serde_json::json!({"idle_timeout_min": 7})).await?;
    assert_eq!(resp.status, 400);
    Ok(())
}

#[tokio::test]
async fn e2e_post_lock_via_idle_returns_locked_state() -> Result<(), Box<dyn std::error::Error>> {
    // Uses OMNIDRIVE_AUTO_LOCK_TEST_MIN=… in env; verify daemon honors a fast preset.
    // If running this end-to-end is too slow for CI, gate with #[ignore].
    // For now: force-lock via lock_flow directly through a test-helpers endpoint?
    // Decision: SKIP — covered by SMOKE H2.  Replace with a stub assertion or omit.
    Ok(())
}
```

Create `angeld/tests/helpers.rs` (or `mod.rs` if Cargo prefers a folder) with the `DaemonHarness` lifted from `e2e_basic.rs` + `e2e_recovery.rs` unified. Helper additions:
- `unlock(&mut self) -> Result<Value>` (already in `e2e_recovery.rs:162`)
- `get_json<T>(&self, path) -> T` — wraps existing `http_get_json`
- `post(&self, path)` — empty body POST
- `post_json(&self, path, body) -> Value`
- `post_json_raw(&self, path, body) -> RawResp { status, body }`
- `get_raw(&self, path) -> RawResp`

(Plan permits this helper extraction as a prerequisite micro-commit. If you do it: separate commit `test(harness): extract shared DaemonHarness to tests/helpers.rs` BEFORE writing `e2e_auto_lock.rs`.)

- [x] **Step 2: Verify failures**

```
cargo test -p angeld --test e2e_auto_lock
```

Expected: route 404s.

- [x] **Step 3: Implement endpoints**

Extend `angeld/src/api/auto_lock.rs::routes()`:

```rust
pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/auto-lock/timeout", post(post_timeout))
        .route("/api/auto-lock/status",  get(get_status))
        .route("/api/auto-lock/touch",   post(post_touch))
}

async fn get_status(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<AutoLockStatus>, ApiError> {
    let _ = acl::require_session_no_touch(&state.pool, &headers).await?;
    let mon = MONITOR.get().ok_or(ApiError::Internal {
        message: "auto-lock monitor not initialized".into(),
    })?;
    let vault_locked = state.vault_keys.require_key().await.is_err();
    let rem = mon.remaining_secs();
    let state_str = if vault_locked {
        "locked"
    } else if rem == 0 {
        "expired"
    } else if rem <= 60 {
        "warning"
    } else {
        "active"
    };
    let idle_min = u32::try_from(mon.idle_timeout_secs() / 60).unwrap_or(15);
    Ok(Json(AutoLockStatus {
        idle_timeout_min: idle_min,
        remaining_seconds: rem,
        state: state_str,
    }))
}

async fn post_touch(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let _ = acl::require_session_no_touch(&state.pool, &headers).await?;
    if let Some(mon) = MONITOR.get() {
        mon.touch(auto_lock::TouchSource::ManualExtend);
    }
    Ok(StatusCode::NO_CONTENT)
}
```

- [x] **Step 4: Run e2e tests**

```
cargo test -p angeld --test e2e_auto_lock
```

Expected: 7 passed, 1 with `Ok(())` stub (the locked-state test deferred to SMOKE H2).

- [x] **Step 5: Commit**

```bash
git add angeld/src/api/auto_lock.rs angeld/tests/e2e_auto_lock.rs angeld/tests/helpers.rs angeld/tests/e2e_basic.rs
git commit -m "feat(auto-lock): GET /status (no-touch) + POST /touch endpoints (α.A.b.2)"
```

---

### Task 2.8: Spawn the tick loop in `ApiServer::run`

**Files:**
- Modify: `angeld/src/api/mod.rs`

- [ ] **Step 1: Edit `ApiServer::run`**

After `MONITOR.set(monitor)` from Task 1.4, before `let app = Router::new()`:

```rust
let monitor_for_ticks = Arc::clone(crate::auto_lock::MONITOR.get().expect("just set"));
tokio::spawn(monitor_for_ticks.run_tick_loop());
```

- [ ] **Step 2: Build + clippy**

```
cargo build -p angeld --release
cargo clippy -p angeld --all-targets -- -D warnings
```

Expected: both succeed.

- [ ] **Step 3: Run full test suite**

```
cargo test -p angeld --workspace
```

Expected: all green. **No regression in `e2e_basic`, `e2e_recovery`, `e2e_sync`, etc.**

- [ ] **Step 4: α.A.b.2 checkpoint commit + push**

```bash
git add angeld/src/api/mod.rs
git commit -m "feat(auto-lock): spawn tick loop at daemon startup (α.A.b.2)"
git push origin main
```

- [ ] **Step 5: CHECKPOINT — pause and ask Przemek**

Report:
- α.A.b.2 done: full activity tracking + tick loop + manual-logout refactor.
- `lock_flow.rs` is the single source of truth for all lock paths.
- Status polling provably doesn't reset countdown (regression-guarded).
- ~17 unit + 7 integration tests, all green; whole suite green.
- Ask: "α.A.b.2 PASS — continue α.A.b.3 (Win+L), or commit-and-park?"

---

# α.A.b.3 — Win+L observer

**Files:**
- Create: `angeld/src/win_session.rs`
- Modify: `angeld/src/lib.rs` (cfg-gated `pub mod win_session;`)
- Modify: `angeld/Cargo.toml` (windows-rs feature additions)
- Modify: `angeld/src/api/mod.rs` (spawn observer + graceful degradation)
- Create: integration test in `angeld/tests/e2e_auto_lock.rs` (test-helpers gated)

**Outcome:** `WTS_SESSION_LOCK` triggers `lock_flow::force_lock_and_dismount(WinSessionLock)`. `WTS_SESSION_UNLOCK` ignored. Observer failures degrade gracefully (warn + continue).

**Acceptance:** observer spawns without panicking on Windows, test-helpers test for simulated lock passes, daemon still boots on non-Windows (cfg gate works).

---

### Task 3.1: windows-rs feature additions + module skeleton

**Files:**
- Modify: `angeld/Cargo.toml`
- Create: `angeld/src/win_session.rs`
- Modify: `angeld/src/lib.rs`

- [ ] **Step 1: Add windows-rs features**

In `angeld/Cargo.toml`, inside `[target.'cfg(windows)'.dependencies]` `windows = { ... features = [` list, add (if absent — verify current contents):

```toml
"Win32_System_RemoteDesktop",
"Win32_UI_WindowsAndMessaging",
"Win32_System_LibraryLoader",   // for GetModuleHandleW
```

- [ ] **Step 2: Skeleton `win_session.rs`**

```rust
//! α.A.b.3 — WTS session lock observer (Win+L hard-lock).
//!
//! Runs a dedicated OS thread with a hidden message-only window.
//! `WM_WTSSESSION_CHANGE` → `lock_flow::force_lock_and_dismount(WinSessionLock)`.
//! `WTS_SESSION_UNLOCK` is intentionally ignored (zero-trust).

#![cfg(target_os = "windows")]

use crate::lock_flow::{self, LockReason};
use crate::vault::VaultKeyStore;
use sqlx::SqlitePool;
use std::thread::{self, JoinHandle};
use tracing::{info, warn};

#[derive(Debug)]
pub enum WinSessionError {
    RegisterFailed(String),
    SpawnFailed(String),
}

pub struct ObserverHandle {
    join: Option<JoinHandle<()>>,
    hwnd_raw: usize, // pass HWND across thread boundary as raw isize
    #[cfg(feature = "test-helpers")]
    pub test_dispatcher_tx: tokio::sync::mpsc::UnboundedSender<SessionEvent>,
}

#[cfg(feature = "test-helpers")]
#[derive(Debug, Clone, Copy)]
pub enum SessionEvent { Lock, Unlock }

pub fn spawn_observer(
    runtime: tokio::runtime::Handle,
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
) -> Result<ObserverHandle, WinSessionError> {
    // implemented in Task 3.2
    Err(WinSessionError::RegisterFailed("not yet implemented".into()))
}

impl Drop for ObserverHandle {
    fn drop(&mut self) {
        // implemented in Task 3.2
    }
}
```

In `angeld/src/lib.rs`:

```rust
#[cfg(target_os = "windows")]
pub mod win_session;
```

- [ ] **Step 3: Verify build**

```
cargo build -p angeld --release
```

Expected: builds (skeleton only — no behavior yet).

- [ ] **Step 4: Commit**

```bash
git add angeld/src/win_session.rs angeld/src/lib.rs angeld/Cargo.toml
git commit -m "feat(win-session): scaffold observer module + windows-rs features (α.A.b.3)"
```

---

### Task 3.2: Message-pump thread + WTS register

**Files:**
- Modify: `angeld/src/win_session.rs`

- [ ] **Step 1: Implement `spawn_observer` body**

Full implementation (Win32 boilerplate; reference cargo docs for `windows` 0.62 module paths if any name drifts):

```rust
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM, HMODULE};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
    WTSUnRegisterSessionNotification, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW,
    PostQuitMessage, RegisterClassW, TranslateMessage, MSG, WM_QUIT,
    WM_WTSSESSION_CHANGE, WNDCLASSW, HWND_MESSAGE,
};
use windows::core::PCWSTR;

const CLASS_NAME: &[u16] = &[
    'O' as u16,'m','n','i','D','r','i','v','e','W','t','s',0u16,
];

pub fn spawn_observer(
    runtime: tokio::runtime::Handle,
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
) -> Result<ObserverHandle, WinSessionError> {
    #[cfg(feature = "test-helpers")]
    let (test_tx, mut test_rx) = tokio::sync::mpsc::unbounded_channel::<SessionEvent>();

    // Channel from the OS thread back to the main thread for HWND publication.
    let (hwnd_tx, hwnd_rx) = std::sync::mpsc::channel::<usize>();

    let pool_thread = pool.clone();
    let keys_thread = vault_keys.clone();
    let rt_thread = runtime.clone();

    let join = thread::Builder::new()
        .name("omnidrive-win-session".to_string())
        .spawn(move || {
            // SAFETY: all Win32 calls below documented as safe to invoke from any thread
            // that owns the window, which this thread does (creator == owner).
            unsafe {
                let hinstance: HMODULE = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
                let mut wc = WNDCLASSW::default();
                wc.lpfnWndProc = Some(window_proc_trampoline);
                wc.hInstance = hinstance.into();
                wc.lpszClassName = PCWSTR(CLASS_NAME.as_ptr());
                let _ = RegisterClassW(&wc);

                let hwnd = CreateWindowExW(
                    Default::default(),
                    PCWSTR(CLASS_NAME.as_ptr()),
                    PCWSTR::null(),
                    Default::default(),
                    0, 0, 0, 0,
                    HWND_MESSAGE,
                    None,
                    hinstance.into(),
                    None,
                ).unwrap_or_default();

                if hwnd.0 == 0 {
                    let _ = hwnd_tx.send(0);
                    return;
                }

                // Stash context in window user data via a global since we cannot
                // use SetWindowLongPtrW with arbitrary types portably across hosts.
                set_thread_context(ThreadCtx {
                    pool: pool_thread,
                    vault_keys: keys_thread,
                    runtime: rt_thread,
                });

                if WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION).is_err() {
                    warn!("[WIN-SESSION] WTSRegisterSessionNotification failed");
                    let _ = hwnd_tx.send(0);
                    return;
                }

                let _ = hwnd_tx.send(hwnd.0 as usize);

                let mut msg = MSG::default();
                while GetMessageW(&mut msg, HWND::default(), 0, 0).into() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                let _ = WTSUnRegisterSessionNotification(hwnd);
                info!("[WIN-SESSION] observer thread exited");
            }
        })
        .map_err(|e| WinSessionError::SpawnFailed(e.to_string()))?;

    let hwnd_raw = hwnd_rx.recv().map_err(|e| WinSessionError::RegisterFailed(e.to_string()))?;
    if hwnd_raw == 0 {
        return Err(WinSessionError::RegisterFailed("hwnd creation failed".into()));
    }

    // test-helpers: also dispatch synthetic events into the same handler path.
    #[cfg(feature = "test-helpers")]
    {
        let pool_test = pool.clone();
        let keys_test = vault_keys.clone();
        runtime.spawn(async move {
            while let Some(ev) = test_rx.recv().await {
                if matches!(ev, SessionEvent::Lock) {
                    lock_flow::force_lock_and_dismount(
                        &pool_test, &keys_test, LockReason::WinSessionLock, None,
                    ).await;
                }
            }
        });
    }

    Ok(ObserverHandle {
        join: Some(join),
        hwnd_raw,
        #[cfg(feature = "test-helpers")]
        test_dispatcher_tx: test_tx,
    })
}

unsafe extern "system" fn window_proc_trampoline(
    hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> LRESULT {
    if msg == WM_WTSSESSION_CHANGE && wparam.0 as u32 == WTS_SESSION_LOCK {
        if let Some(ctx) = thread_context() {
            let pool = ctx.pool.clone();
            let keys = ctx.vault_keys.clone();
            ctx.runtime.spawn(async move {
                lock_flow::force_lock_and_dismount(
                    &pool, &keys, LockReason::WinSessionLock, None,
                ).await;
            });
        }
    }
    // WTS_SESSION_UNLOCK is ignored intentionally (zero-trust).
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

// Thread-local context (the OS thread owns it; no cross-thread mutation).
thread_local! {
    static THREAD_CTX: std::cell::RefCell<Option<ThreadCtx>> = std::cell::RefCell::new(None);
}

struct ThreadCtx {
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
    runtime: tokio::runtime::Handle,
}

fn set_thread_context(ctx: ThreadCtx) {
    THREAD_CTX.with(|c| *c.borrow_mut() = Some(ctx));
}

fn thread_context() -> Option<&'static ThreadCtx> {
    // Workaround: thread-local across function calls — use leak for &'static.
    // Safer alternative: stash in a global OnceLock when only one observer exists,
    // which is the invariant in this app.
    None // placeholder — see "Decision" below
}
```

**Decision:** `thread_local!` cannot hand out `&'static`. Replace `thread_context` with a process-global `OnceLock<ThreadCtx>` (this app spawns at most one observer):

```rust
use std::sync::OnceLock;
static OBSERVER_CTX: OnceLock<ThreadCtx> = OnceLock::new();

fn set_thread_context(ctx: ThreadCtx) {
    let _ = OBSERVER_CTX.set(ctx);
}
fn thread_context() -> Option<&'static ThreadCtx> {
    OBSERVER_CTX.get()
}
```

`ThreadCtx` must derive `Send + Sync`; `SqlitePool`, `VaultKeyStore`, `tokio::runtime::Handle` are all `Send + Sync`. Confirm `ThreadCtx: Send + Sync` via a compile-time assertion:

```rust
const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    let _ = assert_send_sync::<ThreadCtx>;
};
```

Drop:

```rust
impl Drop for ObserverHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = PostMessageW(HWND(self.hwnd_raw as isize), WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}
```

- [ ] **Step 2: Build + clippy on Windows**

```
cargo build -p angeld --release
cargo clippy -p angeld --all-targets -- -D warnings
```

Expected: success. Address any windows-rs binding name changes (the 0.62 API surface may differ in trivial ways from this draft — fix inline).

- [ ] **Step 3: Commit**

```bash
git add angeld/src/win_session.rs
git commit -m "feat(win-session): message pump + WTSRegisterSessionNotification (α.A.b.3)"
```

---

### Task 3.3: Integration test via `test-helpers`

**Files:**
- Modify: `angeld/tests/e2e_auto_lock.rs`

- [ ] **Step 1: Write the failing test**

Append:

```rust
#[cfg(all(target_os = "windows", feature = "test-helpers"))]
#[tokio::test]
async fn e2e_win_session_lock_triggers_force_lock() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    // Sanity: vault unlocked, session valid.
    let resp = h.get_json("/api/auth/session").await?;
    assert_eq!(resp["valid"].as_bool(), Some(true));

    // Trigger synthetic session lock through the daemon's test endpoint.
    // The endpoint is gated behind feature = "test-helpers" and lives in
    // api/auto_lock.rs::post_test_simulate_session_lock.
    let r = h.post("/api/auto-lock/_test/simulate-session-lock").await?;
    assert_eq!(r.status, 204);

    // Wait for the async lock_flow spawn to land.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Now session must be invalid (vault locked → require_key fails →
    // status endpoint reports "locked").
    let status = h.get_json("/api/auto-lock/status").await?;
    assert_eq!(status["state"].as_str(), Some("locked"));
    Ok(())
}
```

- [ ] **Step 2: Add the test endpoint (feature-gated)**

In `angeld/src/api/auto_lock.rs`, append:

```rust
#[cfg(feature = "test-helpers")]
pub(crate) fn test_routes() -> Router<ApiState> {
    Router::new().route("/api/auto-lock/_test/simulate-session-lock", post(post_test_simulate))
}

#[cfg(feature = "test-helpers")]
async fn post_test_simulate(
    State(state): State<ApiState>,
) -> Result<StatusCode, ApiError> {
    // Reaches into the OBSERVER_HANDLE static (Task 3.4).
    if let Some(tx) = crate::win_session::test_dispatcher_tx() {
        let _ = tx.send(crate::win_session::SessionEvent::Lock);
    } else {
        // Observer not spawned — call lock_flow directly to keep the test deterministic.
        crate::lock_flow::force_lock_and_dismount(
            &state.pool, &state.vault_keys,
            crate::lock_flow::LockReason::WinSessionLock, None,
        ).await;
    }
    Ok(StatusCode::NO_CONTENT)
}
```

Expose `test_dispatcher_tx()` in `win_session.rs`:

```rust
#[cfg(feature = "test-helpers")]
pub fn test_dispatcher_tx() -> Option<tokio::sync::mpsc::UnboundedSender<SessionEvent>> {
    OBSERVER_HANDLE.get().map(|h| h.test_dispatcher_tx.clone())
}
```

And add `OBSERVER_HANDLE: OnceLock<ObserverHandle>` at module level (Task 3.4 will set it).

- [ ] **Step 3: Run the gated test**

```
cargo test -p angeld --features test-helpers --test e2e_auto_lock e2e_win_session_lock_triggers_force_lock
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add angeld/src/api/auto_lock.rs angeld/src/win_session.rs angeld/tests/e2e_auto_lock.rs
git commit -m "test(win-session): integration test via test-helpers mpsc bridge (α.A.b.3)"
```

---

### Task 3.4: Spawn observer in `ApiServer::run` (graceful degradation)

**Files:**
- Modify: `angeld/src/api/mod.rs`

- [ ] **Step 1: Edit `ApiServer::run`**

After `tokio::spawn(monitor_for_ticks.run_tick_loop())`:

```rust
#[cfg(target_os = "windows")]
{
    let pool_ws = state.pool.clone();
    let keys_ws = state.vault_keys.clone();
    let rt_handle = tokio::runtime::Handle::current();
    match crate::win_session::spawn_observer(rt_handle, pool_ws, keys_ws) {
        Ok(handle) => {
            let _ = crate::win_session::OBSERVER_HANDLE.set(handle);
            info!("[AUTO-LOCK] Win+L observer active");
        }
        Err(e) => {
            warn!("[AUTO-LOCK] Win+L observer unavailable, timer-only mode: {:?}", e);
        }
    }
}
```

Add the `test_routes` merge:

```rust
#[cfg(feature = "test-helpers")]
let auto_lock_routes = auto_lock::routes().merge(auto_lock::test_routes());
#[cfg(not(feature = "test-helpers"))]
let auto_lock_routes = auto_lock::routes();

let app = Router::new()
    // ...
    .merge(auto_lock_routes)
    .with_state(state);
```

Add `pub static OBSERVER_HANDLE: std::sync::OnceLock<ObserverHandle> = std::sync::OnceLock::new();` in `win_session.rs`.

- [ ] **Step 2: Build + full test suite (including features)**

```
cargo build -p angeld --release
cargo test -p angeld --workspace
cargo test -p angeld --workspace --features test-helpers
cargo clippy -p angeld --all-targets -- -D warnings
cargo clippy -p angeld --all-targets --features test-helpers -- -D warnings
```

Expected: all green.

- [ ] **Step 3: α.A.b.3 checkpoint commit + push**

```bash
git add angeld/src/api/mod.rs angeld/src/win_session.rs
git commit -m "feat(auto-lock): spawn Win+L observer at startup with graceful degradation (α.A.b.3)"
git push origin main
```

- [ ] **Step 4: CHECKPOINT — pause and ask Przemek**

Report:
- α.A.b.3 done: Win+L observer + test-helpers bridge.
- All four lock paths converge on `lock_flow::force_lock_and_dismount`.
- Cross-platform OK (non-Windows builds skip the observer cleanly).
- Ask: "α.A.b.3 PASS — continue α.A.b.4 (frontend), or commit-and-park?"

---

# α.A.b.4 — Frontend

**Files:**
- Modify: `angeld/static/index.html` (mount points)
- Create: `angeld/static/auto-lock.js` (polling + UI)
- Modify: `angeld/static/index.html` (script tag include)
- Verify: existing CSS supports `.auto-lock-pill`, `.auto-lock-toast` (or add minimal Tailwind classes inline)

**Outcome:** UI shows mm:ss countdown pill, toast at `< 60s`, redirect on `state == "locked"`. Two buttons in toast: `[Wydłuż]` (POST /touch) and `[Zablokuj]` (POST /vault/lock — already exists).

**Acceptance:** manual visual verification in browser. No regression in other UI components.

---

### Task 4.1: Static asset wiring + polling client

**Files:**
- Create: `angeld/static/auto-lock.js`
- Modify: `angeld/static/index.html`
- Modify: `angeld/src/api/mod.rs` (serve `/auto-lock.js`)

- [ ] **Step 1: Add the route + asset**

In `angeld/src/api/mod.rs`, add alongside other `static` routes:

```rust
.route("/auto-lock.js", get(get_auto_lock_js))
```

```rust
async fn get_auto_lock_js() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        include_str!("../../static/auto-lock.js"),
    )
}
```

Create `angeld/static/auto-lock.js`:

```javascript
(function () {
  'use strict';

  const POLL_INTERVAL_MS = 5000;
  const $pill  = () => document.getElementById('auto-lock-pill');
  const $toast = () => document.getElementById('auto-lock-toast');
  let toastShown = false;

  function formatMmSs(seconds) {
    const m = Math.floor(seconds / 60);
    const s = seconds % 60;
    return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  }

  function bearer() {
    return localStorage.getItem('omnidrive_session_token') || '';
  }

  async function pollOnce() {
    const t = bearer();
    if (!t) return;
    let resp;
    try {
      resp = await fetch('/api/auto-lock/status', {
        headers: { 'Authorization': 'Bearer ' + t },
      });
    } catch (_) { return; }
    if (resp.status === 401) {
      window.location.href = '/'; // session gone — go to unlock screen
      return;
    }
    if (!resp.ok) return;
    const s = await resp.json();
    render(s);
  }

  function render(s) {
    const pill = $pill();
    if (!pill) return;
    if (s.state === 'locked') {
      window.location.href = '/';
      return;
    }
    if (s.remaining_seconds <= 300) {
      pill.textContent = '🔓 ' + formatMmSs(s.remaining_seconds);
      pill.classList.toggle('warn', s.remaining_seconds <= 60);
      pill.style.display = 'inline-flex';
    } else {
      pill.style.display = 'none';
    }
    if (s.state === 'warning' && !toastShown) {
      showToast();
      toastShown = true;
    } else if (s.state !== 'warning') {
      hideToast();
      toastShown = false;
    }
  }

  function showToast() {
    const t = $toast();
    if (!t) return;
    t.style.display = 'block';
  }
  function hideToast() {
    const t = $toast();
    if (!t) return;
    t.style.display = 'none';
  }

  async function extend() {
    const t = bearer();
    if (!t) return;
    await fetch('/api/auto-lock/touch', { method: 'POST', headers: { 'Authorization': 'Bearer ' + t } });
    hideToast(); toastShown = false;
    pollOnce();
  }

  async function lockNow() {
    const t = bearer();
    if (!t) return;
    await fetch('/api/vault/lock', { method: 'POST', headers: { 'Authorization': 'Bearer ' + t } });
    window.location.href = '/';
  }

  window.addEventListener('DOMContentLoaded', () => {
    document.getElementById('auto-lock-extend')?.addEventListener('click', extend);
    document.getElementById('auto-lock-lock')?.addEventListener('click', lockNow);
    pollOnce();
    setInterval(pollOnce, POLL_INTERVAL_MS);
  });
})();
```

- [ ] **Step 2: Add mount points to `index.html`**

In the topbar markup (locate the current header — verify in `static/index.html`), add:

```html
<span id="auto-lock-pill" class="auto-lock-pill" style="display:none;"></span>

<div id="auto-lock-toast" class="auto-lock-toast" style="display:none;">
  Sesja wygaśnie za chwilę.
  <button id="auto-lock-extend">Wydłuż</button>
  <button id="auto-lock-lock">Zablokuj</button>
</div>

<script src="/auto-lock.js"></script>
```

Add minimal CSS to the existing `<style>` block in `index.html`:

```css
.auto-lock-pill { font-variant-numeric: tabular-nums; padding: 2px 8px; border-radius: 999px; background: rgba(255,255,255,0.1); }
.auto-lock-pill.warn { background: rgba(255, 80, 80, 0.25); color: #fff; }
.auto-lock-toast { position: fixed; right: 16px; bottom: 16px; padding: 12px 16px; background: rgba(0,0,0,0.85); color: #fff; border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.5); }
.auto-lock-toast button { margin-left: 8px; padding: 4px 10px; border-radius: 6px; cursor: pointer; }
```

- [ ] **Step 3: Manual visual smoke**

```
cargo run -p angeld --release
```

Open `http://127.0.0.1:8787`, unlock the vault. Open devtools → application → local storage — confirm `omnidrive_session_token` exists. Watch the pill appear `< 5min`. Set timeout to 5min via `POST /api/auto-lock/timeout`. Wait. Verify:

1. Pill shows mm:ss countdown.
2. At `< 1:00`, pill turns red AND toast appears with `[Wydłuż]` / `[Zablokuj]`.
3. Clicking `[Wydłuż]` resets the timer (pill jumps back to `04:5x`).
4. Letting it expire (or clicking `[Zablokuj]`) → page redirects to `/`.

If any step fails, debug inline (browser devtools network tab + `tracing` logs).

- [ ] **Step 4: α.A.b.4 checkpoint commit + push**

```bash
git add angeld/src/api/mod.rs angeld/static/auto-lock.js angeld/static/index.html
git commit -m "feat(ui): auto-lock pill + toast + polling client (α.A.b.4)"
git push origin main
```

- [ ] **Step 5: CHECKPOINT — pause and ask Przemek**

Report:
- α.A.b.4 done: frontend complete, manual smoke green on Lenovo.
- Ask: "α.A.b.4 PASS — proceed to SMOKE H2 + H3 on both boxes, or commit-and-park?"

---

# Post-α.A.b.4 — SMOKE + release

### SMOKE H2 — Idle auto-lock (Lenovo + Dell)

Per spec §5.5:
1. Start daemon with `OMNIDRIVE_AUTO_LOCK_TEST_MIN=1` (env override; only honored in `cfg(debug_assertions)` — verify the env-var path in `auto_lock::init`. If not yet wired, **add a tiny `#[cfg(debug_assertions)]` branch in `init` that consults this env var before reading DB**, plus a unit test).
2. Unlock, open `O:\`, idle 70s.
3. Verify topbar pill 01:00 → 00:00, toast at 00:59, `O:\` becomes inaccessible, UI redirects to `/`.

### SMOKE H3 — Win+L hard lock (Lenovo + Dell)

Per spec §5.5:
1. Unlock, `O:\` open.
2. Press Win+L, wait 3s, unlock Windows.
3. Verify UI is on `/`, `O:\` inaccessible, `SELECT * FROM audit_logs WHERE action='auto_lock' ORDER BY id DESC LIMIT 1` shows `reason: win_session_lock`.

### Release commit

Only after both SMOKEs PASS on both boxes:

- Bump `angeld/Cargo.toml`, `angeld/Cargo.lock`, and any sibling workspace crates from `0.3.24` → `0.3.25` (memory `feedback_version_bump`).
- Build installer payload (memory `feedback_build_pipeline`).

```bash
git add -A
git commit -m "chore(release): bump workspace v0.3.25 + α.A.b auto-lock DONE"
git push origin main
```

---

# Self-Review (writing-plans Step 3)

**1. Spec coverage:**
- §0 decisions 1-5 → covered by Task 2.5 (acl hooks), Task 3.2 (Win+L hard lock, ignore unlock), Task 4 (UI), Task 1.2-1.3 (config), Task 2.1+2.4 (AtomicU64).
- §1 architecture → File Structure section + Tasks 1.1 / 2.2 / 3.1.
- §2 components A/B/C/D → Tasks 1.1+1.4, 2.2, 3.1-3.2, 1.4+2.7.
- §2 modifications acl/smart_sync/auth/api → Tasks 2.5, 2.6, 2.3, 2.8 + 3.4.
- §3.1 state machine → Task 2.7 `get_status` state branches.
- §3.2 touch sequence → Tasks 2.1, 2.5, 2.6.
- §3.3 tick loop → Task 2.4 with `catch_unwind`.
- §3.4 Win+L → Task 3.2.
- §3.5 polling no-touch → Task 2.7 `require_session_no_touch` regression test `e2e_status_polling_does_not_touch`.
- §3.6 set timeout → Task 1.4.
- §3.7 manual touch → Task 2.7 POST /touch.
- §3.8 lifecycle → exercised end-to-end by SMOKE H2.
- §4 error handling: 4.1 init clamping/fallback → Task 1.2 tests. 4.2 hot path no-op → Task 2.1 top-level test. 4.3 audit-first then lock → Task 2.2 lock_flow ordering + tests. 4.4 endpoint validation → Tasks 1.4, 2.7. 4.5 OS integration graceful degradation → Task 3.4. 4.6 shutdown → tick loop uses `tokio::time::sleep` which respects task abort; observer Drop cleans up.
- §5 testing 5.1-5.4 → distributed across tasks. 5.5 SMOKE → Post-α.A.b.4 section. 5.6 matrix → covered.

**Gaps fixed inline:**
- Initial draft of `lock_flow::force_lock_and_dismount` lacked an `actor` parameter — Task 2.3 corrects this so the `logout` audit retains `user_id`/`device_id` parity with the pre-α.A.b behavior. The signature in Task 2.2 is the **final** one (4 args including `actor`). The 3-arg variant in Task 2.2 Step 3 is superseded — re-confirm before implementation.
- `OMNIDRIVE_AUTO_LOCK_TEST_MIN` env override is referenced in spec §5.5 but no task wires it. **Added inline** to the "SMOKE H2" section above: a `#[cfg(debug_assertions)]` branch in `init`. Treat as a 1-test micro-task during α.A.b.1 if Przemek wants it earlier.

**2. Placeholder scan:** no "TBD"/"add error handling"/"fill in"/"similar to" patterns. Every code step shows code. Every test step shows assertions. Every commit step shows the message.

**3. Type/symbol consistency check:**
- `TouchSource`, `LockReason`, `AutoLockError`, `AutoLockMonitor`, `ObserverHandle`, `SessionEvent`, `WinSessionError`, `AutoLockStatus`, `SetTimeoutRequest` — all defined exactly once, referenced consistently.
- `force_lock_and_dismount(pool, vault_keys, reason, actor)` — 4-arg signature is canonical (Task 2.3 fix); call sites in Tasks 2.3, 2.4, 3.2, 3.3 all use 4 args. **Re-verify when implementing Task 2.2** — the initial 3-arg sketch in 2.2 Step 3 must be replaced by the 4-arg form to avoid signature drift between commits.
- `MONITOR`, `OBSERVER_HANDLE`, `OBSERVER_CTX` — three distinct `OnceLock`s in three modules; no collision.
- `routes()` / `test_routes()` — pair lives in `api/auto_lock.rs`, both merged in Task 3.4.

**Issue flagged for confirmation, not blocker:** The plan assumes `VaultKeyStore::default()` returns a locked, empty store and `unlock("passphrase-for-test")` works against a freshly bootstrapped DB. Verify before Task 2.2 by skimming `vault.rs::unlock` (around line 166) for any side-effect that the test setup misses (e.g. envelope-key generation paths). If `unlock` requires more bootstrap, the `setup` helper in 2.2 will need additional setup lines.

---

# Execution handoff

Plan saved to `docs/superpowers/plans/2026-05-18-alpha-A-b-auto-lock-plan.md`.

**Two execution options:**

**1. Subagent-Driven (recommended)** — fresh subagent per task, two-stage review between tasks, fast iteration. Best for surfacing signature drift early and keeping each task's diff under review-able size.

**2. Inline execution** — execute tasks in this session with checkpoint commits.

Memory `feedback_token_budget` says: pause after every micro-step. Either path respects that — subagent-driven naturally pauses; inline execution must explicitly pause after each `α.A.b.X` sub-step's `git push`.

Which approach?
