# Spec: Auto-Lock Vault (α.A.b / P2-004)

**Data:** 2026-05-18
**Status:** ✅ ZATWIERDZONY (Przemek, sesja 2026-05-18 popołudnie)
**Roadmap ID:** α.A.b (sub-kroki α.A.b.1 — α.A.b.4)
**Issue:** P2-004
**Branch:** main (HEAD `be66897` w momencie zatwierdzenia)
**Wersja workspace:** 0.3.24 (pozostaje do zakończenia α.A.b)

---

## 0. Kontekst i decyzje wejściowe

α.A.a wprowadziła ścieżkę manual logout z pełnym CF + virtual-drive teardown (`api/auth.rs::post_auth_logout`). α.A.b dodaje **automatyczne** wykrywanie nieaktywności użytkownika i hard-lock po Win+L. Wszystkie 4 kluczowe decyzje + optymalizacja AtomicU64 są **finalne** i nie podlegają renegocjacji bez zgody Przemka.

### Decyzje finalne

1. **Activity scope (α.A.b.2)** — Touch monitora wywołują:
   - Każdy **authenticated API call** (hook w `acl::require_session` i `acl::require_role`)
   - **cfapi O:\ events** w hydration path (`smart_sync.rs` callbacks `fetch_data_callback_inner` + `fetch_placeholders_callback_inner`)
   - NIE liczą: anonimowe `/api/health`, `GET /api/auto-lock/status` polling, `cancel_fetch_data_callback`, Windows global input

2. **Win+L behavior (α.A.b.3)** — Hard lock. `WTSSESSION_CHANGE`/`WTS_SESSION_LOCK` → ta sama ścieżka co `post_auth_logout`. `WTS_SESSION_UNLOCK` jest **ignorowany** (zero-trust: powrót do Windowsa nie reaktywuje vaulta).

3. **UI (α.A.b.4)** — Topbar pill `🔓 14:32` (mm:ss countdown przy `remaining < 5 min`); toast warning `< 1:00` z przyciskami `[Wydłuż]`/`[Zablokuj]`; polling `GET /api/auto-lock/status` co **5s** (bez SSE).

4. **Config (α.A.b.1)** — Presety **5 / 15 / 30 / 60 minut** (default 15). Brak opcji "Nigdy". Storage: tabela `system_config`, klucz `vault.auto_lock_idle_min`. Hot-reload via `AtomicU64 idle_timeout_secs` w monitorze (zmiana UI → `POST /api/auto-lock/timeout` → DB update + atomic store, bez restartu).

5. **Wait-free hot path** — `last_activity` i `idle_timeout_secs` w `AutoLockMonitor` są `AtomicU64` (sekundy od `daemon_start`), NIE `Mutex<Instant>`. Powód: cfapi callbacks generują masowy fan-out przy I/O na O:\ — `Mutex` contention zabija SLA `watcher < 1% CPU idle`. `Relaxed` ordering wystarcza (brak koordynacji wielu zmiennych, stale read ≤ 10s = security-first OK).

---

## §1 Architektura — zatwierdzona

**Approach 1: Centralized AutoLockMonitor (single source of truth dla idle state).**

```
┌──────────────────────────────────────────────────────────────────────┐
│  angeld (tokio runtime)                                              │
│                                                                      │
│  ┌─────────────────────────┐                                         │
│  │  AutoLockMonitor        │ ←── shared Arc<>, global OnceLock       │
│  │  - last_activity:       AtomicU64 (sec since daemon_start)        │
│  │  - idle_timeout_secs:   AtomicU64 (hot-reloadable)                │
│  │  - daemon_start:        tokio::time::Instant (pause-aware)        │
│  │  - vault_keys:          VaultKeyStore                             │
│  │  - pool:                SqlitePool                                │
│  └─────────────────────────┘                                         │
│       ▲             ▲             ▲                                  │
│       │ touch()     │ touch()     │ force_lock()                     │
│       │             │             │                                  │
│   acl middleware  smart_sync     win_session_hook                    │
│   (auth API)      (cfapi events) (WTSSESSION_CHANGE / SessionLock)   │
│                                                                      │
│  ┌─────────────────────────┐                                         │
│  │  Tokio task: tick loop  │  co 10s:                                │
│  │  loop { sleep(10s);     │   if elapsed >= timeout                 │
│  │    check_and_lock() }   │   && vault still unlocked → force_lock  │
│  └─────────────────────────┘                                         │
└──────────────────────────────────────────────────────────────────────┘

Storage:                            API endpoints (nowe):
  system_config table               GET  /api/auto-lock/status
    key='vault.auto_lock_idle_min'    → { idle_timeout_min, remaining_seconds, state }
    value='15' (default)              POST /api/auto-lock/timeout
                                        { idle_timeout_min: 5|15|30|60 }
                                      POST /api/auto-lock/touch
                                        (przycisk Wydłuż w toast)
```

