use crate::db::*;
use sqlx::SqlitePool;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultRestoreApplyReport {
    pub vault_id: String,
    pub restored_inodes: i64,
    pub restored_revisions: i64,
    /// Provider names that have configs but no local secrets (need credential setup).
    pub missing_provider_secrets: Vec<String>,
}

// Row types used exclusively by the restore graft to shuttle data from the
// restored snapshot pool into the main DB without using ATTACH.
#[derive(sqlx::FromRow)]
struct RestoredInode {
    id: i64,
    parent_id: Option<i64>,
    name: String,
    kind: String,
    size: i64,
    mode: Option<i64>,
    mtime: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct RestoredRevision {
    revision_id: i64,
    inode_id: i64,
    created_at: i64,
    size: i64,
    is_current: i64,
    immutable_until: Option<i64>,
    device_id: Option<String>,
    parent_revision_id: Option<i64>,
    origin: String,
    conflict_reason: Option<String>,
}

#[derive(sqlx::FromRow)]
struct RestoredSyncPolicy {
    policy_id: i64,
    path_prefix: String,
    require_healthy: i64,
    enable_versioning: i64,
    policy_type: String,
}

#[derive(sqlx::FromRow)]
struct RestoredSmartSyncState {
    inode_id: i64,
    revision_id: i64,
    pin_state: i64,
    hydration_state: i64,
}

#[derive(sqlx::FromRow)]
struct RestoredMetadataBackup {
    backup_id: String,
    created_at: i64,
    snapshot_version: i64,
    object_key: String,
    provider: String,
    encrypted_size: i64,
    status: String,
    last_error: Option<String>,
}

#[derive(sqlx::FromRow)]
struct RestoredPack {
    pack_id: String,
    chunk_id: Vec<u8>,
    plaintext_hash: Option<String>,
    storage_mode: String,
    encryption_version: i64,
    ec_scheme: String,
    logical_size: i64,
    cipher_size: i64,
    shard_size: i64,
    nonce: Vec<u8>,
    gcm_tag: Vec<u8>,
    status: String,
}

#[derive(sqlx::FromRow)]
struct RestoredPackShard {
    id: i64,
    pack_id: String,
    shard_index: i64,
    shard_role: String,
    provider: String,
    object_key: String,
    size: i64,
    checksum: String,
    status: String,
    attempts: Option<i64>,
    last_error: Option<String>,
    last_verified_at: Option<i64>,
    last_verification_method: Option<String>,
    last_verification_status: Option<String>,
    last_verified_size: Option<i64>,
    verification_failures: i64,
}

#[derive(sqlx::FromRow)]
struct RestoredPackLocation {
    chunk_id: Vec<u8>,
    pack_id: String,
    pack_offset: i64,
    encrypted_size: i64,
}

#[derive(sqlx::FromRow)]
struct RestoredChunkRef {
    id: i64,
    revision_id: i64,
    chunk_id: Vec<u8>,
    file_offset: i64,
    size: i64,
}

#[derive(sqlx::FromRow)]
struct RestoredConflictEvent {
    conflict_id: i64,
    inode_id: i64,
    winning_revision_id: i64,
    losing_revision_id: i64,
    reason: String,
    materialized_inode_id: Option<i64>,
    materialized_revision_id: Option<i64>,
    created_at: i64,
}

#[allow(dead_code)]
#[derive(sqlx::FromRow)]
struct RestoredProviderConfig {
    provider_name: String,
    endpoint: String,
    region: String,
    bucket: String,
    force_path_style: i64,
    enabled: i64,
    draft_source: Option<String>,
    last_test_status: Option<String>,
    last_test_error: Option<String>,
    last_test_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

// v0.3.23: Identity tables grafted as part of Single-User-Multi-Device adoption.
// On join-existing the joining device adopts the source vault's owner identity
// instead of inventing a new local user_id; safety_numbers (= SHA256(EVK || user_id))
// are then identical across devices.
#[derive(sqlx::FromRow)]
struct RestoredUser {
    user_id: String,
    display_name: String,
    email: Option<String>,
    auth_provider: String,
    auth_subject: Option<String>,
    created_at: i64,
    google_refresh_token_ciphertext: Option<Vec<u8>>,
}

#[derive(sqlx::FromRow)]
struct RestoredDevice {
    device_id: String,
    user_id: String,
    device_name: String,
    public_key: Vec<u8>,
    wrapped_vault_key: Option<Vec<u8>>,
    vault_key_generation: Option<i64>,
    revoked_at: Option<i64>,
    last_seen_at: Option<i64>,
    created_at: i64,
    safety_numbers_verified_at: Option<i64>,
    enrolled_at: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct RestoredVaultMember {
    user_id: String,
    vault_id: String,
    role: String,
    invited_by: Option<String>,
    joined_at: i64,
}

#[derive(sqlx::FromRow)]
struct RestoredDek {
    dek_id: i64,
    inode_id: i64,
    wrapped_dek: Vec<u8>,
    key_version: i64,
    vault_key_gen: i64,
    created_at: i64,
}

#[derive(sqlx::FromRow)]
struct RestoredRecoveryKey {
    id: i64,
    vault_id: String,
    wrapped_vault_key: Vec<u8>,
    vk_generation: i64,
    created_at: i64,
    created_by: Option<String>,
    revoked_at: Option<i64>,
}

pub async fn graft_restored_metadata_snapshot(
    pool: &SqlitePool,
    restored_db_path: &Path,
) -> Result<VaultRestoreApplyReport, sqlx::Error> {
    // ── Phase 1: read everything from the restored snapshot into memory ──
    // We open the restored DB as a completely separate pool so there is no
    // ATTACH and therefore no cross-database locking on Windows.
    let restored_url = format!(
        "sqlite:{}?mode=ro",
        restored_db_path.to_string_lossy().replace('\\', "/")
    );
    let restored_pool = SqlitePool::connect(&restored_url).await?;

    #[allow(dead_code)]
    #[derive(sqlx::FromRow)]
    struct RestoreVaultRecord {
        id: i64,
        master_key_salt: Vec<u8>,
        argon2_params: String,
        vault_id: String,
        encrypted_vault_key: Option<Vec<u8>>,
        vault_key_generation: Option<i64>,
        legacy_read_key: Option<Vec<u8>>,
    }
    let remote_vault = sqlx::query_as::<_, RestoreVaultRecord>(
        "SELECT id, master_key_salt, argon2_params, vault_id, encrypted_vault_key, \
         vault_key_generation, legacy_read_key FROM vault_state WHERE id = 1",
    )
    .fetch_optional(&restored_pool)
    .await?
    .ok_or(sqlx::Error::Protocol(
        "restored snapshot is missing vault_state row".into(),
    ))?;

    let r_inodes = sqlx::query_as::<_, RestoredInode>(
        "SELECT id, parent_id, name, kind, size, mode, mtime FROM inodes",
    )
    .fetch_all(&restored_pool)
    .await?;

    let r_revisions = sqlx::query_as::<_, RestoredRevision>(
        "SELECT revision_id, inode_id, created_at, size, is_current, immutable_until, \
         device_id, parent_revision_id, origin, conflict_reason FROM file_revisions",
    )
    .fetch_all(&restored_pool)
    .await?;

    let r_policies = sqlx::query_as::<_, RestoredSyncPolicy>(
        "SELECT policy_id, path_prefix, require_healthy, enable_versioning, policy_type \
         FROM sync_policies",
    )
    .fetch_all(&restored_pool)
    .await?;

    let r_sync_state = sqlx::query_as::<_, RestoredSmartSyncState>(
        "SELECT inode_id, revision_id, pin_state, hydration_state FROM smart_sync_state",
    )
    .fetch_all(&restored_pool)
    .await?;

    let r_backups = sqlx::query_as::<_, RestoredMetadataBackup>(
        "SELECT backup_id, created_at, snapshot_version, object_key, provider, \
         encrypted_size, status, last_error FROM metadata_backups",
    )
    .fetch_all(&restored_pool)
    .await?;

    let r_packs = sqlx::query_as::<_, RestoredPack>(
        "SELECT pack_id, chunk_id, plaintext_hash, storage_mode, encryption_version, \
         ec_scheme, logical_size, cipher_size, shard_size, nonce, gcm_tag, status FROM packs",
    )
    .fetch_all(&restored_pool)
    .await?;

    let r_shards = sqlx::query_as::<_, RestoredPackShard>(
        "SELECT id, pack_id, shard_index, shard_role, provider, object_key, size, checksum, \
         status, attempts, last_error, last_verified_at, last_verification_method, \
         last_verification_status, last_verified_size, verification_failures FROM pack_shards",
    )
    .fetch_all(&restored_pool)
    .await?;

    let r_locations = sqlx::query_as::<_, RestoredPackLocation>(
        "SELECT chunk_id, pack_id, pack_offset, encrypted_size FROM pack_locations",
    )
    .fetch_all(&restored_pool)
    .await?;

    let r_chunk_refs = sqlx::query_as::<_, RestoredChunkRef>(
        "SELECT id, revision_id, chunk_id, file_offset, size FROM chunk_refs",
    )
    .fetch_all(&restored_pool)
    .await?;

    let r_conflicts = sqlx::query_as::<_, RestoredConflictEvent>(
        "SELECT conflict_id, inode_id, winning_revision_id, losing_revision_id, reason, \
         materialized_inode_id, materialized_revision_id, created_at FROM conflict_events",
    )
    .fetch_all(&restored_pool)
    .await?;

    let r_provider_configs = sqlx::query_as::<_, RestoredProviderConfig>(
        "SELECT provider_name, endpoint, region, bucket, force_path_style, enabled, \
         draft_source, last_test_status, last_test_error, last_test_at, created_at, \
         updated_at FROM provider_configs",
    )
    .fetch_all(&restored_pool)
    .await
    .unwrap_or_default();

    // v0.3.23: Identity tables. unwrap_or_default so a snapshot from before the
    // multi-user migration (e.g. legacy V1 source) doesn't break the graft —
    // post_join_existing then knows there's no owner to inherit and can fall back.
    let r_users = sqlx::query_as::<_, RestoredUser>(
        "SELECT user_id, display_name, email, auth_provider, auth_subject, created_at, \
         google_refresh_token_ciphertext FROM users",
    )
    .fetch_all(&restored_pool)
    .await
    .unwrap_or_default();

    let r_devices = sqlx::query_as::<_, RestoredDevice>(
        "SELECT device_id, user_id, device_name, public_key, wrapped_vault_key, \
         vault_key_generation, revoked_at, last_seen_at, created_at, \
         safety_numbers_verified_at, enrolled_at FROM devices",
    )
    .fetch_all(&restored_pool)
    .await
    .unwrap_or_default();

    let r_vault_members = sqlx::query_as::<_, RestoredVaultMember>(
        "SELECT user_id, vault_id, role, invited_by, joined_at FROM vault_members",
    )
    .fetch_all(&restored_pool)
    .await
    .unwrap_or_default();

    // Read vault_config (KDF salt + params) — critical for multi-device unlock.
    // Without this, the joining device derives a different vault key from the
    // same passphrase and all decryption fails with aes-gcm errors.
    let r_vault_config = sqlx::query_as::<_, VaultConfigRecord>(
        "SELECT id, salt, parameter_set_version, memory_cost_kib, time_cost, lanes \
         FROM vault_config WHERE id = 1",
    )
    .fetch_optional(&restored_pool)
    .await
    .unwrap_or(None);

    let r_deks = sqlx::query_as::<_, RestoredDek>(
        "SELECT dek_id, inode_id, wrapped_dek, key_version, vault_key_gen, created_at \
         FROM data_encryption_keys",
    )
    .fetch_all(&restored_pool)
    .await
    .unwrap_or_default();

    let r_recovery_keys = sqlx::query_as::<_, RestoredRecoveryKey>(
        "SELECT id, vault_id, wrapped_vault_key, vk_generation, created_at, created_by, \
         revoked_at FROM vault_recovery_keys",
    )
    .fetch_all(&restored_pool)
    .await
    .unwrap_or_default();

    // Done reading — close the restored pool before we touch the main DB.
    // Explicit drop after close() releases the Arc<PoolInner> reference synchronously;
    // yield_now() then gives tokio a slot to flush any deferred cleanup (memory-mapped
    // pages, kernel handles) before A.2's secure_delete tries to remove the file.
    restored_pool.close().await;
    drop(restored_pool);
    tokio::task::yield_now().await;

    // ── Phase 2: write into the main DB inside a single transaction ──
    let mut conn = pool.acquire().await?;
    sqlx::query("PRAGMA busy_timeout = 10000")
        .execute(&mut *conn)
        .await?;
    sqlx::query("BEGIN IMMEDIATE TRANSACTION")
        .execute(&mut *conn)
        .await?;

    let apply_result = async {
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;

        let local_vault = sqlx::query_as::<_, RestoreVaultRecord>(
            "SELECT id, master_key_salt, argon2_params, vault_id, encrypted_vault_key, \
             vault_key_generation, legacy_read_key FROM vault_state WHERE id = 1",
        )
        .fetch_optional(&mut *conn)
        .await?;

        match local_vault {
            Some(local) => {
                sqlx::query(
                    "INSERT INTO vault_state \
                     (id, master_key_salt, argon2_params, vault_id, encrypted_vault_key, \
                      vault_key_generation, legacy_read_key) \
                     VALUES (1, ?, ?, ?, ?, ?, ?) \
                     ON CONFLICT(id) DO UPDATE SET \
                         master_key_salt = excluded.master_key_salt, \
                         argon2_params = excluded.argon2_params, \
                         vault_id = excluded.vault_id, \
                         encrypted_vault_key = excluded.encrypted_vault_key, \
                         vault_key_generation = excluded.vault_key_generation, \
                         legacy_read_key = excluded.legacy_read_key",
                )
                .bind(local.master_key_salt)
                .bind(local.argon2_params)
                .bind(&remote_vault.vault_id)
                .bind(&remote_vault.encrypted_vault_key)
                .bind(remote_vault.vault_key_generation)
                .bind(&remote_vault.legacy_read_key)
                .execute(&mut *conn)
                .await?;
            }
            None => {
                sqlx::query(
                    "INSERT INTO vault_state \
                     (id, master_key_salt, argon2_params, vault_id, encrypted_vault_key, \
                      vault_key_generation, legacy_read_key) \
                     VALUES (1, ?, ?, ?, ?, ?, ?) \
                     ON CONFLICT(id) DO UPDATE SET \
                         master_key_salt = excluded.master_key_salt, \
                         argon2_params = excluded.argon2_params, \
                         vault_id = excluded.vault_id, \
                         encrypted_vault_key = excluded.encrypted_vault_key, \
                         vault_key_generation = excluded.vault_key_generation, \
                         legacy_read_key = excluded.legacy_read_key",
                )
                .bind(&remote_vault.master_key_salt)
                .bind(&remote_vault.argon2_params)
                .bind(&remote_vault.vault_id)
                .bind(&remote_vault.encrypted_vault_key)
                .bind(remote_vault.vault_key_generation)
                .bind(&remote_vault.legacy_read_key)
                .execute(&mut *conn)
                .await?;
            }
        }

        // Graft vault_config (KDF salt + parameters) from snapshot so that
        // the joining device derives the same vault key from the passphrase.
        if let Some(vc) = &r_vault_config {
            sqlx::query(
                "INSERT INTO vault_config (id, salt, parameter_set_version, \
                 memory_cost_kib, time_cost, lanes) \
                 VALUES (1, ?, ?, ?, ?, ?) \
                 ON CONFLICT(id) DO UPDATE SET \
                     salt = excluded.salt, \
                     parameter_set_version = excluded.parameter_set_version, \
                     memory_cost_kib = excluded.memory_cost_kib, \
                     time_cost = excluded.time_cost, \
                     lanes = excluded.lanes",
            )
            .bind(&vc.salt)
            .bind(vc.parameter_set_version)
            .bind(vc.memory_cost_kib)
            .bind(vc.time_cost)
            .bind(vc.lanes)
            .execute(&mut *conn)
            .await?;
        }

        for statement in [
            "DELETE FROM upload_job_targets",
            "DELETE FROM upload_jobs",
            "DELETE FROM cache_entries",
            "DELETE FROM smart_sync_state",
            "DELETE FROM pack_shards",
            "DELETE FROM pack_locations",
            "DELETE FROM packs",
            "DELETE FROM chunk_refs",
            "DELETE FROM conflict_events",
            "DELETE FROM file_revisions",
            "DELETE FROM metadata_backups",
            "DELETE FROM sync_policies",
            "DELETE FROM data_encryption_keys",
            "DELETE FROM vault_recovery_keys",
            "DELETE FROM inodes",
            // v0.3.23: identity tables — wipe local migration phantoms before adopting
            // the source vault's owner/devices/membership. Order matters w.r.t. FKs
            // even with foreign_keys=OFF (vault_members.invited_by → users).
            "DELETE FROM vault_members",
            "DELETE FROM devices",
            "DELETE FROM users",
        ] {
            sqlx::query(statement).execute(&mut *conn).await?;
        }

        // v0.3.23: Insert identity rows from the snapshot. If the snapshot predates
        // the multi-user migration these vectors are empty and post_join_existing
        // bootstraps a fresh owner. When present, the joining device adopts:
        //   - the source vault's owner user_id (so safety_numbers match cross-device)
        //   - the source vault's full devices roster (so MultiDevice tab shows peers)
        //   - the source vault's vault_members roster (ACL works against the grafted vault_id)
        for row in &r_users {
            sqlx::query(
                "INSERT INTO users (user_id, display_name, email, auth_provider, auth_subject, \
                 created_at, google_refresh_token_ciphertext) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&row.user_id)
            .bind(&row.display_name)
            .bind(&row.email)
            .bind(&row.auth_provider)
            .bind(&row.auth_subject)
            .bind(row.created_at)
            .bind(&row.google_refresh_token_ciphertext)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_devices {
            sqlx::query(
                "INSERT INTO devices (device_id, user_id, device_name, public_key, \
                 wrapped_vault_key, vault_key_generation, revoked_at, last_seen_at, \
                 created_at, safety_numbers_verified_at, enrolled_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&row.device_id)
            .bind(&row.user_id)
            .bind(&row.device_name)
            .bind(&row.public_key)
            .bind(&row.wrapped_vault_key)
            .bind(row.vault_key_generation)
            .bind(row.revoked_at)
            .bind(row.last_seen_at)
            .bind(row.created_at)
            .bind(row.safety_numbers_verified_at)
            .bind(row.enrolled_at)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_vault_members {
            sqlx::query(
                "INSERT INTO vault_members (user_id, vault_id, role, invited_by, joined_at) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&row.user_id)
            .bind(&row.vault_id)
            .bind(&row.role)
            .bind(&row.invited_by)
            .bind(row.joined_at)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_inodes {
            sqlx::query(
                "INSERT INTO inodes (id, parent_id, name, kind, size, mode, mtime) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(row.id)
            .bind(row.parent_id)
            .bind(&row.name)
            .bind(&row.kind)
            .bind(row.size)
            .bind(row.mode)
            .bind(row.mtime)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_deks {
            sqlx::query(
                "INSERT INTO data_encryption_keys \
                 (dek_id, inode_id, wrapped_dek, key_version, vault_key_gen, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(row.dek_id)
            .bind(row.inode_id)
            .bind(&row.wrapped_dek)
            .bind(row.key_version)
            .bind(row.vault_key_gen)
            .bind(row.created_at)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_recovery_keys {
            sqlx::query(
                "INSERT INTO vault_recovery_keys \
                 (id, vault_id, wrapped_vault_key, vk_generation, created_at, created_by, \
                  revoked_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(row.id)
            .bind(&row.vault_id)
            .bind(&row.wrapped_vault_key)
            .bind(row.vk_generation)
            .bind(row.created_at)
            .bind(&row.created_by)
            .bind(row.revoked_at)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_revisions {
            sqlx::query(
                "INSERT INTO file_revisions (revision_id, inode_id, created_at, size, \
                 is_current, immutable_until, device_id, parent_revision_id, origin, \
                 conflict_reason) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(row.revision_id)
            .bind(row.inode_id)
            .bind(row.created_at)
            .bind(row.size)
            .bind(row.is_current)
            .bind(row.immutable_until)
            .bind(&row.device_id)
            .bind(row.parent_revision_id)
            .bind(&row.origin)
            .bind(&row.conflict_reason)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_policies {
            sqlx::query(
                "INSERT INTO sync_policies (policy_id, path_prefix, require_healthy, \
                 enable_versioning, policy_type) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(row.policy_id)
            .bind(&row.path_prefix)
            .bind(row.require_healthy)
            .bind(row.enable_versioning)
            .bind(&row.policy_type)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_sync_state {
            sqlx::query(
                "INSERT INTO smart_sync_state (inode_id, revision_id, pin_state, \
                 hydration_state) VALUES (?, ?, ?, ?)",
            )
            .bind(row.inode_id)
            .bind(row.revision_id)
            .bind(row.pin_state)
            .bind(row.hydration_state)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_backups {
            sqlx::query(
                "INSERT INTO metadata_backups (backup_id, created_at, snapshot_version, \
                 object_key, provider, encrypted_size, status, last_error) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&row.backup_id)
            .bind(row.created_at)
            .bind(row.snapshot_version)
            .bind(&row.object_key)
            .bind(&row.provider)
            .bind(row.encrypted_size)
            .bind(&row.status)
            .bind(&row.last_error)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_packs {
            sqlx::query(
                "INSERT INTO packs (pack_id, chunk_id, plaintext_hash, storage_mode, \
                 encryption_version, ec_scheme, logical_size, cipher_size, shard_size, \
                 nonce, gcm_tag, status) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&row.pack_id)
            .bind(&row.chunk_id)
            .bind(&row.plaintext_hash)
            .bind(&row.storage_mode)
            .bind(row.encryption_version)
            .bind(&row.ec_scheme)
            .bind(row.logical_size)
            .bind(row.cipher_size)
            .bind(row.shard_size)
            .bind(&row.nonce)
            .bind(&row.gcm_tag)
            .bind(&row.status)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_shards {
            sqlx::query(
                "INSERT INTO pack_shards (id, pack_id, shard_index, shard_role, provider, \
                 object_key, size, checksum, status, attempts, last_error, last_verified_at, \
                 last_verification_method, last_verification_status, last_verified_size, \
                 verification_failures) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(row.id)
            .bind(&row.pack_id)
            .bind(row.shard_index)
            .bind(&row.shard_role)
            .bind(&row.provider)
            .bind(&row.object_key)
            .bind(row.size)
            .bind(&row.checksum)
            .bind(&row.status)
            .bind(row.attempts)
            .bind(&row.last_error)
            .bind(row.last_verified_at)
            .bind(&row.last_verification_method)
            .bind(&row.last_verification_status)
            .bind(row.last_verified_size)
            .bind(row.verification_failures)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_locations {
            sqlx::query(
                "INSERT INTO pack_locations (chunk_id, pack_id, pack_offset, encrypted_size) \
                 VALUES (?, ?, ?, ?)",
            )
            .bind(&row.chunk_id)
            .bind(&row.pack_id)
            .bind(row.pack_offset)
            .bind(row.encrypted_size)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_chunk_refs {
            sqlx::query(
                "INSERT INTO chunk_refs (id, revision_id, chunk_id, file_offset, size) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(row.id)
            .bind(row.revision_id)
            .bind(&row.chunk_id)
            .bind(row.file_offset)
            .bind(row.size)
            .execute(&mut *conn)
            .await?;
        }

        for row in &r_conflicts {
            sqlx::query(
                "INSERT INTO conflict_events (conflict_id, inode_id, winning_revision_id, \
                 losing_revision_id, reason, materialized_inode_id, \
                 materialized_revision_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(row.conflict_id)
            .bind(row.inode_id)
            .bind(row.winning_revision_id)
            .bind(row.losing_revision_id)
            .bind(&row.reason)
            .bind(row.materialized_inode_id)
            .bind(row.materialized_revision_id)
            .bind(row.created_at)
            .execute(&mut *conn)
            .await?;
        }

        // Graft provider_configs from snapshot (NOT secrets — those are DPAPI-sealed
        // per machine and cannot be transferred).  Use INSERT ... ON CONFLICT IGNORE
        // so we never overwrite a provider the joining device already configured.
        // created_at/updated_at use local epoch so UI shows when *this* device joined,
        // not a timestamp from the owner's machine (possibly a different TZ/clock).
        let local_now = epoch_secs();
        for row in &r_provider_configs {
            sqlx::query(
                "INSERT OR IGNORE INTO provider_configs (provider_name, endpoint, region, \
                 bucket, force_path_style, enabled, draft_source, last_test_status, \
                 last_test_error, last_test_at, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, 0, ?, NULL, NULL, NULL, ?, ?)",
            )
            .bind(&row.provider_name)
            .bind(&row.endpoint)
            .bind(&row.region)
            .bind(&row.bucket)
            .bind(row.force_path_style)
            .bind(&row.draft_source)
            .bind(local_now)
            .bind(local_now)
            .execute(&mut *conn)
            .await?;
        }

        // Detect providers that have configs but no local secrets
        let missing_secrets = sqlx::query_scalar::<_, String>(
            "SELECT pc.provider_name FROM provider_configs pc \
             LEFT JOIN provider_secrets ps ON pc.provider_name = ps.provider_name \
             WHERE ps.provider_name IS NULL",
        )
        .fetch_all(&mut *conn)
        .await?;

        let restored_inodes = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM inodes")
            .fetch_one(&mut *conn)
            .await?;
        let restored_revisions =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM file_revisions")
                .fetch_one(&mut *conn)
                .await?;

        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await?;

        Ok::<_, sqlx::Error>(VaultRestoreApplyReport {
            vault_id: remote_vault.vault_id,
            restored_inodes,
            restored_revisions,
            missing_provider_secrets: missing_secrets,
        })
    }
    .await;

    match apply_result {
        Ok(report) => {
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok(report)
        }
        Err(err) => {
            let _ = sqlx::query("PRAGMA foreign_keys = ON")
                .execute(&mut *conn)
                .await;
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
            Err(err)
        }
    }
}

/// Merge `devices` and `vault_members` from a snapshot DB into the live DB additively.
/// Only rows that do not already exist (by PK) are inserted — existing rows are never
/// overwritten. The function rejects any snapshot whose `vault_state.vault_id` differs
/// from `expected_vault_id`, and it never touches `data_encryption_keys`, `vault_state`,
/// `vault_recovery_keys`, or `local_device_identity`.
pub struct RosterMergeSummary {
    pub devices_added: u64,
    pub members_added: u64,
}

pub async fn graft_roster_additive(
    pool: &SqlitePool,
    snapshot_db_path: &Path,
    expected_vault_id: &str,
) -> Result<RosterMergeSummary, sqlx::Error> {
    let snap_url = format!(
        "sqlite:{}?mode=ro",
        snapshot_db_path.to_string_lossy().replace('\\', "/")
    );
    let snap_pool = SqlitePool::connect(&snap_url).await?;

    #[derive(sqlx::FromRow)]
    struct VaultIdRow {
        vault_id: String,
    }
    let snap_vault_row =
        sqlx::query_as::<_, VaultIdRow>("SELECT vault_id FROM vault_state WHERE id = 1")
            .fetch_optional(&snap_pool)
            .await?;
    let snap_vault = match snap_vault_row {
        Some(row) => row,
        None => {
            snap_pool.close().await;
            drop(snap_pool);
            return Err(sqlx::Error::Protocol(
                "restored snapshot is missing vault_state row".into(),
            ));
        }
    };

    if snap_vault.vault_id != expected_vault_id {
        snap_pool.close().await;
        drop(snap_pool);
        return Err(sqlx::Error::Protocol(format!(
            "snapshot vault_id mismatch: expected '{}', got '{}'",
            expected_vault_id, snap_vault.vault_id
        )));
    }

    let r_devices = sqlx::query_as::<_, RestoredDevice>(
        "SELECT device_id, user_id, device_name, public_key, wrapped_vault_key, \
         vault_key_generation, revoked_at, last_seen_at, created_at, \
         safety_numbers_verified_at, enrolled_at FROM devices",
    )
    .fetch_all(&snap_pool)
    .await
    .unwrap_or_default();

    let r_members = sqlx::query_as::<_, RestoredVaultMember>(
        "SELECT user_id, vault_id, role, invited_by, joined_at FROM vault_members",
    )
    .fetch_all(&snap_pool)
    .await
    .unwrap_or_default();

    snap_pool.close().await;
    drop(snap_pool);
    tokio::task::yield_now().await;

    let mut conn = pool.acquire().await?;
    sqlx::query("PRAGMA busy_timeout = 10000")
        .execute(&mut *conn)
        .await?;
    sqlx::query("BEGIN IMMEDIATE TRANSACTION")
        .execute(&mut *conn)
        .await?;

    let apply_result = async {
        let mut devices_added: u64 = 0;
        let mut members_added: u64 = 0;

        for row in &r_devices {
            let res = sqlx::query(
                "INSERT OR IGNORE INTO devices \
                 (device_id, user_id, device_name, public_key, wrapped_vault_key, \
                  vault_key_generation, revoked_at, last_seen_at, created_at, \
                  safety_numbers_verified_at, enrolled_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&row.device_id)
            .bind(&row.user_id)
            .bind(&row.device_name)
            .bind(&row.public_key)
            .bind(&row.wrapped_vault_key)
            .bind(row.vault_key_generation)
            .bind(row.revoked_at)
            .bind(row.last_seen_at)
            .bind(row.created_at)
            .bind(row.safety_numbers_verified_at)
            .bind(row.enrolled_at)
            .execute(&mut *conn)
            .await?;
            devices_added += res.rows_affected();
        }

        for row in &r_members {
            let res = sqlx::query(
                "INSERT OR IGNORE INTO vault_members \
                 (user_id, vault_id, role, invited_by, joined_at) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&row.user_id)
            .bind(&row.vault_id)
            .bind(&row.role)
            .bind(&row.invited_by)
            .bind(row.joined_at)
            .execute(&mut *conn)
            .await?;
            members_added += res.rows_affected();
        }

        Ok::<_, sqlx::Error>(RosterMergeSummary {
            devices_added,
            members_added,
        })
    }
    .await;

    match apply_result {
        Ok(summary) => {
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok(summary)
        }
        Err(err) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::*;

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
}
