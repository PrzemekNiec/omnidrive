use crate::db::*;
use sqlx::SqlitePool;

// ── Sesja C: OAuth2 state management ────────────────────────────────

pub async fn create_oauth_state(
    pool: &SqlitePool,
    state: &str,
    pkce_verifier: &str,
    ttl_secs: i64,
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "INSERT INTO oauth_states (state, pkce_verifier, created_at, expires_at) VALUES (?, ?, ?, ?)",
    )
    .bind(state)
    .bind(pkce_verifier)
    .bind(now)
    .bind(now + ttl_secs)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_and_delete_oauth_state(
    pool: &SqlitePool,
    state: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT pkce_verifier, expires_at FROM oauth_states WHERE state = ?")
            .bind(state)
            .fetch_optional(pool)
            .await?;
    sqlx::query("DELETE FROM oauth_states WHERE state = ?")
        .bind(state)
        .execute(pool)
        .await?;
    Ok(row.and_then(|(verifier, expires_at)| {
        if epoch_secs() <= expires_at {
            Some(verifier)
        } else {
            None
        }
    }))
}

pub async fn delete_expired_oauth_states(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM oauth_states WHERE expires_at < ?")
        .bind(epoch_secs())
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ── C.1: Sealed OAuth refresh-token storage ───────────────────────────

/// Persist the VK-sealed refresh token ciphertext for `user_id`.
pub async fn store_encrypted_refresh_token(
    pool: &SqlitePool,
    user_id: &str,
    ciphertext: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET google_refresh_token_ciphertext = ? WHERE user_id = ?")
        .bind(ciphertext)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Fetch the sealed refresh token blob for `user_id`, if one has been stored.
pub async fn get_encrypted_refresh_token(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<Option<Vec<u8>>, sqlx::Error> {
    sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT google_refresh_token_ciphertext FROM users WHERE user_id = ? \
         AND google_refresh_token_ciphertext IS NOT NULL",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

/// Return `(user_id, google_refresh_token)` for all users that still have a
/// plaintext refresh token and no sealed ciphertext (migration candidates).
pub async fn users_with_plaintext_refresh_token(
    pool: &SqlitePool,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT user_id, google_refresh_token FROM users \
         WHERE google_refresh_token IS NOT NULL \
         AND google_refresh_token_ciphertext IS NULL",
    )
    .fetch_all(pool)
    .await
}

/// Remove the plaintext refresh token once it has been sealed.
pub async fn clear_plaintext_refresh_token(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET google_refresh_token = NULL WHERE user_id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Sesja C: OAuth state tests ─────────────────────────────────

    #[tokio::test]
    async fn oauth_state_create_and_retrieve() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_oauth_state(&pool, "state-abc", "verifier-xyz", 600)
            .await
            .unwrap();
        let v = get_and_delete_oauth_state(&pool, "state-abc")
            .await
            .unwrap();
        assert_eq!(v.as_deref(), Some("verifier-xyz"));
    }

    #[tokio::test]
    async fn oauth_state_is_single_use() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_oauth_state(&pool, "state-once", "verifier-once", 600)
            .await
            .unwrap();
        assert!(
            get_and_delete_oauth_state(&pool, "state-once")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            get_and_delete_oauth_state(&pool, "state-once")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn oauth_state_csrf_mismatch_returns_none() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_oauth_state(&pool, "real-state", "verifier-real", 600)
            .await
            .unwrap();
        let v = get_and_delete_oauth_state(&pool, "attacker-state")
            .await
            .unwrap();
        assert!(v.is_none());
    }

    #[tokio::test]
    async fn oauth_state_expired_returns_none() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_oauth_state(&pool, "expired-state", "verifier-exp", -1)
            .await
            .unwrap();
        let v = get_and_delete_oauth_state(&pool, "expired-state")
            .await
            .unwrap();
        assert!(v.is_none(), "expired state must return None");
    }

    #[tokio::test]
    async fn oauth_state_cleanup_removes_expired() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_oauth_state(&pool, "exp-1", "v1", -10).await.unwrap();
        create_oauth_state(&pool, "exp-2", "v2", -5).await.unwrap();
        create_oauth_state(&pool, "live-1", "v3", 600)
            .await
            .unwrap();
        assert_eq!(delete_expired_oauth_states(&pool).await.unwrap(), 2);
        assert_eq!(
            get_and_delete_oauth_state(&pool, "live-1")
                .await
                .unwrap()
                .as_deref(),
            Some("v3")
        );
    }
}
