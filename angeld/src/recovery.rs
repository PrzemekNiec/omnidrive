//! Epic 34.6a: Recovery Keys — 24-word BIP-39 mnemonic backup of the Vault Key.
//!
//! Flow:
//!   Generate: BIP-39 24 words (256-bit entropy) → PBKDF2-HMAC-SHA512 (2048 iter,
//!             salt = "mnemonic") → take first 32 bytes → `RecoveryKey` →
//!             AES-256-KW wraps the current `VaultKey` → persisted in
//!             `vault_recovery_keys`.
//!
//!   Restore:  Words → `RecoveryKey` → unwrap `VaultKey`. The caller then sets
//!             a new passphrase (new KEK derived from new Argon2 salt) and
//!             re-wraps the unchanged `VaultKey`; DEKs are NOT re-wrapped.
//!
//! AES-KW's 64-bit integrity check doubles as mnemonic verification, so no
//! separate hash is stored.

use bip39::Mnemonic;
use omnidrive_core::crypto::{self, KEY_LEN, KeyBytes, WRAPPED_KEY_LEN};

#[derive(Debug)]
pub enum RecoveryError {
    InvalidMnemonic(bip39::Error),
    WrongWordCount { expected: usize, actual: usize },
    Crypto(crypto::CryptoError),
}

impl std::fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMnemonic(err) => write!(f, "invalid BIP-39 mnemonic: {err}"),
            Self::WrongWordCount { expected, actual } => {
                write!(f, "expected {expected} words, got {actual}")
            }
            Self::Crypto(err) => write!(f, "crypto error: {err}"),
        }
    }
}

impl std::error::Error for RecoveryError {}

impl From<crypto::CryptoError> for RecoveryError {
    fn from(err: crypto::CryptoError) -> Self {
        Self::Crypto(err)
    }
}

/// 24 words = 256 bits of entropy, matches the Vault Key length.
pub const RECOVERY_WORD_COUNT: usize = 24;

/// Generate a fresh 24-word English BIP-39 mnemonic.
pub fn generate_mnemonic() -> Mnemonic {
    let mut entropy = [0u8; 32];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut entropy);
    Mnemonic::from_entropy(&entropy).expect("32-byte entropy is valid for BIP-39")
}

/// Parse a space-separated mnemonic phrase, enforcing the 24-word count and
/// BIP-39 checksum.
pub fn parse_mnemonic(phrase: &str) -> Result<Mnemonic, RecoveryError> {
    let word_count = phrase.split_whitespace().count();
    if word_count != RECOVERY_WORD_COUNT {
        return Err(RecoveryError::WrongWordCount {
            expected: RECOVERY_WORD_COUNT,
            actual: word_count,
        });
    }
    phrase.parse::<Mnemonic>().map_err(RecoveryError::InvalidMnemonic)
}

/// Derive a 32-byte recovery key from a mnemonic using BIP-39's standard
/// PBKDF2-HMAC-SHA512 (2048 iterations, salt = "mnemonic") and truncating the
/// 64-byte seed to the first 32 bytes.
pub fn derive_recovery_key(mnemonic: &Mnemonic) -> KeyBytes {
    let seed = mnemonic.to_seed("");
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&seed[..KEY_LEN]);
    key
}

/// Wrap a vault key with the recovery key using AES-256-KW.
pub fn wrap_vault_key(
    recovery_key: &KeyBytes,
    vault_key: &KeyBytes,
) -> Result<[u8; WRAPPED_KEY_LEN], RecoveryError> {
    Ok(crypto::wrap_key(recovery_key, vault_key)?)
}

