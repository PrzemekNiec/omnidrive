//! α.A.b.3 — WTS session lock observer (Win+L hard-lock).
//!
//! Runs a dedicated OS thread with a hidden message-only window.
//! `WM_WTSSESSION_CHANGE` → `lock_flow::force_lock_and_dismount(WinSessionLock)`.
//! `WTS_SESSION_UNLOCK` is intentionally ignored (zero-trust).

#![cfg(target_os = "windows")]

use crate::lock_flow::{self, LockReason};
use crate::vault::VaultKeyStore;
use sqlx::SqlitePool;
use std::sync::OnceLock;
use std::thread::{self, JoinHandle};
use tracing::{error, info, warn};
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG,
    PostMessageW, RegisterClassW, TranslateMessage, WM_QUIT, WM_WTSSESSION_CHANGE, WNDCLASSW,
    WTS_SESSION_LOCK,
};
use windows::core::PCWSTR;

#[derive(Debug)]
pub enum WinSessionError {
    RegisterFailed(String),
    SpawnFailed(String),
}

pub struct ObserverHandle {
    join: Option<JoinHandle<()>>,
    hwnd_raw: usize,
    #[cfg(feature = "test-helpers")]
    pub test_dispatcher_tx: tokio::sync::mpsc::UnboundedSender<SessionEvent>,
}

#[cfg(feature = "test-helpers")]
#[derive(Debug, Clone, Copy)]
pub enum SessionEvent {
    Lock,
    #[allow(dead_code)]
    Unlock,
}

const CLASS_NAME: &[u16] = &[
    'O' as u16, 'm' as u16, 'n' as u16, 'i' as u16, 'D' as u16, 'r' as u16, 'i' as u16, 'v' as u16,
    'e' as u16, 'W' as u16, 't' as u16, 's' as u16, 0u16,
];

static OBSERVER_CTX: OnceLock<ThreadCtx> = OnceLock::new();

struct ThreadCtx {
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
    runtime: tokio::runtime::Handle,
}

const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    let _ = assert_send_sync::<ThreadCtx>;
};

pub fn spawn_observer(
    runtime: tokio::runtime::Handle,
    pool: SqlitePool,
    vault_keys: VaultKeyStore,
) -> Result<ObserverHandle, WinSessionError> {
    #[cfg(feature = "test-helpers")]
    let (test_tx, mut test_rx) = tokio::sync::mpsc::unbounded_channel::<SessionEvent>();

    let (hwnd_tx, hwnd_rx) = std::sync::mpsc::channel::<usize>();
    let pool_thread = pool.clone();
    let keys_thread = vault_keys.clone();
    let rt_thread = runtime.clone();

    let join = thread::Builder::new()
        .name("omnidrive-win-session".to_string())
        .spawn(move || {
            // SAFETY: this OS thread is the sole owner of the message-only window
            // and its message pump; the window class is registered, the window is
            // created, pumped, and destroyed entirely within this closure.
            // `OBSERVER_CTX` is set exactly once (single observer per process)
            // before WTS notifications can fire.
            unsafe {
                let hinstance: HMODULE = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(window_proc_trampoline),
                    hInstance: hinstance.into(),
                    lpszClassName: PCWSTR(CLASS_NAME.as_ptr()),
                    ..Default::default()
                };
                let _ = RegisterClassW(&wc);
                let hwnd = CreateWindowExW(
                    Default::default(),
                    PCWSTR(CLASS_NAME.as_ptr()),
                    PCWSTR::null(),
                    Default::default(),
                    0,
                    0,
                    0,
                    0,
                    Some(HWND_MESSAGE),
                    None,
                    Some(hinstance.into()),
                    None,
                )
                .unwrap_or_default();
                if hwnd.0.is_null() {
                    let _ = hwnd_tx.send(0);
                    return;
                }
                if WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION).is_err() {
                    warn!("[WIN-SESSION] WTSRegisterSessionNotification failed");
                    let _ = hwnd_tx.send(0);
                    return;
                }
                let _ = OBSERVER_CTX.set(ThreadCtx {
                    pool: pool_thread,
                    vault_keys: keys_thread,
                    runtime: rt_thread,
                });
                let _ = hwnd_tx.send(hwnd.0 as usize);
                let mut msg = MSG::default();
                loop {
                    let ret = GetMessageW(&mut msg, None, 0, 0);
                    if ret.0 == 0 {
                        break;
                    }
                    if ret.0 == -1 {
                        error!("[WIN-SESSION] GetMessageW error; exiting message pump");
                        break;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let _ = WTSUnRegisterSessionNotification(hwnd);
                info!("[WIN-SESSION] observer thread exited");
            }
        })
        .map_err(|e| WinSessionError::SpawnFailed(e.to_string()))?;

    let hwnd_raw = hwnd_rx
        .recv()
        .map_err(|e| WinSessionError::RegisterFailed(e.to_string()))?;
    if hwnd_raw == 0 {
        return Err(WinSessionError::RegisterFailed(
            "hwnd creation failed".into(),
        ));
    }

    #[cfg(feature = "test-helpers")]
    {
        let pool_test = pool.clone();
        let keys_test = vault_keys.clone();
        runtime.spawn(async move {
            while let Some(ev) = test_rx.recv().await {
                if matches!(ev, SessionEvent::Lock) {
                    lock_flow::force_lock_and_dismount(
                        &pool_test,
                        &keys_test,
                        LockReason::WinSessionLock,
                        None,
                    )
                    .await;
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

/// SAFETY: invoked by Windows from the observer thread's message pump. The
/// dispatch body is wrapped in `catch_unwind` so no panic can unwind across
/// this `extern "system"` FFI boundary (UB); `DefWindowProcW` is always called.
unsafe extern "system" fn window_proc_trampoline(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if msg == WM_WTSSESSION_CHANGE
            && wparam.0 as u32 == WTS_SESSION_LOCK
            && let Some(ctx) = OBSERVER_CTX.get()
        {
            let pool = ctx.pool.clone();
            let keys = ctx.vault_keys.clone();
            ctx.runtime.spawn(async move {
                lock_flow::force_lock_and_dismount(&pool, &keys, LockReason::WinSessionLock, None)
                    .await;
            });
        }
    }));
    if result.is_err() {
        error!("[WIN-SESSION] window_proc panic recovered");
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

impl Drop for ObserverHandle {
    fn drop(&mut self) {
        // SAFETY: `hwnd_raw` is the message-only window owned by the observer
        // thread; posting WM_QUIT breaks its message pump so the thread exits.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.hwnd_raw as *mut std::ffi::c_void)),
                WM_QUIT,
                WPARAM(0),
                LPARAM(0),
            );
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}
