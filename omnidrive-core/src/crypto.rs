use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce, Tag};
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
pub const MANIFEST_MAC_KEY_INFO: &[u8] = b"manifest-mac-key-v1";
pub const LEASE_MAC_KEY_INFO: &[u8] = b"lease-mac-key-v1";
pub const LOCAL_ANCHOR_KEY_INFO: &[u8] = b"local-anchor-key-v1";

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
    let manifest_mac_key = expand_labeled_key(&master_key, MANIFEST_MAC_KEY_INFO)?;
    let lease_mac_key = expand_labeled_key(&master_key, LEASE_MAC_KEY_INFO)?;
    let local_anchor_key = expand_labeled_key(&master_key, LOCAL_ANCHOR_KEY_INFO)?;

    Ok(RootKeys {
        master_key,
        vault_key,
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
    let cipher = Aes256Gcm::new_from_slice(&chunk_enc_key).map_err(|_| {
        CryptoError::Aead(aes_gcm::Error)
    })?;

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
    let cipher = Aes256Gcm::new_from_slice(&chunk_enc_key).map_err(|_| {
        CryptoError::Aead(aes_gcm::Error)
    })?;

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

fn expand_labeled_key(input_key_material: &[u8], info: &[u8]) -> Result<KeyBytes, CryptoError> {
    let hkdf = Hkdf::<Sha256>::from_prk(input_key_material).map_err(CryptoError::HkdfPrk)?;
    let mut key = [0u8; KEY_LEN];
    hkdf.expand(info, &mut key)
        .map_err(CryptoError::HkdfExpand)?;
    Ok(key)
}
