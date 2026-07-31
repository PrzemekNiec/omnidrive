#![allow(clippy::too_many_arguments, dead_code)]

use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, Row, SqlitePool};
use std::path::Path;
use std::str::FromStr;
use uuid::Uuid;

pub mod audit;
pub mod cache;
pub mod chunks;
pub mod conflicts;
pub mod device_identity;
pub mod devices;
pub mod graft;
pub mod ingest;
pub mod inodes;
pub mod invites;
pub mod metadata_backup;
pub mod migration_v2;
pub mod oauth;
pub mod packs;
pub mod projection;
pub mod providers;
pub mod recovery_keys;
pub mod revisions;
pub mod schema;
pub mod sessions;
pub mod shards;
pub mod shares;
pub mod stats;
pub mod sync_policies;
pub mod system_config;
pub mod uploads;
pub mod users;
pub mod vault_state;

pub use audit::*;
pub use cache::*;
pub use chunks::*;
pub use conflicts::*;
pub use device_identity::*;
pub use devices::*;
pub use graft::*;
pub use ingest::*;
pub use inodes::*;
pub use invites::*;
pub use metadata_backup::*;
pub use migration_v2::*;
pub use oauth::*;
pub use packs::*;
pub use projection::*;
pub use providers::*;
pub use recovery_keys::*;
pub use revisions::*;
pub use schema::*;
pub use sessions::*;
pub use shards::*;
pub use shares::*;
pub use stats::*;
pub use sync_policies::*;
pub use system_config::*;
pub use uploads::*;
pub use users::*;
pub use vault_state::*;

pub const SOFT_DELETE_GRACE_MS: i64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackStatus {
    Uploading,
    Healthy,
    Degraded,
    Unreadable,
}

impl PackStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uploading => "UPLOADING",
            Self::Healthy => "COMPLETED_HEALTHY",
            Self::Degraded => "COMPLETED_DEGRADED",
            Self::Unreadable => "UNREADABLE",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShardRole {
    Data,
    Parity,
}

impl ShardRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Data => "DATA",
            Self::Parity => "PARITY",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageMode {
    Ec2_1,
    SingleReplica,
    LocalOnly,
}

impl StorageMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ec2_1 => "EC_2_1",
            Self::SingleReplica => "SINGLE_REPLICA",
            Self::LocalOnly => "LOCAL_ONLY",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Self {
        match value {
            "SINGLE_REPLICA" => Self::SingleReplica,
            "LOCAL_ONLY" => Self::LocalOnly,
            _ => Self::Ec2_1,
        }
    }

    pub fn from_policy_type(value: &str) -> Self {
        match value {
            "STANDARD" => Self::SingleReplica,
            "LOCAL" => Self::LocalOnly,
            _ => Self::Ec2_1,
        }
    }
}

