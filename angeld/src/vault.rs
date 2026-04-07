use crate::db;
use hkdf::Hkdf;
use omnidrive_core::crypto::{
    CryptoError, KeyBytes, RootKdfParams, WRAPPED_KEY_LEN, derive_root_keys,
    generate_random_key, unwrap_key, wrap_key,
};
use secrecy::{ExposeSecret, SecretBox};
use sha2::Sha256;
use sqlx::SqlitePool;
use std::fmt;
use std::sync::Arc;
use rand::RngCore;
use tokio::sync::RwLock;
use tracing::info;

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

#[allow(dead_code)]
struct UnlockedVaultKeys {
    master_key: SecretBox<KeyBytes>,
    /// Deterministic V1 vault key (HKDF from master_key). Used for V1 chunk read/write.
    vault_key: SecretBox<KeyBytes>,
    /// Random V2 Vault Key (unwrapped via AES-KW from DB). `None` if vault is still V1-only.
    envelope_vault_key: Option<SecretBox<KeyBytes>>,
}

impl UnlockedVaultKeys {
    #[cfg(test)]
    fn new(master_key: KeyBytes, vault_key: KeyBytes) -> Self {
        Self {
            master_key: SecretBox::new(Box::new(master_key)),
            vault_key: SecretBox::new(Box::new(vault_key)),
            envelope_vault_key: None,
        }
    }

    fn with_envelope_key(master_key: KeyBytes, vault_key: KeyBytes, envelope_key: KeyBytes) -> Self {
        Self {
            master_key: SecretBox::new(Box::new(master_key)),
            vault_key: SecretBox::new(Box::new(vault_key)),
            envelope_vault_key: Some(SecretBox::new(Box::new(envelope_key))),
        }
    }

    fn master_key(&self) -> KeyBytes {
        *self.master_key.expose_secret()
    }

    fn vault_key(&self) -> KeyBytes {
        *self.vault_key.expose_secret()
    }

