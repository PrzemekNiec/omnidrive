use crate::db::*;
use sqlx::FromRow;
use sqlx::SqlitePool;

// ── Recovery Keys (Epic 34.6a) ──

#[derive(Debug, Clone, FromRow)]
pub struct RecoveryKeyRecord {
    pub id: i64,
    pub vault_id: String,
    pub wrapped_vault_key: Vec<u8>,
    pub vk_generation: i64,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub revoked_at: Option<i64>,
}

pub async fn insert_recovery_key(
    pool: &SqlitePool,
    vault_id: &str,
    wrapped_vault_key: &[u8],
    vk_generation: i64,
    created_by: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "INSERT INTO vault_recovery_keys \
         (vault_id, wrapped_vault_key, vk_generation, created_at, created_by) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(vault_id)
    .bind(wrapped_vault_key)
    .bind(vk_generation)
    .bind(now)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn list_active_recovery_keys(
    pool: &SqlitePool,
    vault_id: &str,
) -> Result<Vec<RecoveryKeyRecord>, sqlx::Error> {
    sqlx::query_as::<_, RecoveryKeyRecord>(
        "SELECT id, vault_id, wrapped_vault_key, vk_generation, created_at, created_by, revoked_at \
         FROM vault_recovery_keys \
         WHERE vault_id = ? AND revoked_at IS NULL \
         ORDER BY created_at DESC",
    )
    .bind(vault_id)
    .fetch_all(pool)
    .await
}

pub async fn revoke_all_recovery_keys(
    pool: &SqlitePool,
    vault_id: &str,
) -> Result<u64, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "UPDATE vault_recovery_keys SET revoked_at = ? \
         WHERE vault_id = ? AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(vault_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recovery_key_insert_list_revoke() {
        let pool = init_db("sqlite::memory:").await.unwrap();

        let blob = vec![0xABu8; 40];
        let id = insert_recovery_key(&pool, "vault-a", &blob, 1, Some("user-1"))
            .await
            .unwrap();
        assert!(id > 0);

        let active = list_active_recovery_keys(&pool, "vault-a").await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].wrapped_vault_key, blob);
        assert_eq!(active[0].vk_generation, 1);
        assert_eq!(active[0].created_by.as_deref(), Some("user-1"));
        assert!(active[0].revoked_at.is_none());

        // A second key for the same vault should also be active.
        insert_recovery_key(&pool, "vault-a", &[0xCDu8; 40], 1, None)
            .await
            .unwrap();
        assert_eq!(
            list_active_recovery_keys(&pool, "vault-a")
                .await
                .unwrap()
                .len(),
            2
        );

        // Other vaults are isolated.
        insert_recovery_key(&pool, "vault-b", &blob, 1, None)
            .await
            .unwrap();
        assert_eq!(
            list_active_recovery_keys(&pool, "vault-b")
                .await
                .unwrap()
                .len(),
            1
        );

        // Revoke marks all keys for vault-a, leaves vault-b alone.
        let affected = revoke_all_recovery_keys(&pool, "vault-a").await.unwrap();
        assert_eq!(affected, 2);
        assert!(
            list_active_recovery_keys(&pool, "vault-a")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            list_active_recovery_keys(&pool, "vault-b")
                .await
                .unwrap()
                .len(),
            1
        );

        // Double revoke is a no-op.
        let again = revoke_all_recovery_keys(&pool, "vault-a").await.unwrap();
        assert_eq!(again, 0);
    }
}
