use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use omnidrive_core::crypto::{KeyBytes, WRAPPED_KEY_LEN, unwrap_key, wrap_key};
use rand::RngCore;
use sha2::Sha256;
use sqlx::SqlitePool;

use crate::db;

const IDENTITY_KEK_INFO: &[u8] = b"omnidrive-identity-kek-v1";
const VAULT_KEY_WRAP_INFO: &[u8] = b"vault-key-wrap-v1";
const NONCE_LEN: usize = 12;

#[derive(Debug)]
pub enum IdentityError {
    Db(sqlx::Error),
    NoDeviceIdentity,
    Crypto(String),
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Db(e) => write!(f, "database error: {e}"),
            Self::NoDeviceIdentity => write!(f, "no local device identity found"),
            Self::Crypto(msg) => write!(f, "crypto error: {msg}"),
        }
    }
}

impl std::error::Error for IdentityError {}

impl From<sqlx::Error> for IdentityError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

/// Derives the Key Encryption Key (KEK) used to protect the X25519 private key at rest.
/// KEK = HKDF-SHA256(master_key, info="omnidrive-identity-kek-v1")
fn derive_identity_kek(master_key: &[u8]) -> Result<[u8; 32], IdentityError> {
    let hkdf = Hkdf::<Sha256>::from_prk(master_key)
        .map_err(|e| IdentityError::Crypto(format!("HKDF-PRK: {e}")))?;
    let mut kek = [0u8; 32];
    hkdf.expand(IDENTITY_KEK_INFO, &mut kek)
        .map_err(|e| IdentityError::Crypto(format!("HKDF-expand: {e}")))?;
    Ok(kek)
}

/// Encrypts a 32-byte X25519 private key with AES-256-GCM under the given KEK.
/// Returns `nonce || ciphertext+tag` (12 + 32 + 16 = 60 bytes).
fn encrypt_private_key(kek: &[u8; 32], private_key: &[u8; 32]) -> Result<Vec<u8>, IdentityError> {
    let cipher = Aes256Gcm::new_from_slice(kek)
        .map_err(|e| IdentityError::Crypto(format!("AES init: {e}")))?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, private_key.as_ref())
        .map_err(|e| IdentityError::Crypto(format!("AES-GCM encrypt: {e}")))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypts a private key blob previously produced by `encrypt_private_key`.
pub fn decrypt_private_key(kek: &[u8; 32], blob: &[u8]) -> Result<[u8; 32], IdentityError> {
    if blob.len() < NONCE_LEN + 32 {
        return Err(IdentityError::Crypto("encrypted private key blob too short".into()));
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(kek)
        .map_err(|e| IdentityError::Crypto(format!("AES init: {e}")))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| IdentityError::Crypto(format!("AES-GCM decrypt: {e}")))?;
    let mut key = [0u8; 32];
    if plaintext.len() != 32 {
        return Err(IdentityError::Crypto("decrypted key is not 32 bytes".into()));
    }
    key.copy_from_slice(&plaintext);
    Ok(key)
}

/// Ensures the local device has an X25519 keypair.
///
/// If the keypair already exists in `local_device_identity`, returns the public key.
/// Otherwise generates a new keypair, encrypts the private key with
/// `AES-256-GCM(KEK, private_key)` where KEK is derived from `master_key` via HKDF,
/// and persists both to `local_device_identity` and `devices.public_key`.
///
/// Returns the 32-byte X25519 public key.
pub async fn ensure_device_keypair(
    pool: &SqlitePool,
    master_key: &[u8],
) -> Result<[u8; 32], IdentityError> {
    let device = db::get_local_device_identity(pool)
        .await?
        .ok_or(IdentityError::NoDeviceIdentity)?;

    // Already has a keypair?
    if let (Some(_enc_priv), Some(pubkey)) =
        (&device.encrypted_private_key, &device.public_key)
    {
        if pubkey.len() == 32 {
            let mut pk = [0u8; 32];
            pk.copy_from_slice(pubkey);
            return Ok(pk);
        }
    }

    // Generate new X25519 keypair
    let mut secret_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut secret_bytes);
    let secret = x25519_dalek::StaticSecret::from(secret_bytes);
    let public = x25519_dalek::PublicKey::from(&secret);

    // Encrypt private key at rest
    let kek = derive_identity_kek(master_key)?;
    let encrypted_private = encrypt_private_key(&kek, &secret_bytes)?;

    let public_bytes = public.to_bytes();

    // Persist to local_device_identity
    db::store_device_keypair(pool, &encrypted_private, &public_bytes).await?;

    // Update devices table if this device exists there (post-migration)
    let _ = db::set_device_public_key(pool, &device.device_id, &public_bytes).await;

    Ok(public_bytes)
}

