use crate::db::*;
use sqlx::FromRow;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct DeviceRecord {
    pub device_id: String,
    pub user_id: String,
    pub device_name: String,
    pub public_key: Vec<u8>,
    pub wrapped_vault_key: Option<Vec<u8>>,
    pub vault_key_generation: Option<i64>,
    pub revoked_at: Option<i64>,
    pub last_seen_at: Option<i64>,
    pub created_at: i64,
    pub enrolled_at: Option<i64>,
    pub kyber_public_key: Option<Vec<u8>>,
    pub wrapped_vault_key_kyber: Option<Vec<u8>>,
}

// ── Devices ──

pub async fn create_device(
    pool: &SqlitePool,
    device_id: &str,
    user_id: &str,
    device_name: &str,
    public_key: &[u8],
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "INSERT INTO devices (device_id, user_id, device_name, public_key, created_at, last_seen_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(device_id)
    .bind(user_id)
    .bind(device_name)
    .bind(public_key)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_device(
    pool: &SqlitePool,
    device_id: &str,
) -> Result<Option<DeviceRecord>, sqlx::Error> {
    sqlx::query_as::<_, DeviceRecord>(
        "SELECT device_id, user_id, device_name, public_key, wrapped_vault_key, \
         vault_key_generation, revoked_at, last_seen_at, created_at, enrolled_at, \
         kyber_public_key, wrapped_vault_key_kyber \
         FROM devices WHERE device_id = ?",
    )
    .bind(device_id)
    .fetch_optional(pool)
    .await
}

pub async fn set_device_safety_verified(
    pool: &SqlitePool,
    device_id: &str,
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query("UPDATE devices SET safety_numbers_verified_at = ? WHERE device_id = ?")
        .bind(now)
        .bind(device_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_device_safety_verified_at(
    pool: &SqlitePool,
    device_id: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let row: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT safety_numbers_verified_at FROM devices WHERE device_id = ?")
            .bind(device_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(ts,)| ts))
}

pub async fn list_devices_for_user(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<Vec<DeviceRecord>, sqlx::Error> {
    sqlx::query_as::<_, DeviceRecord>(
        "SELECT device_id, user_id, device_name, public_key, wrapped_vault_key, \
         vault_key_generation, revoked_at, last_seen_at, created_at, enrolled_at, \
         kyber_public_key, wrapped_vault_key_kyber \
         FROM devices WHERE user_id = ? ORDER BY created_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn set_device_wrapped_vault_key(
    pool: &SqlitePool,
    device_id: &str,
    wrapped_vault_key: &[u8],
    vault_key_generation: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devices SET wrapped_vault_key = ?, vault_key_generation = ? WHERE device_id = ?",
    )
    .bind(wrapped_vault_key)
    .bind(vault_key_generation)
    .bind(device_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn set_device_wrapped_vault_key_kyber(
    pool: &SqlitePool,
    device_id: &str,
    wrapped_vault_key_kyber: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devices SET wrapped_vault_key_kyber = ? WHERE device_id = ?")
        .bind(wrapped_vault_key_kyber)
        .bind(device_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Returns active devices for a user: non-revoked and with a wrapped vault key.
pub async fn get_active_devices_for_user(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<Vec<DeviceRecord>, sqlx::Error> {
    sqlx::query_as::<_, DeviceRecord>(
        "SELECT device_id, user_id, device_name, public_key, wrapped_vault_key, \
         vault_key_generation, revoked_at, last_seen_at, created_at, enrolled_at, \
         kyber_public_key, wrapped_vault_key_kyber \
         FROM devices WHERE user_id = ? AND revoked_at IS NULL AND wrapped_vault_key IS NOT NULL \
         ORDER BY created_at ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn revoke_device(pool: &SqlitePool, device_id: &str) -> Result<bool, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "UPDATE devices SET revoked_at = ?, wrapped_vault_key = NULL, \
         wrapped_vault_key_kyber = NULL, vault_key_generation = NULL \
         WHERE device_id = ? AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(device_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn set_device_public_key(
    pool: &SqlitePool,
    device_id: &str,
    public_key: &[u8],
) -> Result<bool, sqlx::Error> {
    let now = epoch_secs();
    let result =
        sqlx::query("UPDATE devices SET public_key = ?, enrolled_at = ? WHERE device_id = ?")
            .bind(public_key)
            .bind(now)
            .bind(device_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn touch_device_last_seen(pool: &SqlitePool, device_id: &str) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query("UPDATE devices SET last_seen_at = ? WHERE device_id = ?")
        .bind(now)
        .bind(device_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn device_crud_lifecycle() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_user(&pool, "u1", "Alice", None, "local", None)
            .await
            .unwrap();

        let pubkey = vec![0u8; 32];

        // Create device
        create_device(&pool, "dev1", "u1", "Laptop", &pubkey)
            .await
            .unwrap();

        // Read
        let dev = get_device(&pool, "dev1").await.unwrap().unwrap();
        assert_eq!(dev.device_name, "Laptop");
        assert_eq!(dev.user_id, "u1");
        assert_eq!(dev.public_key, pubkey);
        assert!(dev.wrapped_vault_key.is_none());
        assert!(dev.revoked_at.is_none());

        // List by user
        create_device(&pool, "dev2", "u1", "Phone", &pubkey)
            .await
            .unwrap();
        let devs = list_devices_for_user(&pool, "u1").await.unwrap();
        assert_eq!(devs.len(), 2);

        // Set wrapped vault key
        let wvk = vec![1u8; 48];
        assert!(
            set_device_wrapped_vault_key(&pool, "dev1", &wvk, 1)
                .await
                .unwrap()
        );
        let dev = get_device(&pool, "dev1").await.unwrap().unwrap();
        assert_eq!(dev.wrapped_vault_key.as_deref(), Some(wvk.as_slice()));
        assert_eq!(dev.vault_key_generation, Some(1));

        // Revoke
        assert!(revoke_device(&pool, "dev1").await.unwrap());
        let dev = get_device(&pool, "dev1").await.unwrap().unwrap();
        assert!(dev.revoked_at.is_some());

        // Double revoke returns false
        assert!(!revoke_device(&pool, "dev1").await.unwrap());

        // Touch last_seen
        touch_device_last_seen(&pool, "dev2").await.unwrap();
    }

    #[tokio::test]
    async fn set_and_get_safety_verified_roundtrip() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        sqlx::query(
            "INSERT INTO users (user_id, display_name, email, auth_provider, auth_subject, created_at) \
             VALUES ('u1', 'Test User', NULL, 'local', NULL, 1000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO devices (device_id, user_id, device_name, public_key, created_at) \
             VALUES ('d1', 'u1', 'test', X'0102', 1000)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let before = get_device_safety_verified_at(&pool, "d1").await.unwrap();
        assert!(before.is_none());

        set_device_safety_verified(&pool, "d1").await.unwrap();

        let after = get_device_safety_verified_at(&pool, "d1").await.unwrap();
        assert!(after.is_some());
        assert!(after.unwrap() > 0);
    }

    #[tokio::test]
    async fn set_and_read_device_wrapped_kyber() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        sqlx::query(
            "INSERT INTO users (user_id, display_name, email, auth_provider, auth_subject, created_at) \
             VALUES ('u-kyber', 'Test', NULL, 'local', NULL, 1000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO devices (device_id, user_id, device_name, public_key, created_at) \
             VALUES ('dev-x', 'u-kyber', 'PC', X'090909', 1000)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let kyber_pub = vec![0x22u8; 1184];
        let wrapped_kyber = vec![0x44u8; 1128];
        set_device_kyber_public_key(&pool, "dev-x", &kyber_pub)
            .await
            .unwrap();
        set_device_wrapped_vault_key_kyber(&pool, "dev-x", &wrapped_kyber)
            .await
            .unwrap();

        let dev = get_device(&pool, "dev-x").await.unwrap().unwrap();
        assert_eq!(dev.kyber_public_key.as_deref(), Some(kyber_pub.as_slice()));
        assert_eq!(
            dev.wrapped_vault_key_kyber.as_deref(),
            Some(wrapped_kyber.as_slice())
        );
    }

    #[tokio::test]
    async fn revoke_device_nulls_both_wraps() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        sqlx::query(
            "INSERT INTO users (user_id, display_name, email, auth_provider, auth_subject, created_at) \
             VALUES ('u-kyber', 'Test', NULL, 'local', NULL, 1000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO devices (device_id, user_id, device_name, public_key, created_at) \
             VALUES ('dev-x', 'u-kyber', 'PC', X'090909', 1000)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let kyber_pub = vec![0x22u8; 1184];
        let wrapped_kyber = vec![0x44u8; 1128];
        set_device_kyber_public_key(&pool, "dev-x", &kyber_pub)
            .await
            .unwrap();
        set_device_wrapped_vault_key_kyber(&pool, "dev-x", &wrapped_kyber)
            .await
            .unwrap();
        let wvk = vec![0x11u8; 48];
        set_device_wrapped_vault_key(&pool, "dev-x", &wvk, 1)
            .await
            .unwrap();

        assert!(revoke_device(&pool, "dev-x").await.unwrap());
        let dev = get_device(&pool, "dev-x").await.unwrap().unwrap();
        assert!(dev.revoked_at.is_some());
        assert!(dev.wrapped_vault_key.is_none());
        assert!(dev.wrapped_vault_key_kyber.is_none());
        assert!(
            dev.vault_key_generation.is_none(),
            "generation cleared on revoke"
        );
        assert!(
            dev.kyber_public_key.is_some(),
            "public key survives revoke by design"
        );
    }
}
