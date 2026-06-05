use crate::crypto::{CryptoError, KeyBytes, WRAPPED_KEY_LEN, unwrap_key, wrap_key};
use crate::pqkem::{self, ML_KEM_768_CIPHERTEXT_LEN, PqKemError};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroize;

const HYBRID_WRAP_SALT: &[u8] = b"omnidrive-hybrid-wrap-v1";
pub const HYBRID_WRAP_VERSION: &str = "v3-hybrid";
pub const HYBRID_WRAPPED_LEN: usize = ML_KEM_768_CIPHERTEXT_LEN + WRAPPED_KEY_LEN;

/// Anti-splice / anti-downgrade binding folded into the wrapping-KEK transcript.
pub struct HybridWrapContext<'a> {
    pub version: &'a str,
    pub vault_id: &'a str,
    pub device_id: &'a str,
}

#[derive(Debug)]
pub enum HybridError {
    PqKem(PqKemError),
    Crypto(CryptoError),
    InvalidBlobLen(usize),
}

impl std::fmt::Display for HybridError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PqKem(e) => write!(f, "ML-KEM error: {e}"),
            Self::Crypto(e) => write!(f, "crypto error: {e}"),
            Self::InvalidBlobLen(n) => write!(f, "invalid hybrid blob length: {n}"),
        }
    }
}
impl std::error::Error for HybridError {}
impl From<PqKemError> for HybridError {
    fn from(e: PqKemError) -> Self {
        Self::PqKem(e)
    }
}
impl From<CryptoError> for HybridError {
    fn from(e: CryptoError) -> Self {
        Self::Crypto(e)
    }
}

fn append_field(info: &mut Vec<u8>, field: &[u8]) {
    info.extend_from_slice(&(field.len() as u32).to_be_bytes());
    info.extend_from_slice(field);
}

/// `KEK = HKDF-SHA256(salt, ikm = x25519_ss ‖ mlkem_ss, info = length-prefixed transcript)`.
/// The transcript binds version + vault_id + device_id (anti-downgrade/splice) and
/// the kyber ciphertext + encapsulation key (X-Wing anti-rebinding). Never XOR.
fn derive_hybrid_kek(
    x25519_ss: &[u8; 32],
    mlkem_ss: &[u8; 32],
    kyber_ct: &[u8],
    their_ek: &[u8],
    ctx: &HybridWrapContext,
) -> Result<KeyBytes, HybridError> {
    let mut ikm = Vec::with_capacity(64);
    ikm.extend_from_slice(x25519_ss);
    ikm.extend_from_slice(mlkem_ss);
    let hk = Hkdf::<Sha256>::new(Some(HYBRID_WRAP_SALT), &ikm);

    let mut info = Vec::new();
    append_field(&mut info, ctx.version.as_bytes());
    append_field(&mut info, ctx.vault_id.as_bytes());
    append_field(&mut info, ctx.device_id.as_bytes());
    append_field(&mut info, kyber_ct);
    append_field(&mut info, their_ek);

    let mut kek = [0u8; 32];
    let res = hk.expand(&info, &mut kek);
    ikm.zeroize();
    res.map_err(|e| HybridError::Crypto(CryptoError::HkdfExpand(e)))?;

    let out = KeyBytes::from(kek);
    kek.zeroize();
    Ok(out)
}

/// Wraps the Vault Key under the hybrid X25519 + ML-KEM-768 KEK.
/// Returns `kyber_ct (1088) ‖ aes_kw_wrapped_vk (40)` = 1128 bytes. The X25519
/// shared secret is computed by the caller and passed in (keeps this crate free
/// of x25519-dalek).
pub fn hybrid_wrap_vault_key(
    x25519_shared_secret: &[u8; 32],
    their_kyber_encaps_key: &[u8],
    vault_key: &KeyBytes,
    ctx: &HybridWrapContext,
) -> Result<Vec<u8>, HybridError> {
    let (kyber_ct, mut mlkem_ss) = pqkem::ml_kem_encapsulate(their_kyber_encaps_key)?;
    let kek = derive_hybrid_kek(
        x25519_shared_secret,
        &mlkem_ss,
        &kyber_ct,
        their_kyber_encaps_key,
        ctx,
    )?;
    mlkem_ss.zeroize();
    let wrapped = wrap_key(&kek, vault_key)?;
    let mut out = Vec::with_capacity(kyber_ct.len() + wrapped.len());
    out.extend_from_slice(&kyber_ct);
    out.extend_from_slice(&wrapped);
    Ok(out)
}