**Reuse z α.A.a:** wszystkie ścieżki lock (manual logout, idle timeout, Win+L, manual user action) przechodzą przez wspólną funkcję `lock_flow::force_lock_and_dismount(reason: LockReason)`. Zero duplikacji teardown.

---

## §2 Components — moduły, granice, interfejsy

### Nowe pliki (4)

#### A. `angeld/src/auto_lock.rs` — centralny monitor (core logic, OS-agnostic)

```rust
pub struct AutoLockMonitor {
    last_activity: AtomicU64,          // sekundy od daemon_start
    idle_timeout_secs: AtomicU64,      // hot-reloadable (default 900 = 15 min)
    daemon_start: tokio::time::Instant,
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
}

pub static MONITOR: OnceLock<Arc<AutoLockMonitor>> = OnceLock::new();

#[derive(Copy, Clone, Debug)]
pub enum TouchSource { AuthApi, CfApi, ManualExtend }

#[derive(Debug, thiserror::Error)]
pub enum AutoLockError {
    #[error("init failed: {0}")] Init(#[from] sqlx::Error),
    #[error("invalid preset: {0} (allowed: 5,15,30,60)")] InvalidPreset(u32),
    #[error("monitor already initialized")] DoubleInit,
}

impl AutoLockMonitor {
    pub async fn init(pool: SqlitePool, vault_keys: VaultKeyStore)
        -> Result<Arc<Self>, AutoLockError>;
    pub fn now_secs(&self) -> u64;                          // daemon_start.elapsed().as_secs()
    pub fn touch(&self, source: TouchSource);               // wait-free store(Relaxed)
    pub fn remaining_secs(&self) -> u64;                    // saturating_sub
    pub fn idle_timeout_secs(&self) -> u64;
    pub async fn set_timeout_minutes(&self, m: u32) -> Result<(), AutoLockError>;
    pub async fn force_lock(self: &Arc<Self>);              // delegacja do lock_flow
    pub async fn run_tick_loop(self: Arc<Self>);            // tokio::spawn target
}

// Top-level helpers dla call sites (no-op gdy MONITOR uninit):
pub fn touch(source: TouchSource);
pub fn routes() -> axum::Router<crate::api::ApiState>;   // re-export z api/auto_lock.rs
```

**Why OnceLock zamiast `ApiState`:** hook w `acl::extract_session_or_401` i w `smart_sync::fetch_data_callback_inner` (Win32 callback bez dostępu do ApiState). Spójne z istniejącym wzorcem `HYDRATION_CONTEXT: OnceLock<HydrationContext>` w `smart_sync.rs:484`. Zero zmian sygnatur 30+ handlerów API.

#### B. `angeld/src/lock_flow.rs` — wspólna ścieżka force-lock (DRY z α.A.a)

```rust
#[derive(Copy, Clone, Debug)]
pub enum LockReason { Logout, IdleTimeout, WinSessionLock, ManualUserAction }

pub async fn force_lock_and_dismount(
    pool: &SqlitePool,
    vault_keys: &VaultKeyStore,
    reason: LockReason,
) -> bool {
    // 1. snapshot was_unlocked = require_key().is_ok()
    // 2. audit log emission (PRZED lock — wymaga unlocked vault; fail = warn + continue)
    // 3. vault_keys.lock().await  (P1-006: zero plaintext keys in RAM)
    // 4. if was_unlocked → tokio::spawn(dismount_after_lock + unmount_virtual_drive)
    // 5. return was_unlocked
}
```

**Ekstrakt z** `api/auth.rs:215-255` (post_auth_logout — α.A.a). Po refactorze `post_auth_logout` wywoła `lock_flow::force_lock_and_dismount(..., LockReason::Logout)` i zostawi tylko session delete + own response.

#### C. `angeld/src/win_session.rs` — observer Win+L (`cfg(target_os = "windows")`)