    fn envelope_vault_key(&self) -> Option<KeyBytes> {
        self.envelope_vault_key.as_ref().map(|k| *k.expose_secret())
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
        let _ = ensure_local_vault_params(pool, &config).await?;
        let root_keys = derive_root_keys(passphrase.as_bytes(), &config)?;

        // Try to unwrap the V2 envelope Vault Key if it exists in the DB.
        let vault_params = db::get_vault_params(pool).await?;
        let unlocked = match vault_params.as_ref().and_then(|v| v.encrypted_vault_key.as_ref()) {
            Some(wrapped_bytes) if wrapped_bytes.len() == WRAPPED_KEY_LEN => {
                let wrapped: [u8; WRAPPED_KEY_LEN] =
                    wrapped_bytes.as_slice().try_into().unwrap();
                let envelope_key = unwrap_key(&root_keys.kek, &wrapped)?;
                info!("[VAULT] V2 envelope Vault Key unwrapped successfully (generation {})",
                    vault_params.as_ref().and_then(|v| v.vault_key_generation).unwrap_or(0));
                UnlockedVaultKeys::with_envelope_key(
                    root_keys.master_key,
                    root_keys.vault_key,
                    envelope_key,
                )
            }
            _ => {
                // No V2 key yet — first unlock on a fresh/V1 vault.
                // Generate a random Vault Key, wrap it, and store it.
                if initialized {
                    let envelope_key = generate_random_key();
                    let wrapped = wrap_key(&root_keys.kek, &envelope_key)?;
                    db::store_encrypted_vault_key(pool, &wrapped, 1).await?;
                    info!("[VAULT] V2 envelope Vault Key generated and stored (generation 1)");
                    UnlockedVaultKeys::with_envelope_key(
                        root_keys.master_key,
                        root_keys.vault_key,
                        envelope_key,
                    )
                } else {
                    // Existing vault without V2 key — unlock in V1 mode,
                    // generate V2 key on this unlock so future writes use V2.
                    let envelope_key = generate_random_key();
                    let wrapped = wrap_key(&root_keys.kek, &envelope_key)?;
                    db::store_encrypted_vault_key(pool, &wrapped, 1).await?;
                    info!("[VAULT] V2 envelope Vault Key bootstrapped for existing V1 vault (generation 1)");
                    UnlockedVaultKeys::with_envelope_key(
                        root_keys.master_key,
                        root_keys.vault_key,
                        envelope_key,
                    )
                }
            }
        };

        *self.inner.write().await = Some(unlocked);

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

    /// Return the V2 envelope Vault Key (random, unwrapped from DB).
    /// Returns `None` if the vault was unlocked before V2 key was generated.
    #[allow(dead_code)]
    pub async fn require_envelope_key(&self) -> Result<KeyBytes, VaultError> {
        match self.inner.read().await.as_ref() {
            Some(keys) => keys.envelope_vault_key().ok_or(VaultError::Locked),
            None => Err(VaultError::Locked),
        }
    }

    #[allow(dead_code)]
    pub async fn derive_cache_key(&self) -> Result<SecretBox<KeyBytes>, VaultError> {
        let master_key = self.require_master_key().await?;
        let cache_key = derive_cache_key(&master_key)?;
        Ok(SecretBox::new(Box::new(cache_key)))
    }

    /// Get or create a DEK for the given inode.
    ///
    /// - If a wrapped DEK already exists in `data_encryption_keys`, unwrap it
    ///   with the V2 envelope Vault Key and return it.
    /// - If none exists, generate a random 256-bit DEK, wrap it with the
    ///   envelope Vault Key, persist the wrapped form, and return the plaintext.
    ///
    /// Returns `(dek_id, SecretBox<KeyBytes>)`.
    #[allow(dead_code)]
    pub async fn get_or_create_dek(
        &self,
        pool: &SqlitePool,
        inode_id: i64,
    ) -> Result<(i64, SecretBox<KeyBytes>), VaultError> {
        let envelope_key = self.require_envelope_key().await?;

        if let Some(record) = db::get_wrapped_dek(pool, inode_id).await? {
            let wrapped: [u8; WRAPPED_KEY_LEN] = record
                .wrapped_dek
                .as_slice()
                .try_into()
                .map_err(|_| VaultError::InvalidConfig("wrapped_dek has invalid length"))?;
            let dek = unwrap_key(&envelope_key, &wrapped)?;
            return Ok((record.dek_id, SecretBox::new(Box::new(dek))));
        }

        // Generate new DEK, wrap, persist
        let dek = generate_random_key();
        let wrapped = wrap_key(&envelope_key, &dek)?;
        let vault_key_gen = self.current_vault_key_generation(pool).await?;
        let dek_id =
            db::insert_wrapped_dek(pool, inode_id, &wrapped, 1, vault_key_gen).await?;
        info!("[DEK] created dek_id={dek_id} for inode_id={inode_id} (gen={vault_key_gen})");
        Ok((dek_id, SecretBox::new(Box::new(dek))))
    }

    /// Read the current vault_key_generation from DB (defaults to 1).
    async fn current_vault_key_generation(&self, pool: &SqlitePool) -> Result<i64, VaultError> {
        let vault = db::get_vault_params(pool).await?;
        Ok(vault
            .and_then(|v| v.vault_key_generation)
            .unwrap_or(1))
    }

    /// Rotate the vault key to a new passphrase.
    ///
    /// 1. Derive new root keys (Argon2 → new master_key → new KEK) with a fresh salt.
    /// 2. Generate a new random Vault Key.
    /// 3. Wrap new Vault Key with new KEK → store in vault_state, bump generation.
    /// 4. Re-wrap all existing DEKs: unwrap(old_vault_key) → wrap(new_vault_key).
    /// 5. Update vault_state salt + argon2_params for the new passphrase.
    /// 6. Update in-memory keys.
    #[allow(dead_code)]
    pub async fn rotate_vault_key(
        &self,
        pool: &SqlitePool,
        new_passphrase: &str,
    ) -> Result<RotationResult, VaultError> {
        if new_passphrase.is_empty() {
            return Err(VaultError::EmptyPassphrase);
        }

        // Read old keys from memory (vault must be unlocked)
        let old_envelope_key = self.require_envelope_key().await?;
        let old_generation = self.current_vault_key_generation(pool).await?;

        // ── Step 1: Derive new root keys with fresh salt ──
        let new_salt = RootKdfParams::random_salt();
        let existing_config = db::get_vault_config(pool).await?
            .ok_or(VaultError::InvalidConfig("no vault_config found"))?;
        let new_params = RootKdfParams::new(
            u32::try_from(existing_config.parameter_set_version)
                .map_err(|_| VaultError::InvalidConfig("parameter_set_version"))?,
            new_salt.to_vec(),
            u32::try_from(existing_config.memory_cost_kib)
                .map_err(|_| VaultError::InvalidConfig("memory_cost_kib"))?,
            u32::try_from(existing_config.time_cost)
                .map_err(|_| VaultError::InvalidConfig("time_cost"))?,
            u32::try_from(existing_config.lanes)
                .map_err(|_| VaultError::InvalidConfig("lanes"))?,
        );
        let new_root_keys = derive_root_keys(new_passphrase.as_bytes(), &new_params)?;

        // ── Step 2: Generate new random Vault Key ──
        let new_vault_key = generate_random_key();

        // ── Step 3: Wrap new Vault Key with new KEK, bump generation ──
        let new_generation = old_generation + 1;
        let wrapped_new_vault_key = wrap_key(&new_root_keys.kek, &new_vault_key)?;

        // ── Step 4: Re-wrap all DEKs in a transaction ──
        let all_deks = db::get_all_wrapped_deks(pool).await?;
        let deks_count = all_deks.len();

        let mut rewrapped: Vec<(i64, Vec<u8>)> = Vec::with_capacity(deks_count);
        for dek_record in &all_deks {
            let old_wrapped: [u8; WRAPPED_KEY_LEN] = dek_record
                .wrapped_dek
                .as_slice()
                .try_into()
                .map_err(|_| VaultError::InvalidConfig("wrapped_dek has invalid length"))?;
            let plaintext_dek = unwrap_key(&old_envelope_key, &old_wrapped)?;
            let new_wrapped = wrap_key(&new_vault_key, &plaintext_dek)?;
            rewrapped.push((dek_record.dek_id, new_wrapped.to_vec()));
        }

        // Write everything in one logical batch
        let argon2_params_json = format!(
            r#"{{"mode":"LOCAL_VAULT","parameter_set_version":{},"memory_cost_kib":{},"time_cost":{},"lanes":{}}}"#,
            new_params.parameter_set_version,
            new_params.memory_cost_kib,
            new_params.time_cost,
            new_params.lanes
        );

        // Update vault_state (salt, params, wrapped vault key, generation)
        db::rotate_vault_state(
            pool,
            &new_salt,
            &argon2_params_json,
            &wrapped_new_vault_key,
            new_generation,
        )
        .await?;

        // Update vault_config salt for future derivations
        db::set_vault_config(
            pool,
            &new_salt,
            i64::from(new_params.parameter_set_version),
            i64::from(new_params.memory_cost_kib),
            i64::from(new_params.time_cost),
            i64::from(new_params.lanes),
        )
        .await?;

        // Re-wrap each DEK
        for (dek_id, new_wrapped) in &rewrapped {
            db::update_wrapped_dek(pool, *dek_id, new_wrapped, new_generation).await?;
        }

        // ── Step 6: Update in-memory keys ──
        *self.inner.write().await = Some(UnlockedVaultKeys::with_envelope_key(
            new_root_keys.master_key,
            new_root_keys.vault_key,
            new_vault_key,
        ));

        info!(
            "[VAULT] Key rotation complete: generation {} → {}, re-wrapped {} DEKs",
            old_generation, new_generation, deks_count
        );

        Ok(RotationResult {
            new_generation,
            deks_rewrapped: deks_count as u64,
        })
    }

    #[cfg(test)]
    pub async fn set_key_for_tests(&self, key: KeyBytes) {
        *self.inner.write().await = Some(UnlockedVaultKeys::new(key, key));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RotationResult {
    pub new_generation: i64,
    pub deks_rewrapped: u64,
}

pub async fn bootstrap_local_vault(pool: &SqlitePool) -> Result<bool, VaultError> {
    let (config, initialized) = ensure_vault_config(pool).await?;
    let created = ensure_local_vault_params(pool, &config).await?;
    Ok(initialized || created)
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

async fn ensure_local_vault_params(
    pool: &SqlitePool,
    config: &RootKdfParams,
) -> Result<bool, VaultError> {
    if db::get_vault_params(pool).await?.is_some() {
        return Ok(false);
    }

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
    Ok(true)
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

    #[tokio::test]
    async fn envelope_key_generated_on_first_unlock_and_stable_on_relock()
    -> Result<(), Box<dyn std::error::Error>> {
        let pool = db::init_db("sqlite::memory:").await?;

        // First unlock — should generate and store V2 envelope key
        let store_a = VaultKeyStore::new();
        store_a.unlock(&pool, "secret").await?;
        let envelope_a = store_a.require_envelope_key().await?;

        // Second unlock (same passphrase, same DB) — should unwrap same key
        let store_b = VaultKeyStore::new();
        store_b.unlock(&pool, "secret").await?;
        let envelope_b = store_b.require_envelope_key().await?;

        assert_eq!(envelope_a, envelope_b, "envelope key must be stable across unlocks");

        // V1 key is deterministic and separate from envelope key
        let v1_key = store_a.require_key().await?;
        assert_ne!(v1_key, envelope_a, "V1 and V2 keys must differ");

        Ok(())
    }

    #[tokio::test]
    async fn wrong_passphrase_fails_to_unwrap_envelope_key()
    -> Result<(), Box<dyn std::error::Error>> {
        let pool = db::init_db("sqlite::memory:").await?;

        // Create vault with passphrase A
        let store = VaultKeyStore::new();
        store.unlock(&pool, "correct-pass").await?;
        let _ = store.require_envelope_key().await?;

        // Try to unlock with passphrase B — should fail at AES-KW unwrap
        let store2 = VaultKeyStore::new();
        let result = store2.unlock(&pool, "wrong-pass").await;
        assert!(result.is_err(), "wrong passphrase must fail AES-KW unwrap");

        Ok(())
    }

    // ── DEK tests ────────────────────────────────���──────────────────────

    #[tokio::test]
    async fn get_or_create_dek_generates_and_persists() -> Result<(), Box<dyn std::error::Error>> {
        let pool = db::init_db("sqlite::memory:").await?;
        let store = VaultKeyStore::new();
        store.unlock(&pool, "pass123").await?;

        let inode_id = 42;

        // First call — should generate a new DEK
        let (dek_id_a, dek_a) = store.get_or_create_dek(&pool, inode_id).await?;
        assert!(dek_id_a > 0);

        // Second call — should return the same DEK (from DB, not generate new)
        let (dek_id_b, dek_b) = store.get_or_create_dek(&pool, inode_id).await?;
        assert_eq!(dek_id_a, dek_id_b);
        assert_eq!(*dek_a.expose_secret(), *dek_b.expose_secret());

        Ok(())
    }

    #[tokio::test]
    async fn dek_survives_relock_cycle() -> Result<(), Box<dyn std::error::Error>> {
        let pool = db::init_db("sqlite::memory:").await?;
        let passphrase = "vault-pass";
        let inode_id = 99;

        // Unlock #1 — create DEK
        let store1 = VaultKeyStore::new();
        store1.unlock(&pool, passphrase).await?;
        let (_id1, dek1) = store1.get_or_create_dek(&pool, inode_id).await?;

        // Simulate relock by dropping store1 and creating a fresh one
        drop(store1);
        let store2 = VaultKeyStore::new();
        store2.unlock(&pool, passphrase).await?;
        let (_id2, dek2) = store2.get_or_create_dek(&pool, inode_id).await?;

        assert_eq!(
            *dek1.expose_secret(),
            *dek2.expose_secret(),
            "DEK must survive lock/unlock cycle"
        );

        Ok(())
    }

    #[tokio::test]
    async fn different_inodes_get_different_deks() -> Result<(), Box<dyn std::error::Error>> {
        let pool = db::init_db("sqlite::memory:").await?;
        let store = VaultKeyStore::new();
        store.unlock(&pool, "pass").await?;

        let (_, dek_a) = store.get_or_create_dek(&pool, 1).await?;
        let (_, dek_b) = store.get_or_create_dek(&pool, 2).await?;
        let (_, dek_c) = store.get_or_create_dek(&pool, 3).await?;

        assert_ne!(*dek_a.expose_secret(), *dek_b.expose_secret());
        assert_ne!(*dek_b.expose_secret(), *dek_c.expose_secret());
        assert_ne!(*dek_a.expose_secret(), *dek_c.expose_secret());

        Ok(())
    }

    #[tokio::test]
    async fn dek_unwrap_fails_with_wrong_passphrase() -> Result<(), Box<dyn std::error::Error>> {
        let pool = db::init_db("sqlite::memory:").await?;
        let inode_id = 7;

        // Create vault and DEK with passphrase A
        let store_a = VaultKeyStore::new();
        store_a.unlock(&pool, "correct").await?;
        let _ = store_a.get_or_create_dek(&pool, inode_id).await?;

        // Try to read DEK with passphrase B (different envelope key)
        // First we need a fresh vault that somehow has a different envelope key.
        // Since wrong passphrase fails at unlock, this test confirms the chain:
        // wrong pass → wrong KEK → wrong envelope key → can't even unlock.
        let store_b = VaultKeyStore::new();
        let result = store_b.unlock(&pool, "wrong");
        assert!(result.await.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn rotate_vault_key_rewraps_deks_and_new_passphrase_unlocks()
    -> Result<(), Box<dyn std::error::Error>> {
        let pool = db::init_db("sqlite::memory:").await?;

        // ── 1. Create vault, unlock, create DEKs for two inodes ──
        let store = VaultKeyStore::new();
        store.unlock(&pool, "old-passphrase").await?;

        let (_, dek_a_before) = store.get_or_create_dek(&pool, 10).await?;
        let (_, dek_b_before) = store.get_or_create_dek(&pool, 20).await?;
        let dek_a_bytes = *dek_a_before.expose_secret();
        let dek_b_bytes = *dek_b_before.expose_secret();

        // ── 2. Rotate to new passphrase ──
        let result = store.rotate_vault_key(&pool, "new-passphrase").await?;
        assert_eq!(result.new_generation, 2);
        assert_eq!(result.deks_rewrapped, 2);

        // ── 3. Old passphrase must fail ──
        let store_old = VaultKeyStore::new();
        assert!(store_old.unlock(&pool, "old-passphrase").await.is_err());

        // ── 4. New passphrase must unlock and recover same DEKs ──
        let store_new = VaultKeyStore::new();
        store_new.unlock(&pool, "new-passphrase").await?;

        let (_, dek_a_after) = store_new.get_or_create_dek(&pool, 10).await?;
        let (_, dek_b_after) = store_new.get_or_create_dek(&pool, 20).await?;

        assert_eq!(*dek_a_after.expose_secret(), dek_a_bytes, "DEK A must survive rotation");
        assert_eq!(*dek_b_after.expose_secret(), dek_b_bytes, "DEK B must survive rotation");

        // ── 5. Verify generation bumped in DB ──
        let vault = db::get_vault_params(&pool).await?.unwrap();
        assert_eq!(vault.vault_key_generation, Some(2));

        Ok(())
    }
}
