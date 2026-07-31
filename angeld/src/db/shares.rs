use sqlx::FromRow;
use sqlx::SqlitePool;

// ── Shared Links (Epic 33) ───────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct SharedLinkRecord {
    pub share_id: String,
    pub inode_id: i64,
    pub revision_id: i64,
    pub file_name: String,
    pub file_size: i64,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub max_downloads: Option<i64>,
    pub download_count: i64,
    pub revoked: i64,
    pub password_hash: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct SharePasswordToken {
    pub token: String,
    pub share_id: String,
    pub created_at: i64,
    pub expires_at: i64,
}

pub fn is_shared_link_valid(link: &SharedLinkRecord) -> bool {
    if link.revoked != 0 {
        return false;
    }
    if let Some(expires_at) = link.expires_at {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        if now > expires_at {
            return false;
        }
    }
    if let Some(max) = link.max_downloads
        && link.download_count >= max
    {
        return false;
    }
    true
}

pub async fn create_shared_link(
    pool: &SqlitePool,
    share_id: &str,
    inode_id: i64,
    revision_id: i64,
    file_name: &str,
    file_size: i64,
    expires_at: Option<i64>,
    max_downloads: Option<i64>,
    password_hash: Option<&str>,
) -> Result<(), sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    sqlx::query(
        "INSERT INTO shared_links (share_id, inode_id, revision_id, file_name, file_size, \
         created_at, expires_at, max_downloads, password_hash) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(share_id)
    .bind(inode_id)
    .bind(revision_id)
    .bind(file_name)
    .bind(file_size)
    .bind(now)
    .bind(expires_at)
    .bind(max_downloads)
    .bind(password_hash)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_shared_link(
    pool: &SqlitePool,
    share_id: &str,
) -> Result<Option<SharedLinkRecord>, sqlx::Error> {
    sqlx::query_as::<_, SharedLinkRecord>(
        "SELECT share_id, inode_id, revision_id, file_name, file_size, created_at, \
         expires_at, max_downloads, download_count, revoked, password_hash FROM shared_links WHERE share_id = ?",
    )
    .bind(share_id)
    .fetch_optional(pool)
    .await
}

pub async fn list_shared_links(pool: &SqlitePool) -> Result<Vec<SharedLinkRecord>, sqlx::Error> {
    sqlx::query_as::<_, SharedLinkRecord>(
        "SELECT share_id, inode_id, revision_id, file_name, file_size, created_at, \
         expires_at, max_downloads, download_count, revoked, password_hash FROM shared_links \
         ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn list_shared_links_for_inode(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Vec<SharedLinkRecord>, sqlx::Error> {
    sqlx::query_as::<_, SharedLinkRecord>(
        "SELECT share_id, inode_id, revision_id, file_name, file_size, created_at, \
         expires_at, max_downloads, download_count, revoked, password_hash FROM shared_links \
         WHERE inode_id = ? ORDER BY created_at DESC",
    )
    .bind(inode_id)
    .fetch_all(pool)
    .await
}

pub async fn revoke_shared_link(pool: &SqlitePool, share_id: &str) -> Result<bool, sqlx::Error> {
    let result =
        sqlx::query("UPDATE shared_links SET revoked = 1 WHERE share_id = ? AND revoked = 0")
            .bind(share_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn increment_shared_link_download_count(
    pool: &SqlitePool,
    share_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE shared_links SET download_count = download_count + 1 WHERE share_id = ?")
        .bind(share_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_shared_link(pool: &SqlitePool, share_id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM shared_links WHERE share_id = ?")
        .bind(share_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ── Share Password Tokens ────────────────────────────────────────────

pub async fn create_share_password_token(
    pool: &SqlitePool,
    token: &str,
    share_id: &str,
    ttl_seconds: i64,
) -> Result<(), sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let expires_at = now + (ttl_seconds * 1000);
    sqlx::query(
        "INSERT INTO share_password_tokens (token, share_id, created_at, expires_at) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(token)
    .bind(share_id)
    .bind(now)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn validate_share_password_token(
    pool: &SqlitePool,
    token: &str,
    share_id: &str,
) -> Result<bool, sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let row = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM share_password_tokens \
         WHERE token = ? AND share_id = ? AND expires_at > ?",
    )
    .bind(token)
    .bind(share_id)
    .bind(now)
    .fetch_one(pool)
    .await?;
    Ok(row > 0)
}

pub async fn cleanup_expired_share_tokens(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let result = sqlx::query("DELETE FROM share_password_tokens WHERE expires_at <= ?")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::*;

    #[tokio::test]
    async fn shared_link_crud_lifecycle() {
        let pool = init_db("sqlite::memory:").await.unwrap();

        // Create a shared link
        create_shared_link(&pool, "abc123", 1, 10, "test.txt", 4096, None, None, None)
            .await
            .unwrap();

        // Read it back
        let link = get_shared_link(&pool, "abc123").await.unwrap().unwrap();
        assert_eq!(link.share_id, "abc123");
        assert_eq!(link.inode_id, 1);
        assert_eq!(link.revision_id, 10);
        assert_eq!(link.file_name, "test.txt");
        assert_eq!(link.file_size, 4096);
        assert_eq!(link.download_count, 0);
        assert_eq!(link.revoked, 0);
        assert!(link.expires_at.is_none());
        assert!(link.max_downloads.is_none());
        assert!(link.password_hash.is_none());

        // List all
        let all = list_shared_links(&pool).await.unwrap();
        assert_eq!(all.len(), 1);

        // List by inode
        let by_inode = list_shared_links_for_inode(&pool, 1).await.unwrap();
        assert_eq!(by_inode.len(), 1);
        let empty = list_shared_links_for_inode(&pool, 999).await.unwrap();
        assert!(empty.is_empty());

        // Increment download count
        increment_shared_link_download_count(&pool, "abc123")
            .await
            .unwrap();
        let link = get_shared_link(&pool, "abc123").await.unwrap().unwrap();
        assert_eq!(link.download_count, 1);

        // Delete
        let deleted = delete_shared_link(&pool, "abc123").await.unwrap();
        assert!(deleted);
        let gone = get_shared_link(&pool, "abc123").await.unwrap();
        assert!(gone.is_none());

        // Delete non-existent returns false
        let nope = delete_shared_link(&pool, "abc123").await.unwrap();
        assert!(!nope);
    }

    #[tokio::test]
    async fn shared_link_revoke() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_shared_link(&pool, "rev1", 1, 10, "file.bin", 100, None, None, None)
            .await
            .unwrap();

        // Valid before revoke
        let link = get_shared_link(&pool, "rev1").await.unwrap().unwrap();
        assert!(is_shared_link_valid(&link));

        // Revoke
        let revoked = revoke_shared_link(&pool, "rev1").await.unwrap();
        assert!(revoked);

        // Invalid after revoke
        let link = get_shared_link(&pool, "rev1").await.unwrap().unwrap();
        assert!(!is_shared_link_valid(&link));
        assert_eq!(link.revoked, 1);

        // Double revoke returns false
        let again = revoke_shared_link(&pool, "rev1").await.unwrap();
        assert!(!again);
    }

    #[test]
    fn shared_link_expired() {
        let link = SharedLinkRecord {
            share_id: "exp1".into(),
            inode_id: 1,
            revision_id: 10,
            file_name: "old.txt".into(),
            file_size: 50,
            created_at: 1000,
            expires_at: Some(1), // expired long ago (epoch + 1ms)
            max_downloads: None,
            download_count: 0,
            revoked: 0,
            password_hash: None,
        };
        assert!(!is_shared_link_valid(&link));
    }

    #[test]
    fn shared_link_not_yet_expired() {
        let far_future = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
            + 3_600_000; // +1 hour
        let link = SharedLinkRecord {
            share_id: "fut1".into(),
            inode_id: 1,
            revision_id: 10,
            file_name: "future.txt".into(),
            file_size: 50,
            created_at: 1000,
            expires_at: Some(far_future),
            max_downloads: None,
            download_count: 0,
            revoked: 0,
            password_hash: None,
        };
        assert!(is_shared_link_valid(&link));
    }

    #[test]
    fn shared_link_download_limit_reached() {
        let link = SharedLinkRecord {
            share_id: "dl1".into(),
            inode_id: 1,
            revision_id: 10,
            file_name: "limited.txt".into(),
            file_size: 50,
            created_at: 1000,
            expires_at: None,
            max_downloads: Some(3),
            download_count: 3,
            revoked: 0,
            password_hash: None,
        };
        assert!(!is_shared_link_valid(&link));
    }

    #[test]
    fn shared_link_download_limit_not_reached() {
        let link = SharedLinkRecord {
            share_id: "dl2".into(),
            inode_id: 1,
            revision_id: 10,
            file_name: "limited.txt".into(),
            file_size: 50,
            created_at: 1000,
            expires_at: None,
            max_downloads: Some(3),
            download_count: 2,
            revoked: 0,
            password_hash: None,
        };
        assert!(is_shared_link_valid(&link));
    }

    #[tokio::test]
    async fn shared_link_with_password() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_shared_link(
            &pool,
            "pw1",
            1,
            10,
            "secret.pdf",
            2048,
            None,
            None,
            Some("salt$hash"),
        )
        .await
        .unwrap();

        let link = get_shared_link(&pool, "pw1").await.unwrap().unwrap();
        assert_eq!(link.password_hash.as_deref(), Some("salt$hash"));
    }

    #[tokio::test]
    async fn password_token_lifecycle() {
        let pool = init_db("sqlite::memory:").await.unwrap();

        // Create token with 10-second TTL
        create_share_password_token(&pool, "tok1", "share1", 10)
            .await
            .unwrap();

        // Valid immediately
        assert!(
            validate_share_password_token(&pool, "tok1", "share1")
                .await
                .unwrap()
        );

        // Wrong share_id
        assert!(
            !validate_share_password_token(&pool, "tok1", "share2")
                .await
                .unwrap()
        );

        // Wrong token
        assert!(
            !validate_share_password_token(&pool, "tok_bad", "share1")
                .await
                .unwrap()
        );
    }
}