```rust
pub fn spawn_observer(
    handle: tokio::runtime::Handle,        // explicit, NIE Handle::current() w OS thread
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
) -> Result<ObserverHandle, WinSessionError>;
//   - dedykowany OS thread (NIE tokio task — message pump wymaga jednowątkowego loopa)
//   - tworzy hidden message-only window (HWND_MESSAGE)
//   - WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)
//   - GetMessageW loop → WM_WTSSESSION_CHANGE
//   - wParam == WTS_SESSION_LOCK → handle.spawn(
//         lock_flow::force_lock_and_dismount(pool, vault_keys, LockReason::WinSessionLock)
//       )
//   - wParam == WTS_SESSION_UNLOCK → IGNORE (zero-trust)

pub struct ObserverHandle {
    join: JoinHandle<()>,
    hwnd: HWND,
    #[cfg(feature = "test-helpers")]
    test_dispatcher_tx: mpsc::Sender<SessionEvent>,
}
impl Drop for ObserverHandle {
    // PostMessage(WM_QUIT) + WTSUnRegisterSessionNotification + join thread (2s timeout)
}
```

**Dependencies:** `windows` crate features `Win32_System_RemoteDesktop`, `Win32_UI_WindowsAndMessaging`.

#### D. `angeld/src/api/auto_lock.rs` — REST layer

```rust
pub fn routes() -> Router<ApiState>;
//   GET  /api/auto-lock/status         ACL: require_session_no_touch  → AutoLockStatus
//   POST /api/auto-lock/timeout        ACL: require_session           Body: SetTimeoutRequest
//   POST /api/auto-lock/touch          ACL: require_session_no_touch  → 204

#[derive(Serialize)]
struct AutoLockStatus {
    idle_timeout_min: u32,
    remaining_seconds: u64,
    state: &'static str,   // "active" | "warning" | "expired" | "locked"
}

#[derive(Deserialize)]
struct SetTimeoutRequest { idle_timeout_min: u32 }
```

Wszystkie 3 endpointy są dostępne dla każdej autentykowanej sesji (per-user, NIE Role::Member).

### Modyfikacje istniejących plików (4)

| Plik | Zmiana | Linia (HEAD `be66897`) |
|---|---|---|
| `acl.rs` | rozdziel ACL: `require_session` touchuje, **nowy** `require_session_no_touch` pomija touch (używany przez `GET /status` polling). `require_role` touchuje. | `:107-133` |
| `smart_sync.rs` | w `fetch_data_callback_inner` po `decode_file_identity` = `Some` → `auto_lock::touch(CfApi)`. Identycznie w `fetch_placeholders_callback_inner`. Pomijamy `cancel_fetch_data_callback`. | `:454-467`, `:605+` |
| `api/auth.rs::post_auth_logout` | refactor: zastąp inline cleanup `lock_flow::force_lock_and_dismount(LockReason::Logout)` | `:193-264` |
| `api/mod.rs::ApiServer::run` | init monitor + spawn tick loop + spawn win_session observer + `.merge(auto_lock::routes())` | `:233-274` |

### Mapa zależności modułów

```
              ┌────────────────────────────┐
              │  auto_lock.rs (core)       │
              │  - AtomicU64 state         │
              │  - tick loop               │
              └──────────┬─────────────────┘
                         │ wywołuje force_lock()
                         ▼
              ┌────────────────────────────┐
              │  lock_flow.rs              │
              │  - vault_keys.lock()       │
              │  - smart_sync::dismount    │
              │  - virtual_drive::unmount  │
              └────────────────────────────┘
                         ▲           ▲
                         │           │
        ┌────────────────┘           └────────────────┐
        │                                              │
┌───────────────────┐                       ┌─────────────────────┐
│ win_session.rs    │                       │ api/auth.rs         │
│ (WTSSESSION)      │                       │ (post_auth_logout)  │
└───────────────────┘                       └─────────────────────┘

   touch() hooks (read-only side):
   acl.rs ────────────┐
   smart_sync.rs ─────┼──► auto_lock::touch() ──► AtomicU64.store
   api/auto_lock.rs ──┘    (no-op gdy OnceLock uninit)
```

### Co NIE jest częścią α.A.b

- **DB migracja** — istniejąca tabela `system_config` (`db.rs::get/set_system_config_value` już są).
- **Server-Sent Events** dla statusu — odrzucone, polling 5s wystarcza.
- **UI komponenty** (topbar pill, toast) — to α.A.b.4 (frontend); backend dostarcza tylko REST.

---

## §3 Data flow — sekwencje, maszyna stanowa, semantyka odliczania

### 3.0 Fundament: jakie sekundy mierzymy

