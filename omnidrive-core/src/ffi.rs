use crate::crypto::{self, ChunkNonce, GcmTag, KeyBytes, WRAPPED_KEY_LEN};

#[derive(Debug, uniffi::Error)]
pub enum OmniCoreError {
    InvalidKeyLength,
    KeyUnwrap,
    DecryptFailed,
}

impl std::fmt::Display for OmniCoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKeyLength => write!(f, "invalid key length"),
            Self::KeyUnwrap => write!(f, "AES-KW unwrap failed"),
            Self::DecryptFailed => write!(f, "AES-GCM decrypt failed"),
        }
    }
}

impl std::error::Error for OmniCoreError {}

/// Unwrap a 40-byte AES-KW wrapped key using a 32-byte wrapping key.
/// Returns the unwrapped 32-byte key.
#[uniffi::export]
pub fn ffi_unwrap_key(
    wrapping_key: Vec<u8>,
    wrapped_key: Vec<u8>,
) -> Result<Vec<u8>, OmniCoreError> {
    let wk: KeyBytes = wrapping_key
        .try_into()
        .map_err(|_| OmniCoreError::InvalidKeyLength)?;
    let wk_arr: [u8; WRAPPED_KEY_LEN] = wrapped_key
        .try_into()
        .map_err(|_| OmniCoreError::InvalidKeyLength)?;
    crypto::unwrap_key(&wk, &wk_arr)
        .map(|k| k.to_vec())
        .map_err(|_| OmniCoreError::KeyUnwrap)
}

/// Decrypt a V2 chunk using a per-file DEK.
///
/// - `dek`: 32 bytes
/// - `nonce`: 12 bytes (stored in chunk header)
/// - `aad`: additional authenticated data (usually empty)
/// - `ciphertext`: N bytes
/// - `gcm_tag`: 16 bytes
#[uniffi::export]
pub fn ffi_decrypt_chunk_v2(
    dek: Vec<u8>,
    nonce: Vec<u8>,
    aad: Vec<u8>,
    ciphertext: Vec<u8>,
    gcm_tag: Vec<u8>,
) -> Result<Vec<u8>, OmniCoreError> {
    let dek_arr: KeyBytes = dek
        .try_into()
        .map_err(|_| OmniCoreError::InvalidKeyLength)?;
    let nonce_arr: ChunkNonce = nonce
        .try_into()
        .map_err(|_| OmniCoreError::InvalidKeyLength)?;
    let tag_arr: GcmTag = gcm_tag
        .try_into()
        .map_err(|_| OmniCoreError::InvalidKeyLength)?;
    crypto::decrypt_chunk_v2(&dek_arr, &nonce_arr, &aad, &ciphertext, &tag_arr)
        .map_err(|_| OmniCoreError::DecryptFailed)
}
