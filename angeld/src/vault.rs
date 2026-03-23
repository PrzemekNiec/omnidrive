use crate::db;
use omnidrive_core::crypto::{KeyBytes, RootKdfParams, derive_root_keys};
use sqlx::SqlitePool;
use std::fmt;
use std::sync::Arc;
use tokio::sync::RwLock;

const DEFAULT_PARAMETER_SET_VERSION: u32 = 1;
const DEFAULT_MEMORY_COST_KIB: u32 = 65_536;
const DEFAULT_TIME_COST: u32 = 3;
const DEFAULT_LANES: u32 = 1;

#[derive(Clone, Default)]
pub struct VaultKeyStore {
    inner: Arc<RwLock<Option<UnlockedVaultKeys>>>,
}

#[derive(Clone, Copy)]
struct UnlockedVaultKeys {
    master_key: KeyBytes,
    vault_key: KeyBytes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnlockResult {
    pub initialized: bool,
    pub unlocked: bool,
}

#[derive(Debug)]
pub enum VaultError {
    Locked,
    EmptyPassphrase,
    Db(sqlx::Error),
    Crypto(omnidrive_core::crypto::CryptoError),
    InvalidConfig(&'static str),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locked => write!(f, "vault is locked"),
            Self::EmptyPassphrase => write!(f, "passphrase must not be empty"),
            Self::Db(err) => write!(f, "sqlite error: {err}"),
            Self::Crypto(err) => write!(f, "crypto error: {err}"),
            Self::InvalidConfig(reason) => write!(f, "invalid vault configuration: {reason}"),
        }
    }
}

impl std::error::Error for VaultError {}

impl From<sqlx::Error> for VaultError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value)
    }
}

impl From<omnidrive_core::crypto::CryptoError> for VaultError {
    fn from(value: omnidrive_core::crypto::CryptoError) -> Self {
        Self::Crypto(value)
    }
}

impl VaultKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn unlock(
        &self,
        pool: &SqlitePool,
        passphrase: &str,
    ) -> Result<UnlockResult, VaultError> {
        if passphrase.is_empty() {
            return Err(VaultError::EmptyPassphrase);
        }

        let (config, initialized) = ensure_vault_config(pool).await?;
        let root_keys = derive_root_keys(passphrase.as_bytes(), &config)?;
        *self.inner.write().await = Some(UnlockedVaultKeys {
            master_key: root_keys.master_key,
            vault_key: root_keys.vault_key,
        });

        Ok(UnlockResult {
            initialized,
            unlocked: true,
        })
    }

    pub async fn require_key(&self) -> Result<KeyBytes, VaultError> {
        match *self.inner.read().await {
            Some(keys) => Ok(keys.vault_key),
            None => Err(VaultError::Locked),
        }
    }

    pub async fn require_master_key(&self) -> Result<KeyBytes, VaultError> {
        match *self.inner.read().await {
            Some(keys) => Ok(keys.master_key),
            None => Err(VaultError::Locked),
        }
    }

    #[cfg(test)]
    pub async fn set_key_for_tests(&self, key: KeyBytes) {
        *self.inner.write().await = Some(UnlockedVaultKeys {
            master_key: key,
            vault_key: key,
        });
    }
}

async fn ensure_vault_config(pool: &SqlitePool) -> Result<(RootKdfParams, bool), VaultError> {
    if let Some(existing) = db::get_vault_config(pool).await? {
        return Ok((to_root_kdf_params(existing)?, false));
    }

    let salt = RootKdfParams::random_salt().to_vec();
    db::set_vault_config(
        pool,
        &salt,
        i64::from(DEFAULT_PARAMETER_SET_VERSION),
        i64::from(DEFAULT_MEMORY_COST_KIB),
        i64::from(DEFAULT_TIME_COST),
        i64::from(DEFAULT_LANES),
    )
    .await?;

    Ok((
        RootKdfParams::new(
            DEFAULT_PARAMETER_SET_VERSION,
            salt,
            DEFAULT_MEMORY_COST_KIB,
            DEFAULT_TIME_COST,
            DEFAULT_LANES,
        ),
        true,
    ))
}

fn to_root_kdf_params(record: db::VaultConfigRecord) -> Result<RootKdfParams, VaultError> {
    Ok(RootKdfParams::new(
        u32::try_from(record.parameter_set_version)
            .map_err(|_| VaultError::InvalidConfig("parameter_set_version"))?,
        record.salt,
        u32::try_from(record.memory_cost_kib)
            .map_err(|_| VaultError::InvalidConfig("memory_cost_kib"))?,
        u32::try_from(record.time_cost).map_err(|_| VaultError::InvalidConfig("time_cost"))?,
        u32::try_from(record.lanes).map_err(|_| VaultError::InvalidConfig("lanes"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[tokio::test]
    async fn unlock_reuses_same_config_and_derives_stable_key()
    -> Result<(), Box<dyn std::error::Error>> {
        let pool = db::init_db("sqlite::memory:").await?;
        let store_a = VaultKeyStore::new();
        let store_b = VaultKeyStore::new();

        let first = store_a.unlock(&pool, "1234").await?;
        let second = store_b.unlock(&pool, "1234").await?;

        assert!(first.initialized);
        assert!(!second.initialized);
        assert_eq!(store_a.require_key().await?, store_b.require_key().await?);

        Ok(())
    }
}