`last_activity` i `now_secs()` to **sekundy monotoniczne od `daemon_start`** (`tokio::time::Instant::elapsed().as_secs()`). NIE wall-clock — odporne na zmianę zegara systemowego, nigdy nie cofają się. Rozmiar `u64`: ~584 mld lat — rollover praktycznie niemożliwy.

`AtomicU64::Relaxed` wystarcza, bo nie koordynujemy wielu zmiennych. Stale read ≤ 10s drift w wykryciu → security-first OK.

### 3.1 Maszyna stanowa monitora

Monitor sam stanu **NIE trzyma** — czyta z dwóch źródeł: `vault_keys.require_key()` (Ok/Err(Locked)) i `last_activity + idle_timeout_secs` (remaining_secs).

```
                  unlock (α.A.a)
        ┌─────────────────────────────────┐
        ▼                                 │
   ┌─────────┐    touch          ┌──────────────┐
   │ ACTIVE  │◄──────────────────│  WARNING     │
   │ rem>60s │                   │  0<rem<=60s  │
   └─────────┘    rem drops      └──────────────┘
        │  to <=60                      │
        │                               │ rem reaches 0
        │ force_lock  ┌──────────┐      │ (max 10s przed
        │  + dismount │ EXPIRED  │◄─────┘  najbliższym tickiem)
        │             │ transient│
        │             └──────────┘
        ▼                  │
   ┌─────────┐ tick detects expired
   │ LOCKED  │◄────────────┘
   │vault Err│         force_lock + dismount
   │(Locked) │
   └─────────┘
        │
        │ touch w stanie LOCKED nadal aktualizuje last_activity
        │ (no-op effect), tick loop sprawdza require_key i jeśli
        │ locked → continue (idempotent)
```

Server-derived `AutoLockStatus.state` dla GET /status:
- `vault_locked` → `"locked"` (UI redirect na /unlock)
- unlocked && `remaining > 60` → `"active"`
- unlocked && `0 < remaining <= 60` → `"warning"` (UI toast)
- unlocked && `remaining == 0` → `"expired"` (transient, ≤ 10s)

### 3.2 Touch sequence

```
Auth API call                              cfapi hydration callback
─────────────────                          ─────────────────────────
HTTP /api/files/...                        Windows kernel → CfApi
       │                                          │
       ▼                                          ▼
  acl::require_session                     fetch_data_callback_inner
  ├─ parse Bearer token                    ├─ decode_file_identity
  ├─ db::validate_user_session             ├─ if identity = None: skip touch
  │   = Ok(Some(session))                  │
  ▼                                        ▼
  auto_lock::touch(AuthApi)              auto_lock::touch(CfApi)
       │                                        │
       └───────────────────┬────────────────────┘
                           ▼
       MONITOR.get()?.last_activity.store(now_secs, Relaxed)
                           │
                           └─► WAIT-FREE: ~2 ns atomic store.
                               No-op gdy OnceLock uninit (testy, pre-startup).
```

**Co NIE touchuje:**
- `/api/health` (anonymous, brak ACL)
- `GET /api/auto-lock/status` (specjalna ścieżka `require_session_no_touch`)
- `POST /api/auto-lock/touch` przez ACL (ale handler explicit wywołuje `MONITOR.touch(ManualExtend)`)
- Token validation FAIL
- `decode_file_identity` → `None`
- `cancel_fetch_data_callback`

### 3.3 Tick loop

```rust
pub async fn run_tick_loop(self: Arc<Self>) {
    let tick = Duration::from_secs(10);
    loop {
        // catch_unwind wokół iteracji — restart on panic (audit 4.2)
        let me = Arc::clone(&self);
        let _ = AssertUnwindSafe(async move {
            tokio::time::sleep(tick).await;
            let now      = me.now_secs();
            let last     = me.last_activity.load(Ordering::Relaxed);
            let timeout  = me.idle_timeout_secs.load(Ordering::Relaxed);
            let elapsed  = now.saturating_sub(last);
            if elapsed < timeout { return; }
            if me.vault_keys.require_key().await.is_err() { return; }
            info!("[AUTO-LOCK] idle exceeded ({elapsed}s >= {timeout}s) — forcing lock");
            me.force_lock().await;
        }).catch_unwind().await;
    }
}
```

**Precyzja:** worst-case lock fires `timeout + 10s` po ostatnim touch.

### 3.4 Win+L sequence

