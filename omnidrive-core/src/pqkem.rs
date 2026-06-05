use ml_kem::kem::{Decapsulate, Encapsulate};
use ml_kem::{Ciphertext, Encoded, EncodedSizeUser, KemCore, MlKem768};

pub const ML_KEM_768_ENCAPS_KEY_LEN: usize = 1184;
pub const ML_KEM_768_DECAPS_KEY_LEN: usize = 2400;

/// Generates a fresh ML-KEM-768 (FIPS 203) keypair.
///
/// Returns `(encapsulation_key, decapsulation_key)` as raw bytes: the 1184-byte
/// public encapsulation key and the 2400-byte secret decapsulation key. The
/// caller is responsible for sealing the secret.
pub fn generate_ml_kem_768_keypair() -> (Vec<u8>, Vec<u8>) {
    let mut rng = rand::thread_rng();
    let (dk, ek) = MlKem768::generate(&mut rng);
    (ek.as_bytes().to_vec(), dk.as_bytes().to_vec())
}

pub const ML_KEM_768_CIPHERTEXT_LEN: usize = 1088;
pub const ML_KEM_768_SHARED_SECRET_LEN: usize = 32;

type Ek = <MlKem768 as KemCore>::EncapsulationKey;
type Dk = <MlKem768 as KemCore>::DecapsulationKey;

#[derive(Debug, PartialEq, Eq)]
pub enum PqKemError {
    InvalidEncapsKeyLen(usize),
    InvalidDecapsKeyLen(usize),
    InvalidCiphertextLen(usize),
}

impl std::fmt::Display for PqKemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEncapsKeyLen(n) => {
                write!(f, "invalid ML-KEM encapsulation key length: {n}")
            }
            Self::InvalidDecapsKeyLen(n) => {
                write!(f, "invalid ML-KEM decapsulation key length: {n}")
            }
            Self::InvalidCiphertextLen(n) => write!(f, "invalid ML-KEM ciphertext length: {n}"),
        }
    }
}
impl std::error::Error for PqKemError {}

/// Encapsulates against a peer's ML-KEM-768 encapsulation key.
/// Returns `(ciphertext 1088 B, shared_secret 32 B)`.
pub fn ml_kem_encapsulate(their_ek_bytes: &[u8]) -> Result<(Vec<u8>, [u8; 32]), PqKemError> {
    let encoded = Encoded::<Ek>::try_from(their_ek_bytes)
        .map_err(|_| PqKemError::InvalidEncapsKeyLen(their_ek_bytes.len()))?;
    let ek = Ek::from_bytes(&encoded);
    let mut rng = rand::thread_rng();
    let (ct, ss) = ek.encapsulate(&mut rng).expect("ML-KEM encapsulation");
    let mut ss_bytes = [0u8; 32];
    ss_bytes.copy_from_slice(&ss);
    Ok((ct.to_vec(), ss_bytes))
}

/// Decapsulates a ciphertext with the local ML-KEM-768 decapsulation key.
/// Per FIPS 203 a tampered ciphertext does not error here — it yields a
/// pseudo-random secret (implicit rejection); the caller detects tampering
/// downstream (AES-KW integrity).
pub fn ml_kem_decapsulate(my_dk_bytes: &[u8], ct_bytes: &[u8]) -> Result<[u8; 32], PqKemError> {
    let encoded = Encoded::<Dk>::try_from(my_dk_bytes)
        .map_err(|_| PqKemError::InvalidDecapsKeyLen(my_dk_bytes.len()))?;
    let dk = Dk::from_bytes(&encoded);
    let ct = Ciphertext::<MlKem768>::try_from(ct_bytes)
        .map_err(|_| PqKemError::InvalidCiphertextLen(ct_bytes.len()))?;
    let ss = dk.decapsulate(&ct).expect("ML-KEM decapsulation");
    let mut ss_bytes = [0u8; 32];
    ss_bytes.copy_from_slice(&ss);
    Ok(ss_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_correct_sizes() {
        let (ek, dk) = generate_ml_kem_768_keypair();
        assert_eq!(ek.len(), ML_KEM_768_ENCAPS_KEY_LEN);
        assert_eq!(dk.len(), ML_KEM_768_DECAPS_KEY_LEN);
        assert_ne!(ek, vec![0u8; ek.len()], "encaps key must not be all-zero");
        assert_ne!(dk, vec![0u8; dk.len()], "decaps key must not be all-zero");
    }

    #[test]
    fn two_keypairs_differ() {
        let (ek1, dk1) = generate_ml_kem_768_keypair();
        let (ek2, dk2) = generate_ml_kem_768_keypair();
        assert_ne!(ek1, ek2, "fresh keypairs must differ");
        assert_ne!(dk1, dk2);
    }

    #[test]
    fn encapsulate_decapsulate_roundtrip_same_secret() {
        let (ek, dk) = generate_ml_kem_768_keypair();
        let (ct, ss_enc) = ml_kem_encapsulate(&ek).unwrap();
        assert_eq!(ct.len(), ML_KEM_768_CIPHERTEXT_LEN);
        assert_eq!(ss_enc.len(), ML_KEM_768_SHARED_SECRET_LEN);
        let ss_dec = ml_kem_decapsulate(&dk, &ct).unwrap();
        assert_eq!(
            ss_enc, ss_dec,
            "encaps and decaps must agree on the shared secret"
        );
    }

    #[test]
    fn encapsulate_rejects_wrong_encaps_key_length() {
        assert!(ml_kem_encapsulate(&[0u8; 10]).is_err());
    }

    #[test]
    fn decapsulate_rejects_wrong_lengths() {
        let (_ek, dk) = generate_ml_kem_768_keypair();
        assert!(ml_kem_decapsulate(&dk, &[0u8; 10]).is_err());
        assert!(ml_kem_decapsulate(&[0u8; 10], &[0u8; ML_KEM_768_CIPHERTEXT_LEN]).is_err());
    }
}
