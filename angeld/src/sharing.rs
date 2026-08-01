//! Zero-Knowledge File Sharing (Epic 33)
//!
//! Generates share links where the DEK lives in the URL fragment
//! (never sent to the server). Recipients decrypt in-browser via WebCrypto.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use omnidrive_core::crypto::{CryptoError, KeyBytes, decrypt_secret, derive_subkey, encrypt_secret};
use rand::RngCore;

/// Length of the random share ID in bytes (128-bit).
const SHARE_ID_BYTES: usize = 16;

/// Length of the password token in bytes (128-bit).
const TOKEN_BYTES: usize = 16;

/// Salt length for share password hashing.
const PASSWORD_SALT_LEN: usize = 16;

/// Generate a cryptographically random share ID (22-char base64url).
pub fn generate_share_id() -> String {
    let mut buf = [0u8; SHARE_ID_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

/// Encode a 256-bit key as base64url (no padding) for use in URL fragment.
pub fn encode_dek_for_url(dek: &[u8; 32]) -> String {
    URL_SAFE_NO_PAD.encode(dek)
}

/// HKDF label separating the share-link wrapping key from the raw fragment bytes.
const SHARE_DEK_INFO: &[u8] = b"omnidrive-share-dek-v1";

/// Mints the secret that lives in the URL fragment. It never reaches the server:
/// the daemon only ever stores DEKs already sealed under it.
pub fn generate_share_key() -> [u8; 32] {
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    buf
}

fn share_wrapping_key(share_key: &[u8; 32]) -> Result<KeyBytes, CryptoError> {
    derive_subkey(&KeyBytes::from(*share_key), SHARE_DEK_INFO)
}

/// Seals one pack's DEK under the share key. `pack_id` is authenticated as AAD, so a
/// sealed DEK cannot be moved to a different pack of the same share.
pub fn seal_dek_for_share(
    share_key: &[u8; 32],
    pack_id: &str,
    dek: &[u8; 32],
) -> Result<Vec<u8>, CryptoError> {
    encrypt_secret(&share_wrapping_key(share_key)?, dek, pack_id.as_bytes())
}

/// Reverses [`seal_dek_for_share`]. Mirrors what the browser does with WebCrypto.
pub fn open_dek_for_share(
    share_key: &[u8; 32],
    pack_id: &str,
    sealed: &[u8],
) -> Result<[u8; 32], CryptoError> {
    let plaintext = decrypt_secret(&share_wrapping_key(share_key)?, sealed, pack_id.as_bytes())?;
    plaintext
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::Aead(aes_gcm::Error))
}

/// Generate a random password token (22-char base64url).
pub fn generate_share_token() -> String {
    let mut buf = [0u8; TOKEN_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

/// Hash a share password using Argon2id with lightweight params
/// (share passwords are less critical than vault passphrase).
/// Returns "salt_base64url$hash_base64url".
pub fn hash_share_password(password: &str) -> String {
    let mut salt = [0u8; PASSWORD_SALT_LEN];
    rand::rngs::OsRng.fill_bytes(&mut salt);

    let params = Params::new(8192, 2, 1, Some(32)).expect("valid argon2 params");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut hash = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), &salt, &mut hash)
        .expect("argon2 hash");

    format!(
        "{}${}",
        URL_SAFE_NO_PAD.encode(salt),
        URL_SAFE_NO_PAD.encode(hash)
    )
}

/// Verify a password against a stored hash ("salt_base64url$hash_base64url").
pub fn verify_share_password(password: &str, stored: &str) -> bool {
    let parts: Vec<&str> = stored.splitn(2, '$').collect();
    if parts.len() != 2 {
        return false;
    }

    let Ok(salt) = URL_SAFE_NO_PAD.decode(parts[0]) else {
        return false;
    };
    let Ok(expected_hash) = URL_SAFE_NO_PAD.decode(parts[1]) else {
        return false;
    };

    let params = Params::new(8192, 2, 1, Some(32)).expect("valid argon2 params");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut computed = [0u8; 32];
    if argon2
        .hash_password_into(password.as_bytes(), &salt, &mut computed)
        .is_err()
    {
        return false;
    }

    // Constant-time comparison
    use subtle::ConstantTimeEq;
    computed.ct_eq(&expected_hash).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Z4-01 nastepstwo: klucz nalezy do packa, wiec plik wielochunkowy ma wiele
    /// kluczy. Link niesie jeden `share_key`, ktorym odbiorca rozwija kazdy z nich.
    #[test]
    fn share_key_opens_every_pack_dek() {
        let share_key = generate_share_key();
        let dek_a = [0x11u8; 32];
        let dek_b = [0x22u8; 32];

        let sealed_a = seal_dek_for_share(&share_key, "pack-a", &dek_a).unwrap();
        let sealed_b = seal_dek_for_share(&share_key, "pack-b", &dek_b).unwrap();
        assert_ne!(sealed_a, sealed_b, "rozne packi daja rozne szyfrogramy");

        assert_eq!(
            open_dek_for_share(&share_key, "pack-a", &sealed_a).unwrap(),
            dek_a
        );
        assert_eq!(
            open_dek_for_share(&share_key, "pack-b", &sealed_b).unwrap(),
            dek_b
        );
    }

    /// Kontrakt z przegladarka. WebCrypto ma tylko HKDF z krokiem extract, a Rust
    /// uzywa `from_prk` (sam expand). Dla 32 bajtow expand to dokladnie jeden blok:
    /// HMAC-SHA256(PRK, info || 0x01) — i na tym opiera sie deriveShareWrappingKey
    /// w share.html oraz share-sw.js. Jesli ten test padnie, linki share przestana
    /// dzialac w przegladarce, mimo ze cala reszta bedzie zielona.
    #[test]
    fn share_wrapping_key_is_one_hmac_block() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let share_key = [0x5Au8; 32];

        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&share_key).unwrap();
        mac.update(SHARE_DEK_INFO);
        mac.update(&[0x01]);
        let expected = mac.finalize().into_bytes();

        let actual = share_wrapping_key(&share_key).unwrap();
        assert_eq!(
            actual.as_ref() as &[u8],
            expected.as_slice(),
            "rozjazd wyprowadzenia klucza miedzy Rust a WebCrypto"
        );
    }

    #[test]
    fn sealed_dek_is_bound_to_its_pack() {
        let share_key = generate_share_key();
        let dek = [0x33u8; 32];
        let sealed = seal_dek_for_share(&share_key, "pack-a", &dek).unwrap();
        assert!(
            open_dek_for_share(&share_key, "pack-b", &sealed).is_err(),
            "podmiana zawinietego klucza miedzy packami musi byc wykryta"
        );
    }

    #[test]
    fn wrong_share_key_cannot_open_dek() {
        let dek = [0x44u8; 32];
        let sealed = seal_dek_for_share(&generate_share_key(), "pack-a", &dek).unwrap();
        assert!(open_dek_for_share(&generate_share_key(), "pack-a", &sealed).is_err());
    }

    #[test]
    fn share_id_is_22_chars() {
        let id = generate_share_id();
        assert_eq!(id.len(), 22);
    }

    #[test]
    fn token_is_22_chars() {
        let token = generate_share_token();
        assert_eq!(token.len(), 22);
    }

    #[test]
    fn password_hash_verify_roundtrip() {
        let hash = hash_share_password("test-password-123");
        assert!(verify_share_password("test-password-123", &hash));
        assert!(!verify_share_password("wrong-password", &hash));
    }

    #[test]
    fn password_hash_different_salts() {
        let h1 = hash_share_password("same");
        let h2 = hash_share_password("same");
        assert_ne!(h1, h2); // different salts
        assert!(verify_share_password("same", &h1));
        assert!(verify_share_password("same", &h2));
    }

    #[test]
    fn verify_rejects_malformed_hash() {
        assert!(!verify_share_password("pw", "not-a-valid-hash"));
        assert!(!verify_share_password("pw", ""));
        assert!(!verify_share_password("pw", "$"));
    }
}