```
User → press Win+L
       │
       ▼
Windows Session Manager → WTS_SESSION_LOCK broadcast
       │
       ▼
win_session.rs (dedicated OS thread, message pump)
   │ GetMessageW → WM_WTSSESSION_CHANGE (wParam=0x7)
   ▼
handle.spawn(async move {
    lock_flow::force_lock_and_dismount(
        &pool, &vault_keys, LockReason::WinSessionLock
    ).await;
});
```

`WTS_SESSION_UNLOCK` (wParam=0x8) jest **ignorowany**. Powrót do Windowsa wymaga explicit `POST /api/unlock` z passphrase.

### 3.5 UI polling sequence

```
Frontend (5s interval)              Backend
─────────────────────               ────────
setInterval(5000)                   GET /api/auto-lock/status
  → fetch                              │
                                       │ ACL: require_session_no_touch (!)
                                       │      ← NIE touchuje monitora
                                       ▼
                                    MONITOR.get()?
                                       │
                                       ├─ vault_locked = require_key.is_err()
                                       ├─ rem = remaining_secs()
                                       ▼
                                    AutoLockStatus { ... }
  ◄─── 200 OK JSON
  │
  ├─ if state == "warning" → show toast
  ├─ if state == "locked"  → redirect /unlock
  └─ update topbar pill mm:ss
```

**Kluczowe:** polling `/status` musi pomijać touch — w przeciwnym razie aktywne UI w przeglądarce ZAWSZE blokowałoby auto-lock, nawet gdy user wstał od komputera.

### 3.6 Set timeout sequence

```
POST /api/auto-lock/timeout  { "idle_timeout_min": 30 }
       │
       ▼ ACL require_session (touchuje — zmiana ustawień to user action)
       ▼
MONITOR.get()?.set_timeout_minutes(30).await
   │
   ├─ validate: m ∈ {5,15,30,60} else AutoLockError::InvalidPreset → 400
   ├─ db::set_system_config_value(pool, "vault.auto_lock_idle_min", "30").await?
   │     ← jeśli fail: AtomicU64 NIE zmieniany, 500
   ├─ self.idle_timeout_secs.store(30 * 60, Relaxed)   ← hot reload
   ├─ log info "[AUTO-LOCK] timeout updated to 30min"
   └─ Ok(())
       │
       ▼ 204 No Content
   następny tick (≤10s) używa nowej wartości
```

### 3.7 Manual touch sequence

```
User → click [Wydłuż] w toast
       ▼
POST /api/auto-lock/touch  (empty body)
       │
       ▼ ACL require_session_no_touch (handler explicit touchuje)
       ▼
MONITOR.get()?.touch(TouchSource::ManualExtend)
       │
       └─ last_activity.store(now_secs, Relaxed)
       ▼
   204 No Content
```

### 3.8 Pełny lifecycle (od daemon start do auto-lock)

```
T=0    daemon start, MONITOR.init():
       - last_activity = 0
       - idle_timeout_secs = DB read ("vault.auto_lock_idle_min" → 15) * 60 = 900
       - tokio::spawn(tick_loop)
       - win_session::spawn_observer  ← graceful: jeśli fail → log warn + continue

T=42   user unlock → POST /api/unlock → vault_keys.unlock(passphrase)
       (response zawiera Bearer token, kolejne calls przez ACL → touch)
       touch(AuthApi) fires → last_activity.store(42)

T=42..1000  normal use: API calls + cfapi hydration → touch
            UI polling /status co 5s NIE touchuje
            ostatni touch o T=999

T=1010 tick: now=1010, last=999, elapsed=11 < 900 → continue
T=...
T=1900 tick: now=1900, last=999, elapsed=901 >= 900 → fire force_lock

T=1900 force_lock:
       - audit event_type="auto_lock", reason="idle_timeout" BEFORE lock
       - vault_keys.lock() → Some := None
       - tokio::spawn(dismount_after_lock + unmount_virtual_drive)

T=1900+ state machine:
        - vault_keys.require_key = Err(Locked)
        - UI wraca z idle → GET /status → state="locked" → redirect /unlock
```

---

## §4 Error handling

### 4.1 Initialization (daemon start)

| Błąd | Reakcja | Severity |
|---|---|---|
| `db::get_system_config_value` → `Err(sqlx::Error)` | Fail-fast `AutoLockError::Init` | FATAL |
| Wartość nieprasowalna (`"abc"`, `""`, `-5`, `0`) | log warn + fallback default 15 + write-back DB | WARN |
| Wartość poza presetami (`7`, `45`, `99999`) | **clamp** do najbliższego z {5,15,30,60} + log warn + write-back DB | WARN |
| Wartość brak (świeży vault) | insert default `"15"` | INFO |
| `OnceLock.set()` → Err | Fail-fast `AutoLockError::DoubleInit` (programmer error) | FATAL |
| `win_session::spawn_observer` fail | **graceful degradation**: log warn + continue (timer-only mode) | WARN |