/// Unwraps a Vault Key produced by `hybrid_wrap_vault_key`. `their_kyber_encaps_key`
/// is the encapsulation key used at wrap time (needed to reconstruct the transcript).
pub fn hybrid_unwrap_vault_key(
    x25519_shared_secret: &[u8; 32],
    my_kyber_decaps_key: &[u8],
    their_kyber_encaps_key: &[u8],
    wrapped_blob: &[u8],
    ctx: &HybridWrapContext,
) -> Result<KeyBytes, HybridError> {
    if wrapped_blob.len() != HYBRID_WRAPPED_LEN {
        return Err(HybridError::InvalidBlobLen(wrapped_blob.len()));
    }
    let (kyber_ct, wrapped) = wrapped_blob.split_at(ML_KEM_768_CIPHERTEXT_LEN);
    let mut mlkem_ss = pqkem::ml_kem_decapsulate(my_kyber_decaps_key, kyber_ct)?;
    let kek = derive_hybrid_kek(
        x25519_shared_secret,
        &mlkem_ss,
        kyber_ct,
        their_kyber_encaps_key,
        ctx,
    )?;
    mlkem_ss.zeroize();
    let wrapped_arr: &[u8; WRAPPED_KEY_LEN] = wrapped
        .try_into()
        .map_err(|_| HybridError::InvalidBlobLen(wrapped_blob.len()))?;
    Ok(unwrap_key(&kek, wrapped_arr)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyBytes;
    use crate::pqkem::generate_ml_kem_768_keypair;

    fn ctx() -> HybridWrapContext<'static> {
        HybridWrapContext {
            version: HYBRID_WRAP_VERSION,
            vault_id: "vault-1",
            device_id: "dev-1",
        }
    }

    #[test]
    fn wrap_unwrap_roundtrip() {
        let (ek, dk) = generate_ml_kem_768_keypair();
        let x_ss = [0x07u8; 32];
        let vk = KeyBytes::from([0x33u8; 32]);
        let blob = hybrid_wrap_vault_key(&x_ss, &ek, &vk, &ctx()).unwrap();
        assert_eq!(blob.len(), HYBRID_WRAPPED_LEN);
        let out = hybrid_unwrap_vault_key(&x_ss, &dk, &ek, &blob, &ctx()).unwrap();
        assert_eq!(out, vk);
    }

    #[test]
    fn unwrap_rejects_tampered_ciphertext() {
        let (ek, dk) = generate_ml_kem_768_keypair();
        let x_ss = [0x07u8; 32];
        let vk = KeyBytes::from([0x33u8; 32]);
        let mut blob = hybrid_wrap_vault_key(&x_ss, &ek, &vk, &ctx()).unwrap();
        blob[0] ^= 0xFF;
        assert!(hybrid_unwrap_vault_key(&x_ss, &dk, &ek, &blob, &ctx()).is_err());
    }

    #[test]
    fn unwrap_rejects_tampered_wrapped_key() {
        let (ek, dk) = generate_ml_kem_768_keypair();
        let x_ss = [0x07u8; 32];
        let vk = KeyBytes::from([0x33u8; 32]);
        let mut blob = hybrid_wrap_vault_key(&x_ss, &ek, &vk, &ctx()).unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        assert!(hybrid_unwrap_vault_key(&x_ss, &dk, &ek, &blob, &ctx()).is_err());
    }

    #[test]
    fn unwrap_rejects_downgraded_version() {
        let (ek, dk) = generate_ml_kem_768_keypair();
        let x_ss = [0x07u8; 32];
        let vk = KeyBytes::from([0x33u8; 32]);
        let blob = hybrid_wrap_vault_key(&x_ss, &ek, &vk, &ctx()).unwrap();
        let bad = HybridWrapContext {
            version: "v2-x25519",
            vault_id: "vault-1",
            device_id: "dev-1",
        };
        assert!(hybrid_unwrap_vault_key(&x_ss, &dk, &ek, &blob, &bad).is_err());
    }

    #[test]
    fn unwrap_rejects_wrong_device_id() {
        let (ek, dk) = generate_ml_kem_768_keypair();
        let x_ss = [0x07u8; 32];
        let vk = KeyBytes::from([0x33u8; 32]);
        let blob = hybrid_wrap_vault_key(&x_ss, &ek, &vk, &ctx()).unwrap();
        let bad = HybridWrapContext {
            version: HYBRID_WRAP_VERSION,
            vault_id: "vault-1",
            device_id: "dev-OTHER",
        };
        assert!(hybrid_unwrap_vault_key(&x_ss, &dk, &ek, &blob, &bad).is_err());
    }

    #[test]
    fn unwrap_rejects_wrong_x25519_secret() {
        let (ek, dk) = generate_ml_kem_768_keypair();
        let vk = KeyBytes::from([0x33u8; 32]);
        let blob = hybrid_wrap_vault_key(&[0x07u8; 32], &ek, &vk, &ctx()).unwrap();
        assert!(hybrid_unwrap_vault_key(&[0x08u8; 32], &dk, &ek, &blob, &ctx()).is_err());
    }

    #[test]
    fn unwrap_rejects_wrong_decaps_key() {
        let (ek, _dk) = generate_ml_kem_768_keypair();
        let (_ek2, dk2) = generate_ml_kem_768_keypair();
        let x_ss = [0x07u8; 32];
        let vk = KeyBytes::from([0x33u8; 32]);
        let blob = hybrid_wrap_vault_key(&x_ss, &ek, &vk, &ctx()).unwrap();
        assert!(hybrid_unwrap_vault_key(&x_ss, &dk2, &ek, &blob, &ctx()).is_err());
    }
}
