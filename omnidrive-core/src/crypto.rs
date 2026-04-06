use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce, Tag};
use aes_kw::Kek;
use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::{Hkdf, InvalidPrkLength};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use std::fmt;

type HmacSha256 = Hmac<Sha256>;

pub const KEY_LEN: usize = 32;
pub const CHUNK_NONCE_LEN: usize = 12;
pub const GCM_TAG_LEN: usize = 16;
pub const DEFAULT_SALT_LEN: usize = 16;
pub const CHUNK_ENC_INFO: &[u8] = b"chunk-enc-v1";
pub const CHUNK_NONCE_PREFIX: &[u8] = b"nonce";
pub const VAULT_KEY_INFO: &[u8] = b"vault-key-v1";
pub const KEK_V2_INFO: &[u8] = b"kek-v2";
pub const MANIFEST_MAC_KEY_INFO: &[u8] = b"manifest-mac-key-v1";
pub const LEASE_MAC_KEY_INFO: &[u8] = b"lease-mac-key-v1";
pub const LOCAL_ANCHOR_KEY_INFO: &[u8] = b"local-anchor-key-v1";

/// AES-KW wrapped key length: 32-byte key + 8-byte integrity check = 40 bytes.
pub const WRAPPED_KEY_LEN: usize = KEY_LEN + 8;

pub type KeyBytes = [u8; KEY_LEN];
pub type ChunkId = [u8; KEY_LEN];
pub type ChunkNonce = [u8; CHUNK_NONCE_LEN];
pub type GcmTag = [u8; GCM_TAG_LEN];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootKdfParams {
    pub parameter_set_version: u32,
    pub salt: Vec<u8>,
    pub memory_cost_kib: u32,
    pub time_cost: u32,
    pub lanes: u32,
}

impl RootKdfParams {
    pub fn new(
        parameter_set_version: u32,
        salt: Vec<u8>,
        memory_cost_kib: u32,
        time_cost: u32,
        lanes: u32,
    ) -> Self {
        Self {
            parameter_set_version,
            salt,
            memory_cost_kib,
            time_cost,
            lanes,
        }
    }

    pub fn random_salt() -> [u8; DEFAULT_SALT_LEN] {
        let mut salt = [0u8; DEFAULT_SALT_LEN];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        salt
    }

    fn to_argon2_params(&self) -> Result<Params, CryptoError> {
        Params::new(
            self.memory_cost_kib,
            self.time_cost,
            self.lanes,
            Some(KEY_LEN),
        )
        .map_err(CryptoError::Argon2)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootKeys {
    pub master_key: KeyBytes,
    pub vault_key: KeyBytes,
    /// Key Encryption Key for Envelope Encryption V2 (derived via HKDF "kek-v2").
    pub kek: KeyBytes,
    pub manifest_mac_key: KeyBytes,
    pub lease_mac_key: KeyBytes,
    pub local_anchor_key: KeyBytes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncryptedChunk {
    pub chunk_id: ChunkId,
    pub nonce: ChunkNonce,
    pub ciphertext: Vec<u8>,
    pub gcm_tag: GcmTag,
}

#[derive(Debug)]
pub enum CryptoError {
    Argon2(argon2::Error),
    HkdfPrk(InvalidPrkLength),
    HkdfExpand(hkdf::InvalidLength),
    InvalidKeyLength(hmac::digest::InvalidLength),
    Aead(aes_gcm::Error),
    KeyWrap(aes_kw::Error),
    ChunkIdMismatch { expected: ChunkId, actual: ChunkId },
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Argon2(err) => write!(f, "argon2 error: {err}"),
            Self::HkdfPrk(_) => write!(f, "hkdf input key material was invalid"),
            Self::HkdfExpand(_) => write!(f, "hkdf output length was invalid"),
            Self::InvalidKeyLength(_) => write!(f, "hmac key length was invalid"),
            Self::Aead(_) => write!(f, "aes-gcm operation failed"),
            Self::KeyWrap(_) => write!(f, "aes-kw wrap/unwrap failed"),
            Self::ChunkIdMismatch { .. } => write!(f, "decrypted plaintext did not match chunk_id"),
        }
    }
}

impl std::error::Error for CryptoError {}

pub fn derive_root_keys(
    passphrase: &[u8],
    params: &RootKdfParams,
) -> Result<RootKeys, CryptoError> {
    let argon2 = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        params.to_argon2_params()?,
    );

    let mut master_key = [0u8; KEY_LEN];
    argon2
        .hash_password_into(passphrase, &params.salt, &mut master_key)
        .map_err(CryptoError::Argon2)?;