### 4.2 Hot path (touch, tick)

| Błąd | Reakcja |
|---|---|
| `MONITOR.get()` = `None` w `touch()` | Silent no-op (testy, pre-init, cfapi callback przed startup) |
| `AtomicU64::store` panic | Niemożliwe (infallible) |
| Tick: `vault_keys.require_key` = `Err(Locked)` | `continue` (idempotent skip) |
| Tick iteration panic | `catch_unwind` wokół body → log error → restart loop (NIE crash daemona) |

### 4.3 Lock execution (force_lock_and_dismount)

`force_lock_and_dismount` **NIGDY** nie zwraca błędu — vault MUSI być locked.

| Krok | Możliwy błąd | Reakcja |
|---|---|---|
| Audit log emission | DB fail | log warn + **continue** (security > observability) |
| `vault_keys.lock()` | Infallible | — |
| `smart_sync::dismount_after_lock` | `Err(SmartSyncError)` | log warn + continue; CF placeholdery zostają ghost (następny unlock zrobi `mount_after_unlock`) |
| `virtual_drive::unmount_virtual_drive("O:")` | `Err` | log warn + continue |
| Spawned `tokio::spawn` panics | Tokio izoluje |

**Kolejność:** audit FIRST (wymaga unlocked vault), potem `vault_keys.lock()`, potem dismount.

**Mapowanie `LockReason` → audit event_type** (uniknięcie konfliktu z istniejącym `"logout"` event w `auth.rs`):

| `LockReason` | `event_type` | dodatkowy detail |
|---|---|---|
| `Logout` | `"logout"` | (kompatybilność z istniejącym auditem α.A.a) |
| `IdleTimeout` | `"auto_lock"` | `reason="idle_timeout"` |
| `WinSessionLock` | `"auto_lock"` | `reason="win_session_lock"` |
| `ManualUserAction` | `"vault_lock"` | `reason="manual"` |

### 4.4 Configuration (POST /api/auto-lock/timeout)

| Błąd | Status | Body |
|---|---|---|
| `idle_timeout_min` brak | `400` | `{"error":"bad_request","code":"missing_field","field":"idle_timeout_min"}` |
| Poza {5,15,30,60} | `400` | `{"error":"bad_request","code":"invalid_preset","valid":[5,15,30,60]}` |
| Non-integer | `400` | serde deserialization error |
| `db::set_system_config_value` fail | `500` | AtomicU64 NIE aktualizowany — atomicity zachowana |

**Kolejność:** validate → DB write → atomic store → log → return. Gate na DB write.

### 4.5 OS integration (Win+L observer)

| Błąd | Reakcja |
|---|---|
| `WTSRegisterSessionNotification` fail przy startup | (4.1) graceful degradation |
| `WTSSESSION_CHANGE` z `wParam != WTS_SESSION_LOCK` | skip (filter w handler) |
| Message pump panics | `catch_unwind` → log error → thread exits → log warn → NIE re-spawn |
| `tokio::Handle` z OS thread → None | **Fix:** observer dostaje `tokio::runtime::Handle` z `spawn_observer(handle, ...)` |

### 4.6 Shutdown

| Sytuacja | Reakcja |
|---|---|
| Daemon shutdown (Ctrl+C, service stop) | tick loop: `tokio::select! { _ = sleep => ..., _ = shutdown_rx => break }`; `daemon_shutdown_tx` już istnieje w `ApiState` |
| ObserverHandle drop | `PostMessage(WM_QUIT)` → pump exits → `WTSUnRegisterSessionNotification` → `JoinHandle::join()` z timeoutem 2s |
| Lock tuż przed shutdown | Spawnowany teardown może nie skończyć — akceptowalne (CF state persistent, restart robi reconcile) |

### 4.7 Edge case: 3 ścieżki lock równocześnie

Logout + Win+L + idle timeout w jednej sekundzie:
1. Pierwsza: `require_key = true` → lock → tokio::spawn dismount
2. Druga + trzecia: `require_key = false` (już locked) → `vault_keys.lock()` no-op → `was_unlocked = false` → SKIP dismount

Tylko **jeden** dismount się spawnuje. Trzy audit logi (`"logout"`, `"auto_lock"` reason=`win_session_lock`, `"auto_lock"` reason=`idle_timeout`) — diagnostyka bonus.