pub fn epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── α.C.b graft test helpers ──
    const USER_FIXTURE: &str = "user-fixture";

    fn temp_test_dir(tag: &str) -> std::path::PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        std::env::temp_dir().join(format!(
            "omnidrive-acb-{}-{}",
            tag,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    async fn build_source_vault(
        dir: &std::path::Path,
    ) -> Result<
        (
            sqlx::SqlitePool,
            std::path::PathBuf,
            Vec<u8>,
            String,
            i64,
            Vec<u8>,
            String,
        ),
        Box<dyn std::error::Error>,
    > {
        use crate::disaster_recovery::create_metadata_snapshot;
        use crate::vault::VaultKeyStore;

        let source_path = dir.join("source.db");
        let snapshot_path = dir.join("snapshot.db");
        let source_url = format!(
            "sqlite://{}",
            source_path.to_string_lossy().replace('\\', "/")
        );

        let source_pool = init_db(&source_url).await?;

        let store = VaultKeyStore::new();
        store.unlock(&source_pool, "test-pass").await?;
        let envelope_key = store.require_envelope_key().await?.to_vec();
        let safety = store
            .safety_numbers(USER_FIXTURE)
            .await
            .expect("source must produce safety numbers");

        let inode_id = create_inode(&source_pool, None, "graft-test.txt", "FILE", 42).await?;
        store.get_or_create_dek(&source_pool, inode_id).await?;
        let wrapped_dek: Vec<u8> =
            sqlx::query_scalar("SELECT wrapped_dek FROM data_encryption_keys WHERE inode_id = ?")
                .bind(inode_id)
                .fetch_one(&source_pool)
                .await?;

        let vault_id: String = sqlx::query_scalar("SELECT vault_id FROM vault_state WHERE id = 1")
            .fetch_one(&source_pool)
            .await?;

        insert_recovery_key(&source_pool, &vault_id, &[0xABu8; 40], 1, Some("test")).await?;

        sqlx::query("UPDATE vault_state SET legacy_read_key = ? WHERE id = 1")
            .bind(vec![0x5Au8; 60])
            .execute(&source_pool)
            .await?;

        create_metadata_snapshot(&source_pool, &snapshot_path).await?;

        Ok((
            source_pool,
            snapshot_path,
            envelope_key,
            safety,
            inode_id,
            wrapped_dek,
            vault_id,
        ))
    }

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

    #[tokio::test]
    async fn audit_log_lifecycle() {
        let pool = init_db("sqlite::memory:").await.unwrap();

        // Insert logs
        let id1 = insert_audit_log(
            &pool,
            "vault-1",
            "invite",
            Some("u1"),
            Some("dev1"),
            Some("u2"),
            None,
            Some(r#"{"role":"member"}"#),
        )
        .await
        .unwrap();
        assert!(id1 > 0);

        let id2 = insert_audit_log(
            &pool,
            "vault-1",
            "join",
            Some("u2"),
            Some("dev2"),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(id2 > id1);

        // List (DESC order)
        let logs = list_audit_logs(&pool, "vault-1", 10).await.unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].action, "join"); // most recent first
        assert_eq!(logs[1].action, "invite");

        // Limit
        let one = list_audit_logs(&pool, "vault-1", 1).await.unwrap();
        assert_eq!(one.len(), 1);

        // Different vault is empty
        let empty = list_audit_logs(&pool, "vault-other", 10).await.unwrap();
        assert!(empty.is_empty());
    }

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

    #[cfg(feature = "test-helpers")]
    async fn test_pool() -> SqlitePool {
        init_db("sqlite::memory:").await.unwrap()
    }

    #[cfg(feature = "test-helpers")]
    async fn seed_vault_state_v1(pool: &SqlitePool) {
        set_vault_config(pool, &[1u8; 16], 1, 65_536, 3, 1)
            .await
            .unwrap();
        set_vault_params(
            pool,
            &[2u8; 32],
            r#"{"mode":"LOCAL_VAULT","parameter_set_version":1,"memory_cost_kib":65536,"time_cost":3,"lanes":1}"#,
            "vault-test-001",
        )
        .await
        .unwrap();
    }

    #[cfg(feature = "test-helpers")]
    #[tokio::test]
    async fn migrate_kdf_params_tx_writes_all_fields() {
        use omnidrive_core::crypto::WRAPPED_KEY_LEN;

        let pool = test_pool().await;
        seed_vault_state_v1(&pool).await;

        let writes = KdfMigrationWrites {
            new_salt: &[7u8; 16],
            new_argon2_params_json: r#"{"mode":"LOCAL_VAULT","parameter_set_version":2,"memory_cost_kib":262144,"time_cost":3,"lanes":1}"#,
            new_param_version: 2,
            new_memory_cost_kib: 262_144,
            new_time_cost: 3,
            new_lanes: 1,
            new_encrypted_vault_key: &[9u8; WRAPPED_KEY_LEN],
            legacy_read_key_blob: &[5u8; 60],
            new_encrypted_device_private_key: Some(&[6u8; 60]),
        };
        migrate_kdf_params_tx(&pool, writes).await.unwrap();

        let cfg = get_vault_config(&pool).await.unwrap().unwrap();
        assert_eq!(cfg.parameter_set_version, 2);
        assert_eq!(cfg.memory_cost_kib, 262_144);
        assert_eq!(cfg.salt, vec![7u8; 16]);
        let v = get_vault_params(&pool).await.unwrap().unwrap();
        assert_eq!(v.encrypted_vault_key.unwrap(), vec![9u8; WRAPPED_KEY_LEN]);
        assert_eq!(
            get_legacy_read_key(&pool).await.unwrap().unwrap(),
            vec![5u8; 60]
        );
    }

    #[cfg(feature = "test-helpers")]
    #[tokio::test]
    async fn migrate_kdf_params_tx_rolls_back_on_failure() {
        use omnidrive_core::crypto::WRAPPED_KEY_LEN;

        let pool = test_pool().await;
        seed_vault_state_v1(&pool).await;

        set_migration_failpoint(true);
        let writes = KdfMigrationWrites {
            new_salt: &[7u8; 16],
            new_argon2_params_json: "{}",
            new_param_version: 2,
            new_memory_cost_kib: 262_144,
            new_time_cost: 3,
            new_lanes: 1,
            new_encrypted_vault_key: &[9u8; WRAPPED_KEY_LEN],
            legacy_read_key_blob: &[5u8; 60],
            new_encrypted_device_private_key: Some(&[6u8; 60]),
        };
        let result = migrate_kdf_params_tx(&pool, writes).await;
        set_migration_failpoint(false);

        assert!(result.is_err());
        let cfg = get_vault_config(&pool).await.unwrap().unwrap();
        assert_eq!(
            cfg.parameter_set_version, 1,
            "version must be unchanged after rollback"
        );
        assert!(
            get_legacy_read_key(&pool).await.unwrap().is_none(),
            "no legacy key written on rollback"
        );
        let v = get_vault_params(&pool).await.unwrap().unwrap();
        assert_eq!(
            v.master_key_salt,
            vec![2u8; 32],
            "salt unchanged after rollback"
        );
        assert!(
            v.encrypted_vault_key.is_none(),
            "encrypted_vault_key untouched after rollback"
        );
    }

    #[tokio::test]
    async fn graft_copies_encrypted_vault_key_generation_and_legacy_read_key()
    -> Result<(), Box<dyn std::error::Error>> {
        use tokio::fs;
        let dir = temp_test_dir("vaultstate");
        fs::create_dir_all(&dir).await?;

        let (source_pool, snapshot_path, _evk, _safety, _inode, _dek, _vid) =
            build_source_vault(&dir).await?;
        let source_evk: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT encrypted_vault_key FROM vault_state WHERE id = 1")
                .fetch_one(&source_pool)
                .await?;
        let source_gen: Option<i64> =
            sqlx::query_scalar("SELECT vault_key_generation FROM vault_state WHERE id = 1")
                .fetch_one(&source_pool)
                .await?;
        assert!(source_evk.is_some(), "source must have an envelope key");

        let target_url = format!(
            "sqlite://{}",
            dir.join("target.db").to_string_lossy().replace('\\', "/")
        );
        let target_pool = init_db(&target_url).await?;
        crate::vault::VaultKeyStore::new()
            .unlock(&target_pool, "test-pass")
            .await?;
        let dell_evk_before: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT encrypted_vault_key FROM vault_state WHERE id = 1")
                .fetch_one(&target_pool)
                .await?;

        graft_restored_metadata_snapshot(&target_pool, &snapshot_path).await?;

        let after_evk: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT encrypted_vault_key FROM vault_state WHERE id = 1")
                .fetch_one(&target_pool)
                .await?;
        let after_gen: Option<i64> =
            sqlx::query_scalar("SELECT vault_key_generation FROM vault_state WHERE id = 1")
                .fetch_one(&target_pool)
                .await?;
        let after_legacy: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT legacy_read_key FROM vault_state WHERE id = 1")
                .fetch_one(&target_pool)
                .await?;

        assert_eq!(after_evk, source_evk, "EVK must be adopted from snapshot");
        assert_ne!(
            after_evk, dell_evk_before,
            "EVK must overwrite the device's own"
        );
        assert_eq!(after_gen, source_gen, "generation must be adopted");
        assert_eq!(
            after_legacy,
            Some(vec![0x5Au8; 60]),
            "legacy_read_key must be grafted"
        );

        let _ = fs::remove_dir_all(&dir).await;
        Ok(())
    }

    #[tokio::test]
    async fn graft_copies_data_encryption_keys() -> Result<(), Box<dyn std::error::Error>> {
        use tokio::fs;
        let dir = temp_test_dir("deks");
        fs::create_dir_all(&dir).await?;

        let (source_pool, snapshot_path, _evk, _safety, inode_id, wrapped_dek, _vid) =
            build_source_vault(&dir).await?;
        let source_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM data_encryption_keys")
            .fetch_one(&source_pool)
            .await?;
        assert_eq!(source_count, 1, "source has exactly one DEK");

        let target_url = format!(
            "sqlite://{}",
            dir.join("target.db").to_string_lossy().replace('\\', "/")
        );
        let target_pool = init_db(&target_url).await?;

        graft_restored_metadata_snapshot(&target_pool, &snapshot_path).await?;

        let got = get_wrapped_dek(&target_pool, inode_id).await?;
        let got = got.expect("DEK must be grafted for the inode");
        assert_eq!(
            got.wrapped_dek, wrapped_dek,
            "wrapped DEK bytes must match source"
        );

        let src_dek_id: i64 =
            sqlx::query_scalar("SELECT dek_id FROM data_encryption_keys WHERE inode_id = ?")
                .bind(inode_id)
                .fetch_one(&source_pool)
                .await?;
        assert_eq!(got.dek_id, src_dek_id, "dek_id must be preserved verbatim");

        let target_dek_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM data_encryption_keys")
            .fetch_one(&target_pool)
            .await?;
        assert_eq!(
            target_dek_count, 1,
            "exactly one DEK must be grafted (no dupes/misses)"
        );

        let _ = fs::remove_dir_all(&dir).await;
        Ok(())
    }

    #[tokio::test]
    async fn graft_copies_vault_recovery_keys() -> Result<(), Box<dyn std::error::Error>> {
        use tokio::fs;
        let dir = temp_test_dir("recovery");
        fs::create_dir_all(&dir).await?;

        let (source_pool, snapshot_path, _evk, _safety, _inode, _dek, vault_id) =
            build_source_vault(&dir).await?;
        let src_id: i64 =
            sqlx::query_scalar("SELECT id FROM vault_recovery_keys WHERE vault_id = ?")
                .bind(&vault_id)
                .fetch_one(&source_pool)
                .await?;

        let target_url = format!(
            "sqlite://{}",
            dir.join("target.db").to_string_lossy().replace('\\', "/")
        );
        let target_pool = init_db(&target_url).await?;

        graft_restored_metadata_snapshot(&target_pool, &snapshot_path).await?;

        let active = list_active_recovery_keys(&target_pool, &vault_id).await?;
        assert_eq!(active.len(), 1, "the source recovery key must be grafted");
        assert_eq!(active[0].wrapped_vault_key, vec![0xABu8; 40]);
        assert_eq!(active[0].vk_generation, 1);

        let target_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vault_recovery_keys")
            .fetch_one(&target_pool)
            .await?;
        assert_eq!(
            target_count, 1,
            "exactly one recovery key grafted (no dupes/misses)"
        );

        let tgt_id: i64 =
            sqlx::query_scalar("SELECT id FROM vault_recovery_keys WHERE vault_id = ?")
                .bind(&vault_id)
                .fetch_one(&target_pool)
                .await?;
        assert_eq!(tgt_id, src_id, "recovery key id must be preserved verbatim");

        let _ = fs::remove_dir_all(&dir).await;
        Ok(())
    }

    #[tokio::test]
    async fn graft_makes_joining_device_derive_same_evk_safety_and_dek()
    -> Result<(), Box<dyn std::error::Error>> {
        use secrecy::ExposeSecret;
        use tokio::fs;
        let dir = temp_test_dir("roundtrip");
        fs::create_dir_all(&dir).await?;

        let (source_pool, snapshot_path, source_evk, source_safety, inode_id, _dek, _vid) =
            build_source_vault(&dir).await?;
        let source_store = crate::vault::VaultKeyStore::new();
        source_store.unlock(&source_pool, "test-pass").await?;
        let (_id, source_dek) = source_store
            .get_or_create_dek(&source_pool, inode_id)
            .await?;
        let source_dek_bytes = source_dek.expose_secret()[..].to_vec();

        let target_url = format!(
            "sqlite://{}",
            dir.join("target.db").to_string_lossy().replace('\\', "/")
        );
        let target_pool = init_db(&target_url).await?;
        crate::vault::VaultKeyStore::new()
            .unlock(&target_pool, "test-pass")
            .await?;

        graft_restored_metadata_snapshot(&target_pool, &snapshot_path).await?;

        let joined = crate::vault::VaultKeyStore::new();
        joined.unlock(&target_pool, "test-pass").await?;

        let joined_evk = joined.require_envelope_key().await?.to_vec();
        assert_eq!(joined_evk, source_evk, "joined EVK must equal source EVK");

        let joined_safety = joined.safety_numbers(USER_FIXTURE).await.unwrap();
        assert_eq!(
            joined_safety, source_safety,
            "safety numbers must match (P1-005)"
        );

        let (_id2, joined_dek) = joined.get_or_create_dek(&target_pool, inode_id).await?;
        assert_eq!(
            joined_dek.expose_secret()[..].to_vec(),
            source_dek_bytes,
            "grafted DEK must unwrap to the same plaintext (P1-001)"
        );

        let _ = fs::remove_dir_all(&dir).await;
        Ok(())
    }

    #[tokio::test]
    async fn graft_from_legacy_v1_snapshot_does_not_panic() -> Result<(), Box<dyn std::error::Error>>
    {
        use tokio::fs;
        let dir = temp_test_dir("v1compat");
        fs::create_dir_all(&dir).await?;

        let source_url = format!(
            "sqlite://{}",
            dir.join("source.db").to_string_lossy().replace('\\', "/")
        );
        let source_pool = init_db(&source_url).await?;
        sqlx::query(
            "INSERT INTO vault_state (id, master_key_salt, argon2_params, vault_id) \
             VALUES (1, ?, ?, ?)",
        )
        .bind(vec![1u8; 16])
        .bind("v1-params")
        .bind("vault-legacy")
        .execute(&source_pool)
        .await?;

        let snapshot_path = dir.join("snapshot.db");
        crate::disaster_recovery::create_metadata_snapshot(&source_pool, &snapshot_path).await?;

        let target_url = format!(
            "sqlite://{}",
            dir.join("target.db").to_string_lossy().replace('\\', "/")
        );
        let target_pool = init_db(&target_url).await?;

        graft_restored_metadata_snapshot(&target_pool, &snapshot_path).await?;

        let evk: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT encrypted_vault_key FROM vault_state WHERE id = 1")
                .fetch_one(&target_pool)
                .await?;
        assert!(
            evk.is_none(),
            "V1 snapshot has no envelope key — must stay NULL"
        );
        let dek_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM data_encryption_keys")
            .fetch_one(&target_pool)
            .await?;
        assert_eq!(dek_count, 0);

        let _ = fs::remove_dir_all(&dir).await;
        Ok(())
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

    #[tokio::test]
    async fn store_and_read_kyber_keypair() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        upsert_local_device_identity(&pool, "dev-kyber", "TestPC", "tok-1")
            .await
            .unwrap();

        let sealed_priv = vec![0x11u8; 2428];
        let kyber_pub = vec![0x22u8; 1184];
        store_kyber_keypair(&pool, &sealed_priv, &kyber_pub)
            .await
            .unwrap();

        let device = get_local_device_identity(&pool).await.unwrap().unwrap();
        assert_eq!(
            device.encrypted_kyber_private_key.as_deref(),
            Some(sealed_priv.as_slice())
        );
        assert_eq!(
            device.kyber_public_key.as_deref(),
            Some(kyber_pub.as_slice())
        );
    }

    // ── β.1 graft_roster_additive tests ─────────────────────────────────────

    async fn build_roster_snapshot(
        path: &std::path::Path,
        vault_id: &str,
        devices: &[(&str, &str, Option<i64>)],
        members: &[(&str, &str)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        use tokio::fs;
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).await?;
        }
        let url = format!("sqlite://{}", path.to_string_lossy().replace('\\', "/"));
        let pool = init_db(&url).await?;

        sqlx::query(
            "INSERT INTO vault_state (id, master_key_salt, argon2_params, vault_id) \
             VALUES (1, ?, 'test', ?)",
        )
        .bind(vec![0u8; 16])
        .bind(vault_id)
        .execute(&pool)
        .await?;

        for (device_id, user_id, revoked_at) in devices {
            sqlx::query(
                "INSERT OR IGNORE INTO users \
                 (user_id, display_name, email, auth_provider, auth_subject, created_at) \
                 VALUES (?, 'Test', NULL, 'local', NULL, 1000)",
            )
            .bind(user_id)
            .execute(&pool)
            .await?;
            sqlx::query(
                "INSERT INTO devices \
                 (device_id, user_id, device_name, public_key, created_at, revoked_at) \
                 VALUES (?, ?, 'PC', X'01', 1000, ?)",
            )
            .bind(device_id)
            .bind(user_id)
            .bind(revoked_at)
            .execute(&pool)
            .await?;
        }

        for (user_id, role) in members {
            sqlx::query(
                "INSERT OR IGNORE INTO users \
                 (user_id, display_name, email, auth_provider, auth_subject, created_at) \
                 VALUES (?, 'Test', NULL, 'local', NULL, 1000)",
            )
            .bind(user_id)
            .execute(&pool)
            .await?;
            sqlx::query(
                "INSERT INTO vault_members (user_id, vault_id, role, invited_by, joined_at) \
                 VALUES (?, ?, ?, NULL, 1000)",
            )
            .bind(user_id)
            .bind(vault_id)
            .bind(role)
            .execute(&pool)
            .await?;
        }

        pool.close().await;
        drop(pool);
        Ok(())
    }

    #[tokio::test]
    async fn graft_roster_additive_adds_missing_device() -> Result<(), Box<dyn std::error::Error>> {
        use tokio::fs;
        let dir = temp_test_dir("roster-add");
        fs::create_dir_all(&dir).await?;

        let target_url = format!(
            "sqlite://{}",
            dir.join("target.db").to_string_lossy().replace('\\', "/")
        );
        let target = init_db(&target_url).await?;
        sqlx::query(
            "INSERT INTO vault_state (id, master_key_salt, argon2_params, vault_id) \
             VALUES (1, ?, 'test', 'v1')",
        )
        .bind(vec![0u8; 16])
        .execute(&target)
        .await?;
        sqlx::query(
            "INSERT INTO users (user_id, display_name, email, auth_provider, auth_subject, created_at) \
             VALUES ('u1', 'Owner', NULL, 'local', NULL, 1000)",
        )
        .execute(&target)
        .await?;
        sqlx::query(
            "INSERT INTO devices (device_id, user_id, device_name, public_key, created_at) \
             VALUES ('A', 'u1', 'PC-A', X'01', 1000)",
        )
        .execute(&target)
        .await?;

        let snap = dir.join("snap.db");
        build_roster_snapshot(&snap, "v1", &[("A", "u1", None), ("B", "u1", None)], &[]).await?;

        let summary = graft_roster_additive(&target, &snap, "v1").await?;
        assert_eq!(summary.devices_added, 1, "only device B should be added");

        let dev_b = get_device(&target, "B").await?;
        assert!(dev_b.is_some(), "device B must exist after graft");
        let dev_a = get_device(&target, "A").await?;
        assert!(dev_a.is_some(), "device A must still exist");

        let _ = fs::remove_dir_all(&dir).await;
        Ok(())
    }

    #[tokio::test]
    async fn graft_roster_additive_does_not_clobber_existing_device()
    -> Result<(), Box<dyn std::error::Error>> {
        use tokio::fs;
        let dir = temp_test_dir("roster-noclobber");
        fs::create_dir_all(&dir).await?;

        let target_url = format!(
            "sqlite://{}",
            dir.join("target.db").to_string_lossy().replace('\\', "/")
        );
        let target = init_db(&target_url).await?;
        sqlx::query(
            "INSERT INTO vault_state (id, master_key_salt, argon2_params, vault_id) \
             VALUES (1, ?, 'test', 'v1')",
        )
        .bind(vec![0u8; 16])
        .execute(&target)
        .await?;
        sqlx::query(
            "INSERT INTO users (user_id, display_name, email, auth_provider, auth_subject, created_at) \
             VALUES ('u1', 'Owner', NULL, 'local', NULL, 1000)",
        )
        .execute(&target)
        .await?;
        sqlx::query(
            "INSERT INTO devices \
             (device_id, user_id, device_name, public_key, created_at, revoked_at) \
             VALUES ('A', 'u1', 'PC-A', X'01', 1000, 123)",
        )
        .execute(&target)
        .await?;

        let snap = dir.join("snap.db");
        build_roster_snapshot(&snap, "v1", &[("A", "u1", None)], &[]).await?;

        graft_roster_additive(&target, &snap, "v1").await?;

        let dev_a = get_device(&target, "A").await?.unwrap();
        assert_eq!(
            dev_a.revoked_at,
            Some(123),
            "INSERT OR IGNORE must not overwrite local revoked_at"
        );

        let _ = fs::remove_dir_all(&dir).await;
        Ok(())
    }

    #[tokio::test]
    async fn graft_roster_additive_never_touches_dek() -> Result<(), Box<dyn std::error::Error>> {
        use tokio::fs;
        let dir = temp_test_dir("roster-dek-guard");
        fs::create_dir_all(&dir).await?;

        let target_url = format!(
            "sqlite://{}",
            dir.join("target.db").to_string_lossy().replace('\\', "/")
        );
        let target = init_db(&target_url).await?;
        sqlx::query(
            "INSERT INTO vault_state \
             (id, master_key_salt, argon2_params, vault_id, encrypted_vault_key, vault_key_generation) \
             VALUES (1, ?, 'test', 'v1', X'AABB', 7)",
        )
        .bind(vec![0u8; 16])
        .execute(&target)
        .await?;
        let inode_id = create_inode(&target, None, "guard.txt", "FILE", 10).await?;
        insert_wrapped_dek(&target, inode_id, &[0xCCu8; 32], 1, 7).await?;

        let before_dek_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM data_encryption_keys")
            .fetch_one(&target)
            .await?;
        let before_dek_bytes: Vec<u8> =
            sqlx::query_scalar("SELECT wrapped_dek FROM data_encryption_keys WHERE inode_id = ?")
                .bind(inode_id)
                .fetch_one(&target)
                .await?;
        let before_evk: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT encrypted_vault_key FROM vault_state WHERE id = 1")
                .fetch_one(&target)
                .await?;
        let before_gen: Option<i64> =
            sqlx::query_scalar("SELECT vault_key_generation FROM vault_state WHERE id = 1")
                .fetch_one(&target)
                .await?;

        let snap = dir.join("snap.db");
        {
            let url = format!("sqlite://{}", snap.to_string_lossy().replace('\\', "/"));
            let sp = init_db(&url).await?;
            sqlx::query(
                "INSERT INTO vault_state \
                 (id, master_key_salt, argon2_params, vault_id, encrypted_vault_key, vault_key_generation) \
                 VALUES (1, ?, 'test', 'v1', X'DDEE', 99)",
            )
            .bind(vec![1u8; 16])
            .execute(&sp)
            .await?;
            let snap_inode = create_inode(&sp, None, "snap.txt", "FILE", 5).await?;
            insert_wrapped_dek(&sp, snap_inode, &[0xFFu8; 32], 1, 99).await?;
            sp.close().await;
            drop(sp);
        }

        graft_roster_additive(&target, &snap, "v1").await?;

        let after_dek_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM data_encryption_keys")
            .fetch_one(&target)
            .await?;
        let after_dek_bytes: Vec<u8> =
            sqlx::query_scalar("SELECT wrapped_dek FROM data_encryption_keys WHERE inode_id = ?")
                .bind(inode_id)
                .fetch_one(&target)
                .await?;
        let after_evk: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT encrypted_vault_key FROM vault_state WHERE id = 1")
                .fetch_one(&target)
                .await?;
        let after_gen: Option<i64> =
            sqlx::query_scalar("SELECT vault_key_generation FROM vault_state WHERE id = 1")
                .fetch_one(&target)
                .await?;

        assert_eq!(
            after_dek_count, before_dek_count,
            "DEK row count must not change"
        );
        assert_eq!(
            after_dek_bytes, before_dek_bytes,
            "DEK bytes must be unchanged"
        );
        assert_eq!(after_evk, before_evk, "encrypted_vault_key must not change");
        assert_eq!(
            after_gen, before_gen,
            "vault_key_generation must not change"
        );

        let _ = fs::remove_dir_all(&dir).await;
        Ok(())
    }

    #[tokio::test]
    async fn graft_roster_additive_rejects_foreign_vault() -> Result<(), Box<dyn std::error::Error>>
    {
        use tokio::fs;
        let dir = temp_test_dir("roster-foreign");
        fs::create_dir_all(&dir).await?;

        let target_url = format!(
            "sqlite://{}",
            dir.join("target.db").to_string_lossy().replace('\\', "/")
        );
        let target = init_db(&target_url).await?;
        sqlx::query(
            "INSERT INTO vault_state (id, master_key_salt, argon2_params, vault_id) \
             VALUES (1, ?, 'test', 'v1')",
        )
        .bind(vec![0u8; 16])
        .execute(&target)
        .await?;

        let before_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM devices")
            .fetch_one(&target)
            .await?;

        let snap = dir.join("snap.db");
        build_roster_snapshot(&snap, "v-OTHER", &[("X", "u9", None)], &[]).await?;

        let result = graft_roster_additive(&target, &snap, "v1").await;
        assert!(result.is_err(), "must return Err for foreign vault");

        let after_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM devices")
            .fetch_one(&target)
            .await?;
        assert_eq!(
            after_count, before_count,
            "no device must be inserted on vault_id mismatch"
        );

        let _ = fs::remove_dir_all(&dir).await;
        Ok(())
    }

    #[tokio::test]
    async fn roster_snapshot_marker_round_trips() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        assert_eq!(
            get_last_applied_roster_snapshot_at(&pool).await.unwrap(),
            None
        );
        set_last_applied_roster_snapshot_at(&pool, 1234)
            .await
            .unwrap();
        assert_eq!(
            get_last_applied_roster_snapshot_at(&pool).await.unwrap(),
            Some(1234)
        );
        set_last_applied_roster_snapshot_at(&pool, 5678)
            .await
            .unwrap();
        assert_eq!(
            get_last_applied_roster_snapshot_at(&pool).await.unwrap(),
            Some(5678)
        );
    }

    #[tokio::test]
    async fn lineage_same_when_candidate_equals_current() -> Result<(), Box<dyn std::error::Error>>
    {
        let pool = init_db("sqlite::memory:").await?;
        let inode = create_inode(&pool, None, "f.txt", "FILE", 10).await?;
        let rev =
            create_file_revision(&pool, inode, 10, None, None, None, "local_write", None).await?;
        let rel = classify_revision_lineage(&pool, rev, rev).await?;
        assert_eq!(rel, RevisionLineageRelation::Same);
        Ok(())
    }

    #[tokio::test]
    async fn lineage_candidate_descends_from_current_is_fast_forward()
    -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let inode = create_inode(&pool, None, "f.txt", "FILE", 10).await?;
        let current =
            create_file_revision(&pool, inode, 10, None, None, None, "local_write", None).await?;
        let candidate = create_file_revision(
            &pool,
            inode,
            10,
            None,
            None,
            Some(current),
            "local_write",
            None,
        )
        .await?;
        let rel = classify_revision_lineage(&pool, candidate, current).await?;
        assert_eq!(rel, RevisionLineageRelation::CandidateDescendsFromCurrent);
        Ok(())
    }

    #[tokio::test]
    async fn lineage_current_descends_from_candidate_is_stale_base()
    -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let inode = create_inode(&pool, None, "f.txt", "FILE", 10).await?;
        let candidate =
            create_file_revision(&pool, inode, 10, None, None, None, "local_write", None).await?;
        let current = create_file_revision(
            &pool,
            inode,
            10,
            None,
            None,
            Some(candidate),
            "local_write",
            None,
        )
        .await?;
        let rel = classify_revision_lineage(&pool, candidate, current).await?;
        assert_eq!(rel, RevisionLineageRelation::CurrentDescendsFromCandidate);
        Ok(())
    }

    #[tokio::test]
    async fn lineage_siblings_are_parallel() -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let inode = create_inode(&pool, None, "f.txt", "FILE", 10).await?;
        let base =
            create_file_revision(&pool, inode, 10, None, None, None, "local_write", None).await?;
        let branch_a = create_file_revision(
            &pool,
            inode,
            10,
            None,
            None,
            Some(base),
            "local_write",
            None,
        )
        .await?;
        let branch_b = create_file_revision(
            &pool,
            inode,
            10,
            None,
            None,
            Some(base),
            "local_write",
            None,
        )
        .await?;
        let rel = classify_revision_lineage(&pool, branch_a, branch_b).await?;
        assert_eq!(rel, RevisionLineageRelation::Parallel);
        Ok(())
    }

    #[tokio::test]
    async fn materialize_conflict_copy_creates_inode_copies_chunks_and_records_event()
    -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let inode = create_inode(&pool, None, "report.txt", "FILE", 20).await?;
        let source = create_file_revision(
            &pool,
            inode,
            20,
            None,
            Some("dev-a"),
            None,
            "local_write",
            None,
        )
        .await?;
        register_chunk(&pool, source, &[1u8; 32], 0, 20).await?;

        let (copy_inode, copy_rev, name, conflict_id) = materialize_conflict_copy_from_revision(
            &pool,
            source,
            Some("dev-a"),
            "Laptop",
            "parallel_local_edit",
        )
        .await?;

        assert_ne!(copy_inode, inode, "conflict copy must be a distinct inode");
        assert!(
            name.starts_with("report (conflict - Laptop - "),
            "unexpected name: {name}"
        );
        assert!(
            name.ends_with(").txt"),
            "extension must be preserved: {name}"
        );

        let copied = get_chunk_refs_for_revision(&pool, copy_rev).await?;
        assert_eq!(
            copied.len(),
            1,
            "chunk refs must be copied so the conflict copy is recoverable"
        );
        assert_eq!(copied[0].size, 20);

        let events = list_recent_conflicts(&pool, 10).await?;
        let event = events
            .iter()
            .find(|e| e.conflict_id == conflict_id)
            .expect("conflict event surfaced");
        assert_eq!(event.reason, "parallel_local_edit");
        assert_eq!(event.inode_id, inode);
        assert_eq!(event.materialized_inode_id, Some(copy_inode));
        assert_eq!(event.materialized_revision_id, Some(copy_rev));
        Ok(())
    }

    #[tokio::test]
    async fn materialize_conflict_copy_disambiguates_name_on_collision()
    -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let inode = create_inode(&pool, None, "notes.md", "FILE", 5).await?;
        let source = create_file_revision(
            &pool,
            inode,
            5,
            None,
            Some("dev-a"),
            None,
            "local_write",
            None,
        )
        .await?;
        register_chunk(&pool, source, &[2u8; 32], 0, 5).await?;

        let (_i1, _r1, name1, _c1) = materialize_conflict_copy_from_revision(
            &pool,
            source,
            Some("dev-a"),
            "PC",
            "stale_local_base",
        )
        .await?;
        let (_i2, _r2, name2, _c2) = materialize_conflict_copy_from_revision(
            &pool,
            source,
            Some("dev-a"),
            "PC",
            "stale_local_base",
        )
        .await?;

        assert_ne!(name1, name2, "second copy must not collide with the first");
        assert!(
            name2.contains(" [1]"),
            "second copy must be disambiguated: {name2}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn root_level_inode_names_are_unique() -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        create_inode(&pool, None, "dup.txt", "FILE", 1).await?;
        let second = create_inode(&pool, None, "dup.txt", "FILE", 1).await;
        match second {
            Err(sqlx::Error::Database(err)) if err.is_unique_violation() => Ok(()),
            other => panic!("expected unique violation for duplicate root name, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn inodes_deleted_at_defaults_null() -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let inode = create_inode(&pool, None, "f.txt", "FILE", 1).await?;
        let deleted_at: Option<i64> =
            sqlx::query_scalar("SELECT deleted_at FROM inodes WHERE id = ?")
                .bind(inode)
                .fetch_one(&pool)
                .await?;
        assert_eq!(deleted_at, None);
        Ok(())
    }

    #[tokio::test]
    async fn soft_delete_sets_timestamp_and_preserves_chunks()
    -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let inode = create_inode(&pool, None, "f.txt", "FILE", 10).await?;
        let rev =
            create_file_revision(&pool, inode, 10, None, None, None, "local_write", None).await?;
        register_chunk(&pool, rev, &[7u8; 32], 0, 10).await?;

        let changed = soft_delete_inode(&pool, inode, 1_000).await?;
        assert!(changed);

        let deleted_at: Option<i64> =
            sqlx::query_scalar("SELECT deleted_at FROM inodes WHERE id = ?")
                .bind(inode)
                .fetch_one(&pool)
                .await?;
        assert_eq!(deleted_at, Some(1_000));

        let chunk_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM chunk_refs WHERE revision_id = ?")
                .bind(rev)
                .fetch_one(&pool)
                .await?;
        assert_eq!(chunk_count, 1, "soft-delete must not touch chunk_refs");

        let second = soft_delete_inode(&pool, inode, 2_000).await?;
        assert!(!second, "already soft-deleted → no change");
        Ok(())
    }

    #[tokio::test]
    async fn soft_deleted_excluded_from_lookup_but_visible_by_id()
    -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let inode = create_inode(&pool, None, "gone.txt", "FILE", 1).await?;
        soft_delete_inode(&pool, inode, 1_000).await?;

        assert!(
            get_inode_by_path(&pool, None, "gone.txt").await?.is_none(),
            "soft-deleted must not resolve by path"
        );
        assert!(
            resolve_path(&pool, "/gone.txt").await?.is_none(),
            "soft-deleted must not resolve as live"
        );
        assert!(
            get_inode_by_id(&pool, inode).await?.is_some(),
            "raw by-id must still see soft-deleted"
        );
        Ok(())
    }

    #[tokio::test]
    async fn list_and_restore_soft_deleted() -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let a = create_inode(&pool, None, "a.txt", "FILE", 5).await?;
        soft_delete_inode(&pool, a, 1_000).await?;

        let listed = list_soft_deleted(&pool).await?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].inode_id, a);
        assert_eq!(listed[0].name, "a.txt");
        assert_eq!(listed[0].deleted_at, 1_000);

        let name = restore_soft_deleted_inode(&pool, a).await?;
        assert_eq!(name, "a.txt");
        assert!(
            get_inode_by_path(&pool, None, "a.txt").await?.is_some(),
            "restored file resolves again"
        );
        assert!(list_soft_deleted(&pool).await?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn restore_disambiguates_on_name_collision() -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let old = create_inode(&pool, None, "dup.txt", "FILE", 1).await?;
        soft_delete_inode(&pool, old, 1_000).await?;
        create_inode(&pool, None, "dup.txt", "FILE", 1).await?;

        let name = restore_soft_deleted_inode(&pool, old).await?;
        assert_ne!(name, "dup.txt", "must not collide with live file");
        assert!(name.contains("restored"), "restored name: {name}");
        Ok(())
    }

    #[tokio::test]
    async fn list_expired_returns_only_past_cutoff() -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let old = create_inode(&pool, None, "old.txt", "FILE", 1).await?;
        let fresh = create_inode(&pool, None, "fresh.txt", "FILE", 1).await?;
        soft_delete_inode(&pool, old, 1_000).await?;
        soft_delete_inode(&pool, fresh, 9_000).await?;

        let expired = list_expired_soft_deleted(&pool, 5_000).await?;
        assert_eq!(expired, vec![old]);
        Ok(())
    }

    #[tokio::test]
    async fn sweeper_hard_deletes_expired_only() -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let old = create_inode(&pool, None, "old.txt", "FILE", 1).await?;
        let rev =
            create_file_revision(&pool, old, 1, None, None, None, "local_write", None).await?;
        register_chunk(&pool, rev, &[1u8; 32], 0, 1).await?;
        let fresh = create_inode(&pool, None, "fresh.txt", "FILE", 1).await?;
        soft_delete_inode(&pool, old, 1_000).await?;
        soft_delete_inode(&pool, fresh, 9_000).await?;

        for inode_id in list_expired_soft_deleted(&pool, 5_000).await? {
            delete_file_chunks(&pool, inode_id).await?;
            delete_inode_record(&pool, inode_id).await?;
        }

        assert!(
            get_inode_by_id(&pool, old).await?.is_none(),
            "expired hard-deleted"
        );
        assert!(
            get_inode_by_id(&pool, fresh).await?.is_some(),
            "fresh survives"
        );
        let chunks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunk_refs")
            .fetch_one(&pool)
            .await?;
        assert_eq!(chunks, 0, "expired file chunks reclaimed");
        Ok(())
    }
}