    let vault_key = expand_labeled_key(&master_key, VAULT_KEY_INFO)?;
    let kek = expand_labeled_key(&master_key, KEK_V2_INFO)?;
    let manifest_mac_key = expand_labeled_key(&master_key, MANIFEST_MAC_KEY_INFO)?;
    let lease_mac_key = expand_labeled_key(&master_key, LEASE_MAC_KEY_INFO)?;
    let local_anchor_key = expand_labeled_key(&master_key, LOCAL_ANCHOR_KEY_INFO)?;

    Ok(RootKeys {
        master_key,
        vault_key,
        kek,
        manifest_mac_key,
        lease_mac_key,
        local_anchor_key,
    })
}

pub fn chunk_id(vault_key: &KeyBytes, plaintext_chunk: &[u8]) -> Result<ChunkId, CryptoError> {
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(vault_key).map_err(CryptoError::InvalidKeyLength)?;
    mac.update(plaintext_chunk);

    let mut id = [0u8; KEY_LEN];
    id.copy_from_slice(&mac.finalize().into_bytes());
    Ok(id)
}

pub fn chunk_encryption_key(vault_key: &KeyBytes) -> Result<KeyBytes, CryptoError> {
    expand_labeled_key(vault_key, CHUNK_ENC_INFO)
}

pub fn chunk_nonce(vault_key: &KeyBytes, chunk_id: &ChunkId) -> Result<ChunkNonce, CryptoError> {
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(vault_key).map_err(CryptoError::InvalidKeyLength)?;
    mac.update(CHUNK_NONCE_PREFIX);
    mac.update(chunk_id);

    let digest = mac.finalize().into_bytes();
    let mut nonce = [0u8; CHUNK_NONCE_LEN];
    nonce.copy_from_slice(&digest[..CHUNK_NONCE_LEN]);
    Ok(nonce)
}

pub fn encrypt_chunk(
    vault_key: &KeyBytes,
    plaintext_chunk: &[u8],
    aad: &[u8],
) -> Result<EncryptedChunk, CryptoError> {
    let chunk_id = chunk_id(vault_key, plaintext_chunk)?;
    let nonce = chunk_nonce(vault_key, &chunk_id)?;
    let chunk_enc_key = chunk_encryption_key(vault_key)?;
    let cipher =
        Aes256Gcm::new_from_slice(&chunk_enc_key).map_err(|_| CryptoError::Aead(aes_gcm::Error))?;

    let mut ciphertext = plaintext_chunk.to_vec();
    let tag = cipher
        .encrypt_in_place_detached(Nonce::from_slice(&nonce), aad, &mut ciphertext)
        .map_err(CryptoError::Aead)?;

    let mut gcm_tag = [0u8; GCM_TAG_LEN];
    gcm_tag.copy_from_slice(tag.as_slice());

    Ok(EncryptedChunk {
        chunk_id,
        nonce,
        ciphertext,
        gcm_tag,
    })
}

pub fn decrypt_chunk(
    vault_key: &KeyBytes,
    expected_chunk_id: &ChunkId,
    aad: &[u8],
    ciphertext: &[u8],
    gcm_tag: &GcmTag,
) -> Result<Vec<u8>, CryptoError> {
    let nonce = chunk_nonce(vault_key, expected_chunk_id)?;
    let chunk_enc_key = chunk_encryption_key(vault_key)?;
    let cipher =
        Aes256Gcm::new_from_slice(&chunk_enc_key).map_err(|_| CryptoError::Aead(aes_gcm::Error))?;

    let mut plaintext = ciphertext.to_vec();
    cipher
        .decrypt_in_place_detached(
            Nonce::from_slice(&nonce),
            aad,
            &mut plaintext,
            Tag::from_slice(gcm_tag),
        )
        .map_err(CryptoError::Aead)?;

    let actual_chunk_id = chunk_id(vault_key, &plaintext)?;
    if &actual_chunk_id != expected_chunk_id {
        return Err(CryptoError::ChunkIdMismatch {
            expected: *expected_chunk_id,
            actual: actual_chunk_id,
        });
    }

    Ok(plaintext)
}

// ── V2 Envelope Encryption: chunk-level encrypt/decrypt with DEK ─────