---

## §5 Testing strategy

### 5.0 Deterministyczny czas

`daemon_start: tokio::time::Instant` (pause-aware). W `#[tokio::test(start_paused = true)]` pauzowany + sterowalny przez `tokio::time::advance(Duration)`. Cały kod produkcyjny bez modyfikacji.

### 5.1 Unit tests — `auto_lock.rs::tests`

Helper `setup_monitor(timeout_secs)` tworzy in-memory SQLite + `VaultKeyStore::new()` + bootstrap + unlock.

```rust
// Atomic semantics
#[tokio::test] async fn touch_updates_last_activity_to_now_secs();
#[tokio::test] async fn remaining_secs_zero_when_elapsed_exceeds_timeout();
#[tokio::test] async fn idle_timeout_secs_hot_reloads_via_set_timeout();

// Tick loop (paused clock)
#[tokio::test(start_paused = true)] async fn tick_loop_locks_vault_after_timeout();
#[tokio::test(start_paused = true)] async fn tick_loop_touch_during_idle_resets_countdown();
#[tokio::test(start_paused = true)] async fn tick_loop_skips_lock_when_vault_already_locked();
#[tokio::test(start_paused = true)] async fn tick_loop_recovers_after_force_lock_panic();

// Config validation
#[tokio::test] async fn set_timeout_accepts_5_15_30_60();
#[tokio::test] async fn set_timeout_rejects_other_values();
#[tokio::test] async fn set_timeout_db_fail_preserves_atomic();

// Init
#[tokio::test] async fn init_uses_default_15_when_db_empty();
#[tokio::test] async fn init_loads_stored_value();
#[tokio::test] async fn init_clamps_invalid_value_to_nearest_preset();
#[tokio::test] async fn init_writes_back_clamped_value_to_db();
```

### 5.2 Unit tests — `lock_flow.rs::tests`

```rust
#[tokio::test] async fn force_lock_when_unlocked_locks_and_returns_true();
#[tokio::test] async fn force_lock_when_already_locked_returns_false_no_dismount();
#[tokio::test] async fn force_lock_emits_audit_before_lock();
#[tokio::test] async fn force_lock_continues_when_audit_fails();
#[tokio::test] async fn force_lock_continues_when_dismount_fails();
#[tokio::test] async fn concurrent_force_lock_calls_only_first_dismounts();
```

**Mock dismount:** test używa nieistniejącej ścieżki `temp_dir/nonexistent` — `smart_sync::dismount_after_lock` zwraca `Err`, test weryfikuje `force_lock_and_dismount` continues.

### 5.3 Integration tests — `angeld/tests/e2e_auto_lock.rs`

Styl `e2e_basic.rs` (pełny daemon + axum HTTP client + temp DB).

```rust
#[tokio::test] async fn e2e_status_endpoint_returns_remaining_and_state();
#[tokio::test] async fn e2e_status_polling_does_not_touch();          // ⭐ Q2 regression guard
#[tokio::test] async fn e2e_files_call_touches_timer();
#[tokio::test] async fn e2e_set_timeout_endpoint_validates_preset();
#[tokio::test] async fn e2e_set_timeout_endpoint_hot_reloads();
#[tokio::test] async fn e2e_touch_endpoint_resets_remaining();
#[tokio::test] async fn e2e_unauthenticated_health_does_not_touch();
#[tokio::test] async fn e2e_post_lock_returns_401_on_subsequent_call();
```

### 5.4 Win+L mocking via `test-helpers` feature

Spójne z `e2e_recovery` (memory `feedback_e2e_recovery_test.md`).

```rust
#[cfg(feature = "test-helpers")]
pub fn simulate_session_lock(handle: &ObserverHandle) {
    handle.test_dispatcher_tx
        .send(SessionEvent::Lock)
        .expect("observer thread alive");
}
```

Internal dispatcher odbierający `SessionEvent::Lock` wywołuje **dokładnie ten sam handler** co prawdziwe `WM_WTSSESSION_CHANGE` z message pump → `handle.spawn(lock_flow::force_lock_and_dismount(LockReason::WinSessionLock))`. Mpsc istnieje **wyłącznie** w buildzie z `feature = "test-helpers"` jako alternatywne źródło zdarzenia. **Jedna ścieżka kodu, dwa źródła zdarzeń.**

```rust
#[tokio::test]
#[cfg(feature = "test-helpers")]
async fn e2e_win_session_lock_triggers_force_lock();
```

