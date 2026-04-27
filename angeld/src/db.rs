#![allow(clippy::too_many_arguments, dead_code)]

use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, Row, SqlitePool};
use std::path::Path;
use std::str::FromStr;
use uuid::Uuid;

pub fn new_user_id() -> String {
    Uuid::new_v4().to_string()
}

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

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct VaultRecord {
    pub id: i64,
    pub master_key_salt: Vec<u8>,
    pub argon2_params: String,
    pub vault_id: String,
    pub vault_format_version: Option<i64>,
    pub encrypted_vault_key: Option<Vec<u8>>,
    pub vault_key_generation: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultRestoreApplyReport {
    pub vault_id: String,
    pub restored_inodes: i64,
    pub restored_revisions: i64,
    /// Provider names that have configs but no local secrets (need credential setup).
    pub missing_provider_secrets: Vec<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct VaultConfigRecord {
    pub id: i64,
    pub salt: Vec<u8>,
    pub parameter_set_version: i64,
    pub memory_cost_kib: i64,
    pub time_cost: i64,
    pub lanes: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct InodeRecord {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub kind: String,
    pub size: i64,
    pub mode: Option<i64>,
    pub mtime: Option<i64>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ChunkRecord {
    pub id: i64,
    pub revision_id: i64,
    pub chunk_id: Vec<u8>,
    pub file_offset: i64,
    pub size: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct FileRevisionRecord {
    pub revision_id: i64,
    pub inode_id: i64,
    pub created_at: i64,
    pub size: i64,
    pub is_current: i64,
    pub immutable_until: Option<i64>,
    pub device_id: Option<String>,
    pub parent_revision_id: Option<i64>,
    pub origin: String,
    pub conflict_reason: Option<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct FileInventoryRecord {
    pub inode_id: i64,
    pub path: String,
    pub size: i64,
    pub current_revision_id: Option<i64>,
    pub current_revision_created_at: Option<i64>,
    pub smart_sync_pin_state: Option<i64>,
    pub smart_sync_hydration_state: Option<i64>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ProjectionFileRecord {
    pub inode_id: i64,
    pub path: String,
    pub revision_id: i64,
    pub size: i64,
    pub created_at: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct SyncPolicyRecord {
    pub policy_id: i64,
    pub path_prefix: String,
    pub require_healthy: i64,
    pub enable_versioning: i64,
    pub policy_type: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct SmartSyncStateRecord {
    pub inode_id: i64,
    pub revision_id: i64,
    pub pin_state: i64,
    pub hydration_state: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct SmartSyncEvictionRecord {
    pub inode_id: i64,
    pub revision_id: i64,
    pub path: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct CacheEntryRecord {
    pub cache_key: String,
    pub inode_id: i64,
    pub revision_id: i64,
    pub chunk_index: i64,
    pub pack_id: String,
    pub file_path: String,
    pub cache_path: String,
    pub size: i64,
    pub created_at: i64,
    pub last_accessed_at: i64,
    pub access_count: i64,
    pub is_prefetched: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct LocalDeviceIdentityRecord {
    pub device_id: String,
    pub device_name: String,
    pub peer_token: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub encrypted_private_key: Option<Vec<u8>>,
    pub public_key: Option<Vec<u8>>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct TrustedPeerRecord {
    pub peer_id: String,
    pub device_name: String,
    pub vault_id: String,
    pub peer_api_base: String,
    pub trusted: i64,
    pub last_seen_at: i64,
    pub last_handshake_at: Option<i64>,
    pub last_error: Option<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ConflictEventRecord {
    pub conflict_id: i64,
    pub inode_id: i64,
    pub winning_revision_id: i64,
    pub losing_revision_id: i64,
    pub reason: String,
    pub materialized_inode_id: Option<i64>,
    pub materialized_revision_id: Option<i64>,
    pub created_at: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct SystemConfigRecord {
    pub config_key: String,
    pub config_value: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ProviderConfigRecord {
    pub provider_name: String,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub force_path_style: i64,
    pub enabled: i64,
    pub draft_source: Option<String>,
    pub last_test_status: Option<String>,
    pub last_test_error: Option<String>,
    pub last_test_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ProviderSecretRecord {
    pub provider_name: String,
    pub access_key_id_ciphertext: Vec<u8>,
    pub secret_access_key_ciphertext: Vec<u8>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevisionLineageRelation {
    Same,
    CandidateDescendsFromCurrent,
    CurrentDescendsFromCandidate,
    Parallel,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ChunkLookupRecord {
    pub inode_id: i64,
    pub revision_id: i64,
    pub chunk_id: Vec<u8>,
    pub chunk_index: i64,
    pub file_offset: i64,
    pub size: i64,
    pub pack_id: String,
    pub pack_offset: i64,
    pub encrypted_size: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct CacheStatusSummary {
    pub total_entries: i64,
    pub total_bytes: i64,
    pub prefetched_entries: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct MetadataBackupRecord {
    pub backup_id: String,
    pub created_at: i64,
    pub snapshot_version: i64,
    pub object_key: String,
    pub provider: String,
    pub encrypted_size: i64,
    pub status: String,
    pub last_error: Option<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct FileChunkLocation {
    pub chunk_id: Vec<u8>,
    pub chunk_index: i64,
    pub file_offset: i64,
    pub size: i64,
    pub pack_id: String,
    pub pack_offset: i64,
    pub encrypted_size: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct PackRecord {
    pub pack_id: String,
    pub chunk_id: Vec<u8>,
    pub plaintext_hash: Option<String>,
    pub storage_mode: String,
    pub encryption_version: i64,
    pub ec_scheme: String,
    pub logical_size: i64,
    pub cipher_size: i64,
    pub shard_size: i64,
    pub nonce: Vec<u8>,
    pub gcm_tag: Vec<u8>,
    pub status: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct PackShardRecord {
    pub id: i64,
    pub pack_id: String,
    pub shard_index: i64,
    pub shard_role: String,
    pub provider: String,
    pub object_key: String,
    pub size: i64,
    pub checksum: String,
    pub status: String,
    pub attempts: Option<i64>,
    pub last_error: Option<String>,
    pub last_verified_at: Option<i64>,
    pub last_verification_method: Option<String>,
    pub last_verification_status: Option<String>,
    pub last_verified_size: Option<i64>,
    pub verification_failures: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ScrubShardRecord {
    pub id: i64,
    pub pack_id: String,
    pub shard_index: i64,
    pub provider: String,
    pub object_key: String,
    pub size: i64,
    pub checksum: String,
    pub status: String,
    pub last_verified_at: Option<i64>,
    pub last_verification_method: Option<String>,
    pub last_verification_status: Option<String>,
    pub last_verified_size: Option<i64>,
    pub verification_failures: i64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackShardSummary {
    pub total: i64,
    pub completed: i64,
    pub pending: i64,
    pub in_progress: i64,
    pub failed: i64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct UploadJob {
    pub id: i64,
    pub pack_id: String,
    pub status: String,
    pub attempts: Option<i64>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct UploadTargetRecord {
    pub id: i64,
    pub job_id: i64,
    pub provider: String,
    pub status: String,
    pub attempts: Option<i64>,
    pub last_error: Option<String>,
    pub bucket: Option<String>,
    pub object_key: Option<String>,
    pub etag: Option<String>,
    pub version_id: Option<String>,
    pub last_attempt_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub completed_at: Option<i64>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct PackDownloadTarget {
    pub provider: String,
    pub bucket: String,
    pub object_key: String,
    pub attempts: Option<i64>,
    pub last_error: Option<String>,
    pub last_attempt_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub completed_at: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct VaultHealthSummary {
    pub total_packs: i64,
    pub healthy_packs: i64,
    pub degraded_packs: i64,
    pub unreadable_packs: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ScrubStatusSummary {
    pub total_shards: i64,
    pub verified_shards: i64,
    pub healthy_shards: i64,
    pub corrupted_or_missing: i64,
    pub verified_light_shards: i64,
    pub verified_deep_shards: i64,
    pub last_scrub_timestamp: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ScrubErrorRecord {
    pub pack_id: String,
    pub provider: String,
    pub shard_index: i64,
    pub last_verified_at: Option<i64>,
    pub last_verification_status: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct ActiveStorageModeSummary {
    pub storage_mode: String,
    pub active_packs: i64,
    pub logical_bytes: i64,
    pub cipher_bytes: i64,
    pub total_shard_bytes: i64,
    pub physical_bytes: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct OrphanedPackSummary {
    pub pack_count: i64,
    pub physical_bytes: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct CloudUsageDailyRecord {
    pub day_epoch: i64,
    pub read_ops: i64,
    pub write_ops: i64,
    pub egress_bytes: i64,
    pub updated_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CloudUsageDelta {
    pub read_ops: i64,
    pub write_ops: i64,
    pub egress_bytes: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloudUsageApplyResult {
    pub day_epoch: i64,
    pub read_ops: i64,
    pub write_ops: i64,
    pub egress_bytes: i64,
    pub allowed: bool,
}

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

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct AuditLogRecord {
    pub id: i64,
    pub timestamp: i64,
    pub actor_user_id: Option<String>,
    pub actor_device_id: Option<String>,
    pub action: String,
    pub target_user_id: Option<String>,
    pub target_device_id: Option<String>,
    pub details: Option<String>,
    pub vault_id: String,
}

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

#[allow(dead_code)]
pub async fn init_db(db_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(db_url)
        .map_err(|err| sqlx::Error::Configuration(Box::new(err)))?
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new().connect_with(options).await?;
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
    let _ = sqlx::query(
        "ALTER TABLE local_device_identity ADD COLUMN encrypted_private_key BLOB",
    )
    .execute(&pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE local_device_identity ADD COLUMN public_key BLOB",
    )
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

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_ingest_jobs_state ON ingest_jobs(state)",
    )
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

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_shared_links_inode ON shared_links(inode_id)",
    )
    .execute(&pool)
    .await?;

    // Migration: add password_hash column if missing (existing DBs)
    let _ = sqlx::query("ALTER TABLE shared_links ADD COLUMN password_hash TEXT")
        .execute(&pool)
        .await;

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

// ── Ingest Jobs persistence ───────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct IngestJobRow {
    pub id: i64,
    pub file_path: String,
    pub file_size: i64,
    pub state: String,
    pub bytes_processed: i64,
    pub attempt_count: i64,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[allow(dead_code)]
pub async fn create_ingest_job(
    pool: &SqlitePool,
    file_path: &str,
    file_size: i64,
) -> Result<i64, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "INSERT INTO ingest_jobs (file_path, file_size, state, created_at, updated_at) \
         VALUES (?, ?, 'PENDING', ?, ?)",
    )
    .bind(file_path)
    .bind(file_size)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn get_next_pending_ingest_job(
    pool: &SqlitePool,
) -> Result<Option<IngestJobRow>, sqlx::Error> {
    sqlx::query_as::<_, IngestJobRow>(
        "SELECT id, file_path, file_size, state, bytes_processed, attempt_count, \
         error_message, created_at, updated_at \
         FROM ingest_jobs WHERE state = 'PENDING' ORDER BY created_at ASC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
}

pub async fn transition_ingest_job(
    pool: &SqlitePool,
    job_id: i64,
    from_state: &str,
    to_state: &str,
) -> Result<bool, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "UPDATE ingest_jobs SET state = ?, updated_at = ? \
         WHERE id = ? AND state = ?",
    )
    .bind(to_state)
    .bind(now)
    .bind(job_id)
    .bind(from_state)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn update_ingest_progress(
    pool: &SqlitePool,
    job_id: i64,
    bytes_processed: i64,
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "UPDATE ingest_jobs SET bytes_processed = ?, updated_at = ? WHERE id = ?",
    )
    .bind(bytes_processed)
    .bind(now)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn fail_ingest_job(
    pool: &SqlitePool,
    job_id: i64,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "UPDATE ingest_jobs SET state = 'FAILED', error_message = ?, \
         attempt_count = attempt_count + 1, updated_at = ? WHERE id = ?",
    )
    .bind(error_message)
    .bind(now)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn reset_interrupted_ingest_jobs(
    pool: &SqlitePool,
) -> Result<u64, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "UPDATE ingest_jobs SET state = 'PENDING', error_message = 'interrupted by restart', \
         attempt_count = attempt_count + 1, updated_at = ? \
         WHERE state IN ('CHUNKING', 'UPLOADING')",
    )
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[allow(dead_code)]
pub async fn list_ingest_jobs(
    pool: &SqlitePool,
) -> Result<Vec<IngestJobRow>, sqlx::Error> {
    sqlx::query_as::<_, IngestJobRow>(
        "SELECT id, file_path, file_size, state, bytes_processed, attempt_count, \
         error_message, created_at, updated_at \
         FROM ingest_jobs ORDER BY created_at DESC LIMIT 100",
    )
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_ingest_job(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<Option<IngestJobRow>, sqlx::Error> {
    sqlx::query_as::<_, IngestJobRow>(
        "SELECT id, file_path, file_size, state, bytes_processed, attempt_count, \
         error_message, created_at, updated_at \
         FROM ingest_jobs WHERE id = ?",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn requeue_failed_ingest_job(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<bool, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "UPDATE ingest_jobs SET state = 'PENDING', error_message = NULL, \
         bytes_processed = 0, updated_at = ? WHERE id = ? AND state = 'FAILED'",
    )
    .bind(now)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_ingest_job(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM ingest_jobs WHERE id = ? AND state = 'GHOSTED'",
    )
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_failed_ingest_job(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM ingest_jobs WHERE id = ? AND state = 'FAILED'",
    )
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Find all pack_ids associated with an inode via its file_revisions → chunk_refs → pack_locations.
pub async fn get_pack_ids_for_inode(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT DISTINCT pl.pack_id
        FROM file_revisions fr
        INNER JOIN chunk_refs cr ON cr.revision_id = fr.revision_id
        INNER JOIN pack_locations pl ON pl.chunk_id = cr.chunk_id
        WHERE fr.inode_id = ?
        "#,
    )
    .bind(inode_id)
    .fetch_all(pool)
    .await
}

/// Reset a FAILED ingest job to PENDING, clearing error and resetting attempt_count.
pub async fn retry_ingest_job(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<bool, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "UPDATE ingest_jobs SET state = 'PENDING', error_message = NULL, \
         attempt_count = 0, bytes_processed = 0, updated_at = ? \
         WHERE id = ? AND state = 'FAILED'",
    )
    .bind(now)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub fn epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[allow(dead_code)]
pub async fn set_vault_params(
    pool: &SqlitePool,
    salt: &[u8],
    params_json: &str,
    vault_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO vault_state (id, master_key_salt, argon2_params, vault_id)
        VALUES (1, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            master_key_salt = excluded.master_key_salt,
            argon2_params = excluded.argon2_params,
            vault_id = excluded.vault_id
        "#,
    )
    .bind(salt)
    .bind(params_json)
    .bind(vault_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_vault_params(pool: &SqlitePool) -> Result<Option<VaultRecord>, sqlx::Error> {
    sqlx::query_as::<_, VaultRecord>(
        r#"
        SELECT id, master_key_salt, argon2_params, vault_id,
               vault_format_version, encrypted_vault_key, vault_key_generation
        FROM vault_state
        WHERE id = 1
        "#,
    )
    .fetch_optional(pool)
    .await
}

pub async fn store_encrypted_vault_key(
    pool: &SqlitePool,
    encrypted_vault_key: &[u8],
    vault_key_generation: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE vault_state SET encrypted_vault_key = ?, vault_key_generation = ?, \
         vault_format_version = 2 WHERE id = 1",
    )
    .bind(encrypted_vault_key)
    .bind(vault_key_generation)
    .execute(pool)
    .await?;
    Ok(())
}

// ── DEK (Data Encryption Key) persistence ───────────────────────────────

#[allow(dead_code)]
#[derive(Clone, Debug, FromRow)]
pub struct WrappedDekRecord {
    pub dek_id: i64,
    pub inode_id: i64,
    pub wrapped_dek: Vec<u8>,
    pub key_version: i64,
    pub vault_key_gen: i64,
    pub created_at: i64,
}

/// Fetch the latest wrapped DEK for a given inode (highest key_version).
pub async fn get_wrapped_dek(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Option<WrappedDekRecord>, sqlx::Error> {
    sqlx::query_as::<_, WrappedDekRecord>(
        "SELECT dek_id, inode_id, wrapped_dek, key_version, vault_key_gen, created_at \
         FROM data_encryption_keys \
         WHERE inode_id = ? \
         ORDER BY key_version DESC \
         LIMIT 1",
    )
    .bind(inode_id)
    .fetch_optional(pool)
    .await
}

/// Insert a new wrapped DEK for an inode. Returns the assigned dek_id.
pub async fn insert_wrapped_dek(
    pool: &SqlitePool,
    inode_id: i64,
    wrapped_dek: &[u8],
    key_version: i64,
    vault_key_gen: i64,
) -> Result<i64, sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let result = sqlx::query(
        "INSERT INTO data_encryption_keys (inode_id, wrapped_dek, key_version, vault_key_gen, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(inode_id)
    .bind(wrapped_dek)
    .bind(key_version)
    .bind(vault_key_gen)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

// Row types used exclusively by the restore graft to shuttle data from the
// restored snapshot pool into the main DB without using ATTACH.
#[derive(sqlx::FromRow)] struct RestoredInode { id: i64, parent_id: Option<i64>, name: String, kind: String, size: i64, mode: Option<i64>, mtime: Option<i64> }
#[derive(sqlx::FromRow)] struct RestoredRevision { revision_id: i64, inode_id: i64, created_at: i64, size: i64, is_current: i64, immutable_until: Option<i64>, device_id: Option<String>, parent_revision_id: Option<i64>, origin: String, conflict_reason: Option<String> }
#[derive(sqlx::FromRow)] struct RestoredSyncPolicy { policy_id: i64, path_prefix: String, require_healthy: i64, enable_versioning: i64, policy_type: String }
#[derive(sqlx::FromRow)] struct RestoredSmartSyncState { inode_id: i64, revision_id: i64, pin_state: i64, hydration_state: i64 }
#[derive(sqlx::FromRow)] struct RestoredMetadataBackup { backup_id: String, created_at: i64, snapshot_version: i64, object_key: String, provider: String, encrypted_size: i64, status: String, last_error: Option<String> }
#[derive(sqlx::FromRow)] struct RestoredPack { pack_id: String, chunk_id: Vec<u8>, plaintext_hash: Option<String>, storage_mode: String, encryption_version: i64, ec_scheme: String, logical_size: i64, cipher_size: i64, shard_size: i64, nonce: Vec<u8>, gcm_tag: Vec<u8>, status: String }
#[derive(sqlx::FromRow)] struct RestoredPackShard { id: i64, pack_id: String, shard_index: i64, shard_role: String, provider: String, object_key: String, size: i64, checksum: String, status: String, attempts: Option<i64>, last_error: Option<String>, last_verified_at: Option<i64>, last_verification_method: Option<String>, last_verification_status: Option<String>, last_verified_size: Option<i64>, verification_failures: i64 }
#[derive(sqlx::FromRow)] struct RestoredPackLocation { chunk_id: Vec<u8>, pack_id: String, pack_offset: i64, encrypted_size: i64 }
#[derive(sqlx::FromRow)] struct RestoredChunkRef { id: i64, revision_id: i64, chunk_id: Vec<u8>, file_offset: i64, size: i64 }
#[derive(sqlx::FromRow)] struct RestoredConflictEvent { conflict_id: i64, inode_id: i64, winning_revision_id: i64, losing_revision_id: i64, reason: String, materialized_inode_id: Option<i64>, materialized_revision_id: Option<i64>, created_at: i64 }
#[allow(dead_code)]
#[derive(sqlx::FromRow)] struct RestoredProviderConfig { provider_name: String, endpoint: String, region: String, bucket: String, force_path_style: i64, enabled: i64, draft_source: Option<String>, last_test_status: Option<String>, last_test_error: Option<String>, last_test_at: Option<i64>, created_at: i64, updated_at: i64 }

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

    // Use a minimal struct — the restored snapshot may be V1 (no V2 columns).
    #[allow(dead_code)]
    #[derive(sqlx::FromRow)]
    struct RestoreVaultRecord { id: i64, master_key_salt: Vec<u8>, argon2_params: String, vault_id: String }
    let remote_vault = sqlx::query_as::<_, RestoreVaultRecord>(
        "SELECT id, master_key_salt, argon2_params, vault_id FROM vault_state WHERE id = 1",
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

        // Graft vault_id from remote, keep local KDF params if present
        let local_vault = sqlx::query_as::<_, RestoreVaultRecord>(
            "SELECT id, master_key_salt, argon2_params, vault_id FROM vault_state WHERE id = 1",
        )
        .fetch_optional(&mut *conn)
        .await?;

        match local_vault {
            Some(local) => {
                sqlx::query(
                    "INSERT INTO vault_state (id, master_key_salt, argon2_params, vault_id) \
                     VALUES (1, ?, ?, ?) \
                     ON CONFLICT(id) DO UPDATE SET \
                         master_key_salt = excluded.master_key_salt, \
                         argon2_params = excluded.argon2_params, \
                         vault_id = excluded.vault_id",
                )
                .bind(local.master_key_salt)
                .bind(local.argon2_params)
                .bind(&remote_vault.vault_id)
                .execute(&mut *conn)
                .await?;
            }
            None => {
                sqlx::query(
                    "INSERT INTO vault_state (id, master_key_salt, argon2_params, vault_id) \
                     VALUES (1, ?, ?, ?) \
                     ON CONFLICT(id) DO UPDATE SET \
                         master_key_salt = excluded.master_key_salt, \
                         argon2_params = excluded.argon2_params, \
                         vault_id = excluded.vault_id",
                )
                .bind(remote_vault.master_key_salt)
                .bind(remote_vault.argon2_params)
                .bind(&remote_vault.vault_id)
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
            "DELETE FROM inodes",
        ] {
            sqlx::query(statement).execute(&mut *conn).await?;
        }

        for row in &r_inodes {
            sqlx::query(
                "INSERT INTO inodes (id, parent_id, name, kind, size, mode, mtime) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(row.id).bind(row.parent_id).bind(&row.name).bind(&row.kind)
            .bind(row.size).bind(row.mode).bind(row.mtime)
            .execute(&mut *conn).await?;
        }

        for row in &r_revisions {
            sqlx::query(
                "INSERT INTO file_revisions (revision_id, inode_id, created_at, size, \
                 is_current, immutable_until, device_id, parent_revision_id, origin, \
                 conflict_reason) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(row.revision_id).bind(row.inode_id).bind(row.created_at)
            .bind(row.size).bind(row.is_current).bind(row.immutable_until)
            .bind(&row.device_id).bind(row.parent_revision_id)
            .bind(&row.origin).bind(&row.conflict_reason)
            .execute(&mut *conn).await?;
        }

        for row in &r_policies {
            sqlx::query(
                "INSERT INTO sync_policies (policy_id, path_prefix, require_healthy, \
                 enable_versioning, policy_type) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(row.policy_id).bind(&row.path_prefix).bind(row.require_healthy)
            .bind(row.enable_versioning).bind(&row.policy_type)
            .execute(&mut *conn).await?;
        }

        for row in &r_sync_state {
            sqlx::query(
                "INSERT INTO smart_sync_state (inode_id, revision_id, pin_state, \
                 hydration_state) VALUES (?, ?, ?, ?)",
            )
            .bind(row.inode_id).bind(row.revision_id).bind(row.pin_state)
            .bind(row.hydration_state)
            .execute(&mut *conn).await?;
        }

        for row in &r_backups {
            sqlx::query(
                "INSERT INTO metadata_backups (backup_id, created_at, snapshot_version, \
                 object_key, provider, encrypted_size, status, last_error) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&row.backup_id).bind(row.created_at).bind(row.snapshot_version)
            .bind(&row.object_key).bind(&row.provider).bind(row.encrypted_size)
            .bind(&row.status).bind(&row.last_error)
            .execute(&mut *conn).await?;
        }

        for row in &r_packs {
            sqlx::query(
                "INSERT INTO packs (pack_id, chunk_id, plaintext_hash, storage_mode, \
                 encryption_version, ec_scheme, logical_size, cipher_size, shard_size, \
                 nonce, gcm_tag, status) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&row.pack_id).bind(&row.chunk_id).bind(&row.plaintext_hash)
            .bind(&row.storage_mode).bind(row.encryption_version).bind(&row.ec_scheme)
            .bind(row.logical_size).bind(row.cipher_size).bind(row.shard_size)
            .bind(&row.nonce).bind(&row.gcm_tag).bind(&row.status)
            .execute(&mut *conn).await?;
        }

        for row in &r_shards {
            sqlx::query(
                "INSERT INTO pack_shards (id, pack_id, shard_index, shard_role, provider, \
                 object_key, size, checksum, status, attempts, last_error, last_verified_at, \
                 last_verification_method, last_verification_status, last_verified_size, \
                 verification_failures) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(row.id).bind(&row.pack_id).bind(row.shard_index).bind(&row.shard_role)
            .bind(&row.provider).bind(&row.object_key).bind(row.size).bind(&row.checksum)
            .bind(&row.status).bind(row.attempts).bind(&row.last_error)
            .bind(row.last_verified_at).bind(&row.last_verification_method)
            .bind(&row.last_verification_status).bind(row.last_verified_size)
            .bind(row.verification_failures)
            .execute(&mut *conn).await?;
        }

        for row in &r_locations {
            sqlx::query(
                "INSERT INTO pack_locations (chunk_id, pack_id, pack_offset, encrypted_size) \
                 VALUES (?, ?, ?, ?)",
            )
            .bind(&row.chunk_id).bind(&row.pack_id).bind(row.pack_offset)
            .bind(row.encrypted_size)
            .execute(&mut *conn).await?;
        }

        for row in &r_chunk_refs {
            sqlx::query(
                "INSERT INTO chunk_refs (id, revision_id, chunk_id, file_offset, size) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(row.id).bind(row.revision_id).bind(&row.chunk_id)
            .bind(row.file_offset).bind(row.size)
            .execute(&mut *conn).await?;
        }

        for row in &r_conflicts {
            sqlx::query(
                "INSERT INTO conflict_events (conflict_id, inode_id, winning_revision_id, \
                 losing_revision_id, reason, materialized_inode_id, \
                 materialized_revision_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(row.conflict_id).bind(row.inode_id).bind(row.winning_revision_id)
            .bind(row.losing_revision_id).bind(&row.reason)
            .bind(row.materialized_inode_id).bind(row.materialized_revision_id)
            .bind(row.created_at)
            .execute(&mut *conn).await?;
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
            .bind(&row.provider_name).bind(&row.endpoint).bind(&row.region)
            .bind(&row.bucket).bind(row.force_path_style)
            .bind(&row.draft_source)
            .bind(local_now).bind(local_now)
            .execute(&mut *conn).await?;
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

#[allow(dead_code)]
pub async fn get_vault_config(pool: &SqlitePool) -> Result<Option<VaultConfigRecord>, sqlx::Error> {
    sqlx::query_as::<_, VaultConfigRecord>(
        r#"
        SELECT id, salt, parameter_set_version, memory_cost_kib, time_cost, lanes
        FROM vault_config
        WHERE id = 1
        "#,
    )
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn set_vault_config(
    pool: &SqlitePool,
    salt: &[u8],
    parameter_set_version: i64,
    memory_cost_kib: i64,
    time_cost: i64,
    lanes: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO vault_config (
            id,
            salt,
            parameter_set_version,
            memory_cost_kib,
            time_cost,
            lanes
        )
        VALUES (1, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            salt = excluded.salt,
            parameter_set_version = excluded.parameter_set_version,
            memory_cost_kib = excluded.memory_cost_kib,
            time_cost = excluded.time_cost,
            lanes = excluded.lanes
        "#,
    )
    .bind(salt)
    .bind(parameter_set_version)
    .bind(memory_cost_kib)
    .bind(time_cost)
    .bind(lanes)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_system_config_value(
    pool: &SqlitePool,
    config_key: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT config_value
        FROM system_config
        WHERE config_key = ?
        "#,
    )
    .bind(config_key)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn list_system_config(pool: &SqlitePool) -> Result<Vec<SystemConfigRecord>, sqlx::Error> {
    sqlx::query_as::<_, SystemConfigRecord>(
        r#"
        SELECT config_key, config_value, created_at, updated_at
        FROM system_config
        ORDER BY config_key ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn set_system_config_value(
    pool: &SqlitePool,
    config_key: &str,
    config_value: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO system_config (
            config_key,
            config_value,
            created_at,
            updated_at
        )
        VALUES (
            ?,
            ?,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        )
        ON CONFLICT(config_key) DO UPDATE SET
            config_value = excluded.config_value,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(config_key)
    .bind(config_value)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_cloud_usage_for_day(
    pool: &SqlitePool,
    day_epoch: i64,
) -> Result<Option<CloudUsageDailyRecord>, sqlx::Error> {
    sqlx::query_as::<_, CloudUsageDailyRecord>(
        r#"
        SELECT day_epoch, read_ops, write_ops, egress_bytes, updated_at
        FROM cloud_usage_daily
        WHERE day_epoch = ?
        "#,
    )
    .bind(day_epoch)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn apply_cloud_usage_delta_with_limits(
    pool: &SqlitePool,
    day_epoch: i64,
    delta: CloudUsageDelta,
    read_limit: i64,
    write_limit: i64,
    egress_limit: i64,
) -> Result<CloudUsageApplyResult, sqlx::Error> {
    let mut conn = pool.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE TRANSACTION")
        .execute(&mut *conn)
        .await?;

    let apply_result = async {
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO cloud_usage_daily (
                day_epoch, read_ops, write_ops, egress_bytes, updated_at
            )
            VALUES (
                ?, 0, 0, 0,
                CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
            )
            "#,
        )
        .bind(day_epoch)
        .execute(&mut *conn)
        .await?;

        let existing = sqlx::query_as::<_, CloudUsageDailyRecord>(
            r#"
            SELECT day_epoch, read_ops, write_ops, egress_bytes, updated_at
            FROM cloud_usage_daily
            WHERE day_epoch = ?
            "#,
        )
        .bind(day_epoch)
        .fetch_one(&mut *conn)
        .await?;

        let next_read_ops = existing.read_ops.saturating_add(delta.read_ops);
        let next_write_ops = existing.write_ops.saturating_add(delta.write_ops);
        let next_egress_bytes = existing.egress_bytes.saturating_add(delta.egress_bytes);
        let allowed = next_read_ops <= read_limit
            && next_write_ops <= write_limit
            && next_egress_bytes <= egress_limit;

        if allowed {
            sqlx::query(
                r#"
                UPDATE cloud_usage_daily
                SET
                    read_ops = ?,
                    write_ops = ?,
                    egress_bytes = ?,
                    updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
                WHERE day_epoch = ?
                "#,
            )
            .bind(next_read_ops)
            .bind(next_write_ops)
            .bind(next_egress_bytes)
            .bind(day_epoch)
            .execute(&mut *conn)
            .await?;
        }

        Ok::<_, sqlx::Error>(CloudUsageApplyResult {
            day_epoch,
            read_ops: if allowed {
                next_read_ops
            } else {
                existing.read_ops
            },
            write_ops: if allowed {
                next_write_ops
            } else {
                existing.write_ops
            },
            egress_bytes: if allowed {
                next_egress_bytes
            } else {
                existing.egress_bytes
            },
            allowed,
        })
    }
    .await;

    match apply_result {
        Ok(result) => {
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok(result)
        }
        Err(err) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
            Err(err)
        }
    }
}

#[allow(dead_code)]
pub async fn get_provider_config(
    pool: &SqlitePool,
    provider_name: &str,
) -> Result<Option<ProviderConfigRecord>, sqlx::Error> {
    sqlx::query_as::<_, ProviderConfigRecord>(
        r#"
        SELECT
            provider_name,
            endpoint,
            region,
            bucket,
            force_path_style,
            enabled,
            draft_source,
            last_test_status,
            last_test_error,
            last_test_at,
            created_at,
            updated_at
        FROM provider_configs
        WHERE provider_name = ?
        "#,
    )
    .bind(provider_name)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn list_provider_configs(
    pool: &SqlitePool,
) -> Result<Vec<ProviderConfigRecord>, sqlx::Error> {
    sqlx::query_as::<_, ProviderConfigRecord>(
        r#"
        SELECT
            provider_name,
            endpoint,
            region,
            bucket,
            force_path_style,
            enabled,
            draft_source,
            last_test_status,
            last_test_error,
            last_test_at,
            created_at,
            updated_at
        FROM provider_configs
        ORDER BY provider_name ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn upsert_provider_config(
    pool: &SqlitePool,
    provider_name: &str,
    endpoint: &str,
    region: &str,
    bucket: &str,
    force_path_style: bool,
    enabled: bool,
    draft_source: Option<&str>,
    last_test_status: Option<&str>,
    last_test_error: Option<&str>,
    last_test_at: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO provider_configs (
            provider_name,
            endpoint,
            region,
            bucket,
            force_path_style,
            enabled,
            draft_source,
            last_test_status,
            last_test_error,
            last_test_at,
            created_at,
            updated_at
        )
        VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        )
        ON CONFLICT(provider_name) DO UPDATE SET
            endpoint = excluded.endpoint,
            region = excluded.region,
            bucket = excluded.bucket,
            force_path_style = excluded.force_path_style,
            enabled = excluded.enabled,
            draft_source = excluded.draft_source,
            last_test_status = excluded.last_test_status,
            last_test_error = excluded.last_test_error,
            last_test_at = excluded.last_test_at,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(provider_name)
    .bind(endpoint)
    .bind(region)
    .bind(bucket)
    .bind(i64::from(force_path_style))
    .bind(i64::from(enabled))
    .bind(draft_source)
    .bind(last_test_status)
    .bind(last_test_error)
    .bind(last_test_at)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_provider_secret(
    pool: &SqlitePool,
    provider_name: &str,
) -> Result<Option<ProviderSecretRecord>, sqlx::Error> {
    sqlx::query_as::<_, ProviderSecretRecord>(
        r#"
        SELECT
            provider_name,
            access_key_id_ciphertext,
            secret_access_key_ciphertext,
            created_at,
            updated_at
        FROM provider_secrets
        WHERE provider_name = ?
        "#,
    )
    .bind(provider_name)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn upsert_provider_secret(
    pool: &SqlitePool,
    provider_name: &str,
    access_key_id_ciphertext: &[u8],
    secret_access_key_ciphertext: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO provider_secrets (
            provider_name,
            access_key_id_ciphertext,
            secret_access_key_ciphertext,
            created_at,
            updated_at
        )
        VALUES (
            ?, ?, ?,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        )
        ON CONFLICT(provider_name) DO UPDATE SET
            access_key_id_ciphertext = excluded.access_key_id_ciphertext,
            secret_access_key_ciphertext = excluded.secret_access_key_ciphertext,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(provider_name)
    .bind(access_key_id_ciphertext)
    .bind(secret_access_key_ciphertext)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_local_device_identity(
    pool: &SqlitePool,
) -> Result<Option<LocalDeviceIdentityRecord>, sqlx::Error> {
    sqlx::query_as::<_, LocalDeviceIdentityRecord>(
        r#"
        SELECT device_id, device_name, peer_token, created_at, updated_at,
               encrypted_private_key, public_key
        FROM local_device_identity
        WHERE id = 1
        "#,
    )
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn upsert_local_device_identity(
    pool: &SqlitePool,
    device_id: &str,
    device_name: &str,
    peer_token: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO local_device_identity (
            id,
            device_id,
            device_name,
            peer_token,
            created_at,
            updated_at
        )
        VALUES (
            1,
            ?,
            ?,
            ?,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        )
        ON CONFLICT(id) DO UPDATE SET
            device_id = excluded.device_id,
            device_name = excluded.device_name,
            peer_token = excluded.peer_token,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(device_id)
    .bind(device_name)
    .bind(peer_token)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn update_local_device_name(
    pool: &SqlitePool,
    device_name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE local_device_identity
        SET device_name = ?,
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        WHERE id = 1
        "#,
    )
    .bind(device_name)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn store_device_keypair(
    pool: &SqlitePool,
    encrypted_private_key: &[u8],
    public_key: &[u8],
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "UPDATE local_device_identity \
         SET encrypted_private_key = ?, public_key = ?, updated_at = ? \
         WHERE id = 1",
    )
    .bind(encrypted_private_key)
    .bind(public_key)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn upsert_trusted_peer(
    pool: &SqlitePool,
    peer_id: &str,
    device_name: &str,
    vault_id: &str,
    peer_api_base: &str,
    last_error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO trusted_peers (
            peer_id,
            device_name,
            vault_id,
            peer_api_base,
            trusted,
            last_seen_at,
            last_handshake_at,
            last_error
        )
        VALUES (
            ?,
            ?,
            ?,
            ?,
            1,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            ?
        )
        ON CONFLICT(peer_id) DO UPDATE SET
            device_name = excluded.device_name,
            vault_id = excluded.vault_id,
            peer_api_base = excluded.peer_api_base,
            trusted = 1,
            last_seen_at = excluded.last_seen_at,
            last_handshake_at = excluded.last_handshake_at,
            last_error = excluded.last_error
        "#,
    )
    .bind(peer_id)
    .bind(device_name)
    .bind(vault_id)
    .bind(peer_api_base)
    .bind(last_error)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn note_peer_seen(
    pool: &SqlitePool,
    peer_id: &str,
    device_name: &str,
    vault_id: &str,
    peer_api_base: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO trusted_peers (
            peer_id,
            device_name,
            vault_id,
            peer_api_base,
            trusted,
            last_seen_at,
            last_error
        )
        VALUES (
            ?,
            ?,
            ?,
            ?,
            1,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            NULL
        )
        ON CONFLICT(peer_id) DO UPDATE SET
            device_name = excluded.device_name,
            vault_id = excluded.vault_id,
            peer_api_base = excluded.peer_api_base,
            trusted = 1,
            last_seen_at = excluded.last_seen_at
        "#,
    )
    .bind(peer_id)
    .bind(device_name)
    .bind(vault_id)
    .bind(peer_api_base)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn update_peer_error(
    pool: &SqlitePool,
    peer_id: &str,
    last_error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE trusted_peers
        SET last_error = ?,
            last_seen_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        WHERE peer_id = ?
        "#,
    )
    .bind(last_error)
    .bind(peer_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn list_trusted_peers(pool: &SqlitePool) -> Result<Vec<TrustedPeerRecord>, sqlx::Error> {
    sqlx::query_as::<_, TrustedPeerRecord>(
        r#"
        SELECT
            peer_id,
            device_name,
            vault_id,
            peer_api_base,
            trusted,
            last_seen_at,
            last_handshake_at,
            last_error
        FROM trusted_peers
        WHERE trusted = 1
        ORDER BY last_seen_at DESC, device_name ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_trusted_peer_by_id(
    pool: &SqlitePool,
    peer_id: &str,
) -> Result<Option<TrustedPeerRecord>, sqlx::Error> {
    sqlx::query_as::<_, TrustedPeerRecord>(
        r#"
        SELECT
            peer_id,
            device_name,
            vault_id,
            peer_api_base,
            trusted,
            last_seen_at,
            last_handshake_at,
            last_error
        FROM trusted_peers
        WHERE peer_id = ?
        LIMIT 1
        "#,
    )
    .bind(peer_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn create_inode(
    pool: &SqlitePool,
    parent_id: Option<i64>,
    name: &str,
    kind: &str,
    size: i64,
) -> Result<i64, sqlx::Error> {
    validate_inode_kind(kind)?;

    let result = sqlx::query(
        r#"
        INSERT INTO inodes (parent_id, name, kind, size)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(parent_id)
    .bind(name)
    .bind(kind)
    .bind(size)
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

#[allow(dead_code)]
pub async fn upsert_inode(
    pool: &SqlitePool,
    parent_id: Option<i64>,
    name: &str,
    kind: &str,
    size: i64,
    mtime: Option<i64>,
) -> Result<i64, sqlx::Error> {
    validate_inode_kind(kind)?;

    if let Some(existing) = get_inode_by_path(pool, parent_id, name).await? {
        if existing.kind != kind {
            return Err(sqlx::Error::InvalidArgument(format!(
                "inode kind mismatch for '{name}': existing={}, new={kind}",
                existing.kind
            )));
        }

        sqlx::query(
            r#"
            UPDATE inodes
            SET size = ?, mtime = ?
            WHERE id = ?
            "#,
        )
        .bind(size)
        .bind(mtime)
        .bind(existing.id)
        .execute(pool)
        .await?;

        return Ok(existing.id);
    }

    let result = sqlx::query(
        r#"
        INSERT INTO inodes (parent_id, name, kind, size, mtime)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(parent_id)
    .bind(name)
    .bind(kind)
    .bind(size)
    .bind(mtime)
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

#[allow(dead_code)]
pub async fn get_inode_by_path(
    pool: &SqlitePool,
    parent_id: Option<i64>,
    name: &str,
) -> Result<Option<InodeRecord>, sqlx::Error> {
    sqlx::query_as::<_, InodeRecord>(
        r#"
        SELECT id, parent_id, name, kind, size, mode, mtime
        FROM inodes
        WHERE ((parent_id IS NULL AND ? IS NULL) OR parent_id = ?)
          AND name = ?
        "#,
    )
    .bind(parent_id)
    .bind(parent_id)
    .bind(name)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_inode_by_id(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Option<InodeRecord>, sqlx::Error> {
    sqlx::query_as::<_, InodeRecord>(
        r#"
        SELECT id, parent_id, name, kind, size, mode, mtime
        FROM inodes
        WHERE id = ?
        "#,
    )
    .bind(inode_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn resolve_path(pool: &SqlitePool, path: &str) -> Result<Option<i64>, sqlx::Error> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return Ok(None);
    }

    let mut current_parent_id = None;

    for segment in trimmed.split('/').filter(|segment| !segment.is_empty()) {
        let inode = match get_inode_by_path(pool, current_parent_id, segment).await? {
            Some(inode) => inode,
            None => return Ok(None),
        };

        current_parent_id = Some(inode.id);
    }

    Ok(current_parent_id)
}

#[allow(dead_code)]
pub async fn create_file_revision(
    pool: &SqlitePool,
    inode_id: i64,
    size: i64,
    immutable_until: Option<i64>,
    device_id: Option<&str>,
    parent_revision_id: Option<i64>,
    origin: &str,
    conflict_reason: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        UPDATE file_revisions
        SET is_current = 0
        WHERE inode_id = ?
        "#,
    )
    .bind(inode_id)
    .execute(&mut *tx)
    .await?;

    let result = sqlx::query(
        r#"
        INSERT INTO file_revisions (
            inode_id,
            created_at,
            size,
            is_current,
            immutable_until,
            device_id,
            parent_revision_id,
            origin,
            conflict_reason
        )
        VALUES (
            ?,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            ?,
            1,
            ?,
            ?,
            ?,
            ?,
            ?
        )
        "#,
    )
    .bind(inode_id)
    .bind(size)
    .bind(immutable_until)
    .bind(device_id)
    .bind(parent_revision_id)
    .bind(origin)
    .bind(conflict_reason)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(result.last_insert_rowid())
}

#[allow(dead_code)]
pub async fn upsert_sync_policy(
    pool: &SqlitePool,
    path_prefix: &str,
    require_healthy: bool,
    enable_versioning: bool,
) -> Result<i64, sqlx::Error> {
    let policy_type = if require_healthy {
        "PARANOIA"
    } else {
        "STANDARD"
    };
    sqlx::query(
        r#"
        INSERT INTO sync_policies (path_prefix, require_healthy, enable_versioning, policy_type)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(path_prefix) DO UPDATE SET
            require_healthy = excluded.require_healthy,
            enable_versioning = excluded.enable_versioning,
            policy_type = excluded.policy_type
        "#,
    )
    .bind(path_prefix)
    .bind(if require_healthy { 1 } else { 0 })
    .bind(if enable_versioning { 1 } else { 0 })
    .bind(policy_type)
    .execute(pool)
    .await?;

    let policy_id = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT policy_id
        FROM sync_policies
        WHERE path_prefix = ?
        LIMIT 1
        "#,
    )
    .bind(path_prefix)
    .fetch_one(pool)
    .await?;

    Ok(policy_id)
}

#[allow(dead_code)]
pub async fn list_sync_policies(pool: &SqlitePool) -> Result<Vec<SyncPolicyRecord>, sqlx::Error> {
    sqlx::query_as::<_, SyncPolicyRecord>(
        r#"
        SELECT
            policy_id,
            path_prefix,
            require_healthy,
            enable_versioning,
            COALESCE(policy_type, 'PARANOIA') AS policy_type
        FROM sync_policies
        ORDER BY LENGTH(path_prefix) DESC, policy_id ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn set_sync_policy_type_for_path(
    pool: &SqlitePool,
    path_prefix: &str,
    policy_type: &str,
) -> Result<i64, sqlx::Error> {
    let (require_healthy, enable_versioning) = match policy_type {
        "PARANOIA" => (1_i64, 1_i64),
        "STANDARD" => (0_i64, 1_i64),
        "LOCAL" => (0_i64, 1_i64),
        _ => (1_i64, 1_i64),
    };

    sqlx::query(
        r#"
        INSERT INTO sync_policies (path_prefix, require_healthy, enable_versioning, policy_type)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(path_prefix) DO UPDATE SET
            require_healthy = excluded.require_healthy,
            enable_versioning = excluded.enable_versioning,
            policy_type = excluded.policy_type
        "#,
    )
    .bind(path_prefix)
    .bind(require_healthy)
    .bind(enable_versioning)
    .bind(policy_type)
    .execute(pool)
    .await?;

    let policy_id = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT policy_id
        FROM sync_policies
        WHERE path_prefix = ?
        LIMIT 1
        "#,
    )
    .bind(path_prefix)
    .fetch_one(pool)
    .await?;

    Ok(policy_id)
}

#[allow(dead_code)]
pub async fn ensure_smart_sync_state(
    pool: &SqlitePool,
    inode_id: i64,
    revision_id: i64,
) -> Result<SmartSyncStateRecord, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO smart_sync_state (inode_id, revision_id, pin_state, hydration_state)
        VALUES (?, ?, 0, 0)
        ON CONFLICT(inode_id) DO UPDATE SET
            revision_id = excluded.revision_id
        "#,
    )
    .bind(inode_id)
    .bind(revision_id)
    .execute(pool)
    .await?;

    get_smart_sync_state(pool, inode_id)
        .await?
        .ok_or_else(|| sqlx::Error::RowNotFound)
}

#[allow(dead_code)]
pub async fn get_smart_sync_state(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Option<SmartSyncStateRecord>, sqlx::Error> {
    sqlx::query_as::<_, SmartSyncStateRecord>(
        r#"
        SELECT inode_id, revision_id, pin_state, hydration_state
        FROM smart_sync_state
        WHERE inode_id = ?
        "#,
    )
    .bind(inode_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn record_metadata_backup_attempt(
    pool: &SqlitePool,
    backup_id: &str,
    created_at: i64,
    snapshot_version: i64,
    object_key: &str,
    provider: &str,
    encrypted_size: i64,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO metadata_backups (
            backup_id,
            created_at,
            snapshot_version,
            object_key,
            provider,
            encrypted_size,
            status,
            last_error
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
        ON CONFLICT(backup_id) DO UPDATE SET
            created_at = excluded.created_at,
            snapshot_version = excluded.snapshot_version,
            object_key = excluded.object_key,
            provider = excluded.provider,
            encrypted_size = excluded.encrypted_size,
            status = excluded.status,
            last_error = NULL
        "#,
    )
    .bind(backup_id)
    .bind(created_at)
    .bind(snapshot_version)
    .bind(object_key)
    .bind(provider)
    .bind(encrypted_size)
    .bind(status)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn update_metadata_backup_status(
    pool: &SqlitePool,
    backup_id: &str,
    status: &str,
    last_error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE metadata_backups
        SET status = ?, last_error = ?
        WHERE backup_id = ?
        "#,
    )
    .bind(status)
    .bind(last_error)
    .bind(backup_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_last_successful_metadata_backup_at(
    pool: &SqlitePool,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, Option<i64>>(
        r#"
        SELECT MAX(created_at)
        FROM metadata_backups
        WHERE status = 'COMPLETED'
        "#,
    )
    .fetch_one(pool)
    .await
}

#[allow(dead_code)]
pub async fn list_recent_metadata_backups(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<MetadataBackupRecord>, sqlx::Error> {
    sqlx::query_as::<_, MetadataBackupRecord>(
        r#"
        SELECT
            backup_id,
            created_at,
            snapshot_version,
            object_key,
            provider,
            encrypted_size,
            status,
            last_error
        FROM metadata_backups
        ORDER BY created_at DESC, backup_id DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn set_pin_state(
    pool: &SqlitePool,
    inode_id: i64,
    pin_state: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE smart_sync_state
        SET pin_state = ?
        WHERE inode_id = ?
        "#,
    )
    .bind(pin_state)
    .bind(inode_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn set_hydration_state(
    pool: &SqlitePool,
    inode_id: i64,
    hydration_state: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE smart_sync_state
        SET hydration_state = ?
        WHERE inode_id = ?
        "#,
    )
    .bind(hydration_state)
    .bind(inode_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn find_sync_policy_for_path(
    pool: &SqlitePool,
    path: &str,
) -> Result<Option<SyncPolicyRecord>, sqlx::Error> {
    let normalized_path = normalize_policy_path(path);
    let policies = list_sync_policies(pool).await?;

    Ok(policies
        .into_iter()
        .filter(|policy| path_matches_policy(&normalized_path, &policy.path_prefix))
        .max_by_key(|policy| policy.path_prefix.len()))
}

#[allow(dead_code)]
pub async fn get_current_file_revision(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Option<FileRevisionRecord>, sqlx::Error> {
    sqlx::query_as::<_, FileRevisionRecord>(
        r#"
        SELECT revision_id, inode_id, created_at, size, is_current, immutable_until, device_id, parent_revision_id, origin, conflict_reason
        FROM file_revisions
        WHERE inode_id = ?
          AND is_current = 1
        ORDER BY revision_id DESC
        LIMIT 1
        "#,
    )
    .bind(inode_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_storage_mode_for_inode(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<StorageMode, sqlx::Error> {
    let inode_path = get_inode_path(pool, inode_id)
        .await?
        .unwrap_or_else(|| format!("inode/{inode_id}"));
    let policy_type = find_sync_policy_for_path(pool, &inode_path)
        .await?
        .map(|policy| policy.policy_type)
        .unwrap_or_else(|| "PARANOIA".to_string());
    Ok(StorageMode::from_policy_type(&policy_type))
}

#[allow(dead_code)]
pub async fn get_file_revision(
    pool: &SqlitePool,
    inode_id: i64,
    revision_id: i64,
) -> Result<Option<FileRevisionRecord>, sqlx::Error> {
    sqlx::query_as::<_, FileRevisionRecord>(
        r#"
        SELECT revision_id, inode_id, created_at, size, is_current, immutable_until, device_id, parent_revision_id, origin, conflict_reason
        FROM file_revisions
        WHERE inode_id = ?
          AND revision_id = ?
        LIMIT 1
        "#,
    )
    .bind(inode_id)
    .bind(revision_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn list_file_revisions(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Vec<FileRevisionRecord>, sqlx::Error> {
    sqlx::query_as::<_, FileRevisionRecord>(
        r#"
        SELECT revision_id, inode_id, created_at, size, is_current, immutable_until, device_id, parent_revision_id, origin, conflict_reason
        FROM file_revisions
        WHERE inode_id = ?
        ORDER BY created_at DESC, revision_id DESC
        "#,
    )
    .bind(inode_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_referencing_inode_ids_for_pack(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT DISTINCT fr.inode_id
        FROM pack_locations pl
        INNER JOIN chunk_refs cr
            ON cr.chunk_id = pl.chunk_id
        INNER JOIN file_revisions fr
            ON fr.revision_id = cr.revision_id
        WHERE pl.pack_id = ?
        ORDER BY fr.inode_id ASC
        "#,
    )
    .bind(pack_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn promote_revision_to_current(
    pool: &SqlitePool,
    revision_id: i64,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    let inode_id = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT inode_id
        FROM file_revisions
        WHERE revision_id = ?
        "#,
    )
    .bind(revision_id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE file_revisions
        SET is_current = 0
        WHERE inode_id = ?
        "#,
    )
    .bind(inode_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE file_revisions
        SET is_current = 1
        WHERE revision_id = ?
        "#,
    )
    .bind(revision_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn classify_revision_lineage(
    pool: &SqlitePool,
    candidate_revision_id: i64,
    current_revision_id: i64,
) -> Result<RevisionLineageRelation, sqlx::Error> {
    if candidate_revision_id == current_revision_id {
        return Ok(RevisionLineageRelation::Same);
    }

    if is_revision_ancestor(pool, current_revision_id, candidate_revision_id).await? {
        return Ok(RevisionLineageRelation::CandidateDescendsFromCurrent);
    }

    if is_revision_ancestor(pool, candidate_revision_id, current_revision_id).await? {
        return Ok(RevisionLineageRelation::CurrentDescendsFromCandidate);
    }

    Ok(RevisionLineageRelation::Parallel)
}

#[allow(dead_code)]
pub async fn register_chunk(
    pool: &SqlitePool,
    revision_id: i64,
    chunk_id: &[u8],
    offset: i64,
    size: i64,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        INSERT INTO chunk_refs (revision_id, chunk_id, file_offset, size)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(revision_id)
    .bind(chunk_id)
    .bind(offset)
    .bind(size)
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

#[allow(dead_code)]
pub async fn copy_chunk_refs(
    pool: &SqlitePool,
    from_revision_id: i64,
    to_revision_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO chunk_refs (revision_id, chunk_id, file_offset, size)
        SELECT ?, chunk_id, file_offset, size
        FROM chunk_refs
        WHERE revision_id = ?
        ORDER BY file_offset ASC
        "#,
    )
    .bind(to_revision_id)
    .bind(from_revision_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn create_conflict_event(
    pool: &SqlitePool,
    inode_id: i64,
    winning_revision_id: i64,
    losing_revision_id: i64,
    reason: &str,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        INSERT INTO conflict_events (
            inode_id,
            winning_revision_id,
            losing_revision_id,
            reason,
            created_at
        )
        VALUES (
            ?,
            ?,
            ?,
            ?,
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        )
        "#,
    )
    .bind(inode_id)
    .bind(winning_revision_id)
    .bind(losing_revision_id)
    .bind(reason)
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

#[allow(dead_code)]
pub async fn materialize_conflict_copy_from_revision(
    pool: &SqlitePool,
    source_revision_id: i64,
    device_id: Option<&str>,
    device_name: &str,
    reason: &str,
) -> Result<(i64, i64, String, i64), sqlx::Error> {
    let source_revision = sqlx::query_as::<_, FileRevisionRecord>(
        r#"
        SELECT revision_id, inode_id, created_at, size, is_current, immutable_until, device_id, parent_revision_id, origin, conflict_reason
        FROM file_revisions
        WHERE revision_id = ?
        LIMIT 1
        "#,
    )
    .bind(source_revision_id)
    .fetch_one(pool)
    .await?;

    let source_inode = get_inode_by_id(pool, source_revision.inode_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

    let timestamp = source_revision.created_at;
    let base_name = build_conflict_copy_name(&source_inode.name, device_name, timestamp);

    let mut created_inode_id = None;
    let mut final_name = base_name.clone();
    for attempt in 0..16 {
        let candidate = if attempt == 0 {
            base_name.clone()
        } else {
            disambiguate_conflict_copy_name(&base_name, attempt)
        };

        match create_inode(
            pool,
            source_inode.parent_id,
            &candidate,
            &source_inode.kind,
            source_revision.size,
        )
        .await
        {
            Ok(inode_id) => {
                created_inode_id = Some(inode_id);
                final_name = candidate;
                break;
            }
            Err(sqlx::Error::Database(err)) if err.is_unique_violation() => continue,
            Err(err) => return Err(err),
        }
    }

    let inode_id = created_inode_id.ok_or(sqlx::Error::RowNotFound)?;
    let revision_id = create_file_revision(
        pool,
        inode_id,
        source_revision.size,
        source_revision.immutable_until,
        device_id,
        Some(source_revision.revision_id),
        "conflict_copy",
        Some(reason),
    )
    .await?;
    copy_chunk_refs(pool, source_revision.revision_id, revision_id).await?;
    let conflict_id = create_conflict_event(
        pool,
        source_revision.inode_id,
        source_revision.revision_id,
        revision_id,
        reason,
    )
    .await?;
    attach_conflict_materialization(pool, conflict_id, inode_id, revision_id).await?;

    Ok((inode_id, revision_id, final_name, conflict_id))
}

#[allow(dead_code)]
pub async fn attach_conflict_materialization(
    pool: &SqlitePool,
    conflict_id: i64,
    materialized_inode_id: i64,
    materialized_revision_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE conflict_events
        SET materialized_inode_id = ?,
            materialized_revision_id = ?
        WHERE conflict_id = ?
        "#,
    )
    .bind(materialized_inode_id)
    .bind(materialized_revision_id)
    .bind(conflict_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn list_recent_conflicts(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<ConflictEventRecord>, sqlx::Error> {
    sqlx::query_as::<_, ConflictEventRecord>(
        r#"
        SELECT
            conflict_id,
            inode_id,
            winning_revision_id,
            losing_revision_id,
            reason,
            materialized_inode_id,
            materialized_revision_id,
            created_at
        FROM conflict_events
        ORDER BY created_at DESC, conflict_id DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_chunk_lookup_by_chunk_id(
    pool: &SqlitePool,
    chunk_id: &[u8],
) -> Result<Option<ChunkLookupRecord>, sqlx::Error> {
    sqlx::query_as::<_, ChunkLookupRecord>(
        r#"
        WITH ordered_chunks AS (
            SELECT
                fr.inode_id,
                fr.revision_id,
                cr.chunk_id,
                cr.file_offset,
                cr.size,
                ROW_NUMBER() OVER (
                    PARTITION BY fr.revision_id
                    ORDER BY cr.file_offset ASC
                ) - 1 AS chunk_index
            FROM chunk_refs cr
            INNER JOIN file_revisions fr
                ON fr.revision_id = cr.revision_id
            WHERE cr.chunk_id = ?
            ORDER BY fr.is_current DESC, fr.created_at DESC, fr.revision_id DESC
        )
        SELECT
            oc.inode_id,
            oc.revision_id,
            oc.chunk_id,
            oc.chunk_index,
            oc.file_offset,
            oc.size,
            pl.pack_id,
            pl.pack_offset,
            pl.encrypted_size
        FROM ordered_chunks oc
        INNER JOIN pack_locations pl
            ON pl.chunk_id = oc.chunk_id
        LIMIT 1
        "#,
    )
    .bind(chunk_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn delete_file_chunks(pool: &SqlitePool, inode_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        DELETE FROM chunk_refs
        WHERE revision_id IN (
            SELECT revision_id
            FROM file_revisions
            WHERE inode_id = ?
        )
        "#,
    )
    .bind(inode_id)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        DELETE FROM file_revisions
        WHERE inode_id = ?
        "#,
    )
    .bind(inode_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn delete_inode_record(pool: &SqlitePool, inode_id: i64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        DELETE FROM inodes
        WHERE id = ?
        "#,
    )
    .bind(inode_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

#[allow(dead_code)]
pub async fn create_pack(
    pool: &SqlitePool,
    pack_id: &str,
    chunk_id: &[u8],
    plaintext_hash: &str,
    storage_mode: StorageMode,
    encryption_version: i64,
    ec_scheme: &str,
    logical_size: i64,
    cipher_size: i64,
    shard_size: i64,
    nonce: &[u8],
    gcm_tag: &[u8],
    status: PackStatus,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO packs (
            pack_id,
            chunk_id,
            plaintext_hash,
            storage_mode,
            encryption_version,
            ec_scheme,
            logical_size,
            cipher_size,
            shard_size,
            nonce,
            gcm_tag,
            status
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(pack_id) DO UPDATE SET
            chunk_id = excluded.chunk_id,
            plaintext_hash = excluded.plaintext_hash,
            storage_mode = excluded.storage_mode,
            encryption_version = excluded.encryption_version,
            ec_scheme = excluded.ec_scheme,
            logical_size = excluded.logical_size,
            cipher_size = excluded.cipher_size,
            shard_size = excluded.shard_size,
            nonce = excluded.nonce,
            gcm_tag = excluded.gcm_tag,
            status = excluded.status
        "#,
    )
    .bind(pack_id)
    .bind(chunk_id)
    .bind(plaintext_hash)
    .bind(storage_mode.as_str())
    .bind(encryption_version)
    .bind(ec_scheme)
    .bind(logical_size)
    .bind(cipher_size)
    .bind(shard_size)
    .bind(nonce)
    .bind(gcm_tag)
    .bind(status.as_str())
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn update_pack_status(
    pool: &SqlitePool,
    pack_id: &str,
    status: PackStatus,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE packs
        SET status = ?
        WHERE pack_id = ?
        "#,
    )
    .bind(status.as_str())
    .bind(pack_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_pack(pool: &SqlitePool, pack_id: &str) -> Result<Option<PackRecord>, sqlx::Error> {
    sqlx::query_as::<_, PackRecord>(
        r#"
        SELECT
            pack_id,
            chunk_id,
            plaintext_hash,
            storage_mode,
            encryption_version,
            ec_scheme,
            logical_size,
            cipher_size,
            shard_size,
            nonce,
            gcm_tag,
            status
        FROM packs
        WHERE pack_id = ?
        "#,
    )
    .bind(pack_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn find_pack_by_plaintext_hash(
    pool: &SqlitePool,
    plaintext_hash: &str,
    storage_mode: StorageMode,
) -> Result<Option<PackRecord>, sqlx::Error> {
    sqlx::query_as::<_, PackRecord>(
        r#"
        SELECT
            pack_id,
            chunk_id,
            plaintext_hash,
            storage_mode,
            encryption_version,
            ec_scheme,
            logical_size,
            cipher_size,
            shard_size,
            nonce,
            gcm_tag,
            status
        FROM packs
        WHERE plaintext_hash = ?
          AND storage_mode = ?
          AND status != 'UNREADABLE'
        ORDER BY
            CASE status
                WHEN 'COMPLETED_HEALTHY' THEN 0
                WHEN 'COMPLETED_DEGRADED' THEN 1
                WHEN 'UPLOADING' THEN 2
                ELSE 3
            END,
            pack_id ASC
        LIMIT 1
        "#,
    )
    .bind(plaintext_hash)
    .bind(storage_mode.as_str())
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_orphaned_pack_ids(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT p.pack_id
        FROM packs p
        LEFT JOIN pack_locations pl
            ON pl.pack_id = p.pack_id
        WHERE pl.pack_id IS NULL
          AND p.status != 'UPLOADING'
        ORDER BY p.pack_id ASC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_next_degraded_pack(pool: &SqlitePool) -> Result<Option<PackRecord>, sqlx::Error> {
    sqlx::query_as::<_, PackRecord>(
        r#"
        SELECT
            pack_id,
            chunk_id,
            plaintext_hash,
            storage_mode,
            encryption_version,
            ec_scheme,
            logical_size,
            cipher_size,
            shard_size,
            nonce,
            gcm_tag,
            status
        FROM packs
        WHERE status = 'COMPLETED_DEGRADED'
        ORDER BY pack_id ASC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_vault_health_summary(
    pool: &SqlitePool,
) -> Result<VaultHealthSummary, sqlx::Error> {
    sqlx::query_as::<_, VaultHealthSummary>(
        r#"
        SELECT
            COUNT(*) AS total_packs,
            COALESCE(SUM(CASE WHEN status = 'COMPLETED_HEALTHY' THEN 1 ELSE 0 END), 0) AS healthy_packs,
            COALESCE(SUM(CASE WHEN status = 'COMPLETED_DEGRADED' THEN 1 ELSE 0 END), 0) AS degraded_packs,
            COALESCE(SUM(CASE WHEN status = 'UNREADABLE' THEN 1 ELSE 0 END), 0) AS unreadable_packs
        FROM packs
        "#,
    )
    .fetch_one(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_scrub_status_summary(
    pool: &SqlitePool,
) -> Result<ScrubStatusSummary, sqlx::Error> {
    sqlx::query_as::<_, ScrubStatusSummary>(
        r#"
        SELECT
            COUNT(*) AS total_shards,
            COALESCE(SUM(CASE WHEN last_verified_at IS NOT NULL THEN 1 ELSE 0 END), 0) AS verified_shards,
            COALESCE(SUM(CASE WHEN last_verification_status = 'HEALTHY' THEN 1 ELSE 0 END), 0) AS healthy_shards,
            COALESCE(SUM(CASE WHEN last_verification_status IN ('MISSING', 'SIZE_MISMATCH', 'CORRUPTED') THEN 1 ELSE 0 END), 0) AS corrupted_or_missing,
            COALESCE(SUM(CASE WHEN last_verification_method = 'LIGHT' THEN 1 ELSE 0 END), 0) AS verified_light_shards,
            COALESCE(SUM(CASE WHEN last_verification_method = 'DEEP' THEN 1 ELSE 0 END), 0) AS verified_deep_shards,
            MAX(last_verified_at) AS last_scrub_timestamp
        FROM pack_shards
        "#,
    )
    .fetch_one(pool)
    .await
}

#[allow(dead_code)]
pub async fn list_scrub_errors(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<ScrubErrorRecord>, sqlx::Error> {
    sqlx::query_as::<_, ScrubErrorRecord>(
        r#"
        SELECT
            pack_id,
            provider,
            shard_index,
            last_verified_at,
            last_verification_status
        FROM pack_shards
        WHERE last_verification_status IS NOT NULL
          AND last_verification_status != 'HEALTHY'
        ORDER BY COALESCE(last_verified_at, 0) DESC, pack_id ASC, shard_index ASC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_cache_entry(
    pool: &SqlitePool,
    cache_key: &str,
) -> Result<Option<CacheEntryRecord>, sqlx::Error> {
    sqlx::query_as::<_, CacheEntryRecord>(
        r#"
        SELECT
            cache_key,
            inode_id,
            revision_id,
            chunk_index,
            pack_id,
            file_path,
            cache_path,
            size,
            created_at,
            last_accessed_at,
            access_count,
            is_prefetched
        FROM cache_entries
        WHERE cache_key = ?
        "#,
    )
    .bind(cache_key)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn upsert_cache_entry(
    pool: &SqlitePool,
    cache_key: &str,
    inode_id: i64,
    revision_id: i64,
    chunk_index: i64,
    pack_id: &str,
    file_path: &str,
    cache_path: &str,
    size: i64,
    is_prefetched: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO cache_entries (
            cache_key,
            inode_id,
            revision_id,
            chunk_index,
            pack_id,
            file_path,
            cache_path,
            size,
            created_at,
            last_accessed_at,
            access_count,
            is_prefetched
        )
        VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?,
            CAST(strftime('%s','now') AS INTEGER),
            CAST(strftime('%s','now') AS INTEGER),
            1,
            ?
        )
        ON CONFLICT(cache_key) DO UPDATE SET
            inode_id = excluded.inode_id,
            revision_id = excluded.revision_id,
            chunk_index = excluded.chunk_index,
            pack_id = excluded.pack_id,
            file_path = excluded.file_path,
            cache_path = excluded.cache_path,
            size = excluded.size,
            last_accessed_at = CAST(strftime('%s','now') AS INTEGER),
            access_count = cache_entries.access_count + 1,
            is_prefetched = excluded.is_prefetched
        "#,
    )
    .bind(cache_key)
    .bind(inode_id)
    .bind(revision_id)
    .bind(chunk_index)
    .bind(pack_id)
    .bind(file_path)
    .bind(cache_path)
    .bind(size)
    .bind(if is_prefetched { 1 } else { 0 })
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn touch_cache_entry(pool: &SqlitePool, cache_key: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE cache_entries
        SET last_accessed_at = CAST(strftime('%s','now') AS INTEGER),
            access_count = access_count + 1
        WHERE cache_key = ?
        "#,
    )
    .bind(cache_key)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn list_cache_entries_by_lru(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<CacheEntryRecord>, sqlx::Error> {
    sqlx::query_as::<_, CacheEntryRecord>(
        r#"
        SELECT
            cache_key,
            inode_id,
            revision_id,
            chunk_index,
            pack_id,
            file_path,
            cache_path,
            size,
            created_at,
            last_accessed_at,
            access_count,
            is_prefetched
        FROM cache_entries
        ORDER BY last_accessed_at ASC, created_at ASC, cache_key ASC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_total_cache_size(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(SUM(size), 0)
        FROM cache_entries
        "#,
    )
    .fetch_one(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_cache_status_summary(
    pool: &SqlitePool,
) -> Result<CacheStatusSummary, sqlx::Error> {
    sqlx::query_as::<_, CacheStatusSummary>(
        r#"
        SELECT
            COUNT(*) AS total_entries,
            COALESCE(SUM(size), 0) AS total_bytes,
            COALESCE(SUM(CASE WHEN is_prefetched = 1 THEN 1 ELSE 0 END), 0) AS prefetched_entries
        FROM cache_entries
        "#,
    )
    .fetch_one(pool)
    .await
}

#[allow(dead_code)]
pub async fn delete_cache_entry(pool: &SqlitePool, cache_key: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        DELETE FROM cache_entries
        WHERE cache_key = ?
        "#,
    )
    .bind(cache_key)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_physical_usage_for_provider(
    pool: &SqlitePool,
    provider_name: &str,
) -> Result<u64, sqlx::Error> {
    let total = sqlx::query_scalar::<_, Option<i64>>(
        r#"
        SELECT COALESCE(SUM(size), 0)
        FROM pack_shards
        WHERE provider = ?
          AND status IN ('PENDING', 'IN_PROGRESS', 'COMPLETED')
        "#,
    )
    .bind(provider_name)
    .fetch_one(pool)
    .await?
    .unwrap_or(0);

    Ok(u64::try_from(total).unwrap_or(0))
}

#[allow(dead_code)]
pub async fn get_active_storage_mode_summaries(
    pool: &SqlitePool,
) -> Result<Vec<ActiveStorageModeSummary>, sqlx::Error> {
    sqlx::query_as::<_, ActiveStorageModeSummary>(
        r#"
        WITH active_packs AS (
            SELECT DISTINCT
                p.pack_id,
                p.storage_mode,
                p.logical_size,
                p.cipher_size,
                p.shard_size
            FROM packs p
            INNER JOIN pack_locations pl
                ON pl.pack_id = p.pack_id
        ),
        physical_by_pack AS (
            SELECT
                pack_id,
                COALESCE(SUM(size), 0) AS physical_bytes
            FROM pack_shards
            WHERE status IN ('PENDING', 'IN_PROGRESS', 'COMPLETED')
            GROUP BY pack_id
        )
        SELECT
            ap.storage_mode,
            COUNT(*) AS active_packs,
            COALESCE(SUM(ap.logical_size), 0) AS logical_bytes,
            COALESCE(SUM(ap.cipher_size), 0) AS cipher_bytes,
            COALESCE(SUM(ap.shard_size), 0) AS total_shard_bytes,
            COALESCE(SUM(COALESCE(pb.physical_bytes, 0)), 0) AS physical_bytes
        FROM active_packs ap
        LEFT JOIN physical_by_pack pb
            ON pb.pack_id = ap.pack_id
        GROUP BY ap.storage_mode
        ORDER BY ap.storage_mode ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_orphaned_pack_summary(
    pool: &SqlitePool,
) -> Result<OrphanedPackSummary, sqlx::Error> {
    sqlx::query_as::<_, OrphanedPackSummary>(
        r#"
        WITH orphaned AS (
            SELECT p.pack_id
            FROM packs p
            LEFT JOIN pack_locations pl
                ON pl.pack_id = p.pack_id
            WHERE pl.pack_id IS NULL
              AND p.status != 'UPLOADING'
        )
        SELECT
            COUNT(*) AS pack_count,
            COALESCE((
                SELECT SUM(ps.size)
                FROM pack_shards ps
                INNER JOIN orphaned o
                    ON o.pack_id = ps.pack_id
                WHERE ps.status IN ('PENDING', 'IN_PROGRESS', 'COMPLETED')
            ), 0) AS physical_bytes
        FROM orphaned
        "#,
    )
    .fetch_one(pool)
    .await
}

#[allow(dead_code)]
pub async fn register_pack_shard(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
    shard_role: ShardRole,
    provider: &str,
    object_key: &str,
    size: i64,
    checksum: &str,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO pack_shards (
            pack_id,
            shard_index,
            shard_role,
            provider,
            object_key,
            size,
            checksum,
            status
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(pack_id, shard_index) DO UPDATE SET
            shard_role = excluded.shard_role,
            provider = excluded.provider,
            object_key = excluded.object_key,
            size = excluded.size,
            checksum = excluded.checksum,
            status = excluded.status,
            last_error = NULL
        "#,
    )
    .bind(pack_id)
    .bind(shard_index)
    .bind(shard_role.as_str())
    .bind(provider)
    .bind(object_key)
    .bind(size)
    .bind(checksum)
    .bind(status)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_pack_shards(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Vec<PackShardRecord>, sqlx::Error> {
    sqlx::query_as::<_, PackShardRecord>(
        r#"
        SELECT
            id,
            pack_id,
            shard_index,
            shard_role,
            provider,
            object_key,
            size,
            checksum,
            status,
            attempts,
            last_error,
            last_verified_at,
            last_verification_method,
            last_verification_status,
            last_verified_size,
            COALESCE(verification_failures, 0) AS verification_failures
        FROM pack_shards
        WHERE pack_id = ?
        ORDER BY shard_index ASC
        "#,
    )
    .bind(pack_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn delete_pack_metadata(pool: &SqlitePool, pack_id: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        DELETE FROM upload_jobs
        WHERE pack_id = ?
        "#,
    )
    .bind(pack_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        DELETE FROM pack_locations
        WHERE pack_id = ?
        "#,
    )
    .bind(pack_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        DELETE FROM packs
        WHERE pack_id = ?
        "#,
    )
    .bind(pack_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn get_incomplete_pack_shards(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Vec<PackShardRecord>, sqlx::Error> {
    sqlx::query_as::<_, PackShardRecord>(
        r#"
        SELECT
            id,
            pack_id,
            shard_index,
            shard_role,
            provider,
            object_key,
            size,
            checksum,
            status,
            attempts,
            last_error,
            last_verified_at,
            last_verification_method,
            last_verification_status,
            last_verified_size,
            COALESCE(verification_failures, 0) AS verification_failures
        FROM pack_shards
        WHERE pack_id = ?
          AND status != 'COMPLETED'
        ORDER BY shard_index ASC
        "#,
    )
    .bind(pack_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn mark_pack_shard_in_progress(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE pack_shards
        SET status = 'IN_PROGRESS',
            last_error = NULL
        WHERE pack_id = ?
          AND shard_index = ?
        "#,
    )
    .bind(pack_id)
    .bind(shard_index)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn mark_pack_shard_completed(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE pack_shards
        SET status = 'COMPLETED',
            last_error = NULL
        WHERE pack_id = ?
          AND shard_index = ?
        "#,
    )
    .bind(pack_id)
    .bind(shard_index)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn requeue_pack_shard(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
    error_message: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE pack_shards
        SET status = 'PENDING',
            attempts = COALESCE(attempts, 0) + 1,
            last_error = ?
        WHERE pack_id = ?
          AND shard_index = ?
        "#,
    )
    .bind(error_message)
    .bind(pack_id)
    .bind(shard_index)
    .execute(pool)
    .await?;

    let attempts = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(attempts, 0)
        FROM pack_shards
        WHERE pack_id = ?
          AND shard_index = ?
        "#,
    )
    .bind(pack_id)
    .bind(shard_index)
    .fetch_one(pool)
    .await?;

    Ok(attempts)
}

#[allow(dead_code)]
pub async fn mark_pack_shard_failed(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE pack_shards
        SET status = 'FAILED',
            attempts = COALESCE(attempts, 0) + 1,
            last_error = ?
        WHERE pack_id = ?
          AND shard_index = ?
        "#,
    )
    .bind(error_message)
    .bind(pack_id)
    .bind(shard_index)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_next_shards_for_scrub(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<ScrubShardRecord>, sqlx::Error> {
    sqlx::query_as::<_, ScrubShardRecord>(
        r#"
        SELECT
            id,
            pack_id,
            shard_index,
            provider,
            object_key,
            size,
            checksum,
            status,
            last_verified_at,
            last_verification_method,
            last_verification_status,
            last_verified_size,
            COALESCE(verification_failures, 0) AS verification_failures
        FROM pack_shards
        ORDER BY
            CASE WHEN last_verified_at IS NULL THEN 0 ELSE 1 END ASC,
            COALESCE(last_verified_at, 0) ASC,
            COALESCE(verification_failures, 0) DESC,
            id ASC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn update_shard_verification_status(
    pool: &SqlitePool,
    pack_id: &str,
    shard_index: i64,
    verification_method: &str,
    verification_status: &str,
    verified_size: Option<i64>,
    increment_failures: bool,
    last_error: Option<&str>,
) -> Result<(), sqlx::Error> {
    let operational_status = if verification_status == "HEALTHY" {
        "COMPLETED"
    } else {
        "FAILED"
    };

    sqlx::query(
        r#"
        UPDATE pack_shards
        SET status = ?,
            last_verified_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            last_verification_method = ?,
            last_verification_status = ?,
            last_verified_size = ?,
            verification_failures = COALESCE(verification_failures, 0) + CASE WHEN ? THEN 1 ELSE 0 END,
            last_error = ?
        WHERE pack_id = ?
          AND shard_index = ?
        "#,
    )
    .bind(operational_status)
    .bind(verification_method)
    .bind(verification_status)
    .bind(verified_size)
    .bind(increment_failures)
    .bind(last_error)
    .bind(pack_id)
    .bind(shard_index)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn reset_in_progress_pack_shards(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE pack_shards
        SET status = 'PENDING'
        WHERE status = 'IN_PROGRESS'
        "#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

#[allow(dead_code)]
pub async fn summarize_pack_shards(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<PackShardSummary, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT status, COUNT(*) AS count
        FROM pack_shards
        WHERE pack_id = ?
        GROUP BY status
        "#,
    )
    .bind(pack_id)
    .fetch_all(pool)
    .await?;

    let mut summary = PackShardSummary::default();
    for row in rows {
        let status: String = row.try_get("status")?;
        let count: i64 = row.try_get("count")?;
        summary.total += count;
        match status.as_str() {
            "COMPLETED" => summary.completed += count,
            "PENDING" => summary.pending += count,
            "IN_PROGRESS" => summary.in_progress += count,
            "FAILED" => summary.failed += count,
            _ => {}
        }
    }

    Ok(summary)
}

#[allow(dead_code)]
pub fn resolve_pack_status(summary: PackShardSummary) -> PackStatus {
    resolve_pack_status_for_mode(StorageMode::Ec2_1, summary)
}

#[allow(dead_code)]
pub fn resolve_pack_status_for_mode(
    storage_mode: StorageMode,
    summary: PackShardSummary,
) -> PackStatus {
    match storage_mode {
        StorageMode::Ec2_1 => {
            if summary.completed >= 3 {
                PackStatus::Healthy
            } else if summary.completed >= 2 {
                PackStatus::Degraded
            } else if summary.pending > 0 || summary.in_progress > 0 {
                PackStatus::Uploading
            } else {
                PackStatus::Unreadable
            }
        }
        StorageMode::SingleReplica => {
            if summary.completed >= 1 {
                PackStatus::Healthy
            } else if summary.pending > 0 || summary.in_progress > 0 {
                PackStatus::Uploading
            } else {
                PackStatus::Unreadable
            }
        }
        StorageMode::LocalOnly => PackStatus::Healthy,
    }
}

#[allow(dead_code)]
pub async fn list_active_packs(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<PackRecord>, sqlx::Error> {
    sqlx::query_as::<_, PackRecord>(
        r#"
        SELECT DISTINCT
            p.pack_id,
            p.chunk_id,
            p.plaintext_hash,
            p.storage_mode,
            p.encryption_version,
            p.ec_scheme,
            p.logical_size,
            p.cipher_size,
            p.shard_size,
            p.nonce,
            p.gcm_tag,
            p.status
        FROM packs p
        INNER JOIN pack_locations pl
            ON pl.pack_id = p.pack_id
        ORDER BY p.pack_id ASC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_desired_storage_mode_for_pack(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<StorageMode, sqlx::Error> {
    let inode_ids = get_referencing_inode_ids_for_pack(pool, pack_id).await?;
    if inode_ids.is_empty() {
        return Ok(StorageMode::Ec2_1);
    }

    let mut desired = StorageMode::LocalOnly;
    for inode_id in inode_ids {
        let inode_path = get_inode_path(pool, inode_id)
            .await?
            .unwrap_or_else(|| format!("inode/{inode_id}"));
        let policy_type = find_sync_policy_for_path(pool, &inode_path)
            .await?
            .map(|policy| policy.policy_type)
            .unwrap_or_else(|| "PARANOIA".to_string());
        match StorageMode::from_policy_type(&policy_type) {
            StorageMode::Ec2_1 => return Ok(StorageMode::Ec2_1),
            StorageMode::SingleReplica => desired = StorageMode::SingleReplica,
            StorageMode::LocalOnly => {}
        }
    }

    Ok(desired)
}

#[allow(dead_code)]
pub async fn get_next_pack_requiring_reconciliation(
    pool: &SqlitePool,
) -> Result<Option<PackRecord>, sqlx::Error> {
    for pack in list_active_packs(pool, 256).await? {
        let desired = get_desired_storage_mode_for_pack(pool, &pack.pack_id).await?;
        if StorageMode::from_str(&pack.storage_mode) != desired {
            return Ok(Some(pack));
        }
    }

    Ok(None)
}

#[allow(dead_code)]
pub async fn get_chunks_for_pack(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Vec<ChunkRecord>, sqlx::Error> {
    sqlx::query_as::<_, ChunkRecord>(
        r#"
        SELECT cr.id, cr.revision_id, cr.chunk_id, cr.file_offset, cr.size
        FROM pack_locations pl
        INNER JOIN chunk_refs cr
            ON cr.chunk_id = pl.chunk_id
        WHERE pl.pack_id = ?
        ORDER BY cr.file_offset ASC, cr.id ASC
        "#,
    )
    .bind(pack_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn link_chunk_to_pack(
    pool: &SqlitePool,
    chunk_id: &[u8],
    pack_id: &str,
    pack_offset: i64,
    enc_size: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO pack_locations (chunk_id, pack_id, pack_offset, encrypted_size)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(chunk_id) DO UPDATE SET
            pack_id = excluded.pack_id,
            pack_offset = excluded.pack_offset,
            encrypted_size = excluded.encrypted_size
        "#,
    )
    .bind(chunk_id)
    .bind(pack_id)
    .bind(pack_offset)
    .bind(enc_size)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_file_chunks(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Vec<ChunkRecord>, sqlx::Error> {
    sqlx::query_as::<_, ChunkRecord>(
        r#"
        SELECT cr.id, cr.revision_id, cr.chunk_id, cr.file_offset, cr.size
        FROM chunk_refs cr
        INNER JOIN file_revisions fr
            ON fr.revision_id = cr.revision_id
        WHERE fr.inode_id = ?
          AND fr.is_current = 1
        ORDER BY file_offset ASC
        "#,
    )
    .bind(inode_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn list_active_files(pool: &SqlitePool) -> Result<Vec<FileInventoryRecord>, sqlx::Error> {
    sqlx::query_as::<_, FileInventoryRecord>(
        r#"
        WITH RECURSIVE inode_paths AS (
            SELECT
                id,
                parent_id,
                name AS path
            FROM inodes
            WHERE parent_id IS NULL

            UNION ALL

            SELECT
                child.id,
                child.parent_id,
                inode_paths.path || '/' || child.name AS path
            FROM inodes child
            INNER JOIN inode_paths
                ON child.parent_id = inode_paths.id
        )
        SELECT
            i.id AS inode_id,
            inode_paths.path AS path,
            COALESCE(fr.size, i.size) AS size,
            fr.revision_id AS current_revision_id,
            fr.created_at AS current_revision_created_at,
            ss.pin_state AS smart_sync_pin_state,
            ss.hydration_state AS smart_sync_hydration_state
        FROM inodes i
        INNER JOIN inode_paths
            ON inode_paths.id = i.id
        LEFT JOIN file_revisions fr
            ON fr.inode_id = i.id
           AND fr.is_current = 1
        LEFT JOIN smart_sync_state ss
            ON ss.inode_id = i.id
        WHERE i.kind = 'FILE'
        ORDER BY inode_paths.path ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_active_files_for_projection(
    pool: &SqlitePool,
) -> Result<Vec<ProjectionFileRecord>, sqlx::Error> {
    let mut records = sqlx::query_as::<_, ProjectionFileRecord>(
        r#"
        WITH RECURSIVE inode_paths AS (
            SELECT
                id,
                parent_id,
                name AS path
            FROM inodes
            WHERE parent_id IS NULL

            UNION ALL

            SELECT
                child.id,
                child.parent_id,
                inode_paths.path || '/' || child.name AS path
            FROM inodes child
            INNER JOIN inode_paths
                ON child.parent_id = inode_paths.id
        )
        SELECT
            i.id AS inode_id,
            inode_paths.path AS path,
            fr.revision_id AS revision_id,
            fr.size AS size,
            fr.created_at AS created_at
        FROM inodes i
        INNER JOIN inode_paths
            ON inode_paths.id = i.id
        INNER JOIN file_revisions fr
            ON fr.inode_id = i.id
           AND fr.is_current = 1
        WHERE i.kind = 'FILE'
        ORDER BY inode_paths.path ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut base_paths = list_sync_policies(pool)
        .await?
        .into_iter()
        .map(|policy| policy.path_prefix)
        .collect::<Vec<_>>();
    if let Ok(watch_dir) = std::env::var("OMNIDRIVE_WATCH_DIR") {
        base_paths.push(watch_dir);
    }

    for record in &mut records {
        record.path = projection_relative_path(&record.path, &base_paths);
    }

    Ok(records)
}

#[allow(dead_code)]
pub async fn get_active_file_for_projection_by_inode(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Option<ProjectionFileRecord>, sqlx::Error> {
    let mut records = sqlx::query_as::<_, ProjectionFileRecord>(
        r#"
        WITH RECURSIVE inode_paths AS (
            SELECT
                id,
                parent_id,
                name AS path
            FROM inodes
            WHERE parent_id IS NULL

            UNION ALL

            SELECT
                child.id,
                child.parent_id,
                inode_paths.path || '/' || child.name AS path
            FROM inodes child
            INNER JOIN inode_paths
                ON child.parent_id = inode_paths.id
        )
        SELECT
            i.id AS inode_id,
            inode_paths.path AS path,
            fr.revision_id AS revision_id,
            fr.size AS size,
            fr.created_at AS created_at
        FROM inodes i
        INNER JOIN inode_paths
            ON inode_paths.id = i.id
        INNER JOIN file_revisions fr
            ON fr.inode_id = i.id
           AND fr.is_current = 1
        WHERE i.kind = 'FILE'
          AND i.id = ?
        LIMIT 1
        "#,
    )
    .bind(inode_id)
    .fetch_all(pool)
    .await?;

    let Some(mut record) = records.pop() else {
        return Ok(None);
    };

    let mut base_paths = list_sync_policies(pool)
        .await?
        .into_iter()
        .map(|policy| policy.path_prefix)
        .collect::<Vec<_>>();
    if let Ok(watch_dir) = std::env::var("OMNIDRIVE_WATCH_DIR") {
        base_paths.push(watch_dir);
    }
    record.path = projection_relative_path(&record.path, &base_paths);

    Ok(Some(record))
}

#[allow(dead_code)]
pub async fn list_unpinned_hydrated_files_for_eviction(
    pool: &SqlitePool,
) -> Result<Vec<SmartSyncEvictionRecord>, sqlx::Error> {
    let mut records = sqlx::query_as::<_, SmartSyncEvictionRecord>(
        r#"
        WITH RECURSIVE inode_paths AS (
            SELECT
                id,
                parent_id,
                name AS path
            FROM inodes
            WHERE parent_id IS NULL

            UNION ALL

            SELECT
                child.id,
                child.parent_id,
                inode_paths.path || '/' || child.name AS path
            FROM inodes child
            INNER JOIN inode_paths
                ON child.parent_id = inode_paths.id
        )
        SELECT
            s.inode_id AS inode_id,
            s.revision_id AS revision_id,
            inode_paths.path AS path
        FROM smart_sync_state s
        INNER JOIN inodes i
            ON i.id = s.inode_id
        INNER JOIN inode_paths
            ON inode_paths.id = i.id
        WHERE i.kind = 'FILE'
          AND s.pin_state = 0
          AND s.hydration_state = 1
        ORDER BY inode_paths.path ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut base_paths = list_sync_policies(pool)
        .await?
        .into_iter()
        .map(|policy| policy.path_prefix)
        .collect::<Vec<_>>();
    if let Ok(watch_dir) = std::env::var("OMNIDRIVE_WATCH_DIR") {
        base_paths.push(watch_dir);
    }

    for record in &mut records {
        record.path = projection_relative_path(&record.path, &base_paths);
    }

    Ok(records)
}

fn projection_relative_path(path: &str, base_paths: &[String]) -> String {
    let normalized = path.replace('\\', "/");
    let normalized = normalized.trim().trim_start_matches('/').to_string();
    if !normalized.contains(':') {
        return normalized;
    }

    let candidate = format!("/{}", normalized);
    let mut best_match_len = 0usize;
    let mut best_suffix = normalized.clone();

    for base in base_paths {
        let base_normalized = normalize_policy_path(base);
        if base_normalized.is_empty() {
            continue;
        }

        for prefix in [base_normalized.clone(), format!("/{}", base_normalized)] {
            if let Some(stripped) = candidate.strip_prefix(&prefix) {
                let stripped = stripped.trim_start_matches('/').trim_start_matches('\\');
                if !stripped.is_empty() && prefix.len() > best_match_len {
                    best_match_len = prefix.len();
                    best_suffix = stripped.replace('\\', "/");
                }
            }
        }
    }

    best_suffix
}

#[allow(dead_code)]
pub async fn get_inode_path(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Option<String>, sqlx::Error> {
    let mut names = Vec::new();
    let mut current = get_inode_by_id(pool, inode_id).await?;

    while let Some(inode) = current {
        names.push(inode.name);
        current = match inode.parent_id {
            Some(parent_id) => get_inode_by_id(pool, parent_id).await?,
            None => None,
        };
    }

    if names.is_empty() {
        return Ok(None);
    }

    names.reverse();
    Ok(Some(names.join("/")))
}

#[allow(dead_code)]
pub async fn pack_requires_healthy(pool: &SqlitePool, pack_id: &str) -> Result<bool, sqlx::Error> {
    let inode_ids = get_referencing_inode_ids_for_pack(pool, pack_id).await?;
    if inode_ids.is_empty() {
        return Ok(false);
    }

    let mut saw_policy = false;
    for inode_id in inode_ids {
        let Some(path) = get_inode_path(pool, inode_id).await? else {
            continue;
        };
        match find_sync_policy_for_path(pool, &path).await? {
            Some(policy) => {
                saw_policy = true;
                if policy.require_healthy != 0 {
                    return Ok(true);
                }
            }
            None => return Ok(true),
        }
    }

    Ok(!saw_policy)
}

#[allow(dead_code)]
pub async fn get_file_chunk_locations(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<Vec<FileChunkLocation>, sqlx::Error> {
    sqlx::query_as::<_, FileChunkLocation>(
        r#"
        WITH ordered_chunks AS (
            SELECT
                cr.chunk_id,
                cr.file_offset,
                cr.size,
                ROW_NUMBER() OVER (ORDER BY cr.file_offset ASC) - 1 AS chunk_index
            FROM chunk_refs cr
            INNER JOIN file_revisions fr
                ON fr.revision_id = cr.revision_id
            WHERE fr.inode_id = ?
              AND fr.is_current = 1
        )
        SELECT
            oc.chunk_id,
            oc.chunk_index,
            oc.file_offset,
            oc.size,
            pl.pack_id,
            pl.pack_offset,
            pl.encrypted_size
        FROM ordered_chunks oc
        INNER JOIN pack_locations pl
            ON pl.chunk_id = oc.chunk_id
        ORDER BY oc.file_offset ASC
        "#,
    )
    .bind(inode_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_revision_chunk_locations_in_range(
    pool: &SqlitePool,
    inode_id: i64,
    revision_id: i64,
    start_offset: i64,
    end_offset: i64,
) -> Result<Vec<FileChunkLocation>, sqlx::Error> {
    sqlx::query_as::<_, FileChunkLocation>(
        r#"
        WITH ordered_chunks AS (
            SELECT
                cr.chunk_id,
                cr.file_offset,
                cr.size,
                ROW_NUMBER() OVER (ORDER BY cr.file_offset ASC) - 1 AS chunk_index
            FROM chunk_refs cr
            INNER JOIN file_revisions fr
                ON fr.revision_id = cr.revision_id
            WHERE fr.inode_id = ?
              AND fr.revision_id = ?
        )
        SELECT
            oc.chunk_id,
            oc.chunk_index,
            oc.file_offset,
            oc.size,
            pl.pack_id,
            pl.pack_offset,
            pl.encrypted_size
        FROM ordered_chunks oc
        INNER JOIN pack_locations pl
            ON pl.chunk_id = oc.chunk_id
        WHERE (oc.file_offset + oc.size) > ?
          AND oc.file_offset < ?
        ORDER BY oc.file_offset ASC
        "#,
    )
    .bind(inode_id)
    .bind(revision_id)
    .bind(start_offset)
    .bind(end_offset)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn queue_pack_for_upload(pool: &SqlitePool, pack_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO upload_jobs (pack_id, status)
        VALUES (?, 'PENDING')
        ON CONFLICT(pack_id) DO UPDATE SET
            status = CASE
                WHEN upload_jobs.status = 'COMPLETED' THEN 'PENDING'
                ELSE upload_jobs.status
            END
        "#,
    )
    .bind(pack_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_next_upload_job(pool: &SqlitePool) -> Result<Option<UploadJob>, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let pending_job = sqlx::query_as::<_, UploadJob>(
        r#"
        SELECT id, pack_id, status, attempts
        FROM upload_jobs
        WHERE status = 'PENDING'
        ORDER BY id ASC
        LIMIT 1
        "#,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(mut job) = pending_job else {
        tx.commit().await?;
        return Ok(None);
    };

    sqlx::query(
        r#"
        UPDATE upload_jobs
        SET status = 'IN_PROGRESS'
        WHERE id = ?
        "#,
    )
    .bind(job.id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    job.status = "IN_PROGRESS".to_string();

    Ok(Some(job))
}

#[allow(dead_code)]
pub async fn mark_upload_job_completed(pool: &SqlitePool, job_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_jobs
        SET status = 'COMPLETED'
        WHERE id = ?
        "#,
    )
    .bind(job_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn get_upload_job_by_pack_id(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Option<UploadJob>, sqlx::Error> {
    sqlx::query_as::<_, UploadJob>(
        r#"
        SELECT id, pack_id, status, attempts
        FROM upload_jobs
        WHERE pack_id = ?
        LIMIT 1
        "#,
    )
    .bind(pack_id)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn ensure_upload_targets(
    pool: &SqlitePool,
    job_id: i64,
    providers: &[&str],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    for provider in providers {
        sqlx::query(
            r#"
            INSERT INTO upload_job_targets (job_id, provider, status)
            VALUES (?, ?, 'PENDING')
            ON CONFLICT(job_id, provider) DO NOTHING
            "#,
        )
        .bind(job_id)
        .bind(*provider)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn get_incomplete_upload_targets(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<Vec<UploadTargetRecord>, sqlx::Error> {
    sqlx::query_as::<_, UploadTargetRecord>(
        r#"
        SELECT
            id,
            job_id,
            provider,
            status,
            attempts,
            last_error,
            bucket,
            object_key,
            etag,
            version_id,
            last_attempt_at,
            updated_at,
            completed_at
        FROM upload_job_targets
        WHERE job_id = ?
          AND status != 'COMPLETED'
        ORDER BY id ASC
        "#,
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn mark_upload_target_in_progress(
    pool: &SqlitePool,
    job_id: i64,
    provider: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_job_targets
        SET status = 'IN_PROGRESS',
            last_attempt_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        WHERE job_id = ?
          AND provider = ?
        "#,
    )
    .bind(job_id)
    .bind(provider)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn mark_upload_target_completed(
    pool: &SqlitePool,
    job_id: i64,
    provider: &str,
    bucket: &str,
    object_key: &str,
    etag: Option<&str>,
    version_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_job_targets
        SET status = 'COMPLETED',
            last_error = NULL,
            bucket = ?,
            object_key = ?,
            etag = ?,
            version_id = ?,
            last_attempt_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            completed_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        WHERE job_id = ?
          AND provider = ?
        "#,
    )
    .bind(bucket)
    .bind(object_key)
    .bind(etag)
    .bind(version_id)
    .bind(job_id)
    .bind(provider)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn requeue_upload_target(
    pool: &SqlitePool,
    job_id: i64,
    provider: &str,
    error_message: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_job_targets
        SET status = 'PENDING',
            attempts = COALESCE(attempts, 0) + 1,
            last_error = ?,
            last_attempt_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            completed_at = NULL
        WHERE job_id = ?
          AND provider = ?
        "#,
    )
    .bind(error_message)
    .bind(job_id)
    .bind(provider)
    .execute(pool)
    .await?;

    let attempts = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(attempts, 0)
        FROM upload_job_targets
        WHERE job_id = ?
          AND provider = ?
        "#,
    )
    .bind(job_id)
    .bind(provider)
    .fetch_one(pool)
    .await?;

    Ok(attempts)
}

#[allow(dead_code)]
pub async fn mark_upload_target_failed(
    pool: &SqlitePool,
    job_id: i64,
    provider: &str,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_job_targets
        SET status = 'FAILED',
            attempts = COALESCE(attempts, 0) + 1,
            last_error = ?,
            last_attempt_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER),
            completed_at = NULL
        WHERE job_id = ?
          AND provider = ?
        "#,
    )
    .bind(error_message)
    .bind(job_id)
    .bind(provider)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn has_incomplete_upload_targets(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<bool, sqlx::Error> {
    let count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM upload_job_targets
        WHERE job_id = ?
          AND status != 'COMPLETED'
        "#,
    )
    .bind(job_id)
    .fetch_one(pool)
    .await?;

    Ok(count > 0)
}

#[allow(dead_code)]
pub async fn requeue_upload_job(pool: &SqlitePool, job_id: i64) -> Result<i64, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_jobs
        SET status = 'PENDING',
            attempts = COALESCE(attempts, 0) + 1
        WHERE id = ?
        "#,
    )
    .bind(job_id)
    .execute(pool)
    .await?;

    let attempts = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(attempts, 0)
        FROM upload_jobs
        WHERE id = ?
        "#,
    )
    .bind(job_id)
    .fetch_one(pool)
    .await?;

    Ok(attempts)
}

#[allow(dead_code)]
pub async fn mark_upload_job_failed(pool: &SqlitePool, job_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_jobs
        SET status = 'FAILED'
        WHERE id = ?
        "#,
    )
    .bind(job_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn reset_in_progress_upload_targets(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE upload_job_targets
        SET status = 'PENDING',
            updated_at = CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
        WHERE status = 'IN_PROGRESS'
        "#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

#[allow(dead_code)]
pub async fn get_upload_targets_for_job(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<Vec<UploadTargetRecord>, sqlx::Error> {
    sqlx::query_as::<_, UploadTargetRecord>(
        r#"
        SELECT
            id,
            job_id,
            provider,
            status,
            attempts,
            last_error,
            bucket,
            object_key,
            etag,
            version_id,
            last_attempt_at,
            updated_at,
            completed_at
        FROM upload_job_targets
        WHERE job_id = ?
        ORDER BY provider ASC
        "#,
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn list_recent_upload_jobs(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<UploadJob>, sqlx::Error> {
    sqlx::query_as::<_, UploadJob>(
        r#"
        SELECT id, pack_id, status, attempts
        FROM upload_jobs
        ORDER BY
            CASE WHEN status IN ('PENDING', 'IN_PROGRESS') THEN 0 ELSE 1 END,
            id DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_pending_upload_queue_size(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT COUNT(*) AS count
        FROM upload_jobs
        WHERE status = 'PENDING'
        "#,
    )
    .fetch_one(pool)
    .await?;

    row.try_get("count")
}

#[allow(dead_code)]
pub async fn get_latest_upload_error(pool: &SqlitePool) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT last_error
        FROM upload_job_targets
        WHERE last_error IS NOT NULL
          AND last_error != ''
        ORDER BY COALESCE(last_attempt_at, updated_at, completed_at, 0) DESC, id DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.and_then(|row| row.try_get("last_error").ok()))
}

#[allow(dead_code)]
pub async fn get_latest_upload_target_for_provider(
    pool: &SqlitePool,
    provider: &str,
) -> Result<Option<UploadTargetRecord>, sqlx::Error> {
    sqlx::query_as::<_, UploadTargetRecord>(
        r#"
        SELECT
            id,
            job_id,
            provider,
            status,
            attempts,
            last_error,
            bucket,
            object_key,
            etag,
            version_id,
            last_attempt_at,
            updated_at,
            completed_at
        FROM upload_job_targets
        WHERE provider = ?
        ORDER BY COALESCE(last_attempt_at, updated_at, completed_at, 0) DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(provider)
    .fetch_optional(pool)
    .await
}

#[allow(dead_code)]
pub async fn get_completed_pack_targets(
    pool: &SqlitePool,
    pack_id: &str,
) -> Result<Vec<PackDownloadTarget>, sqlx::Error> {
    sqlx::query_as::<_, PackDownloadTarget>(
        r#"
        SELECT
            ut.provider,
            ut.bucket,
            ut.object_key,
            ut.attempts,
            ut.last_error,
            ut.last_attempt_at,
            ut.updated_at,
            ut.completed_at
        FROM upload_jobs uj
        INNER JOIN upload_job_targets ut
            ON ut.job_id = uj.id
        WHERE uj.pack_id = ?
          AND ut.status = 'COMPLETED'
          AND ut.bucket IS NOT NULL
          AND ut.object_key IS NOT NULL
        ORDER BY COALESCE(ut.completed_at, ut.updated_at, ut.last_attempt_at, 0) DESC, ut.id ASC
        "#,
    )
    .bind(pack_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
pub async fn reset_in_progress_upload_jobs(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE upload_jobs
        SET status = 'PENDING'
        WHERE status = 'IN_PROGRESS'
        "#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

fn validate_inode_kind(kind: &str) -> Result<(), sqlx::Error> {
    match kind {
        "FILE" | "DIR" => Ok(()),
        _ => Err(sqlx::Error::InvalidArgument(format!(
            "invalid inode kind '{kind}', expected FILE or DIR"
        ))),
    }
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

fn normalize_policy_path(path: &str) -> String {
    let replaced = path.replace('\\', "/");
    let mut normalized = replaced.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        normalized.push('/');
    }
    normalized
}

fn path_matches_policy(path: &str, prefix: &str) -> bool {
    let path = normalize_policy_path(path);
    let prefix = normalize_policy_path(prefix);

    if prefix == "/" {
        return true;
    }

    if path == prefix {
        return true;
    }

    path.strip_prefix(&prefix)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

fn build_conflict_copy_name(original_name: &str, device_name: &str, timestamp_ms: i64) -> String {
    let (stem, extension) = split_file_name(original_name);
    format!(
        "{stem} (conflict - {} - {timestamp_ms}){extension}",
        sanitize_conflict_component(device_name)
    )
}

fn disambiguate_conflict_copy_name(base_name: &str, attempt: usize) -> String {
    let (stem, extension) = split_file_name(base_name);
    format!("{stem} [{attempt}]{extension}")
}

fn split_file_name(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, &name[stem.len()..]),
        _ => (name, ""),
    }
}

fn sanitize_conflict_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ if ch.is_control() => '_',
            _ => ch,
        })
        .collect()
}

async fn is_revision_ancestor(
    pool: &SqlitePool,
    ancestor_revision_id: i64,
    descendant_revision_id: i64,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar::<_, i64>(
        r#"
        WITH RECURSIVE lineage(revision_id, parent_revision_id) AS (
            SELECT revision_id, parent_revision_id
            FROM file_revisions
            WHERE revision_id = ?

            UNION ALL

            SELECT fr.revision_id, fr.parent_revision_id
            FROM file_revisions fr
            INNER JOIN lineage l
                ON fr.revision_id = l.parent_revision_id
        )
        SELECT 1
        FROM lineage
        WHERE revision_id = ?
        LIMIT 1
        "#,
    )
    .bind(descendant_revision_id)
    .bind(ancestor_revision_id)
    .fetch_optional(pool)
    .await?;

    Ok(found.is_some())
}

// ── V1→V2 Migration queries ──────────────────────────────────────────────

/// A V1 pack together with the inode that references it (for migration).
#[allow(dead_code)]
#[derive(Clone, Debug, FromRow)]
pub struct V1PackForMigration {
    pub pack_id: String,
    pub chunk_id: Vec<u8>,
    pub plaintext_hash: Option<String>,
    pub storage_mode: String,
    pub logical_size: i64,
    pub cipher_size: i64,
    pub shard_size: i64,
    pub nonce: Vec<u8>,
    pub gcm_tag: Vec<u8>,
    pub ec_scheme: String,
    pub inode_id: i64,
}

/// Fetch a batch of V1 packs that need migration, each joined with an owning inode.
#[allow(dead_code)]
pub async fn get_v1_packs_for_migration(
    pool: &SqlitePool,
    batch_size: i64,
) -> Result<Vec<V1PackForMigration>, sqlx::Error> {
    sqlx::query_as::<_, V1PackForMigration>(
        r#"
        SELECT
            p.pack_id,
            p.chunk_id,
            p.plaintext_hash,
            p.storage_mode,
            p.logical_size,
            p.cipher_size,
            p.shard_size,
            p.nonce,
            p.gcm_tag,
            p.ec_scheme,
            fr.inode_id
        FROM packs p
        INNER JOIN pack_locations pl ON pl.pack_id = p.pack_id
        INNER JOIN chunk_refs cr     ON cr.chunk_id = pl.chunk_id
        INNER JOIN file_revisions fr ON fr.revision_id = cr.revision_id
        WHERE p.encryption_version = 1
          AND p.status IN ('COMPLETED_HEALTHY', 'COMPLETED_DEGRADED')
        GROUP BY p.pack_id
        ORDER BY p.pack_id ASC
        LIMIT ?
        "#,
    )
    .bind(batch_size)
    .fetch_all(pool)
    .await
}

/// Count how many active V1 packs remain in the vault (healthy or degraded).
#[allow(dead_code)]
pub async fn count_v1_packs(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM packs \
         WHERE encryption_version = 1 \
         AND status IN ('COMPLETED_HEALTHY', 'COMPLETED_DEGRADED')",
    )
    .fetch_one(pool)
    .await
}

/// Update a pack's encryption_version to 2 after successful re-encryption.
/// Also stores the new nonce and gcm_tag produced by the V2 encryption.
#[allow(dead_code)]
pub async fn mark_pack_migrated_v2(
    pool: &SqlitePool,
    pack_id: &str,
    new_nonce: &[u8],
    new_gcm_tag: &[u8],
    new_cipher_size: i64,
    new_shard_size: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE packs
        SET encryption_version = 2,
            nonce = ?,
            gcm_tag = ?,
            cipher_size = ?,
            shard_size = ?
        WHERE pack_id = ?
        "#,
    )
    .bind(new_nonce)
    .bind(new_gcm_tag)
    .bind(new_cipher_size)
    .bind(new_shard_size)
    .bind(pack_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Set vault_format_version = 2 when all V1 packs have been migrated.
#[allow(dead_code)]
pub async fn finalize_vault_format_v2(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE vault_state SET vault_format_version = 2 WHERE id = 1",
    )
    .execute(pool)
    .await?;
    Ok(())
}

// ── Vault Key rotation queries ────────────────────────────────────────────

/// Fetch all wrapped DEKs (for re-wrapping during key rotation).
#[allow(dead_code)]
pub async fn get_all_wrapped_deks(pool: &SqlitePool) -> Result<Vec<WrappedDekRecord>, sqlx::Error> {
    sqlx::query_as::<_, WrappedDekRecord>(
        "SELECT dek_id, inode_id, wrapped_dek, key_version, vault_key_gen, created_at \
         FROM data_encryption_keys \
         ORDER BY dek_id ASC",
    )
    .fetch_all(pool)
    .await
}

/// Update a single DEK's wrapped blob and vault_key_gen after rotation.
#[allow(dead_code)]
pub async fn update_wrapped_dek(
    pool: &SqlitePool,
    dek_id: i64,
    new_wrapped_dek: &[u8],
    new_vault_key_gen: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE data_encryption_keys \
         SET wrapped_dek = ?, vault_key_gen = ? \
         WHERE dek_id = ?",
    )
    .bind(new_wrapped_dek)
    .bind(new_vault_key_gen)
    .bind(dek_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update vault_state with new salt, argon2_params, encrypted_vault_key and bumped generation.
#[allow(dead_code)]
pub async fn rotate_vault_state(
    pool: &SqlitePool,
    new_salt: &[u8],
    new_argon2_params: &str,
    new_encrypted_vault_key: &[u8],
    new_vault_key_generation: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE vault_state SET \
         master_key_salt = ?, \
         argon2_params = ?, \
         encrypted_vault_key = ?, \
         vault_key_generation = ? \
         WHERE id = 1",
    )
    .bind(new_salt)
    .bind(new_argon2_params)
    .bind(new_encrypted_vault_key)
    .bind(new_vault_key_generation)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update only encrypted_vault_key and generation (no salt/params change).
/// Used by VK rotation triggered by device revocation (passphrase unchanged).
pub async fn rotate_vault_key_only(
    pool: &SqlitePool,
    new_encrypted_vault_key: &[u8],
    new_vault_key_generation: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE vault_state SET \
         encrypted_vault_key = ?, \
         vault_key_generation = ? \
         WHERE id = 1",
    )
    .bind(new_encrypted_vault_key)
    .bind(new_vault_key_generation)
    .execute(pool)
    .await?;
    Ok(())
}

// ── DEK re-wrap queue (Epic 34.2b) ──────────────────────────────────

#[derive(Debug, Clone, FromRow)]
pub struct RewrapQueueItem {
    pub dek_id: i64,
    pub source_vk_generation: i64,
    pub target_vk_generation: i64,
    pub status: String,
    pub attempted_at: Option<i64>,
    pub error: Option<String>,
}

/// Enqueue all DEKs with vault_key_gen < target_generation for re-wrapping.
pub async fn enqueue_deks_for_rewrap(
    pool: &SqlitePool,
    target_vk_generation: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT OR IGNORE INTO dek_rewrap_queue (dek_id, source_vk_generation, target_vk_generation, status) \
         SELECT dek_id, vault_key_gen, ?, 'PENDING' \
         FROM data_encryption_keys \
         WHERE vault_key_gen < ?",
    )
    .bind(target_vk_generation)
    .bind(target_vk_generation)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Fetch a batch of PENDING re-wrap items (with their current wrapped DEK).
pub async fn get_pending_rewrap_batch(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<(RewrapQueueItem, Vec<u8>)>, sqlx::Error> {
    let items = sqlx::query_as::<_, RewrapQueueItem>(
        "SELECT dek_id, source_vk_generation, target_vk_generation, status, attempted_at, error \
         FROM dek_rewrap_queue WHERE status = 'PENDING' ORDER BY dek_id ASC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut result = Vec::with_capacity(items.len());
    for item in items {
        let dek = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT wrapped_dek FROM data_encryption_keys WHERE dek_id = ?",
        )
        .bind(item.dek_id)
        .fetch_one(pool)
        .await?;
        result.push((item, dek));
    }
    Ok(result)
}

/// Remove a successfully re-wrapped DEK from the queue.
pub async fn complete_rewrap_item(pool: &SqlitePool, dek_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM dek_rewrap_queue WHERE dek_id = ?")
        .bind(dek_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Mark a re-wrap item as FAILED with an error message.
pub async fn fail_rewrap_item(
    pool: &SqlitePool,
    dek_id: i64,
    error: &str,
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "UPDATE dek_rewrap_queue SET status = 'FAILED', attempted_at = ?, error = ? WHERE dek_id = ?",
    )
    .bind(now)
    .bind(error)
    .bind(dek_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get queue status: total, pending, failed counts.
pub async fn get_rewrap_status(pool: &SqlitePool) -> Result<(i64, i64, i64), sqlx::Error> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dek_rewrap_queue")
        .fetch_one(pool)
        .await?;
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM dek_rewrap_queue WHERE status = 'PENDING'",
    )
    .fetch_one(pool)
    .await?;
    let failed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM dek_rewrap_queue WHERE status = 'FAILED'",
    )
    .fetch_one(pool)
    .await?;
    Ok((total, pending, failed))
}

/// Get DEKs with a specific vault_key_gen (for lookup during dual-VK read).
pub async fn get_deks_by_generation(
    pool: &SqlitePool,
    vault_key_gen: i64,
) -> Result<Vec<WrappedDekRecord>, sqlx::Error> {
    sqlx::query_as::<_, WrappedDekRecord>(
        "SELECT dek_id, inode_id, wrapped_dek, key_version, vault_key_gen, created_at \
         FROM data_encryption_keys WHERE vault_key_gen = ? ORDER BY dek_id ASC",
    )
    .bind(vault_key_gen)
    .fetch_all(pool)
    .await
}

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
        && link.download_count >= max {
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

pub async fn list_shared_links(
    pool: &SqlitePool,
) -> Result<Vec<SharedLinkRecord>, sqlx::Error> {
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

pub async fn revoke_shared_link(
    pool: &SqlitePool,
    share_id: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE shared_links SET revoked = 1 WHERE share_id = ? AND revoked = 0",
    )
    .bind(share_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn increment_shared_link_download_count(
    pool: &SqlitePool,
    share_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE shared_links SET download_count = download_count + 1 WHERE share_id = ?",
    )
    .bind(share_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_shared_link(
    pool: &SqlitePool,
    share_id: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM shared_links WHERE share_id = ?")
        .bind(share_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Get chunk locations for a specific revision (for sharing).
pub async fn get_chunk_locations_for_revision(
    pool: &SqlitePool,
    revision_id: i64,
) -> Result<Vec<FileChunkLocation>, sqlx::Error> {
    sqlx::query_as::<_, FileChunkLocation>(
        r#"
        WITH ordered_chunks AS (
            SELECT
                cr.chunk_id,
                cr.file_offset,
                cr.size,
                ROW_NUMBER() OVER (ORDER BY cr.file_offset ASC) - 1 AS chunk_index
            FROM chunk_refs cr
            WHERE cr.revision_id = ?
        )
        SELECT
            oc.chunk_id,
            oc.chunk_index,
            oc.file_offset,
            oc.size,
            pl.pack_id,
            pl.pack_offset,
            pl.encrypted_size
        FROM ordered_chunks oc
        INNER JOIN pack_locations pl
            ON pl.chunk_id = oc.chunk_id
        ORDER BY oc.file_offset ASC
        "#,
    )
    .bind(revision_id)
    .fetch_all(pool)
    .await
}

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct ChunkRefRecord {
    pub id: i64,
    pub revision_id: i64,
    pub chunk_id: Vec<u8>,
    pub file_offset: i64,
    pub size: i64,
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

/// Get chunk refs for a specific revision, ordered by file_offset.
#[allow(dead_code)]
pub async fn get_chunk_refs_for_revision(
    pool: &SqlitePool,
    revision_id: i64,
) -> Result<Vec<ChunkRefRecord>, sqlx::Error> {
    sqlx::query_as::<_, ChunkRefRecord>(
        "SELECT id, revision_id, chunk_id, file_offset, size \
         FROM chunk_refs WHERE revision_id = ? ORDER BY file_offset ASC",
    )
    .bind(revision_id)
    .fetch_all(pool)
    .await
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

pub async fn get_device(pool: &SqlitePool, device_id: &str) -> Result<Option<DeviceRecord>, sqlx::Error> {
    sqlx::query_as::<_, DeviceRecord>(
        "SELECT device_id, user_id, device_name, public_key, wrapped_vault_key, \
         vault_key_generation, revoked_at, last_seen_at, created_at, enrolled_at \
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
    let row: Option<(Option<i64>,)> = sqlx::query_as(
        "SELECT safety_numbers_verified_at FROM devices WHERE device_id = ?",
    )
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
         vault_key_generation, revoked_at, last_seen_at, created_at, enrolled_at \
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

/// Returns active devices for a user: non-revoked and with a wrapped vault key.
pub async fn get_active_devices_for_user(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<Vec<DeviceRecord>, sqlx::Error> {
    sqlx::query_as::<_, DeviceRecord>(
        "SELECT device_id, user_id, device_name, public_key, wrapped_vault_key, \
         vault_key_generation, revoked_at, last_seen_at, created_at, enrolled_at \
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
        "UPDATE devices SET revoked_at = ?, wrapped_vault_key = NULL, vault_key_generation = NULL \
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
    let result = sqlx::query(
        "UPDATE devices SET public_key = ?, enrolled_at = ? WHERE device_id = ?",
    )
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
    let result = sqlx::query(
        "UPDATE vault_members SET role = ? WHERE user_id = ? AND vault_id = ?",
    )
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
    let result = sqlx::query(
        "DELETE FROM vault_members WHERE user_id = ? AND vault_id = ?",
    )
    .bind(user_id)
    .bind(vault_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

// ── Audit Logs ──

pub async fn insert_audit_log(
    pool: &SqlitePool,
    vault_id: &str,
    action: &str,
    actor_user_id: Option<&str>,
    actor_device_id: Option<&str>,
    target_user_id: Option<&str>,
    target_device_id: Option<&str>,
    details: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let now = epoch_secs();
    let result = sqlx::query(
        "INSERT INTO audit_logs (timestamp, actor_user_id, actor_device_id, action, \
         target_user_id, target_device_id, details, vault_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(now)
    .bind(actor_user_id)
    .bind(actor_device_id)
    .bind(action)
    .bind(target_user_id)
    .bind(target_device_id)
    .bind(details)
    .bind(vault_id)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn list_audit_logs(
    pool: &SqlitePool,
    vault_id: &str,
    limit: i64,
) -> Result<Vec<AuditLogRecord>, sqlx::Error> {
    sqlx::query_as::<_, AuditLogRecord>(
        "SELECT id, timestamp, actor_user_id, actor_device_id, action, \
         target_user_id, target_device_id, details, vault_id \
         FROM audit_logs WHERE vault_id = ? ORDER BY timestamp DESC LIMIT ?",
    )
    .bind(vault_id)
    .bind(limit)
    .fetch_all(pool)
    .await
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
        && epoch_secs() > exp {
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
    let user_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users")
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

    // Placeholder 32-byte zero public key — replaced in Epic 34.1a with real X25519 keypair
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
    let result = sqlx::query(
        "UPDATE user_sessions SET expires_at = ? WHERE token = ? AND expires_at > ?",
    )
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

// ── Stats (Epic 36 G-BE) ────────────────────────────────────────────

#[derive(Debug, Clone, FromRow)]
pub struct StatsOverview {
    pub files_count: i64,
    pub logical_size_bytes: i64,
}

pub async fn get_stats_overview(pool: &SqlitePool) -> Result<StatsOverview, sqlx::Error> {
    sqlx::query_as::<_, StatsOverview>(
        r#"
        SELECT
            COALESCE(COUNT(*), 0) AS files_count,
            COALESCE(SUM(size), 0) AS logical_size_bytes
        FROM inodes
        WHERE kind = 'file'
        "#,
    )
    .fetch_one(pool)
    .await
}

pub async fn count_active_devices(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM devices WHERE revoked_at IS NULL",
    )
    .fetch_one(pool)
    .await
}

/// Record upload or download bytes into a 2-hour bucket.
pub async fn record_traffic(
    pool: &SqlitePool,
    upload_bytes: i64,
    download_bytes: i64,
) -> Result<(), sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let bucket_epoch = now - (now % 7200); // 2-hour bucket

    sqlx::query(
        r#"
        INSERT INTO traffic_stats (bucket_epoch, upload_bytes, download_bytes)
        VALUES (?, ?, ?)
        ON CONFLICT(bucket_epoch) DO UPDATE SET
            upload_bytes = upload_bytes + excluded.upload_bytes,
            download_bytes = download_bytes + excluded.download_bytes
        "#,
    )
    .bind(bucket_epoch)
    .bind(upload_bytes)
    .bind(download_bytes)
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TrafficBucket {
    pub bucket_epoch: i64,
    pub upload_bytes: i64,
    pub download_bytes: i64,
}

/// Return traffic buckets for the last N hours (default 24).
pub async fn get_traffic_buckets(
    pool: &SqlitePool,
    hours: u32,
) -> Result<Vec<TrafficBucket>, sqlx::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let cutoff = now - (hours as i64 * 3600);
    let cutoff_bucket = cutoff - (cutoff % 7200);

    sqlx::query_as::<_, TrafficBucket>(
        r#"
        SELECT bucket_epoch, upload_bytes, download_bytes
        FROM traffic_stats
        WHERE bucket_epoch >= ?
        ORDER BY bucket_epoch ASC
        "#,
    )
    .bind(cutoff_bucket)
    .fetch_all(pool)
    .await
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
        increment_shared_link_download_count(&pool, "abc123").await.unwrap();
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
        create_share_password_token(&pool, "tok1", "share1", 10).await.unwrap();

        // Valid immediately
        assert!(validate_share_password_token(&pool, "tok1", "share1").await.unwrap());

        // Wrong share_id
        assert!(!validate_share_password_token(&pool, "tok1", "share2").await.unwrap());

        // Wrong token
        assert!(!validate_share_password_token(&pool, "tok_bad", "share1").await.unwrap());
    }

    // ── Epic 34: Multi-user CRUD tests ──────────────────────────────

    #[tokio::test]
    async fn user_crud_lifecycle() {
        let pool = init_db("sqlite::memory:").await.unwrap();

        // Create
        create_user(&pool, "u1", "Alice", Some("alice@example.com"), "local", None)
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
        assert!(update_user_display_name(&pool, "u1", "Alice Z").await.unwrap());
        let updated = get_user(&pool, "u1").await.unwrap().unwrap();
        assert_eq!(updated.display_name, "Alice Z");

        // Update non-existent
        assert!(!update_user_display_name(&pool, "u999", "Ghost").await.unwrap());

        // Delete
        assert!(delete_user(&pool, "u2").await.unwrap());
        assert!(get_user(&pool, "u2").await.unwrap().is_none());
        assert!(!delete_user(&pool, "u2").await.unwrap());
    }

    #[tokio::test]
    async fn device_crud_lifecycle() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_user(&pool, "u1", "Alice", None, "local", None).await.unwrap();

        let pubkey = vec![0u8; 32];

        // Create device
        create_device(&pool, "dev1", "u1", "Laptop", &pubkey).await.unwrap();

        // Read
        let dev = get_device(&pool, "dev1").await.unwrap().unwrap();
        assert_eq!(dev.device_name, "Laptop");
        assert_eq!(dev.user_id, "u1");
        assert_eq!(dev.public_key, pubkey);
        assert!(dev.wrapped_vault_key.is_none());
        assert!(dev.revoked_at.is_none());

        // List by user
        create_device(&pool, "dev2", "u1", "Phone", &pubkey).await.unwrap();
        let devs = list_devices_for_user(&pool, "u1").await.unwrap();
        assert_eq!(devs.len(), 2);

        // Set wrapped vault key
        let wvk = vec![1u8; 48];
        assert!(set_device_wrapped_vault_key(&pool, "dev1", &wvk, 1).await.unwrap());
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
        create_user(&pool, "u1", "Alice", None, "local", None).await.unwrap();
        create_user(&pool, "u2", "Bob", None, "local", None).await.unwrap();

        // Add members
        add_vault_member(&pool, "u1", "vault-1", "owner", None).await.unwrap();
        add_vault_member(&pool, "u2", "vault-1", "member", Some("u1")).await.unwrap();

        // Get
        let member = get_vault_member(&pool, "u2", "vault-1").await.unwrap().unwrap();
        assert_eq!(member.role, "member");
        assert_eq!(member.invited_by.as_deref(), Some("u1"));

        // List
        let members = list_vault_members(&pool, "vault-1").await.unwrap();
        assert_eq!(members.len(), 2);

        // Update role
        assert!(update_vault_member_role(&pool, "u2", "vault-1", "admin").await.unwrap());
        let updated = get_vault_member(&pool, "u2", "vault-1").await.unwrap().unwrap();
        assert_eq!(updated.role, "admin");

        // Remove
        assert!(remove_vault_member(&pool, "u2", "vault-1").await.unwrap());
        assert!(get_vault_member(&pool, "u2", "vault-1").await.unwrap().is_none());
        assert!(!remove_vault_member(&pool, "u2", "vault-1").await.unwrap());
    }

    #[tokio::test]
    async fn audit_log_lifecycle() {
        let pool = init_db("sqlite::memory:").await.unwrap();

        // Insert logs
        let id1 = insert_audit_log(
            &pool, "vault-1", "invite", Some("u1"), Some("dev1"),
            Some("u2"), None, Some(r#"{"role":"member"}"#),
        )
        .await
        .unwrap();
        assert!(id1 > 0);

        let id2 = insert_audit_log(
            &pool, "vault-1", "join", Some("u2"), Some("dev2"),
            None, None, None,
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
        create_user(&pool, "u1", "Alice", None, "local", None).await.unwrap();

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
        let migrated = migrate_single_to_multi_user(&pool, "vault-42").await.unwrap();
        assert!(migrated);

        // Verify owner user created with UUID v4
        let users = list_users(&pool).await.unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].user_id.len(), 36, "user_id must be UUID v4");
        assert!(!users[0].user_id.starts_with("owner-"), "user_id must not use legacy owner- prefix");
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
        assert!(migrate_single_to_multi_user(&pool, "vault-42").await.unwrap());

        // Second call is a no-op
        assert!(!migrate_single_to_multi_user(&pool, "vault-42").await.unwrap());

        // Still only one user
        let users = list_users(&pool).await.unwrap();
        assert_eq!(users.len(), 1);
    }

    #[tokio::test]
    async fn migrate_single_to_multi_user_noop_without_device() {
        let pool = init_db("sqlite::memory:").await.unwrap();

        // No device identity → migration is a no-op
        assert!(!migrate_single_to_multi_user(&pool, "vault-42").await.unwrap());
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
        create_user(&pool, "user-1", "Alice", None, "local", None).await.unwrap();

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
        let bogus = validate_user_session(&pool, "not-a-real-token").await.unwrap();
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
        create_user(&pool, "user-1", "Alice", None, "local", None).await.unwrap();

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
        create_user(&pool, "user-1", "Alice", None, "local", None).await.unwrap();

        let token = generate_session_token();
        let session = create_user_session(&pool, &token, "user-1", "dev-a", 3600)
            .await
            .unwrap();
        let old_expires = session.expires_at;

        // Renew with longer TTL
        assert!(renew_user_session(&pool, &token, SESSION_TTL_SECONDS).await.unwrap());

        let renewed = validate_user_session(&pool, &token).await.unwrap().unwrap();
        assert!(renewed.expires_at > old_expires);
    }

    #[tokio::test]
    async fn session_delete_all_for_user() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_user(&pool, "user-1", "Alice", None, "local", None).await.unwrap();

        // Create 3 sessions
        for i in 0..3 {
            let t = generate_session_token();
            create_user_session(&pool, &t, "user-1", &format!("dev-{i}"), SESSION_TTL_SECONDS)
                .await
                .unwrap();
        }

        let deleted = delete_user_sessions_for_user(&pool, "user-1").await.unwrap();
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
        insert_recovery_key(&pool, "vault-a", &vec![0xCDu8; 40], 1, None)
            .await
            .unwrap();
        assert_eq!(list_active_recovery_keys(&pool, "vault-a").await.unwrap().len(), 2);

        // Other vaults are isolated.
        insert_recovery_key(&pool, "vault-b", &blob, 1, None).await.unwrap();
        assert_eq!(list_active_recovery_keys(&pool, "vault-b").await.unwrap().len(), 1);

        // Revoke marks all keys for vault-a, leaves vault-b alone.
        let affected = revoke_all_recovery_keys(&pool, "vault-a").await.unwrap();
        assert_eq!(affected, 2);
        assert!(list_active_recovery_keys(&pool, "vault-a").await.unwrap().is_empty());
        assert_eq!(list_active_recovery_keys(&pool, "vault-b").await.unwrap().len(), 1);

        // Double revoke is a no-op.
        let again = revoke_all_recovery_keys(&pool, "vault-a").await.unwrap();
        assert_eq!(again, 0);
    }

    // ── Sesja C: OAuth state tests ─────────────────────────────────

    #[tokio::test]
    async fn oauth_state_create_and_retrieve() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_oauth_state(&pool, "state-abc", "verifier-xyz", 600).await.unwrap();
        let v = get_and_delete_oauth_state(&pool, "state-abc").await.unwrap();
        assert_eq!(v.as_deref(), Some("verifier-xyz"));
    }

    #[tokio::test]
    async fn oauth_state_is_single_use() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_oauth_state(&pool, "state-once", "verifier-once", 600).await.unwrap();
        assert!(get_and_delete_oauth_state(&pool, "state-once").await.unwrap().is_some());
        assert!(get_and_delete_oauth_state(&pool, "state-once").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn oauth_state_csrf_mismatch_returns_none() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_oauth_state(&pool, "real-state", "verifier-real", 600).await.unwrap();
        let v = get_and_delete_oauth_state(&pool, "attacker-state").await.unwrap();
        assert!(v.is_none());
    }

    #[tokio::test]
    async fn oauth_state_expired_returns_none() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_oauth_state(&pool, "expired-state", "verifier-exp", -1).await.unwrap();
        let v = get_and_delete_oauth_state(&pool, "expired-state").await.unwrap();
        assert!(v.is_none(), "expired state must return None");
    }

    #[tokio::test]
    async fn oauth_state_cleanup_removes_expired() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        create_oauth_state(&pool, "exp-1", "v1", -10).await.unwrap();
        create_oauth_state(&pool, "exp-2", "v2", -5).await.unwrap();
        create_oauth_state(&pool, "live-1", "v3", 600).await.unwrap();
        assert_eq!(delete_expired_oauth_states(&pool).await.unwrap(), 2);
        assert_eq!(get_and_delete_oauth_state(&pool, "live-1").await.unwrap().as_deref(), Some("v3"));
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
}