/// Encrypt a chunk using a per-file DEK with a random nonce (V2 path).
/// Returns chunk_id = HMAC-SHA256(dek, plaintext) and a random 12-byte nonce.
pub fn encrypt_chunk_v2(
    dek: &KeyBytes,
    plaintext_chunk: &[u8],
    aad: &[u8],
) -> Result<EncryptedChunk, CryptoError> {
    // chunk_id is HMAC(dek, plaintext) — deterministic per-DEK, not per-vault
    let cid = chunk_id(dek, plaintext_chunk)?;

    // Random nonce — no deterministic derivation in V2
    let mut nonce = [0u8; CHUNK_NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);

    let cipher =
        Aes256Gcm::new_from_slice(dek).map_err(|_| CryptoError::Aead(aes_gcm::Error))?;
    let mut ciphertext = plaintext_chunk.to_vec();
    let tag = cipher
        .encrypt_in_place_detached(Nonce::from_slice(&nonce), aad, &mut ciphertext)
        .map_err(CryptoError::Aead)?;

    let mut gcm_tag = [0u8; GCM_TAG_LEN];
    gcm_tag.copy_from_slice(tag.as_slice());

    Ok(EncryptedChunk {
        chunk_id: cid,
        nonce,
        ciphertext,
        gcm_tag,
    })
}

/// Decrypt a V2 chunk using the per-file DEK and the nonce stored in the prefix.
/// Does NOT verify chunk_id (caller should do that if needed).
pub fn decrypt_chunk_v2(
    dek: &KeyBytes,
    nonce: &ChunkNonce,
    aad: &[u8],
    ciphertext: &[u8],
    gcm_tag: &GcmTag,
) -> Result<Vec<u8>, CryptoError> {
    let cipher =
        Aes256Gcm::new_from_slice(dek).map_err(|_| CryptoError::Aead(aes_gcm::Error))?;
    let mut plaintext = ciphertext.to_vec();
    cipher
        .decrypt_in_place_detached(
            Nonce::from_slice(nonce),
            aad,
            &mut plaintext,
            Tag::from_slice(gcm_tag),
        )
        .map_err(CryptoError::Aead)?;
    Ok(plaintext)
}

fn expand_labeled_key(input_key_material: &[u8], info: &[u8]) -> Result<KeyBytes, CryptoError> {
    let hkdf = Hkdf::<Sha256>::from_prk(input_key_material).map_err(CryptoError::HkdfPrk)?;
    let mut key = [0u8; KEY_LEN];
    hkdf.expand(info, &mut key)
        .map_err(CryptoError::HkdfExpand)?;
    Ok(key)
}

// ── Envelope Encryption V2 ─────────────────────────────────────────────

/// Generate a random 256-bit key (used for Vault Key and DEK generation).
pub fn generate_random_key() -> KeyBytes {
    let mut key = [0u8; KEY_LEN];
    rand::rngs::OsRng.fill_bytes(&mut key);
    key
}

/// Wrap a 256-bit key using AES-256-KW (RFC 3394).
/// Returns 40 bytes (32-byte key + 8-byte integrity check value).
pub fn wrap_key(wrapping_key: &KeyBytes, plaintext_key: &KeyBytes) -> Result<[u8; WRAPPED_KEY_LEN], CryptoError> {
    let kek = Kek::from(*wrapping_key);
    let mut output = [0u8; WRAPPED_KEY_LEN];
    kek.wrap(plaintext_key, &mut output)
        .map_err(CryptoError::KeyWrap)?;
    Ok(output)
}

/// Unwrap a 40-byte AES-KW ciphertext back to a 256-bit key.
pub fn unwrap_key(wrapping_key: &KeyBytes, wrapped_key: &[u8; WRAPPED_KEY_LEN]) -> Result<KeyBytes, CryptoError> {
    let kek = Kek::from(*wrapping_key);
    let mut output = [0u8; KEY_LEN];
    kek.unwrap(wrapped_key, &mut output)
        .map_err(CryptoError::KeyWrap)?;
    Ok(output)
}

