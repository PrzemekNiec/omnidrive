use ml_kem::{EncodedSizeUser, KemCore, MlKem768};

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
}
