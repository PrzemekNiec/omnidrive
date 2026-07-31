#![allow(clippy::too_many_arguments, dead_code)]

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
#[cfg(test)]
mod test_support;
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