/// Derive the KEK from a master key (convenience wrapper).
pub fn derive_kek(master_key: &KeyBytes) -> Result<KeyBytes, CryptoError> {
    expand_labeled_key(master_key, KEK_V2_INFO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_unwrap_round_trip() {
        let kek = generate_random_key();
        let vault_key = generate_random_key();

        let wrapped = wrap_key(&kek, &vault_key).unwrap();
        assert_eq!(wrapped.len(), WRAPPED_KEY_LEN);

        let unwrapped = unwrap_key(&kek, &wrapped).unwrap();
        assert_eq!(unwrapped, vault_key);
    }

    #[test]
    fn unwrap_with_wrong_kek_fails() {
        let kek_a = generate_random_key();
        let kek_b = generate_random_key();
        let vault_key = generate_random_key();

        let wrapped = wrap_key(&kek_a, &vault_key).unwrap();
        let result = unwrap_key(&kek_b, &wrapped);
        assert!(result.is_err());
    }

    #[test]
    fn wrapped_key_is_deterministic() {
        let kek = generate_random_key();
        let vault_key = generate_random_key();

        let wrapped_a = wrap_key(&kek, &vault_key).unwrap();
        let wrapped_b = wrap_key(&kek, &vault_key).unwrap();
        assert_eq!(wrapped_a, wrapped_b);
    }

    #[test]
    fn kek_is_derived_separately_from_vault_key() {
        let params = RootKdfParams::new(1, vec![0u8; 16], 256, 1, 1);
        let root_keys = derive_root_keys(b"test-passphrase", &params).unwrap();

        assert_ne!(root_keys.kek, root_keys.vault_key);
        assert_ne!(root_keys.kek, root_keys.master_key);
    }

    #[test]
    fn full_envelope_flow() {
        // Simulate vault creation + unlock
        let params = RootKdfParams::new(1, vec![0u8; 16], 256, 1, 1);
        let root_keys = derive_root_keys(b"my-passphrase", &params).unwrap();

        // Create vault: generate random Vault Key, wrap with KEK
        let vault_key = generate_random_key();
        let wrapped = wrap_key(&root_keys.kek, &vault_key).unwrap();

        // Simulate unlock: derive same KEK from passphrase, unwrap
        let root_keys_2 = derive_root_keys(b"my-passphrase", &params).unwrap();
        let unwrapped = unwrap_key(&root_keys_2.kek, &wrapped).unwrap();

        assert_eq!(unwrapped, vault_key);
    }

    #[test]
    fn rfc3394_test_vector_256bit() {
        // RFC 3394 § 4.6: 256-bit KEK wrapping 256-bit data
        let kek_bytes: [u8; 32] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
            0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
            0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
        ];
        let plaintext: [u8; 32] = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
            0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
            0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
        ];
        let expected_wrapped: [u8; 40] = [
            0x28, 0xC9, 0xF4, 0x04, 0xC4, 0xB8, 0x10, 0xF4,
            0xCB, 0xCC, 0xB3, 0x5C, 0xFB, 0x87, 0xF8, 0x26,
            0x3F, 0x57, 0x86, 0xE2, 0xD8, 0x0E, 0xD3, 0x26,
            0xCB, 0xC7, 0xF0, 0xE7, 0x1A, 0x99, 0xF4, 0x3B,
            0xFB, 0x98, 0x8B, 0x9B, 0x7A, 0x02, 0xDD, 0x21,
        ];

        let wrapped = wrap_key(&kek_bytes, &plaintext).unwrap();
        assert_eq!(wrapped, expected_wrapped);

        let unwrapped = unwrap_key(&kek_bytes, &wrapped).unwrap();
        assert_eq!(unwrapped, plaintext);
    }

    #[test]
    fn v2_encrypt_decrypt_round_trip() {
        let dek = generate_random_key();
        let plaintext = b"hello envelope encryption v2!";
        let aad = b"";

        let encrypted = encrypt_chunk_v2(&dek, plaintext, aad).unwrap();
        assert_ne!(encrypted.ciphertext.as_slice(), plaintext.as_slice());

        let decrypted = decrypt_chunk_v2(
            &dek,
            &encrypted.nonce,
            aad,
            &encrypted.ciphertext,
            &encrypted.gcm_tag,
        )
        .unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn v2_wrong_dek_fails_decrypt() {
        let dek_a = generate_random_key();
        let dek_b = generate_random_key();
        let encrypted = encrypt_chunk_v2(&dek_a, b"secret data", b"").unwrap();

        let result = decrypt_chunk_v2(&dek_b, &encrypted.nonce, b"", &encrypted.ciphertext, &encrypted.gcm_tag);
        assert!(result.is_err());
    }

    #[test]
    fn v2_random_nonce_differs_per_call() {
        let dek = generate_random_key();
        let a = encrypt_chunk_v2(&dek, b"same data", b"").unwrap();
        let b = encrypt_chunk_v2(&dek, b"same data", b"").unwrap();
        // Random nonce means different ciphertext each time
        assert_ne!(a.nonce, b.nonce);
        // But chunk_id is deterministic (HMAC of plaintext with DEK)
        assert_eq!(a.chunk_id, b.chunk_id);
    }
}
