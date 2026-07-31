use crate::db::*;
use sqlx::FromRow;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct InviteCodeRecord {
    pub code: String,
    pub vault_id: String,
    pub created_by: String,
    pub role: String,
    pub max_uses: i64,
    pub used_count: i64,
    pub expires_at: Option<i64>,
    pub created_at: i64,
}

// ── Invite Codes ──

pub async fn create_invite_code(
    pool: &SqlitePool,
    code: &str,
    vault_id: &str,
    created_by: &str,
    role: &str,
    max_uses: i64,
    expires_at: Option<i64>,
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "INSERT INTO invite_codes (code, vault_id, created_by, role, max_uses, used_count, expires_at, created_at) \
         VALUES (?, ?, ?, ?, ?, 0, ?, ?)",
    )
    .bind(code)
    .bind(vault_id)
    .bind(created_by)
    .bind(role)
    .bind(max_uses)
    .bind(expires_at)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_invite_code(
    pool: &SqlitePool,
    code: &str,
) -> Result<Option<InviteCodeRecord>, sqlx::Error> {
    sqlx::query_as::<_, InviteCodeRecord>(
        "SELECT code, vault_id, created_by, role, max_uses, used_count, expires_at, created_at \
         FROM invite_codes WHERE code = ?",
    )
    .bind(code)
    .fetch_optional(pool)
    .await
}

pub fn is_invite_code_valid(code: &InviteCodeRecord) -> bool {
    if code.used_count >= code.max_uses {
        return false;
    }
    if let Some(exp) = code.expires_at
        && epoch_secs() > exp
    {
        return false;
    }
    true
}

pub async fn consume_invite_code(pool: &SqlitePool, code: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE invite_codes SET used_count = used_count + 1 \
         WHERE code = ? AND used_count < max_uses",
    )
    .bind(code)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_invite_codes(
    pool: &SqlitePool,
    vault_id: &str,
) -> Result<Vec<InviteCodeRecord>, sqlx::Error> {
    sqlx::query_as::<_, InviteCodeRecord>(
        "SELECT code, vault_id, created_by, role, max_uses, used_count, expires_at, created_at \
         FROM invite_codes WHERE vault_id = ? ORDER BY created_at DESC",
    )
    .bind(vault_id)
    .fetch_all(pool)
    .await
}

pub async fn delete_invite_code(pool: &SqlitePool, code: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM invite_codes WHERE code = ?")
        .bind(code)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn invite_code_crud_lifecycle() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_user(&pool, "u1", "Alice", None, "local", None)
            .await
            .unwrap();

        // Create invite
        create_invite_code(&pool, "INV123", "vault-1", "u1", "member", 2, None)
            .await
            .unwrap();

        // Read
        let inv = get_invite_code(&pool, "INV123").await.unwrap().unwrap();
        assert_eq!(inv.vault_id, "vault-1");
        assert_eq!(inv.max_uses, 2);
        assert_eq!(inv.used_count, 0);
        assert!(is_invite_code_valid(&inv));

        // Consume once
        assert!(consume_invite_code(&pool, "INV123").await.unwrap());
        let inv = get_invite_code(&pool, "INV123").await.unwrap().unwrap();
        assert_eq!(inv.used_count, 1);
        assert!(is_invite_code_valid(&inv));

        // Consume again (max=2)
        assert!(consume_invite_code(&pool, "INV123").await.unwrap());
        let inv = get_invite_code(&pool, "INV123").await.unwrap().unwrap();
        assert_eq!(inv.used_count, 2);
        assert!(!is_invite_code_valid(&inv));

        // Can't consume past max
        assert!(!consume_invite_code(&pool, "INV123").await.unwrap());

        // List
        create_invite_code(&pool, "INV456", "vault-1", "u1", "viewer", 1, None)
            .await
            .unwrap();
        let all = list_invite_codes(&pool, "vault-1").await.unwrap();
        assert_eq!(all.len(), 2);

        // Delete
        assert!(delete_invite_code(&pool, "INV456").await.unwrap());
        assert!(!delete_invite_code(&pool, "INV456").await.unwrap());
        let remaining = list_invite_codes(&pool, "vault-1").await.unwrap();
        assert_eq!(remaining.len(), 1);
    }

    #[test]
    fn invite_code_expired() {
        let code = InviteCodeRecord {
            code: "EXP1".into(),
            vault_id: "v1".into(),
            created_by: "u1".into(),
            role: "member".into(),
            max_uses: 10,
            used_count: 0,
            expires_at: Some(1), // long expired
            created_at: 0,
        };
        assert!(!is_invite_code_valid(&code));
    }
}