CI: `cargo test --workspace --features test-helpers` (jak e2e_recovery — security gate).

### 5.5 SMOKE H2 + H3 (manual gates)

**H2 — Idle auto-lock** (Lenovo + Dell):
1. `OMNIDRIVE_AUTO_LOCK_TEST_MIN=1` (env override, `#[cfg(debug_assertions)]`, NIE w release)
2. Unlock vault, otwórz O:\
3. Idle 70s
4. Verify: topbar pill 01:00→00:00, toast przy 00:59, O:\ znika, UI → /unlock
5. **PASS** jeśli wszystkie 4

**H3 — Win+L hard lock** (Lenovo + Dell):
1. Unlock vault, O:\ otwarte
2. Win+L → wait 3s → Windows unlock
3. Verify: OmniDrive UI = /unlock, O:\ niedostępne dopóki passphrase, audit log `reason=WinSessionLock`
4. **PASS** jeśli wszystkie 3

### 5.6 Coverage matryca

| Scenariusz | Unit | Integration | SMOKE |
|---|:---:|:---:|:---:|
| Touch zwiększa last_activity | ✓ | — | — |
| Tick wykrywa expired (paused clock) | ✓ | — | H2 |
| Idempotent force_lock | ✓ | — | — |
| Hot-reload timeout via API | ✓ | ✓ | — |
| Auth API call touchuje | — | ✓ | — |
| **Status polling NIE touchuje** | — | ✓ | H2 (passive) |
| Win+L hard lock | — | ✓ (test-helpers) | H3 |
| Vault already locked → skip | ✓ | — | — |
| Invalid preset rejected | ✓ | ✓ | — |
| Init clamping (7→5) | ✓ | — | — |
| Audit log emission | ✓ | — | H3 (DB check) |
| Catch_unwind tick loop | ✓ | — | — |
| Concurrent lock paths | ✓ | — | — |
| Graceful degradation (no Win+L observer) | — | ✓ | — |

**Total:** ~14 unit + 8 integration + 1 win-session = **23 testy** + 2 manual smokes.

---

## Dependencies & API surface

- **Nowe crates:** żadne (windows-rs już używane, feature flags doprecyzowane przy implementacji).
- **Nowy ENV (debug-only):** `OMNIDRIVE_AUTO_LOCK_TEST_MIN` — override 15min na 1min dla SMOKE H2.
- **Nowe endpointy API:** 3 (`GET /api/auto-lock/status`, `POST /api/auto-lock/timeout`, `POST /api/auto-lock/touch`).
- **Nowy klucz `system_config`:** `vault.auto_lock_idle_min` (string `"5"|"15"|"30"|"60"`, default `"15"`).
- **Nowy DB migrate:** brak (reuse `system_config`).
- **Breaking changes:** żadne (ACL signature `require_session`/`require_role` bez zmian; `require_session_no_touch` to NOWY symbol).

---

## Roadmap implementacji (do writing-plans)

α.A.b dekomponuje się na 4 sub-kroki (TDD, commit+push po każdym):

1. **α.A.b.1 — Config layer** — `auto_lock.rs::init`, `set_timeout_minutes`, schema klucza w `system_config`, endpoint `POST /api/auto-lock/timeout`, walidacja presetów, clamping. Unit tests 5.1 (config + init).
2. **α.A.b.2 — Activity tracking** — `AutoLockMonitor::touch`, `run_tick_loop`, hooki w `acl.rs` (touchowanie + `require_session_no_touch`), `smart_sync.rs` (cfapi callbacks), `lock_flow.rs` (extract z post_auth_logout), endpoint `GET /api/auto-lock/status` + `POST /api/auto-lock/touch`. Unit tests 5.1 (tick loop) + 5.2 + integration 5.3.
3. **α.A.b.3 — Win+L observer** — `win_session.rs`, integracja z `ApiServer::run`, graceful degradation. Integration 5.4 (z test-helpers).
4. **α.A.b.4 — Frontend** — topbar pill, toast, polling client, redirect na "locked".

SMOKE H2 + H3 po α.A.b.4 przed bumpem wersji.

---

## Status

Spec **ZATWIERDZONY** przez Przemka 2026-05-18 (sesja popołudniowa pending).

Następny krok: `superpowers:writing-plans` → `docs/superpowers/plans/2026-05-18-alpha-A-b-auto-lock-plan.md`.

**HARD-GATE aktywny:** zero kodu produkcyjnego dopóki plan implementacyjny nie zostanie zapisany i zaakceptowany.
