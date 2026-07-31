use crate::db::*;
use sqlx::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

// ── Epic 34: Multi-user record types ─────────────────────────────────

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct UserRecord {
    pub user_id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub auth_provider: String,
    pub auth_subject: Option<String>,
    pub created_at: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct VaultMemberRecord {
    pub user_id: String,
    pub vault_id: String,
    pub role: String,
    pub invited_by: Option<String>,
    pub joined_at: i64,
}

pub fn new_user_id() -> String {
    Uuid::new_v4().to_string()
}

// ── Epic 34: Multi-user CRUD ─────────────────────────────────────────

pub async fn create_user(
    pool: &SqlitePool,
    user_id: &str,
    display_name: &str,
    email: Option<&str>,
    auth_provider: &str,
    auth_subject: Option<&str>,
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "INSERT INTO users (user_id, display_name, email, auth_provider, auth_subject, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(display_name)
    .bind(email)
    .bind(auth_provider)
    .bind(auth_subject)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_user(pool: &SqlitePool, user_id: &str) -> Result<Option<UserRecord>, sqlx::Error> {
    sqlx::query_as::<_, UserRecord>(
        "SELECT user_id, display_name, email, auth_provider, auth_subject, created_at \
         FROM users WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

pub async fn list_users(pool: &SqlitePool) -> Result<Vec<UserRecord>, sqlx::Error> {
    sqlx::query_as::<_, UserRecord>(
        "SELECT user_id, display_name, email, auth_provider, auth_subject, created_at \
         FROM users ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await
}

pub async fn update_user_display_name(
    pool: &SqlitePool,
    user_id: &str,
    display_name: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE users SET display_name = ? WHERE user_id = ?")
        .bind(display_name)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_user(pool: &SqlitePool, user_id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM users WHERE user_id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ── Vault Members ──

pub async fn add_vault_member(
    pool: &SqlitePool,
    user_id: &str,
    vault_id: &str,
    role: &str,
    invited_by: Option<&str>,
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "INSERT INTO vault_members (user_id, vault_id, role, invited_by, joined_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(vault_id)
    .bind(role)
    .bind(invited_by)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_vault_member(
    pool: &SqlitePool,
    user_id: &str,
    vault_id: &str,
) -> Result<Option<VaultMemberRecord>, sqlx::Error> {
    sqlx::query_as::<_, VaultMemberRecord>(
        "SELECT user_id, vault_id, role, invited_by, joined_at \
         FROM vault_members WHERE user_id = ? AND vault_id = ?",
    )
    .bind(user_id)
    .bind(vault_id)
    .fetch_optional(pool)
    .await
}

/// Count members in a vault. v0.3.19: used by `/api/vault/status` to drive
/// adaptive UI — Google login button stays hidden when count == 1 (solo user).
#[allow(dead_code)]
pub async fn count_vault_members(pool: &SqlitePool, vault_id: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM vault_members WHERE vault_id = ?")
        .bind(vault_id)
        .fetch_one(pool)
        .await
}

pub async fn list_vault_members(
    pool: &SqlitePool,
    vault_id: &str,
) -> Result<Vec<VaultMemberRecord>, sqlx::Error> {
    sqlx::query_as::<_, VaultMemberRecord>(
        "SELECT user_id, vault_id, role, invited_by, joined_at \
         FROM vault_members WHERE vault_id = ? ORDER BY joined_at ASC",
    )
    .bind(vault_id)
    .fetch_all(pool)
    .await
}

pub async fn update_vault_member_role(
    pool: &SqlitePool,
    user_id: &str,
    vault_id: &str,
    new_role: &str,
) -> Result<bool, sqlx::Error> {
    let result =
        sqlx::query("UPDATE vault_members SET role = ? WHERE user_id = ? AND vault_id = ?")
            .bind(new_role)
            .bind(user_id)
            .bind(vault_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn remove_vault_member(
    pool: &SqlitePool,
    user_id: &str,
    vault_id: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM vault_members WHERE user_id = ? AND vault_id = ?")
        .bind(user_id)
        .bind(vault_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ── Epic 34.0b: Single→Multi-user migration ─────────────────────────

/// Migrates a single-user vault to the multi-user schema.
///
/// If the `users` table is empty and a `local_device_identity` exists,
/// auto-creates an owner user, links the existing device, and adds a
/// vault_member entry with `role = 'owner'`.
///
/// Returns `true` if migration was performed, `false` if already migrated or
/// no device identity exists yet.
pub async fn migrate_single_to_multi_user(
    pool: &SqlitePool,
    vault_id: &str,
) -> Result<bool, sqlx::Error> {
    // Already migrated?
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    if user_count > 0 {
        return Ok(false);
    }

    // Need existing device identity to migrate
    let device = match get_local_device_identity(pool).await? {
        Some(d) => d,
        None => return Ok(false),
    };

    let now = epoch_secs();
    let owner_user_id = new_user_id();

    let placeholder_pubkey = vec![0u8; 32];

    // Create owner user
    sqlx::query(
        "INSERT INTO users (user_id, display_name, email, auth_provider, auth_subject, created_at) \
         VALUES (?, ?, NULL, 'local', NULL, ?)",
    )
    .bind(&owner_user_id)
    .bind(&device.device_name)
    .bind(now)
    .execute(pool)
    .await?;

    // Link existing device to owner (wrapped_vault_key = NULL — owner derives VK from passphrase)
    sqlx::query(
        "INSERT INTO devices (device_id, user_id, device_name, public_key, \
         wrapped_vault_key, vault_key_generation, revoked_at, last_seen_at, created_at) \
         VALUES (?, ?, ?, ?, NULL, NULL, NULL, ?, ?)",
    )
    .bind(&device.device_id)
    .bind(&owner_user_id)
    .bind(&device.device_name)
    .bind(&placeholder_pubkey)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    // Owner vault membership
    sqlx::query(
        "INSERT INTO vault_members (user_id, vault_id, role, invited_by, joined_at) \
         VALUES (?, ?, 'owner', NULL, ?)",
    )
    .bind(&owner_user_id)
    .bind(vault_id)
    .bind(now)
    .execute(pool)
    .await?;

    // Audit log
    sqlx::query(
        "INSERT INTO audit_logs (timestamp, actor_user_id, actor_device_id, action, \
         target_user_id, target_device_id, details, vault_id) \
         VALUES (?, ?, ?, 'migrate_single_to_multi', ?, ?, 'auto-migration from single-user vault', ?)",
    )
    .bind(now)
    .bind(&owner_user_id)
    .bind(&device.device_id)
    .bind(&owner_user_id)
    .bind(&device.device_id)
    .bind(vault_id)
    .execute(pool)
    .await?;

    Ok(true)
}

/// After grafting from a snapshot, the local device may not appear in the `devices`
/// multi-user table (the snapshot only contains the source device's entries).
/// This function registers the local device under the vault owner so that session
/// creation works on the newly joined device.
/// Safe to call at every startup — no-op when the device is already registered.
pub async fn ensure_local_device_in_vault(
    pool: &SqlitePool,
    vault_id: &str,
) -> Result<bool, sqlx::Error> {
    let device = match get_local_device_identity(pool).await? {
        Some(d) => d,
        None => return Ok(false),
    };

    // Already in multi-user devices table?
    if get_device(pool, &device.device_id).await?.is_some() {
        return Ok(false);
    }

    // Find the vault owner to associate this device with
    let owner_user_id: Option<String> = sqlx::query_scalar(
        "SELECT user_id FROM vault_members WHERE vault_id = ? AND role = 'owner' LIMIT 1",
    )
    .bind(vault_id)
    .fetch_optional(pool)
    .await?;

    let user_id = match owner_user_id {
        Some(id) => id,
        None => return Ok(false),
    };

    let now = epoch_secs();
    let placeholder_pubkey = vec![0u8; 32];

    sqlx::query(
        "INSERT OR IGNORE INTO devices \
         (device_id, user_id, device_name, public_key, wrapped_vault_key, vault_key_generation, \
          revoked_at, last_seen_at, created_at) \
         VALUES (?, ?, ?, ?, NULL, NULL, NULL, ?, ?)",
    )
    .bind(&device.device_id)
    .bind(&user_id)
    .bind(&device.device_name)
    .bind(&placeholder_pubkey)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(true)
}

/// Asserts that `device_id` is linked (via `devices.user_id → vault_members`) to
/// `expected_vault_id`.  Skips the check when `expected_vault_id` is `"local-vault"`
/// (vault not yet initialised).  Returns `Err` describing the mismatch on failure —
/// the caller should panic, as a mismatch indicates wrong key-pairing.
pub async fn verify_vault_device_binding(
    pool: &SqlitePool,
    expected_vault_id: &str,
    device_id: &str,
) -> Result<(), String> {
    if expected_vault_id == "local-vault" {
        return Ok(());
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM devices d \
         JOIN vault_members vm ON vm.user_id = d.user_id \
         WHERE d.device_id = ? AND vm.vault_id = ?",
    )
    .bind(device_id)
    .bind(expected_vault_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("vault_id consistency query failed: {e}"))?;

    if count == 0 {
        return Err(format!(
            "device '{device_id}' is not bound to vault '{expected_vault_id}' — \
             possible vault_id / user_id mismatch after identity refactor"
        ));
    }
    Ok(())
}

/// Rewrites any legacy `owner-{device_id}` user IDs to UUID v4.
/// Safe to call at every startup — no-op when no legacy IDs remain.
pub async fn backfill_uuid_user_ids(pool: &SqlitePool) -> Result<u32, sqlx::Error> {
    let old_ids: Vec<String> =
        sqlx::query_scalar("SELECT user_id FROM users WHERE user_id LIKE 'owner-%'")
            .fetch_all(pool)
            .await?;
    if old_ids.is_empty() {
        return Ok(0);
    }

    let mut conn = pool.acquire().await?;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *conn)
        .await?;

    let mut count = 0u32;
    for old_id in &old_ids {
        let new_id = new_user_id();
        sqlx::query(
            "INSERT INTO users (user_id, display_name, email, auth_provider, auth_subject, created_at) \
             SELECT ?, display_name, email, auth_provider, auth_subject, created_at \
             FROM users WHERE user_id = ?",
        )
        .bind(&new_id)
        .bind(old_id)
        .execute(&mut *conn)
        .await?;

        for (table, col) in &[
            ("devices", "user_id"),
            ("vault_members", "user_id"),
            ("vault_members", "invited_by"),
            ("audit_logs", "actor_user_id"),
            ("audit_logs", "target_user_id"),
            ("user_sessions", "user_id"),
            ("invite_codes", "created_by"),
        ] {
            sqlx::query(&format!("UPDATE {table} SET {col} = ? WHERE {col} = ?"))
                .bind(&new_id)
                .bind(old_id)
                .execute(&mut *conn)
                .await?;
        }

        sqlx::query("DELETE FROM users WHERE user_id = ?")
            .bind(old_id)
            .execute(&mut *conn)
            .await?;
        count += 1;
    }

    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *conn)
        .await?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Epic 34: Multi-user CRUD tests ──────────────────────────────

    #[tokio::test]
    async fn user_crud_lifecycle() {
        let pool = init_db("sqlite::memory:").await.unwrap();

        // Create
        create_user(
            &pool,
            "u1",
            "Alice",
            Some("alice@example.com"),
            "local",
            None,
        )
        .await
        .unwrap();

        // Read
        let user = get_user(&pool, "u1").await.unwrap().unwrap();
        assert_eq!(user.display_name, "Alice");
        assert_eq!(user.email.as_deref(), Some("alice@example.com"));
        assert_eq!(user.auth_provider, "local");

        // List
        create_user(&pool, "u2", "Bob", None, "google", Some("goog-sub-1"))
            .await
            .unwrap();
        let all = list_users(&pool).await.unwrap();
        assert_eq!(all.len(), 2);

        // Update display name
        assert!(
            update_user_display_name(&pool, "u1", "Alice Z")
                .await
                .unwrap()
        );
        let updated = get_user(&pool, "u1").await.unwrap().unwrap();
        assert_eq!(updated.display_name, "Alice Z");

        // Update non-existent
        assert!(
            !update_user_display_name(&pool, "u999", "Ghost")
                .await
                .unwrap()
        );

        // Delete
        assert!(delete_user(&pool, "u2").await.unwrap());
        assert!(get_user(&pool, "u2").await.unwrap().is_none());
        assert!(!delete_user(&pool, "u2").await.unwrap());
    }

    #[tokio::test]
    async fn vault_member_crud_lifecycle() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_user(&pool, "u1", "Alice", None, "local", None)
            .await
            .unwrap();
        create_user(&pool, "u2", "Bob", None, "local", None)
            .await
            .unwrap();

        // Add members
        add_vault_member(&pool, "u1", "vault-1", "owner", None)
            .await
            .unwrap();
        add_vault_member(&pool, "u2", "vault-1", "member", Some("u1"))
            .await
            .unwrap();

        // Get
        let member = get_vault_member(&pool, "u2", "vault-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(member.role, "member");
        assert_eq!(member.invited_by.as_deref(), Some("u1"));

        // List
        let members = list_vault_members(&pool, "vault-1").await.unwrap();
        assert_eq!(members.len(), 2);

        // Update role
        assert!(
            update_vault_member_role(&pool, "u2", "vault-1", "admin")
                .await
                .unwrap()
        );
        let updated = get_vault_member(&pool, "u2", "vault-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.role, "admin");

        // Remove
        assert!(remove_vault_member(&pool, "u2", "vault-1").await.unwrap());
        assert!(
            get_vault_member(&pool, "u2", "vault-1")
                .await
                .unwrap()
                .is_none()
        );
        assert!(!remove_vault_member(&pool, "u2", "vault-1").await.unwrap());
    }

    // ── Epic 34.0b: Migration tests ────────────────────────────────

    #[tokio::test]
    async fn migrate_single_to_multi_user_creates_owner() {
        let pool = init_db("sqlite::memory:").await.unwrap();

        // Simulate existing single-user vault: device identity exists, no users
        upsert_local_device_identity(&pool, "dev-abc123", "TestPC", "tok-secret")
            .await
            .unwrap();

        // Migration should succeed
        let migrated = migrate_single_to_multi_user(&pool, "vault-42")
            .await
            .unwrap();
        assert!(migrated);

        // Verify owner user created with UUID v4
        let users = list_users(&pool).await.unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].user_id.len(), 36, "user_id must be UUID v4");
        assert!(
            !users[0].user_id.starts_with("owner-"),
            "user_id must not use legacy owner- prefix"
        );
        assert_eq!(users[0].display_name, "TestPC");
        assert_eq!(users[0].auth_provider, "local");
        let owner_uid = users[0].user_id.clone();

        // Verify device linked to owner
        let dev = get_device(&pool, "dev-abc123").await.unwrap().unwrap();
        assert_eq!(dev.user_id, owner_uid);
        assert_eq!(dev.device_name, "TestPC");
        assert!(dev.wrapped_vault_key.is_none()); // owner uses passphrase
        assert!(dev.revoked_at.is_none());

        // Verify vault membership
        let member = get_vault_member(&pool, &owner_uid, "vault-42")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(member.role, "owner");
        assert!(member.invited_by.is_none());

        // Verify audit log
        let logs = list_audit_logs(&pool, "vault-42", 10).await.unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].action, "migrate_single_to_multi");
        assert_eq!(logs[0].actor_user_id.as_deref(), Some(owner_uid.as_str()));
    }

    #[tokio::test]
    async fn migrate_single_to_multi_user_is_idempotent() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        upsert_local_device_identity(&pool, "dev-abc123", "TestPC", "tok-secret")
            .await
            .unwrap();

        // First migration
        assert!(
            migrate_single_to_multi_user(&pool, "vault-42")
                .await
                .unwrap()
        );

        // Second call is a no-op
        assert!(
            !migrate_single_to_multi_user(&pool, "vault-42")
                .await
                .unwrap()
        );

        // Still only one user
        let users = list_users(&pool).await.unwrap();
        assert_eq!(users.len(), 1);
    }

    #[tokio::test]
    async fn migrate_single_to_multi_user_noop_without_device() {
        let pool = init_db("sqlite::memory:").await.unwrap();

        // No device identity → migration is a no-op
        assert!(
            !migrate_single_to_multi_user(&pool, "vault-42")
                .await
                .unwrap()
        );
        let users = list_users(&pool).await.unwrap();
        assert!(users.is_empty());
    }

    #[tokio::test]
    async fn backfill_uuid_user_ids_renames_legacy() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        // Insert a legacy owner- user directly
        let now = epoch_secs();
        sqlx::query(
            "INSERT INTO users (user_id, display_name, email, auth_provider, auth_subject, created_at) \
             VALUES ('owner-dev-abc', 'Alice', NULL, 'local', NULL, ?)",
        )
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        // Insert a device referencing the legacy user
        sqlx::query(
            "INSERT INTO devices (device_id, user_id, device_name, public_key, created_at) \
             VALUES ('dev-abc', 'owner-dev-abc', 'PC', x'00', ?)",
        )
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

        let count = backfill_uuid_user_ids(&pool).await.unwrap();
        assert_eq!(count, 1);

        let users = list_users(&pool).await.unwrap();
        assert_eq!(users.len(), 1);
        assert!(!users[0].user_id.starts_with("owner-"));
        assert_eq!(users[0].user_id.len(), 36);

        let dev = get_device(&pool, "dev-abc").await.unwrap().unwrap();
        assert_eq!(dev.user_id, users[0].user_id);

        // Second call is no-op
        assert_eq!(backfill_uuid_user_ids(&pool).await.unwrap(), 0);
    }
}