/// Retrieves and decrypts the local device's X25519 private key.
///
/// Requires the vault to be unlocked (needs `master_key` to derive KEK).
pub async fn get_device_private_key(
    pool: &SqlitePool,
    master_key: &[u8],
) -> Result<[u8; 32], IdentityError> {
    let device = db::get_local_device_identity(pool)
        .await?
        .ok_or(IdentityError::NoDeviceIdentity)?;

    let encrypted = device
        .encrypted_private_key
        .ok_or(IdentityError::Crypto("no encrypted private key stored".into()))?;

    let kek = derive_identity_kek(master_key)?;
    decrypt_private_key(&kek, &encrypted)
}

// ── ECDH key wrapping for vault key distribution ─────────────────────

/// Derives the AES-256 wrapping key from an X25519 shared secret via HKDF.
fn derive_wrapping_key(shared_secret: &[u8; 32]) -> Result<KeyBytes, IdentityError> {
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret);
    let mut wrapping_key: KeyBytes = [0u8; 32];
    hkdf.expand(VAULT_KEY_WRAP_INFO, &mut wrapping_key)
        .map_err(|e| IdentityError::Crypto(format!("HKDF-expand wrapping key: {e}")))?;
    Ok(wrapping_key)
}

/// Wraps a Vault Key for a target device using ECDH + AES-KW.
///
/// 1. `ECDH(my_private, their_public)` → shared_secret
/// 2. `HKDF(shared_secret, "vault-key-wrap-v1")` → wrapping_key
/// 3. `AES-256-KW(wrapping_key, vault_key)` → wrapped_vault_key (40 bytes)
pub fn wrap_vault_key_for_device(
    my_private_key: &[u8; 32],
    their_public_key: &[u8; 32],
    vault_key: &KeyBytes,
) -> Result<[u8; WRAPPED_KEY_LEN], IdentityError> {
    let secret = x25519_dalek::StaticSecret::from(*my_private_key);
    let their_pub = x25519_dalek::PublicKey::from(*their_public_key);
    let shared = secret.diffie_hellman(&their_pub);

    let wrapping_key = derive_wrapping_key(shared.as_bytes())?;
    wrap_key(&wrapping_key, vault_key).map_err(|e| IdentityError::Crypto(format!("AES-KW wrap: {e}")))
}