/// Unwrap a previously wrapped vault key using the recovery key.
/// Returns `RecoveryError::Crypto` with an AES-KW integrity failure if the
/// mnemonic (and therefore recovery key) is wrong.
pub fn unwrap_vault_key(
    recovery_key: &KeyBytes,
    wrapped: &[u8; WRAPPED_KEY_LEN],
) -> Result<KeyBytes, RecoveryError> {
    Ok(crypto::unwrap_key(recovery_key, wrapped)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_mnemonic_has_24_words() {
        let m = generate_mnemonic();
        assert_eq!(m.to_string().split_whitespace().count(), RECOVERY_WORD_COUNT);
    }

    #[test]
    fn round_trip_wrap_unwrap() {
        let mnemonic = generate_mnemonic();
        let recovery_key = derive_recovery_key(&mnemonic);
        let vault_key = crypto::generate_random_key();

        let wrapped = wrap_vault_key(&recovery_key, &vault_key).unwrap();
        let unwrapped = unwrap_vault_key(&recovery_key, &wrapped).unwrap();

        assert_eq!(unwrapped, vault_key);
    }

    #[test]
    fn parse_accepts_generated_phrase() {
        let m = generate_mnemonic();
        let parsed = parse_mnemonic(&m.to_string()).unwrap();
        assert_eq!(parsed.to_string(), m.to_string());
    }

    #[test]
    fn parse_rejects_wrong_word_count() {
        let err = parse_mnemonic("just a few words here").unwrap_err();
        assert!(matches!(err, RecoveryError::WrongWordCount { .. }));
    }

    #[test]
    fn parse_rejects_bad_checksum() {
        // 24 valid wordlist words but arranged to fail the checksum.
        let bad = std::iter::repeat("abandon").take(24).collect::<Vec<_>>().join(" ");
        let err = parse_mnemonic(&bad).unwrap_err();
        assert!(matches!(err, RecoveryError::InvalidMnemonic(_)));
    }

    #[test]
    fn wrong_mnemonic_fails_to_unwrap() {
        let original = generate_mnemonic();
        let other = generate_mnemonic();
        assert_ne!(original.to_string(), other.to_string());

        let vault_key = crypto::generate_random_key();
        let wrapped = wrap_vault_key(&derive_recovery_key(&original), &vault_key).unwrap();

        let wrong = derive_recovery_key(&other);
        let err = unwrap_vault_key(&wrong, &wrapped).unwrap_err();
        assert!(matches!(err, RecoveryError::Crypto(_)));
    }

    #[test]
    fn deterministic_key_from_phrase() {
        let m = generate_mnemonic();
        let k1 = derive_recovery_key(&m);
        let k2 = derive_recovery_key(&parse_mnemonic(&m.to_string()).unwrap());
        assert_eq!(k1, k2);
    }

    /// Full generate → restore flow: original passphrase "forgotten", mnemonic
    /// resets the vault to a new passphrase while preserving the envelope VK
    /// (and therefore every DEK).
    #[tokio::test]
    async fn generate_and_restore_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        use crate::db;
        use crate::vault::VaultKeyStore;
        use omnidrive_core::crypto::{RootKdfParams, derive_root_keys, wrap_key};
        use secrecy::ExposeSecret;

        let pool = db::init_db("sqlite::memory:").await?;

        // 1. Unlock with original passphrase and create a DEK.
        let store = VaultKeyStore::new();
        store.unlock(&pool, "original-passphrase").await?;
        let envelope_before = store.require_envelope_key().await?;
        let (_, dek_before) = store.get_or_create_dek(&pool, 42).await?;
        let dek_bytes = *dek_before.expose_secret();

        // 2. Generate recovery key record (mimics POST /api/recovery/generate).
        let vault = db::get_vault_params(&pool).await?.unwrap();
        let mnemonic = generate_mnemonic();
        let recovery_key = derive_recovery_key(&mnemonic);
        let wrapped = wrap_vault_key(&recovery_key, &envelope_before)?;
        db::insert_recovery_key(
            &pool,
            &vault.vault_id,
            &wrapped,
            vault.vault_key_generation.unwrap_or(1),
            None,
        )
        .await?;

        // 3. Restore (mimics POST /api/recovery/restore).
        let active = db::list_active_recovery_keys(&pool, &vault.vault_id).await?;
        assert_eq!(active.len(), 1);
        let stored_wrapped: [u8; 40] = active[0].wrapped_vault_key.as_slice().try_into().unwrap();
        let recovered_envelope = unwrap_vault_key(&recovery_key, &stored_wrapped)?;
        assert_eq!(recovered_envelope, envelope_before);

        let cfg = db::get_vault_config(&pool).await?.unwrap();
        let new_salt = RootKdfParams::random_salt();
        let params = RootKdfParams::new(
            cfg.parameter_set_version as u32,
            new_salt.to_vec(),
            cfg.memory_cost_kib as u32,
            cfg.time_cost as u32,
            cfg.lanes as u32,
        );
        let new_root = derive_root_keys(b"new-passphrase", &params)?;
        let new_wrapped = wrap_key(&new_root.kek, &recovered_envelope)?;
        let argon2_json = format!(
            r#"{{"mode":"LOCAL_VAULT","parameter_set_version":{},"memory_cost_kib":{},"time_cost":{},"lanes":{}}}"#,
            params.parameter_set_version,
            params.memory_cost_kib,
            params.time_cost,
            params.lanes
        );
        db::rotate_vault_state(
            &pool,
            &new_salt,
            &argon2_json,
            &new_wrapped,
            vault.vault_key_generation.unwrap_or(1),
        )
        .await?;
        db::set_vault_config(
            &pool,
            &new_salt,
            cfg.parameter_set_version,
            cfg.memory_cost_kib,
            cfg.time_cost,
            cfg.lanes,
        )
        .await?;

        // 4. Original passphrase must no longer unlock.
        let store_old = VaultKeyStore::new();
        assert!(store_old.unlock(&pool, "original-passphrase").await.is_err());

        // 5. New passphrase unlocks, exposes the same envelope VK, and the
        //    DEK wrapped under the old passphrase still decrypts.
        let store_new = VaultKeyStore::new();
        store_new.unlock(&pool, "new-passphrase").await?;
        assert_eq!(store_new.require_envelope_key().await?, envelope_before);
        let (_, dek_after) = store_new.get_or_create_dek(&pool, 42).await?;
        assert_eq!(*dek_after.expose_secret(), dek_bytes);

        // 6. Wrong mnemonic cannot restore.
        let other_key = derive_recovery_key(&generate_mnemonic());
        assert!(unwrap_vault_key(&other_key, &stored_wrapped).is_err());

        // 7. Revoke removes the recovery key from the active set.
        let revoked = db::revoke_all_recovery_keys(&pool, &vault.vault_id).await?;
        assert_eq!(revoked, 1);
        assert!(db::list_active_recovery_keys(&pool, &vault.vault_id).await?.is_empty());

        Ok(())
    }
}
