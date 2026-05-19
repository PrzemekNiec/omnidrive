//! α.A.b — single source of truth for "lock + teardown".
//!
//! Used by manual logout (α.A.a refactor), idle timeout (α.A.b.2),
//! Win+L (α.A.b.3) and manual user action. Always succeeds (vault
//! MUST end up locked); individual teardown failures are logged + ignored.

use crate::db;
use crate::runtime_paths::RuntimePaths;
use crate::smart_sync;
use crate::vault::VaultKeyStore;
use sqlx::SqlitePool;
use tracing::{info, warn};

#[derive(Copy, Clone, Debug)]
#[allow(dead_code)]
pub enum LockReason {
    Logout,
    IdleTimeout,
    WinSessionLock,
    ManualUserAction,
}

impl LockReason {
    pub(crate) fn audit(&self) -> (&'static str, Option<&'static str>) {
        match self {
            LockReason::Logout => ("logout", None),
            LockReason::IdleTimeout => ("auto_lock", Some(r#"{"reason":"idle_timeout"}"#)),
            LockReason::WinSessionLock => ("auto_lock", Some(r#"{"reason":"win_session_lock"}"#)),
            LockReason::ManualUserAction => ("vault_lock", Some(r#"{"reason":"manual"}"#)),
        }
    }
}

pub async fn force_lock_and_dismount(
    pool: &SqlitePool,
    vault_keys: &VaultKeyStore,
    reason: LockReason,
    actor: Option<(&str, &str)>, // (user_id, device_id)
) -> bool {
    let was_unlocked = vault_keys.require_key().await.is_ok();

    if was_unlocked {
        match db::get_vault_params(pool).await {
            Ok(Some(vault)) => {
                let (action, details) = reason.audit();
                if let Err(e) = db::insert_audit_log(
                    pool,
                    &vault.vault_id,
                    action,
                    actor.map(|(u, _)| u),
                    actor.map(|(_, d)| d),
                    None,
                    None,
                    details,
                )
                .await
                {
                    warn!(
                        "[LOCK_FLOW] audit emission failed (reason={:?}): {}",
                        reason, e
                    );
                }
            }
            Ok(None) => warn!(
                "[LOCK_FLOW] audit skipped — no vault_state row (reason={:?})",
                reason
            ),
            Err(e) => warn!(
                "[LOCK_FLOW] audit skipped — db::get_vault_params error (reason={:?}): {}",
                reason, e
            ),
        }
    }

    vault_keys.lock().await;

    if was_unlocked {
        info!(
            "[LOCK_FLOW] locked, reason={:?} — spawning teardown",
            reason
        );
        tokio::spawn(async move {
            let paths = RuntimePaths::detect();
            if let Err(err) = smart_sync::dismount_after_lock(&paths.sync_root).await {
                warn!("[LOCK_FLOW] CF dismount failed: {err}");
            }
            let drive_letter =
                std::env::var("OMNIDRIVE_DRIVE_LETTER").unwrap_or_else(|_| "O:".to_string());
            if let Err(err) = crate::virtual_drive::unmount_virtual_drive(&drive_letter) {
                warn!("[LOCK_FLOW] virtual drive unmount warning: {err}");
            }
        });
    }

    was_unlocked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::VaultKeyStore;

    async fn setup() -> (sqlx::SqlitePool, VaultKeyStore) {
        let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
        let keys = VaultKeyStore::default();
        keys.unlock(&pool, "passphrase-for-test").await.unwrap();
        (pool, keys)
    }

    #[tokio::test]
    async fn force_lock_when_unlocked_locks_and_returns_true() {
        let (pool, keys) = setup().await;
        assert!(keys.require_key().await.is_ok());
        let was = force_lock_and_dismount(&pool, &keys, LockReason::IdleTimeout, None).await;
        assert!(was);
        assert!(keys.require_key().await.is_err());
    }

    #[tokio::test]
    async fn force_lock_when_already_locked_returns_false() {
        let (pool, keys) = setup().await;
        keys.lock().await;
        let was = force_lock_and_dismount(&pool, &keys, LockReason::IdleTimeout, None).await;
        assert!(!was);
    }

    #[tokio::test]
    async fn force_lock_emits_audit_with_reason_idle_timeout() {
        let (pool, keys) = setup().await;
        force_lock_and_dismount(&pool, &keys, LockReason::IdleTimeout, None).await;
        let logs: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT action, details FROM audit_logs ORDER BY id DESC LIMIT 1")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(!logs.is_empty(), "expected audit log row to be inserted");
        assert_eq!(logs[0].0, "auto_lock");
        assert!(logs[0].1.as_deref().unwrap_or("").contains("idle_timeout"));
    }
}