/// Unwraps a Vault Key received from another device using ECDH + AES-KW.
///
/// 1. `ECDH(my_private, their_public)` → shared_secret
/// 2. `HKDF(shared_secret, "vault-key-wrap-v1")` → wrapping_key
/// 3. `AES-KW-Unwrap(wrapping_key, wrapped_vault_key)` → vault_key
pub fn unwrap_vault_key_from_device(
    my_private_key: &[u8; 32],
    their_public_key: &[u8; 32],
    wrapped_vault_key: &[u8; WRAPPED_KEY_LEN],
) -> Result<KeyBytes, IdentityError> {
    let secret = x25519_dalek::StaticSecret::from(*my_private_key);
    let their_pub = x25519_dalek::PublicKey::from(*their_public_key);
    let shared = secret.diffie_hellman(&their_pub);

    let wrapping_key = derive_wrapping_key(shared.as_bytes())?;
    unwrap_key(&wrapping_key, wrapped_vault_key)
        .map_err(|e| IdentityError::Crypto(format!("AES-KW unwrap: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_private_key_roundtrip() {
        let kek = [0xAAu8; 32];
        let private_key = [0xBBu8; 32];

        let encrypted = encrypt_private_key(&kek, &private_key).unwrap();
        assert_eq!(encrypted.len(), NONCE_LEN + 32 + 16); // nonce + ciphertext + tag

        let decrypted = decrypt_private_key(&kek, &encrypted).unwrap();
        assert_eq!(decrypted, private_key);
    }

    #[test]
    fn decrypt_with_wrong_kek_fails() {
        let kek = [0xAAu8; 32];
        let wrong_kek = [0xCCu8; 32];
        let private_key = [0xBBu8; 32];

        let encrypted = encrypt_private_key(&kek, &private_key).unwrap();
        let result = decrypt_private_key(&wrong_kek, &encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn kek_derivation_is_deterministic() {
        let master = [0x42u8; 32];
        let kek1 = derive_identity_kek(&master).unwrap();
        let kek2 = derive_identity_kek(&master).unwrap();
        assert_eq!(kek1, kek2);
    }

    #[test]
    fn different_master_keys_produce_different_keks() {
        let kek1 = derive_identity_kek(&[0x01u8; 32]).unwrap();
        let kek2 = derive_identity_kek(&[0x02u8; 32]).unwrap();
        assert_ne!(kek1, kek2);
    }

    #[tokio::test]
    async fn ensure_device_keypair_generates_and_persists() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let master_key = [0x42u8; 32];

        // Create device identity first
        db::upsert_local_device_identity(&pool, "dev-test", "TestPC", "tok-123")
            .await
            .unwrap();

        // First call generates keypair
        let pubkey = ensure_device_keypair(&pool, &master_key).await.unwrap();
        assert_eq!(pubkey.len(), 32);
        assert_ne!(pubkey, [0u8; 32]); // should not be all zeros

        // Second call returns same pubkey (idempotent)
        let pubkey2 = ensure_device_keypair(&pool, &master_key).await.unwrap();
        assert_eq!(pubkey, pubkey2);

        // Verify persisted in DB
        let device = db::get_local_device_identity(&pool).await.unwrap().unwrap();
        assert!(device.encrypted_private_key.is_some());
        assert_eq!(device.public_key.as_deref(), Some(pubkey.as_slice()));
    }

    #[tokio::test]
    async fn private_key_roundtrip_via_db() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let master_key = [0x42u8; 32];

        db::upsert_local_device_identity(&pool, "dev-test", "TestPC", "tok-123")
            .await
            .unwrap();

        // Generate keypair
        let pubkey = ensure_device_keypair(&pool, &master_key).await.unwrap();

        // Retrieve private key
        let private_key = get_device_private_key(&pool, &master_key).await.unwrap();

        // Verify public key matches: PublicKey::from(StaticSecret::from(private_key))
        let secret = x25519_dalek::StaticSecret::from(private_key);
        let derived_public = x25519_dalek::PublicKey::from(&secret);
        assert_eq!(derived_public.to_bytes(), pubkey);
    }

    #[tokio::test]
    async fn wrong_master_key_fails_to_decrypt() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let master_key = [0x42u8; 32];
        let wrong_master = [0x99u8; 32];

        db::upsert_local_device_identity(&pool, "dev-test", "TestPC", "tok-123")
            .await
            .unwrap();

        ensure_device_keypair(&pool, &master_key).await.unwrap();

        let result = get_device_private_key(&pool, &wrong_master).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn ecdh_shared_secret_roundtrip() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let master_key = [0x42u8; 32];

        db::upsert_local_device_identity(&pool, "dev-test", "TestPC", "tok-123")
            .await
            .unwrap();

        // Device A keypair (from DB)
        let pubkey_a = ensure_device_keypair(&pool, &master_key).await.unwrap();
        let privkey_a = get_device_private_key(&pool, &master_key).await.unwrap();

        // Device B keypair (in-memory, simulates remote device)
        let mut secret_b_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut secret_b_bytes);
        let secret_b = x25519_dalek::StaticSecret::from(secret_b_bytes);
        let pubkey_b = x25519_dalek::PublicKey::from(&secret_b);

        // ECDH: A computes shared secret with B's public key
        let secret_a = x25519_dalek::StaticSecret::from(privkey_a);
        let shared_ab = secret_a.diffie_hellman(&pubkey_b);

        // ECDH: B computes shared secret with A's public key
        let pubkey_a_obj = x25519_dalek::PublicKey::from(pubkey_a);
        let shared_ba = secret_b.diffie_hellman(&pubkey_a_obj);

        // Both sides must derive the same shared secret
        assert_eq!(shared_ab.as_bytes(), shared_ba.as_bytes());
    }

    #[test]
    fn wrap_unwrap_vault_key_roundtrip() {
        // Owner keypair
        let mut owner_priv = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut owner_priv);
        let owner_secret = x25519_dalek::StaticSecret::from(owner_priv);
        let owner_pub = x25519_dalek::PublicKey::from(&owner_secret).to_bytes();

        // Member keypair
        let mut member_priv = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut member_priv);
        let member_secret = x25519_dalek::StaticSecret::from(member_priv);
        let member_pub = x25519_dalek::PublicKey::from(&member_secret).to_bytes();

        // Vault key to wrap
        let vault_key: KeyBytes = [0x42u8; 32];

        // Owner wraps VK for member
        let wrapped = wrap_vault_key_for_device(&owner_priv, &member_pub, &vault_key).unwrap();
        assert_eq!(wrapped.len(), WRAPPED_KEY_LEN);

        // Member unwraps VK using owner's public key
        let unwrapped = unwrap_vault_key_from_device(&member_priv, &owner_pub, &wrapped).unwrap();
        assert_eq!(unwrapped, vault_key);
    }

    #[test]
    fn unwrap_with_wrong_key_fails() {
        let mut owner_priv = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut owner_priv);
        let owner_secret = x25519_dalek::StaticSecret::from(owner_priv);
        let owner_pub = x25519_dalek::PublicKey::from(&owner_secret).to_bytes();

        let mut member_priv = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut member_priv);
        let member_secret = x25519_dalek::StaticSecret::from(member_priv);
        let member_pub = x25519_dalek::PublicKey::from(&member_secret).to_bytes();

        let vault_key: KeyBytes = [0x42u8; 32];
        let wrapped = wrap_vault_key_for_device(&owner_priv, &member_pub, &vault_key).unwrap();

        // Third party tries to unwrap — should fail
        let mut attacker_priv = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut attacker_priv);
        let result = unwrap_vault_key_from_device(&attacker_priv, &owner_pub, &wrapped);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn full_invite_accept_unwrap_roundtrip() {
        // Simulates the full flow: owner generates keypair, member generates keypair,
        // owner wraps VK for member, member unwraps VK.
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let master_key = [0x42u8; 32];

        // Setup owner device
        db::upsert_local_device_identity(&pool, "dev-owner", "OwnerPC", "tok-own")
            .await
            .unwrap();
        let owner_pubkey = ensure_device_keypair(&pool, &master_key).await.unwrap();
        let owner_privkey = get_device_private_key(&pool, &master_key).await.unwrap();

        // Member generates keypair independently (simulate remote device)
        let mut member_priv = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut member_priv);
        let member_secret = x25519_dalek::StaticSecret::from(member_priv);
        let member_pubkey = x25519_dalek::PublicKey::from(&member_secret).to_bytes();

        // The vault key that needs to be distributed
        let vault_key: KeyBytes = [0xAB; 32];

        // Owner wraps VK for member's public key
        let wrapped = wrap_vault_key_for_device(&owner_privkey, &member_pubkey, &vault_key).unwrap();

        // Member unwraps VK using owner's public key
        let recovered = unwrap_vault_key_from_device(&member_priv, &owner_pubkey, &wrapped).unwrap();
        assert_eq!(recovered, vault_key);
    }
}
