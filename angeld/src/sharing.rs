//! Zero-Knowledge File Sharing (Epic 33)
//!
//! Generates share links where the DEK lives in the URL fragment
//! (never sent to the server). Recipients decrypt in-browser via WebCrypto.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
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

/// Encode a 256-bit DEK as base64url (no padding) for use in URL fragment.
pub fn encode_dek_for_url(dek: &[u8; 32]) -> String {
    URL_SAFE_NO_PAD.encode(dek)
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
