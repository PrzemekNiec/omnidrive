use crate::db;
use hkdf::Hkdf;
use omnidrive_core::crypto::{CryptoError, KeyBytes, RootKdfParams, derive_root_keys};
use secrecy::{ExposeSecret, SecretBox};
use sha2::Sha256;
use sqlx::SqlitePool;
use std::fmt;
use std::sync::Arc;
use rand::RngCore;
use tokio::sync::RwLock;

const DEFAULT_PARAMETER_SET_VERSION: u32 = 1;
const DEFAULT_MEMORY_COST_KIB: u32 = 65_536;
const DEFAULT_TIME_COST: u32 = 3;
const DEFAULT_LANES: u32 = 1;
#[allow(dead_code)]
const LOCAL_CACHE_KEY_INFO: &[u8] = b"omnidrive-local-cache-v1";

#[derive(Clone, Default)]
pub struct VaultKeyStore {
    inner: Arc<RwLock<Option<UnlockedVaultKeys>>>,
}

struct UnlockedVaultKeys {
    master_key: SecretBox<KeyBytes>,
    vault_key: SecretBox<KeyBytes>,
}

impl UnlockedVaultKeys {
    fn new(master_key: KeyBytes, vault_key: KeyBytes) -> Self {
        Self {
            master_key: SecretBox::new(Box::new(master_key)),
            vault_key: SecretBox::new(Box::new(vault_key)),
        }
    }

    fn master_key(&self) -> KeyBytes {
        *self.master_key.expose_secret()
    }

    fn vault_key(&self) -> KeyBytes {
        *self.vault_key.expose_secret()
    }
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
        *self.inner.write().await =
            Some(UnlockedVaultKeys::new(root_keys.master_key, root_keys.vault_key));

        Ok(UnlockResult {
            initialized,
            unlocked: true,
        })
    }

    pub async fn require_key(&self) -> Result<KeyBytes, VaultError> {
        match self.inner.read().await.as_ref() {
            Some(keys) => Ok(keys.vault_key()),
            None => Err(VaultError::Locked),
        }
    }

    pub async fn require_master_key(&self) -> Result<KeyBytes, VaultError> {
        match self.inner.read().await.as_ref() {
            Some(keys) => Ok(keys.master_key()),
            None => Err(VaultError::Locked),
        }
    }

    #[allow(dead_code)]
    pub async fn derive_cache_key(&self) -> Result<SecretBox<KeyBytes>, VaultError> {
        let master_key = self.require_master_key().await?;
        let cache_key = derive_cache_key(&master_key)?;
        Ok(SecretBox::new(Box::new(cache_key)))
    }

    #[cfg(test)]
    pub async fn set_key_for_tests(&self, key: KeyBytes) {
        *self.inner.write().await = Some(UnlockedVaultKeys::new(key, key));
    }
}

pub async fn bootstrap_local_vault(pool: &SqlitePool) -> Result<bool, VaultError> {
    let (config, initialized) = ensure_vault_config(pool).await?;
    if db::get_vault_params(pool).await?.is_none() {
        let mut salt = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut salt);
        let vault_id = format!("local-vault-{}", hex_prefix(&config.salt));
        let params_json = format!(
            r#"{{"mode":"LOCAL_VAULT","parameter_set_version":{},"memory_cost_kib":{},"time_cost":{},"lanes":{}}}"#,
            config.parameter_set_version,
            config.memory_cost_kib,
            config.time_cost,
            config.lanes
        );
        db::set_vault_params(pool, &salt, &params_json, &vault_id).await?;
        return Ok(true);
    }
    Ok(initialized)
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

#[allow(dead_code)]
pub fn derive_cache_key(master_key: &[u8]) -> Result<KeyBytes, VaultError> {
    let hkdf = Hkdf::<Sha256>::from_prk(master_key).map_err(CryptoError::HkdfPrk)?;
    let mut derived_key = [0u8; 32];
    hkdf.expand(LOCAL_CACHE_KEY_INFO, &mut derived_key)
        .map_err(CryptoError::HkdfExpand)?;
    Ok(derived_key)
}

fn hex_prefix(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use secrecy::ExposeSecret;

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

    #[tokio::test]
    async fn cache_key_derivation_is_stable_and_separate() -> Result<(), Box<dyn std::error::Error>>
    {
        let pool = db::init_db("sqlite::memory:").await?;
        let store = VaultKeyStore::new();
        store.unlock(&pool, "1234").await?;

        let master_key = store.require_master_key().await?;
        let vault_key = store.require_key().await?;
        let cache_key_a = store.derive_cache_key().await?;
        let cache_key_b = store.derive_cache_key().await?;

        assert_eq!(*cache_key_a.expose_secret(), *cache_key_b.expose_secret());
        assert_ne!(*cache_key_a.expose_secret(), master_key);
        assert_ne!(*cache_key_a.expose_secret(), vault_key);

        Ok(())
    }
}
