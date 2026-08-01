use sqlx::Row;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqlitePoolOptions;
use std::str::FromStr;

#[allow(dead_code)]
pub async fn init_db(db_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(db_url)
        .map_err(|err| sqlx::Error::Configuration(Box::new(err)))?
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .connect_with(options)
        .await?;
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await?;

    sqlx::query("DROP TABLE IF EXISTS files")
        .execute(&pool)
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS vault_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            master_key_salt BLOB NOT NULL,
            argon2_params TEXT NOT NULL,
            vault_id TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS vault_config (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            salt BLOB NOT NULL,
            parameter_set_version INTEGER NOT NULL,
            memory_cost_kib INTEGER NOT NULL,
            time_cost INTEGER NOT NULL,
            lanes INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS system_config (
            config_key TEXT PRIMARY KEY,
            config_value TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS provider_configs (
            provider_name TEXT PRIMARY KEY,
            endpoint TEXT NOT NULL,
            region TEXT NOT NULL,
            bucket TEXT NOT NULL,
            force_path_style INTEGER NOT NULL DEFAULT 0,
            enabled INTEGER NOT NULL DEFAULT 0,
            draft_source TEXT,
            last_test_status TEXT,
            last_test_error TEXT,
            last_test_at INTEGER,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS provider_secrets (
            provider_name TEXT PRIMARY KEY REFERENCES provider_configs(provider_name) ON DELETE CASCADE,
            access_key_id_ciphertext BLOB NOT NULL,
            secret_access_key_ciphertext BLOB NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS cloud_usage_daily (
            day_epoch INTEGER PRIMARY KEY,
            read_ops INTEGER NOT NULL DEFAULT 0,
            write_ops INTEGER NOT NULL DEFAULT 0,
            egress_bytes INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS inodes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            parent_id INTEGER REFERENCES inodes(id),
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            size INTEGER DEFAULT 0,
            mode INTEGER,
            mtime INTEGER,
            UNIQUE(parent_id, name)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS file_revisions (
            revision_id INTEGER PRIMARY KEY AUTOINCREMENT,
            inode_id INTEGER NOT NULL REFERENCES inodes(id) ON DELETE CASCADE,
            created_at INTEGER NOT NULL,
            size INTEGER NOT NULL,
            is_current INTEGER NOT NULL DEFAULT 0,
            immutable_until INTEGER,
            device_id TEXT,
            parent_revision_id INTEGER REFERENCES file_revisions(revision_id) ON DELETE SET NULL,
            origin TEXT NOT NULL DEFAULT 'local',
            conflict_reason TEXT
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS local_device_identity (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            device_id TEXT NOT NULL UNIQUE,
            device_name TEXT NOT NULL,
            peer_token TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // Epic 34.1a: add X25519 keypair columns (existing DBs)
    let _ = sqlx::query("ALTER TABLE local_device_identity ADD COLUMN encrypted_private_key BLOB")
        .execute(&pool)
        .await;
    let _ = sqlx::query("ALTER TABLE local_device_identity ADD COLUMN public_key BLOB")
        .execute(&pool)
        .await;
    let _ = sqlx::query(
        "ALTER TABLE local_device_identity ADD COLUMN encrypted_kyber_private_key BLOB",
    )
    .execute(&pool)
    .await;
    let _ = sqlx::query("ALTER TABLE local_device_identity ADD COLUMN kyber_public_key BLOB")
        .execute(&pool)
        .await;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS trusted_peers (
            peer_id TEXT PRIMARY KEY,
            device_name TEXT NOT NULL,
            vault_id TEXT NOT NULL,
            peer_api_base TEXT NOT NULL,
            trusted INTEGER NOT NULL DEFAULT 1,
            last_seen_at INTEGER NOT NULL,
            last_handshake_at INTEGER,
            last_error TEXT
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS conflict_events (
            conflict_id INTEGER PRIMARY KEY AUTOINCREMENT,
            inode_id INTEGER NOT NULL REFERENCES inodes(id) ON DELETE CASCADE,
            winning_revision_id INTEGER NOT NULL REFERENCES file_revisions(revision_id) ON DELETE CASCADE,
            losing_revision_id INTEGER NOT NULL REFERENCES file_revisions(revision_id) ON DELETE CASCADE,
            reason TEXT NOT NULL,
            materialized_inode_id INTEGER REFERENCES inodes(id) ON DELETE SET NULL,
            materialized_revision_id INTEGER REFERENCES file_revisions(revision_id) ON DELETE SET NULL,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sync_policies (
            policy_id INTEGER PRIMARY KEY AUTOINCREMENT,
            path_prefix TEXT NOT NULL UNIQUE,
            require_healthy INTEGER NOT NULL DEFAULT 1,
            enable_versioning INTEGER NOT NULL DEFAULT 1,
            policy_type TEXT NOT NULL DEFAULT 'PARANOIA'
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS smart_sync_state (
            inode_id INTEGER PRIMARY KEY REFERENCES inodes(id) ON DELETE CASCADE,
            revision_id INTEGER NOT NULL REFERENCES file_revisions(revision_id) ON DELETE CASCADE,
            pin_state INTEGER NOT NULL DEFAULT 0,
            hydration_state INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS metadata_backups (
            backup_id TEXT PRIMARY KEY,
            created_at INTEGER NOT NULL,
            snapshot_version INTEGER NOT NULL,
            object_key TEXT NOT NULL,
            provider TEXT NOT NULL,
            encrypted_size INTEGER NOT NULL,
            status TEXT NOT NULL,
            last_error TEXT
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS cache_entries (
            cache_key TEXT PRIMARY KEY,
            inode_id INTEGER NOT NULL REFERENCES inodes(id) ON DELETE CASCADE,
            revision_id INTEGER NOT NULL REFERENCES file_revisions(revision_id) ON DELETE CASCADE,
            chunk_index INTEGER NOT NULL,
            pack_id TEXT NOT NULL,
            file_path TEXT NOT NULL,
            cache_path TEXT NOT NULL,
            size INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            last_accessed_at INTEGER NOT NULL,
            access_count INTEGER NOT NULL DEFAULT 0,
            is_prefetched INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chunk_refs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            revision_id INTEGER REFERENCES file_revisions(revision_id) ON DELETE CASCADE,
            chunk_id BLOB NOT NULL,
            file_offset INTEGER NOT NULL,
            size INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS packs (
            pack_id TEXT PRIMARY KEY,
            chunk_id BLOB NOT NULL,
            plaintext_hash TEXT,
            storage_mode TEXT NOT NULL DEFAULT 'EC_2_1',
            encryption_version INTEGER NOT NULL,
            ec_scheme TEXT NOT NULL DEFAULT 'rs_2_1',
            logical_size INTEGER NOT NULL,
            cipher_size INTEGER NOT NULL,
            shard_size INTEGER NOT NULL,
            nonce BLOB NOT NULL,
            gcm_tag BLOB NOT NULL,
            status TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS pack_shards (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            pack_id TEXT NOT NULL REFERENCES packs(pack_id) ON DELETE CASCADE,
            shard_index INTEGER NOT NULL,
            shard_role TEXT NOT NULL,
            provider TEXT NOT NULL,
            object_key TEXT NOT NULL,
            size INTEGER NOT NULL,
            checksum TEXT NOT NULL,
            status TEXT NOT NULL,
            attempts INTEGER DEFAULT 0,
            last_error TEXT,
            last_verified_at INTEGER,
            last_verification_method TEXT,
            last_verification_status TEXT,
            last_verified_size INTEGER,
            verification_failures INTEGER NOT NULL DEFAULT 0,
            UNIQUE(pack_id, shard_index),
            UNIQUE(pack_id, provider)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS pack_locations (
            chunk_id BLOB PRIMARY KEY,
            pack_id TEXT NOT NULL,
            pack_offset INTEGER NOT NULL,
            encrypted_size INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS upload_jobs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            pack_id TEXT UNIQUE NOT NULL,
            status TEXT NOT NULL,
            attempts INTEGER DEFAULT 0
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS upload_job_targets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            job_id INTEGER NOT NULL REFERENCES upload_jobs(id) ON DELETE CASCADE,
            provider TEXT NOT NULL,
            status TEXT NOT NULL,
            attempts INTEGER DEFAULT 0,
            last_error TEXT,
            bucket TEXT,
            object_key TEXT,
            etag TEXT,
            version_id TEXT,
            last_attempt_at INTEGER,
            updated_at INTEGER,
            completed_at INTEGER,
            UNIQUE(job_id, provider)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    ensure_column_exists(&pool, "upload_job_targets", "last_attempt_at", "INTEGER").await?;
    ensure_column_exists(&pool, "upload_job_targets", "updated_at", "INTEGER").await?;
    ensure_column_exists(&pool, "upload_jobs", "next_attempt_at", "INTEGER").await?;
    ensure_column_exists(&pool, "file_revisions", "device_id", "TEXT").await?;
    ensure_column_exists(
        &pool,
        "file_revisions",
        "parent_revision_id",
        "INTEGER REFERENCES file_revisions(revision_id) ON DELETE SET NULL",
    )
    .await?;
    ensure_column_exists(
        &pool,
        "file_revisions",
        "origin",
        "TEXT NOT NULL DEFAULT 'local'",
    )
    .await?;
    ensure_column_exists(&pool, "file_revisions", "conflict_reason", "TEXT").await?;
    ensure_column_exists(&pool, "inodes", "deleted_at", "INTEGER").await?;
    sqlx::query("DROP INDEX IF EXISTS idx_inodes_parent_name_root")
        .execute(&pool)
        .await?;
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_inodes_parent_name_root \
         ON inodes(COALESCE(parent_id, -1), name) WHERE deleted_at IS NULL",
    )
    .execute(&pool)
    .await?;
    ensure_column_exists(&pool, "pack_shards", "last_error", "TEXT").await?;
    ensure_column_exists(&pool, "pack_shards", "last_verified_at", "INTEGER").await?;
    ensure_column_exists(&pool, "pack_shards", "last_verification_method", "TEXT").await?;
    ensure_column_exists(&pool, "pack_shards", "last_verification_status", "TEXT").await?;
    ensure_column_exists(&pool, "pack_shards", "last_verified_size", "INTEGER").await?;
    ensure_column_exists(
        &pool,
        "pack_shards",
        "verification_failures",
        "INTEGER NOT NULL DEFAULT 0",
    )
    .await?;
    ensure_column_exists(&pool, "packs", "plaintext_hash", "TEXT").await?;
    ensure_column_exists(
        &pool,
        "packs",
        "storage_mode",
        "TEXT NOT NULL DEFAULT 'EC_2_1'",
    )
    .await?;
    ensure_column_exists(
        &pool,
        "chunk_refs",
        "revision_id",
        "INTEGER REFERENCES file_revisions(revision_id) ON DELETE CASCADE",
    )
    .await?;
    ensure_column_exists(
        &pool,
        "sync_policies",
        "policy_type",
        "TEXT NOT NULL DEFAULT 'PARANOIA'",
    )
    .await?;

    // ── Envelope Encryption (V2) schema additions ───────────────────────
    ensure_column_exists(
        &pool,
        "vault_state",
        "vault_format_version",
        "INTEGER NOT NULL DEFAULT 1",
    )
    .await?;
    ensure_column_exists(&pool, "vault_state", "encrypted_vault_key", "BLOB").await?;
    ensure_column_exists(&pool, "vault_state", "legacy_read_key", "BLOB").await?;
    ensure_column_exists(
        &pool,
        "vault_state",
        "vault_key_generation",
        "INTEGER NOT NULL DEFAULT 0",
    )
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS data_encryption_keys (
            dek_id          INTEGER PRIMARY KEY AUTOINCREMENT,
            inode_id        INTEGER NOT NULL,
            wrapped_dek     BLOB NOT NULL,
            key_version     INTEGER NOT NULL DEFAULT 1,
            vault_key_gen   INTEGER NOT NULL DEFAULT 1,
            created_at      INTEGER NOT NULL,
            UNIQUE(inode_id, key_version)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS pack_deks (
            pack_id TEXT PRIMARY KEY,
            dek_id  INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS ingest_jobs (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path       TEXT NOT NULL,
            file_size       INTEGER NOT NULL,
            state           TEXT NOT NULL DEFAULT 'PENDING',
            bytes_processed INTEGER NOT NULL DEFAULT 0,
            attempt_count   INTEGER NOT NULL DEFAULT 0,
            error_message   TEXT,
            created_at      INTEGER NOT NULL,
            updated_at      INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_ingest_jobs_state ON ingest_jobs(state)")
        .execute(&pool)
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS shared_links (
            share_id        TEXT PRIMARY KEY,
            inode_id        INTEGER NOT NULL,
            revision_id     INTEGER NOT NULL,
            file_name       TEXT NOT NULL,
            file_size       INTEGER NOT NULL,
            created_at      INTEGER NOT NULL,
            expires_at      INTEGER,
            max_downloads   INTEGER,
            download_count  INTEGER NOT NULL DEFAULT 0,
            revoked         INTEGER NOT NULL DEFAULT 0,
            password_hash   TEXT
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_shared_links_inode ON shared_links(inode_id)")
        .execute(&pool)
        .await?;

    // Migration: add password_hash column if missing (existing DBs)
    let _ = sqlx::query("ALTER TABLE shared_links ADD COLUMN password_hash TEXT")
        .execute(&pool)
        .await;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS share_pack_keys (
            share_id   TEXT NOT NULL,
            pack_id    TEXT NOT NULL,
            sealed_dek BLOB NOT NULL,
            PRIMARY KEY (share_id, pack_id)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS share_password_tokens (
            token       TEXT PRIMARY KEY,
            share_id    TEXT NOT NULL,
            created_at  INTEGER NOT NULL,
            expires_at  INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // ── Epic 34: Multi-user identity & membership tables ──────────────

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            user_id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            email TEXT,
            auth_provider TEXT NOT NULL DEFAULT 'local',
            auth_subject TEXT,
            created_at INTEGER NOT NULL,
            UNIQUE(auth_provider, auth_subject)
        )
        "#,
    )
    .execute(&pool)
    .await?;
    // Sesja C: store Google refresh_token for session auto-renewal
    ensure_column_exists(&pool, "users", "google_refresh_token", "TEXT").await?;
    // C.1: sealed (AES-GCM + VK-derived key) refresh token replaces plaintext column.
    ensure_column_exists(&pool, "users", "google_refresh_token_ciphertext", "BLOB").await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS devices (
            device_id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(user_id),
            device_name TEXT NOT NULL,
            public_key BLOB NOT NULL,
            wrapped_vault_key BLOB,
            vault_key_generation INTEGER,
            revoked_at INTEGER,
            last_seen_at INTEGER,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;
    ensure_column_exists(&pool, "devices", "safety_numbers_verified_at", "INTEGER").await?;
    // N.5 A.3: track when a device has set a real X25519 key (not the [0;32] placeholder).
    // accept_device checks enrolled_at IS NOT NULL before wrapping the vault key.
    ensure_column_exists(&pool, "devices", "enrolled_at", "INTEGER").await?;
    ensure_column_exists(&pool, "devices", "kyber_public_key", "BLOB").await?;
    ensure_column_exists(&pool, "devices", "wrapped_vault_key_kyber", "BLOB").await?;
    // Backfill existing devices that already have a real public key.
    sqlx::query(
        "UPDATE devices SET enrolled_at = created_at \
         WHERE enrolled_at IS NULL \
         AND length(public_key) = 32 \
         AND public_key != X'0000000000000000000000000000000000000000000000000000000000000000'",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS vault_members (
            user_id TEXT NOT NULL REFERENCES users(user_id),
            vault_id TEXT NOT NULL,
            role TEXT NOT NULL DEFAULT 'member',
            invited_by TEXT REFERENCES users(user_id),
            joined_at INTEGER NOT NULL,
            PRIMARY KEY (user_id, vault_id)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS audit_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            actor_user_id TEXT,
            actor_device_id TEXT,
            action TEXT NOT NULL,
            target_user_id TEXT,
            target_device_id TEXT,
            details TEXT,
            vault_id TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_audit_logs_vault ON audit_logs(vault_id, timestamp)",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS invite_codes (
            code TEXT PRIMARY KEY,
            vault_id TEXT NOT NULL,
            created_by TEXT NOT NULL REFERENCES users(user_id),
            role TEXT NOT NULL DEFAULT 'member',
            max_uses INTEGER NOT NULL DEFAULT 1,
            used_count INTEGER NOT NULL DEFAULT 0,
            expires_at INTEGER,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // Epic 34.2b: DEK re-wrap queue for lazy VK rotation
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS dek_rewrap_queue (
            dek_id INTEGER PRIMARY KEY,
            source_vk_generation INTEGER NOT NULL,
            target_vk_generation INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'PENDING',
            attempted_at INTEGER,
            error TEXT
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // Epic 34.3a: User session tokens
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_sessions (
            token       TEXT PRIMARY KEY,
            user_id     TEXT NOT NULL REFERENCES users(user_id),
            device_id   TEXT NOT NULL,
            created_at  INTEGER NOT NULL,
            expires_at  INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // Sesja C: OAuth2 flow state (PKCE + CSRF)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS oauth_states (
            state        TEXT PRIMARY KEY,
            pkce_verifier TEXT NOT NULL,
            created_at   INTEGER NOT NULL,
            expires_at   INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // Epic 34.6a: Recovery keys (24-word BIP-39 mnemonic wraps Vault Key via AES-KW)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS vault_recovery_keys (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            vault_id          TEXT NOT NULL,
            wrapped_vault_key BLOB NOT NULL,
            vk_generation     INTEGER NOT NULL,
            created_at        INTEGER NOT NULL,
            created_by        TEXT,
            revoked_at        INTEGER
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_vault_recovery_keys_active \
         ON vault_recovery_keys(vault_id) WHERE revoked_at IS NULL",
    )
    .execute(&pool)
    .await?;

    // Epic 36 G.2: Traffic stats (2-hour bucket granularity)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS traffic_stats (
            bucket_epoch INTEGER PRIMARY KEY,
            upload_bytes  INTEGER NOT NULL DEFAULT 0,
            download_bytes INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

async fn ensure_column_exists(
    pool: &SqlitePool,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), sqlx::Error> {
    let pragma = format!("PRAGMA table_info({table})");
    let columns = sqlx::query(&pragma).fetch_all(pool).await?;
    let exists = columns.iter().any(|row| {
        row.try_get::<String, _>("name")
            .map(|name| name == column)
            .unwrap_or(false)
    });

    if !exists {
        let alter = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
        sqlx::query(&alter).execute(pool).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::*;

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
}
