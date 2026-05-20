//! α.A.b.3 — WTS session lock observer (Win+L hard-lock).
//!
//! Runs a dedicated OS thread with a hidden message-only window.
//! `WM_WTSSESSION_CHANGE` → `lock_flow::force_lock_and_dismount(WinSessionLock)`.
//! `WTS_SESSION_UNLOCK` is intentionally ignored (zero-trust).

#![cfg(target_os = "windows")]
#![allow(unused_imports)]

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

#[allow(dead_code)]
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
    Unlock,
}

pub fn spawn_observer(
    _runtime: tokio::runtime::Handle,
    _pool: SqlitePool,
    _vault_keys: VaultKeyStore,
) -> Result<ObserverHandle, WinSessionError> {
    Err(WinSessionError::RegisterFailed(
        "not yet implemented".into(),
    ))
}

impl Drop for ObserverHandle {
    fn drop(&mut self) {}
}
