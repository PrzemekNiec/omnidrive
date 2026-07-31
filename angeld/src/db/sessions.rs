use crate::db::*;
use sqlx::FromRow;
use sqlx::SqlitePool;

// ── Epic 34.3a: User Sessions ───────────────────────────────────────

/// 24 hours in seconds
pub const SESSION_TTL_SECONDS: i64 = 86400;

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct UserSession {
    pub token: String,
    pub user_id: String,
    pub device_id: String,
    pub created_at: i64,
    pub expires_at: i64,
}

/// Generate a 256-bit random session token (base64url, no padding).
pub fn generate_session_token() -> String {
    use base64::Engine;
    use rand::{RngCore, rngs::OsRng};
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub async fn create_user_session(
    pool: &SqlitePool,
    token: &str,
    user_id: &str,
    device_id: &str,
    ttl_seconds: i64,
) -> Result<UserSession, sqlx::Error> {
    let now = epoch_secs();
    let expires_at = now + ttl_seconds;
    sqlx::query(
        "INSERT INTO user_sessions (token, user_id, device_id, created_at, expires_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(token)
    .bind(user_id)
    .bind(device_id)
    .bind(now)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(UserSession {
        token: token.to_string(),
        user_id: user_id.to_string(),
        device_id: device_id.to_string(),
        created_at: now,
        expires_at,
    })
}

/// Validate a session token. Returns the session if valid and not expired.
///
/// Conscious decision — no application-level constant-time comparison:
/// tokens are 256-bit OsRng values; the comparison happens inside SQLite
/// (`WHERE token = ?`) which has non-constant timing, but a timing
/// side-channel is not exploitable here because (a) the daemon only listens
/// on loopback/LAN, (b) SQLite query overhead (~µs) swamps any byte-compare
/// difference (~ns), and (c) an attacker would need millions of same-machine
/// measurements — see docs/crypto-spec.md §11 for full rationale.
pub async fn validate_user_session(
    pool: &SqlitePool,
    token: &str,
) -> Result<Option<UserSession>, sqlx::Error> {
    let now = epoch_secs();
    sqlx::query_as::<_, UserSession>(
        "SELECT token, user_id, device_id, created_at, expires_at \
         FROM user_sessions WHERE token = ? AND expires_at > ?",
    )
    .bind(token)
    .bind(now)
    .fetch_optional(pool)
    .await
}

/// Renew a session token's expiry by TTL seconds from now.
pub async fn renew_user_session(
    pool: &SqlitePool,
    token: &str,
    ttl_seconds: i64,
) -> Result<bool, sqlx::Error> {
    let now = epoch_secs();
    let new_expires = now + ttl_seconds;
    let result =
        sqlx::query("UPDATE user_sessions SET expires_at = ? WHERE token = ? AND expires_at > ?")
            .bind(new_expires)
            .bind(token)
            .bind(now)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

/// Delete a specific session (logout).
pub async fn delete_user_session(pool: &SqlitePool, token: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM user_sessions WHERE token = ?")
        .bind(token)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Delete all sessions for a user (e.g. on password change or revocation).
pub async fn delete_user_sessions_for_user(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM user_sessions WHERE user_id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Clean up expired sessions.
pub async fn cleanup_expired_sessions(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query("DELETE FROM user_sessions WHERE expires_at <= ?")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Epic 34.3a: Session token tests ─────────────────────────────

    #[tokio::test]
    async fn session_create_validate_delete() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_user(&pool, "user-1", "Alice", None, "local", None)
            .await
            .unwrap();

        let token = generate_session_token();
        assert_eq!(token.len(), 43); // 32 bytes → 43 base64url chars (no pad)

        let session = create_user_session(&pool, &token, "user-1", "dev-a", SESSION_TTL_SECONDS)
            .await
            .unwrap();
        assert_eq!(session.user_id, "user-1");
        assert_eq!(session.device_id, "dev-a");
        assert!(session.expires_at > session.created_at);

        // Validate
        let valid = validate_user_session(&pool, &token).await.unwrap();
        assert!(valid.is_some());
        let valid = valid.unwrap();
        assert_eq!(valid.user_id, "user-1");

        // Invalid token returns None
        let bogus = validate_user_session(&pool, "not-a-real-token")
            .await
            .unwrap();
        assert!(bogus.is_none());

        // Delete (logout)
        assert!(delete_user_session(&pool, &token).await.unwrap());
        let gone = validate_user_session(&pool, &token).await.unwrap();
        assert!(gone.is_none());

        // Double-delete returns false
        assert!(!delete_user_session(&pool, &token).await.unwrap());
    }

    #[tokio::test]
    async fn session_expires() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_user(&pool, "user-1", "Alice", None, "local", None)
            .await
            .unwrap();

        // Create session with TTL=0 so it's already expired
        let token = generate_session_token();
        create_user_session(&pool, &token, "user-1", "dev-a", 0)
            .await
            .unwrap();

        // Should not validate — already expired
        let result = validate_user_session(&pool, &token).await.unwrap();
        assert!(result.is_none());

        // Cleanup removes it
        let cleaned = cleanup_expired_sessions(&pool).await.unwrap();
        assert_eq!(cleaned, 1);
    }

    #[tokio::test]
    async fn session_renew() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_user(&pool, "user-1", "Alice", None, "local", None)
            .await
            .unwrap();

        let token = generate_session_token();
        let session = create_user_session(&pool, &token, "user-1", "dev-a", 3600)
            .await
            .unwrap();
        let old_expires = session.expires_at;

        // Renew with longer TTL
        assert!(
            renew_user_session(&pool, &token, SESSION_TTL_SECONDS)
                .await
                .unwrap()
        );

        let renewed = validate_user_session(&pool, &token).await.unwrap().unwrap();
        assert!(renewed.expires_at > old_expires);
    }

    #[tokio::test]
    async fn session_delete_all_for_user() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_user(&pool, "user-1", "Alice", None, "local", None)
            .await
            .unwrap();

        // Create 3 sessions
        for i in 0..3 {
            let t = generate_session_token();
            create_user_session(
                &pool,
                &t,
                "user-1",
                &format!("dev-{i}"),
                SESSION_TTL_SECONDS,
            )
            .await
            .unwrap();
        }

        let deleted = delete_user_sessions_for_user(&pool, "user-1")
            .await
            .unwrap();
        assert_eq!(deleted, 3);
    }
}
