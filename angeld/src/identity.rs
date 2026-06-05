// reserved for Epic 30 (device identity, Vault Key derivation per-device)
#![allow(dead_code)]

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
        return Err(IdentityError::Crypto(
            "encrypted private key blob too short".into(),
        ));
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
        return Err(IdentityError::Crypto(
            "decrypted key is not 32 bytes".into(),
        ));
    }
    key.copy_from_slice(&plaintext);
    Ok(key)
}

/// Encrypts an arbitrary-length secret with AES-256-GCM under `kek`.
/// Returns `nonce(12) || ciphertext+tag`. Unlike `encrypt_private_key` this is
/// not locked to 32-byte inputs (used for the 2400-byte ML-KEM decapsulation key).
fn seal_secret_blob(kek: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, IdentityError> {
    let cipher = Aes256Gcm::new_from_slice(kek)
        .map_err(|e| IdentityError::Crypto(format!("AES init: {e}")))?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| IdentityError::Crypto(format!("AES-GCM encrypt: {e}")))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn open_secret_blob(kek: &[u8; 32], blob: &[u8]) -> Result<Vec<u8>, IdentityError> {
    if blob.len() < NONCE_LEN + 16 {
        return Err(IdentityError::Crypto("sealed blob too short".into()));
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(kek)
        .map_err(|e| IdentityError::Crypto(format!("AES init: {e}")))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| IdentityError::Crypto(format!("AES-GCM decrypt: {e}")))
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
    if let (Some(_enc_priv), Some(pubkey)) = (&device.encrypted_private_key, &device.public_key)
        && pubkey.len() == 32
    {
        let mut pk = [0u8; 32];
        pk.copy_from_slice(pubkey);
        return Ok(pk);
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

/// Ensures the local device has an ML-KEM-768 keypair, sealed at rest under the
/// identity KEK (same KEK as the X25519 private key). Idempotent: returns the
/// existing encapsulation key if one is already stored. Sibling to
/// `ensure_device_keypair` (deliberately not folded in, so existing X25519-only
/// devices get backfilled with a kyber keypair on the next unlock).
///
/// Returns the 1184-byte ML-KEM encapsulation (public) key.
pub async fn ensure_device_kyber_keypair(
    pool: &SqlitePool,
    master_key: &[u8],
) -> Result<Vec<u8>, IdentityError> {
    let device = db::get_local_device_identity(pool)
        .await?
        .ok_or(IdentityError::NoDeviceIdentity)?;

    if let (Some(_enc), Some(pubkey)) = (
        &device.encrypted_kyber_private_key,
        &device.kyber_public_key,
    ) && pubkey.len() == omnidrive_core::pqkem::ML_KEM_768_ENCAPS_KEY_LEN
    {
        return Ok(pubkey.clone());
    }

    let (encaps_key, decaps_key) = omnidrive_core::pqkem::generate_ml_kem_768_keypair();
    let kek = derive_identity_kek(master_key)?;
    let sealed = seal_secret_blob(&kek, &decaps_key)?;

    db::store_kyber_keypair(pool, &sealed, &encaps_key).await?;
    let _ = db::set_device_kyber_public_key(pool, &device.device_id, &encaps_key).await;

    Ok(encaps_key)
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

    let encrypted = device.encrypted_private_key.ok_or(IdentityError::Crypto(
        "no encrypted private key stored".into(),
    ))?;

    let kek = derive_identity_kek(master_key)?;
    decrypt_private_key(&kek, &encrypted)
}

pub fn reseal_local_device_private_key(
    old_master: &[u8],
    new_master: &[u8],
    current_blob: &[u8],
) -> Result<Vec<u8>, IdentityError> {
    let old_kek = derive_identity_kek(old_master)?;
    let private_key = decrypt_private_key(&old_kek, current_blob)?;
    let new_kek = derive_identity_kek(new_master)?;
    encrypt_private_key(&new_kek, &private_key)
}

// ── ECDH key wrapping for vault key distribution ─────────────────────

/// Rejects public keys that are known to produce a zero shared secret with X25519.
///
/// RFC 7748 §5: implementations SHOULD abort if the result of X25519 is the all-zero value.
/// [0u8;32] is the low-order "identity" point — X25519(scalar, [0;32]) = [0;32] for any scalar,
/// making HKDF(shared_secret, ...) deterministic and known to anyone reading the source.
fn validate_x25519_pubkey(pk: &[u8; 32]) -> Result<(), IdentityError> {
    if pk == &[0u8; 32] {
        return Err(IdentityError::Crypto(
            "low-order X25519 public key rejected (all-zero point)".into(),
        ));
    }
    Ok(())
}

/// Derives the AES-256 wrapping key from an X25519 shared secret via HKDF.
fn derive_wrapping_key(shared_secret: &[u8; 32]) -> Result<KeyBytes, IdentityError> {
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret);
    let mut wrapping_key = [0u8; 32];
    hkdf.expand(VAULT_KEY_WRAP_INFO, &mut wrapping_key)
        .map_err(|e| IdentityError::Crypto(format!("HKDF-expand wrapping key: {e}")))?;
    Ok(wrapping_key.into())
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
    validate_x25519_pubkey(their_public_key)?;
    let secret = x25519_dalek::StaticSecret::from(*my_private_key);
    let their_pub = x25519_dalek::PublicKey::from(*their_public_key);
    let shared = secret.diffie_hellman(&their_pub);
    if shared.as_bytes() == &[0u8; 32] {
        return Err(IdentityError::Crypto(
            "ECDH produced all-zero shared secret: low-order point attack rejected".into(),
        ));
    }
    let wrapping_key = derive_wrapping_key(shared.as_bytes())?;
    wrap_key(&wrapping_key, vault_key)
        .map_err(|e| IdentityError::Crypto(format!("AES-KW wrap: {e}")))
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
    validate_x25519_pubkey(their_public_key)?;
    let secret = x25519_dalek::StaticSecret::from(*my_private_key);
    let their_pub = x25519_dalek::PublicKey::from(*their_public_key);
    let shared = secret.diffie_hellman(&their_pub);
    if shared.as_bytes() == &[0u8; 32] {
        return Err(IdentityError::Crypto(
            "ECDH produced all-zero shared secret: low-order point attack rejected".into(),
        ));
    }
    let wrapping_key = derive_wrapping_key(shared.as_bytes())?;
    unwrap_key(&wrapping_key, wrapped_vault_key)
        .map_err(|e| IdentityError::Crypto(format!("AES-KW unwrap: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_secret_blob_roundtrip_variable_length() {
        let kek = [0x77u8; 32];
        let secret = vec![0x5Au8; 2400];
        let sealed = seal_secret_blob(&kek, &secret).unwrap();
        assert_eq!(sealed.len(), NONCE_LEN + 2400 + 16);
        assert_eq!(open_secret_blob(&kek, &sealed).unwrap(), secret);

        let wrong = [0x88u8; 32];
        assert!(open_secret_blob(&wrong, &sealed).is_err());
    }

    #[tokio::test]
    async fn ensure_device_kyber_keypair_generates_persists_and_is_idempotent() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let master_key = [0x42u8; 32];
        db::upsert_local_device_identity(&pool, "dev-kyber", "TestPC", "tok-1")
            .await
            .unwrap();

        let pub1 = ensure_device_kyber_keypair(&pool, &master_key)
            .await
            .unwrap();
        assert_eq!(pub1.len(), omnidrive_core::pqkem::ML_KEM_768_ENCAPS_KEY_LEN);
        assert_ne!(pub1, vec![0u8; pub1.len()]);

        let pub2 = ensure_device_kyber_keypair(&pool, &master_key)
            .await
            .unwrap();
        assert_eq!(pub1, pub2);

        let device = db::get_local_device_identity(&pool).await.unwrap().unwrap();
        let sealed = device.encrypted_kyber_private_key.unwrap();
        let kek = derive_identity_kek(&master_key).unwrap();
        let decaps = open_secret_blob(&kek, &sealed).unwrap();
        assert_eq!(
            decaps.len(),
            omnidrive_core::pqkem::ML_KEM_768_DECAPS_KEY_LEN
        );
    }

    #[test]
    fn reseal_private_key_rebinds_to_new_master() {
        let old_master = [0x11u8; 32];
        let new_master = [0x22u8; 32];
        let private_key = [0x33u8; 32];

        let old_kek = derive_identity_kek(&old_master).unwrap();
        let old_blob = encrypt_private_key(&old_kek, &private_key).unwrap();

        let new_blob =
            reseal_local_device_private_key(&old_master, &new_master, &old_blob).unwrap();

        let new_kek = derive_identity_kek(&new_master).unwrap();
        assert_eq!(
            decrypt_private_key(&new_kek, &new_blob).unwrap(),
            private_key
        );
        assert!(decrypt_private_key(&old_kek, &new_blob).is_err());
    }

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
        let vault_key: KeyBytes = [0x42u8; 32].into();

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

        let vault_key: KeyBytes = [0x42u8; 32].into();
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
        let vault_key: KeyBytes = [0xAB; 32].into();

        // Owner wraps VK for member's public key
        let wrapped =
            wrap_vault_key_for_device(&owner_privkey, &member_pubkey, &vault_key).unwrap();

        // Member unwraps VK using owner's public key
        let recovered =
            unwrap_vault_key_from_device(&member_priv, &owner_pubkey, &wrapped).unwrap();
        assert_eq!(recovered, vault_key);
    }

    #[tokio::test]
    async fn multi_device_key_distribution() {
        // 34.1c: Existing user adds a second device.
        // Owner wraps VK for device-1, then wraps VK again for device-2.
        // Both devices can independently unwrap the same VK.
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let master_key = [0x42u8; 32];

        // Setup owner device
        db::upsert_local_device_identity(&pool, "dev-owner", "OwnerPC", "tok-own")
            .await
            .unwrap();
        let owner_pubkey = ensure_device_keypair(&pool, &master_key).await.unwrap();
        let owner_privkey = get_device_private_key(&pool, &master_key).await.unwrap();

        let vault_key: KeyBytes = [0xAB; 32].into();

        // Device 1 for member
        let mut dev1_priv = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut dev1_priv);
        let dev1_secret = x25519_dalek::StaticSecret::from(dev1_priv);
        let dev1_pubkey = x25519_dalek::PublicKey::from(&dev1_secret).to_bytes();

        // Device 2 for same member (new machine)
        let mut dev2_priv = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut dev2_priv);
        let dev2_secret = x25519_dalek::StaticSecret::from(dev2_priv);
        let dev2_pubkey = x25519_dalek::PublicKey::from(&dev2_secret).to_bytes();

        // Owner wraps VK for both devices
        let wrapped_1 =
            wrap_vault_key_for_device(&owner_privkey, &dev1_pubkey, &vault_key).unwrap();
        let wrapped_2 =
            wrap_vault_key_for_device(&owner_privkey, &dev2_pubkey, &vault_key).unwrap();

        // Wrapped blobs differ (different ECDH shared secrets)
        assert_ne!(wrapped_1, wrapped_2);

        // Both devices can unwrap to the same VK
        let vk_1 = unwrap_vault_key_from_device(&dev1_priv, &owner_pubkey, &wrapped_1).unwrap();
        let vk_2 = unwrap_vault_key_from_device(&dev2_priv, &owner_pubkey, &wrapped_2).unwrap();
        assert_eq!(vk_1, vault_key);
        assert_eq!(vk_2, vault_key);

        // Cross-device unwrap must fail (device 2 can't unwrap device 1's blob)
        let cross_unwrap = unwrap_vault_key_from_device(&dev2_priv, &owner_pubkey, &wrapped_1);
        assert!(cross_unwrap.is_err());
    }

    #[tokio::test]
    async fn multi_device_db_registration_and_active_lookup() {
        // 34.1c: Verify DB-level registration and active device lookup.
        let pool = db::init_db("sqlite::memory:").await.unwrap();

        let user_id = "user-alice";
        db::create_user(&pool, user_id, "Alice", None, "local", None)
            .await
            .unwrap();

        // Device 1: created but no wrapped VK yet → not active
        db::create_device(&pool, "dev-1", user_id, "Laptop", &[1u8; 32])
            .await
            .unwrap();
        let active = db::get_active_devices_for_user(&pool, user_id)
            .await
            .unwrap();
        assert!(
            active.is_empty(),
            "device without wrapped VK should not be active"
        );

        // Accept device 1 → becomes active
        db::set_device_wrapped_vault_key(&pool, "dev-1", &[0xAA; 40], 1)
            .await
            .unwrap();
        let active = db::get_active_devices_for_user(&pool, user_id)
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].device_id, "dev-1");

        // Device 2: add and accept → 2 active devices
        db::create_device(&pool, "dev-2", user_id, "Desktop", &[2u8; 32])
            .await
            .unwrap();
        db::set_device_wrapped_vault_key(&pool, "dev-2", &[0xBB; 40], 1)
            .await
            .unwrap();
        let active = db::get_active_devices_for_user(&pool, user_id)
            .await
            .unwrap();
        assert_eq!(active.len(), 2);

        // Revoke device 1 → only device 2 active
        db::revoke_device(&pool, "dev-1").await.unwrap();
        let active = db::get_active_devices_for_user(&pool, user_id)
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].device_id, "dev-2");
    }

    #[tokio::test]
    async fn revoke_device_clears_wrapped_vault_key() {
        // 34.2a: Revoking a device must NULL out wrapped_vault_key so the
        // device immediately loses the ability to unwrap VK.
        let pool = db::init_db("sqlite::memory:").await.unwrap();

        let user_id = "user-bob";
        db::create_user(&pool, user_id, "Bob", None, "local", None)
            .await
            .unwrap();
        db::create_device(&pool, "dev-bob-1", user_id, "WorkPC", &[1u8; 32])
            .await
            .unwrap();
        db::set_device_wrapped_vault_key(&pool, "dev-bob-1", &[0xCC; 40], 1)
            .await
            .unwrap();

        // Confirm device has wrapped VK
        let dev = db::get_device(&pool, "dev-bob-1").await.unwrap().unwrap();
        assert!(dev.wrapped_vault_key.is_some());
        assert!(dev.vault_key_generation.is_some());
        assert!(dev.revoked_at.is_none());

        // Revoke
        let revoked = db::revoke_device(&pool, "dev-bob-1").await.unwrap();
        assert!(revoked);

        // Verify: wrapped_vault_key cleared, revoked_at set
        let dev = db::get_device(&pool, "dev-bob-1").await.unwrap().unwrap();
        assert!(
            dev.wrapped_vault_key.is_none(),
            "wrapped VK must be cleared on revoke"
        );
        assert!(
            dev.vault_key_generation.is_none(),
            "VK generation must be cleared on revoke"
        );
        assert!(dev.revoked_at.is_some(), "revoked_at must be set");

        // Double-revoke is a no-op
        let revoked_again = db::revoke_device(&pool, "dev-bob-1").await.unwrap();
        assert!(!revoked_again, "second revoke should be a no-op");
    }

    #[tokio::test]
    async fn revoked_device_cannot_unwrap_vault_key() {
        // 34.2a: Even if attacker has the old wrapped blob, a revoked device's
        // ECDH relationship is broken because the owner would rotate VK (34.2b).
        // Here we verify that the wrapped VK is no longer in the DB after revoke.
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let master_key = [0x42u8; 32];

        // Setup owner
        db::upsert_local_device_identity(&pool, "dev-owner", "OwnerPC", "tok-own")
            .await
            .unwrap();
        let owner_pubkey = ensure_device_keypair(&pool, &master_key).await.unwrap();
        let owner_privkey = get_device_private_key(&pool, &master_key).await.unwrap();

        // Setup member device
        let mut member_priv = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut member_priv);
        let member_secret = x25519_dalek::StaticSecret::from(member_priv);
        let member_pubkey = x25519_dalek::PublicKey::from(&member_secret).to_bytes();

        let vault_key: KeyBytes = [0xAB; 32].into();
        let wrapped =
            wrap_vault_key_for_device(&owner_privkey, &member_pubkey, &vault_key).unwrap();

        // Register member in DB
        let user_id = "user-member";
        db::create_user(&pool, user_id, "Member", None, "local", None)
            .await
            .unwrap();
        db::create_device(&pool, "dev-member", user_id, "MemberPC", &member_pubkey)
            .await
            .unwrap();
        db::set_device_wrapped_vault_key(&pool, "dev-member", &wrapped, 1)
            .await
            .unwrap();

        // Before revoke: member can retrieve and unwrap
        let dev = db::get_device(&pool, "dev-member").await.unwrap().unwrap();
        let stored_wrapped: [u8; 40] = dev.wrapped_vault_key.unwrap().try_into().unwrap();
        let recovered =
            unwrap_vault_key_from_device(&member_priv, &owner_pubkey, &stored_wrapped).unwrap();
        assert_eq!(recovered, vault_key);

        // Revoke device
        db::revoke_device(&pool, "dev-member").await.unwrap();

        // After revoke: wrapped_vault_key is gone from DB
        let dev = db::get_device(&pool, "dev-member").await.unwrap().unwrap();
        assert!(
            dev.wrapped_vault_key.is_none(),
            "revoked device must not have wrapped VK in DB"
        );
        assert!(dev.revoked_at.is_some());
    }

    #[tokio::test]
    async fn user_removal_revokes_all_devices_and_deletes_membership() {
        // 34.2c: Removing a user revokes all their devices and deletes vault membership.
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let vault_id = "test-vault";

        // Create user with 2 active devices
        let user_id = "user-carol";
        db::create_user(&pool, user_id, "Carol", None, "local", None)
            .await
            .unwrap();
        db::create_device(&pool, "dev-carol-1", user_id, "Laptop", &[1u8; 32])
            .await
            .unwrap();
        db::set_device_wrapped_vault_key(&pool, "dev-carol-1", &[0xAA; 40], 1)
            .await
            .unwrap();
        db::create_device(&pool, "dev-carol-2", user_id, "Phone", &[2u8; 32])
            .await
            .unwrap();
        db::set_device_wrapped_vault_key(&pool, "dev-carol-2", &[0xBB; 40], 1)
            .await
            .unwrap();

        // Add as vault member
        // First create the vault_state stub for the vault_id reference
        db::add_vault_member(&pool, user_id, vault_id, "member", None)
            .await
            .unwrap();

        // Verify: 2 active devices, 1 membership
        let active = db::get_active_devices_for_user(&pool, user_id)
            .await
            .unwrap();
        assert_eq!(active.len(), 2);
        assert!(
            db::get_vault_member(&pool, user_id, vault_id)
                .await
                .unwrap()
                .is_some()
        );

        // Simulate user removal: revoke all devices + delete membership
        let devices = db::list_devices_for_user(&pool, user_id).await.unwrap();
        for dev in &devices {
            if dev.revoked_at.is_none() {
                db::revoke_device(&pool, &dev.device_id).await.unwrap();
            }
        }
        db::remove_vault_member(&pool, user_id, vault_id)
            .await
            .unwrap();

        // Verify: 0 active devices, no membership, devices are revoked
        let active = db::get_active_devices_for_user(&pool, user_id)
            .await
            .unwrap();
        assert!(active.is_empty(), "all devices must be revoked");

        assert!(
            db::get_vault_member(&pool, user_id, vault_id)
                .await
                .unwrap()
                .is_none(),
            "membership must be deleted"
        );

        let dev1 = db::get_device(&pool, "dev-carol-1").await.unwrap().unwrap();
        assert!(dev1.revoked_at.is_some());
        assert!(dev1.wrapped_vault_key.is_none());

        let dev2 = db::get_device(&pool, "dev-carol-2").await.unwrap().unwrap();
        assert!(dev2.revoked_at.is_some());
        assert!(dev2.wrapped_vault_key.is_none());
    }
}
