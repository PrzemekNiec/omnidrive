# α.B.b — ML-KEM-768 Hybrid Vault-Key Wrap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give each device a post-quantum ML-KEM-768 keypair (alongside its X25519 keypair) so the Vault Key can later be wrapped under a hybrid X25519 + ML-KEM scheme (harvest-now-decrypt-later resistance), without changing solo unlock or any existing X25519 path.

**Architecture:** Additive only. New `omnidrive-core::pqkem` module owns the `ml-kem` dependency and keygen. New BLOB columns hold the ML-KEM encapsulation key and the sealed decapsulation key; existing X25519 storage is untouched. A sibling `identity::ensure_device_kyber_keypair` generates + seals the keypair idempotently (preserving the X25519 hot-path early-return and enabling backfill on existing α.C.a devices). Wrapping the VK is **not** in this plan — Task 1 only establishes keys.

**Tech Stack:** Rust (Edition 2024), `ml-kem = "0.2"` (RustCrypto, pure-Rust), `aes-gcm`, `hkdf`, `sha2`, `sqlx` (SQLite), Tokio.

**Spec:** `docs/superpowers/specs/2026-06-05-alpha-B-b-mlkem-hybrid-wrap-design.md`

---

## File Structure

- **Create:** `omnidrive-core/src/pqkem.rs` — ML-KEM-768 keygen + size constants + unit tests. Sole owner of the `ml-kem` dependency.
- **Modify:** `omnidrive-core/Cargo.toml` — add `ml-kem = "0.2"`.
- **Modify:** `omnidrive-core/src/lib.rs` — `pub mod pqkem;`.
- **Modify:** `angeld/src/db.rs` — additive schema (`local_device_identity` +2 cols, `devices` +2 cols), `LocalDeviceIdentityRecord` +2 fields, extend the SELECT, new `store_kyber_keypair` + `set_device_kyber_public_key`.
- **Modify:** `angeld/src/identity.rs` — variable-length `seal_secret_blob`/`open_secret_blob`, new `ensure_device_kyber_keypair`, tests.

**Task 2 (α.B.b.2)** additionally: extend `omnidrive-core/src/pqkem.rs` (encapsulate/decapsulate wrappers) and **create** `omnidrive-core/src/hybrid.rs` (combiner + wrap/unwrap), registered in `omnidrive-core/src/lib.rs`. Still `omnidrive-core`-only — no new dependencies.

No changes to `vault.rs`, no wiring into the live unlock path (that is α.B.b.3). No version bump.

---

## Task 1 (α.B.b.1): ML-KEM-768 keypair generation, storage, and sealing

### 1A — Core: dependency + `pqkem` module

**Files:**
- Modify: `omnidrive-core/Cargo.toml`
- Create: `omnidrive-core/src/pqkem.rs`
- Modify: `omnidrive-core/src/lib.rs`

- [ ] **Step 1: Add the dependency**

In `omnidrive-core/Cargo.toml`, under `[dependencies]`, add after the `hkdf` line:

```toml
ml-kem = "0.2"
```

> **Implementer note (crate API):** `ml-kem` 0.2 re-exports `KemCore`, `MlKem768`, and the `EncodedSizeUser` trait (provides `as_bytes()`). `MlKem768::generate(&mut rng)` returns `(DecapsulationKey, EncapsulationKey)` — **decapsulation first**. `.as_bytes()` returns an array that derefs to `[u8]` (so `.to_vec()` works). If a name differs in the pinned patch release, confirm on docs.rs; the size assertions in the tests below are the guard. `rand::thread_rng()` (rand 0.8, already a dep) satisfies the RNG bound.

- [ ] **Step 2: Write the failing core tests**

Create `omnidrive-core/src/pqkem.rs` with the tests first (module body added in Step 3):

```rust
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
```

- [ ] **Step 3: Run the tests, verify they FAIL to compile**

Run: `cargo test -p omnidrive-core pqkem`
Expected: FAIL — `generate_ml_kem_768_keypair` / constants not defined.

- [ ] **Step 4: Implement the module body**

Prepend above the `#[cfg(test)]` block in `omnidrive-core/src/pqkem.rs`:

```rust
use ml_kem::{EncodedSizeUser, KemCore, MlKem768};

pub const ML_KEM_768_ENCAPS_KEY_LEN: usize = 1184;
pub const ML_KEM_768_DECAPS_KEY_LEN: usize = 2400;

/// Generates a fresh ML-KEM-768 (FIPS 203) keypair.
///
/// Returns `(encapsulation_key, decapsulation_key)` as raw bytes:
/// the 1184-byte public encapsulation key and the 2400-byte secret
/// decapsulation key. The caller is responsible for sealing the secret.
pub fn generate_ml_kem_768_keypair() -> (Vec<u8>, Vec<u8>) {
    let mut rng = rand::thread_rng();
    let (dk, ek) = MlKem768::generate(&mut rng);
    (ek.as_bytes().to_vec(), dk.as_bytes().to_vec())
}
```

- [ ] **Step 5: Register the module**

In `omnidrive-core/src/lib.rs`, add alongside the other `pub mod` declarations:

```rust
pub mod pqkem;
```

- [ ] **Step 6: Run the tests, verify they PASS**

Run: `cargo test -p omnidrive-core pqkem`
Expected: PASS (2 tests).

- [ ] **Step 7: Commit**

```bash
git add omnidrive-core/Cargo.toml omnidrive-core/src/pqkem.rs omnidrive-core/src/lib.rs Cargo.lock
git commit -m "feat(core): α.B.b ML-KEM-768 keypair generation (pqkem module)"
```

---

### 1B — DB: additive schema + struct + persistence helpers

**Files:**
- Modify: `angeld/src/db.rs` (schema `db.rs:744-749` + `db.rs:1151-1154`; struct `db.rs:219-227`; SELECT `db.rs:2874-2881`; new fns after `store_device_keypair` `db.rs:2964`)

- [ ] **Step 1: Write the failing persistence test**

Add to the `db` test module (`mod tests` in `db.rs`):

```rust
#[tokio::test]
async fn store_and_read_kyber_keypair() {
    let pool = init_db("sqlite::memory:").await.unwrap();
    upsert_local_device_identity(&pool, "dev-kyber", "TestPC", "tok-1")
        .await
        .unwrap();

    let sealed_priv = vec![0x11u8; 2428];
    let kyber_pub = vec![0x22u8; 1184];
    store_kyber_keypair(&pool, &sealed_priv, &kyber_pub).await.unwrap();

    let device = get_local_device_identity(&pool).await.unwrap().unwrap();
    assert_eq!(device.encrypted_kyber_private_key.as_deref(), Some(sealed_priv.as_slice()));
    assert_eq!(device.kyber_public_key.as_deref(), Some(kyber_pub.as_slice()));
}
```

- [ ] **Step 2: Run it, verify it FAILS to compile**

Run: `cargo test -p angeld --lib store_and_read_kyber_keypair`
Expected: FAIL — `store_kyber_keypair` and the struct fields do not exist.

- [ ] **Step 3: Add the `local_device_identity` columns**

In `init_db`, immediately after the two X25519 `ALTER TABLE local_device_identity` statements (`db.rs:744-749`), add:

```rust
    let _ = sqlx::query(
        "ALTER TABLE local_device_identity ADD COLUMN encrypted_kyber_private_key BLOB",
    )
    .execute(&pool)
    .await;
    let _ = sqlx::query("ALTER TABLE local_device_identity ADD COLUMN kyber_public_key BLOB")
        .execute(&pool)
        .await;
```

- [ ] **Step 4: Add the `devices` columns**

After the `enrolled_at` `ensure_column_exists` call (`db.rs:1154`), add:

```rust
    ensure_column_exists(&pool, "devices", "kyber_public_key", "BLOB").await?;
    ensure_column_exists(&pool, "devices", "wrapped_vault_key_kyber", "BLOB").await?;
```

- [ ] **Step 5: Extend `LocalDeviceIdentityRecord`**

Add two fields to the struct (`db.rs:219-227`), after `public_key`:

```rust
    pub encrypted_kyber_private_key: Option<Vec<u8>>,
    pub kyber_public_key: Option<Vec<u8>>,
```

- [ ] **Step 6: Extend the SELECT**

In `get_local_device_identity` (`db.rs:2874-2881`), change the column list to:

```rust
        SELECT device_id, device_name, peer_token, created_at, updated_at,
               encrypted_private_key, public_key,
               encrypted_kyber_private_key, kyber_public_key
        FROM local_device_identity
        WHERE id = 1
```

- [ ] **Step 7: Add the persistence helpers**

After `store_device_keypair` (`db.rs:2964`), add:

```rust
pub async fn store_kyber_keypair(
    pool: &SqlitePool,
    encrypted_kyber_private_key: &[u8],
    kyber_public_key: &[u8],
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "UPDATE local_device_identity \
         SET encrypted_kyber_private_key = ?, kyber_public_key = ?, updated_at = ? \
         WHERE id = 1",
    )
    .bind(encrypted_kyber_private_key)
    .bind(kyber_public_key)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_device_kyber_public_key(
    pool: &SqlitePool,
    device_id: &str,
    kyber_public_key: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devices SET kyber_public_key = ? WHERE device_id = ?")
        .bind(kyber_public_key)
        .bind(device_id)
        .execute(pool)
        .await?;
    Ok(())
}
```

- [ ] **Step 8: Run the test, verify it PASSES**

Run: `cargo test -p angeld --lib store_and_read_kyber_keypair`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add angeld/src/db.rs
git commit -m "feat(db): α.B.b additive kyber schema + store_kyber_keypair"
```

---

### 1C — Identity: variable-length seal + `ensure_device_kyber_keypair`

**Files:**
- Modify: `angeld/src/identity.rs` (helpers after `decrypt_private_key` `identity.rs:93`; new fn after `ensure_device_keypair` `identity.rs:139`; tests in `mod tests` `identity.rs:244`)

- [ ] **Step 1: Write the failing seal round-trip test**

Add to `mod tests` in `identity.rs`:

```rust
#[test]
fn seal_open_secret_blob_roundtrip_variable_length() {
    let kek = [0x77u8; 32];
    let secret = vec![0x5Au8; 2400]; // ML-KEM-768 decaps key size
    let sealed = seal_secret_blob(&kek, &secret).unwrap();
    assert_eq!(sealed.len(), NONCE_LEN + 2400 + 16);
    assert_eq!(open_secret_blob(&kek, &sealed).unwrap(), secret);

    let wrong = [0x88u8; 32];
    assert!(open_secret_blob(&wrong, &sealed).is_err());
}
```

- [ ] **Step 2: Run it, verify it FAILS to compile**

Run: `cargo test -p angeld --lib seal_open_secret_blob_roundtrip_variable_length`
Expected: FAIL — `seal_secret_blob` / `open_secret_blob` not defined.

- [ ] **Step 3: Implement the variable-length helpers**

After `decrypt_private_key` (`identity.rs:93`), add:

```rust
/// Encrypts an arbitrary-length secret with AES-256-GCM under `kek`.
/// Returns `nonce(12) || ciphertext+tag`. Unlike `encrypt_private_key`
/// this is not locked to 32-byte inputs (used for the 2400-byte ML-KEM
/// decapsulation key).
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

/// Decrypts a blob produced by `seal_secret_blob`.
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
```

- [ ] **Step 4: Run it, verify it PASSES**

Run: `cargo test -p angeld --lib seal_open_secret_blob_roundtrip_variable_length`
Expected: PASS.

- [ ] **Step 5: Write the failing keygen test**

Add to `mod tests` in `identity.rs`:

```rust
#[tokio::test]
async fn ensure_device_kyber_keypair_generates_persists_and_is_idempotent() {
    let pool = db::init_db("sqlite::memory:").await.unwrap();
    let master_key = [0x42u8; 32];
    db::upsert_local_device_identity(&pool, "dev-kyber", "TestPC", "tok-1")
        .await
        .unwrap();

    let pub1 = ensure_device_kyber_keypair(&pool, &master_key).await.unwrap();
    assert_eq!(pub1.len(), omnidrive_core::pqkem::ML_KEM_768_ENCAPS_KEY_LEN);
    assert_ne!(pub1, vec![0u8; pub1.len()]);

    // Idempotent: second call returns the same public key.
    let pub2 = ensure_device_kyber_keypair(&pool, &master_key).await.unwrap();
    assert_eq!(pub1, pub2);

    // The sealed decaps key unseals under the identity KEK to a 2400-byte secret.
    let device = db::get_local_device_identity(&pool).await.unwrap().unwrap();
    let sealed = device.encrypted_kyber_private_key.unwrap();
    let kek = derive_identity_kek(&master_key).unwrap();
    let decaps = open_secret_blob(&kek, &sealed).unwrap();
    assert_eq!(decaps.len(), omnidrive_core::pqkem::ML_KEM_768_DECAPS_KEY_LEN);
}
```

- [ ] **Step 6: Run it, verify it FAILS to compile**

Run: `cargo test -p angeld --lib ensure_device_kyber_keypair_generates_persists_and_is_idempotent`
Expected: FAIL — `ensure_device_kyber_keypair` not defined.

- [ ] **Step 7: Implement `ensure_device_kyber_keypair`**

After `ensure_device_keypair` (`identity.rs:139`), add:

```rust
/// Ensures the local device has an ML-KEM-768 keypair, sealed at rest under the
/// identity KEK (same KEK as the X25519 private key). Idempotent: returns the
/// existing encapsulation key if one is already stored. Sibling to
/// `ensure_device_keypair` (not folded in, so existing X25519-only devices get
/// backfilled with a kyber keypair on the next unlock).
///
/// Returns the 1184-byte ML-KEM encapsulation (public) key.
pub async fn ensure_device_kyber_keypair(
    pool: &SqlitePool,
    master_key: &[u8],
) -> Result<Vec<u8>, IdentityError> {
    let device = db::get_local_device_identity(pool)
        .await?
        .ok_or(IdentityError::NoDeviceIdentity)?;

    if let (Some(_enc), Some(pubkey)) =
        (&device.encrypted_kyber_private_key, &device.kyber_public_key)
        && pubkey.len() == omnidrive_core::pqkem::ML_KEM_768_ENCAPS_KEY_LEN
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
```

- [ ] **Step 8: Run it, verify it PASSES**

Run: `cargo test -p angeld --lib ensure_device_kyber_keypair_generates_persists_and_is_idempotent`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add angeld/src/identity.rs
git commit -m "feat(identity): α.B.b ensure_device_kyber_keypair + variable-length seal"
```

---

## Final verification (after Task 1)

- [ ] **Full workspace gate** (mirrors the pre-push hook):

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features test-helpers -- -D warnings
cargo build --release --workspace
cargo test -p omnidrive-core
cargo test -p angeld --lib
```
Expected: all green. New tests live in `omnidrive-core` (`pqkem`) and the `angeld` `db`/`identity` modules → covered by `--lib`.

- [ ] **DoD confirmation (α.B.b.1 Rust gate):**
  - `generate_produces_correct_sizes` / `two_keypairs_differ` — keygen yields 1184/2400-byte keys, fresh each call.
  - `store_and_read_kyber_keypair` — additive columns persist and read back.
  - `seal_open_secret_blob_roundtrip_variable_length` — 2400-byte secret seals/opens; wrong KEK fails.
  - `ensure_device_kyber_keypair_generates_persists_and_is_idempotent` — keygen persists, is idempotent, decaps unseals to 2400 bytes.
- [ ] **No version bump.** Push (pre-push hook active, never `--no-verify`).

---

## Task 2 (α.B.b.2): Hybrid wrap/unwrap crypto (pure `omnidrive-core`)

**Scope:** `omnidrive-core` only — ML-KEM encapsulate/decapsulate byte wrappers (in `pqkem`), and a new `hybrid` module composing X25519 (shared secret passed in) + ML-KEM + HKDF-SHA256 combiner + AES-256-KW. **No new dependencies** (`ml-kem`, `aes-kw`, `hkdf`, `sha2`, `zeroize`, `rand` are all already deps). No DB, no `angeld` changes, no x25519 ECDH (the caller computes the X25519 shared secret in Task 3 and passes it in — keeps `omnidrive-core` free of `x25519-dalek`).

### Key design points (read before implementing)

- **"AAD" without an AEAD.** AES-KW (RFC 3394) has **no AAD parameter**. The anti-splice / anti-downgrade binding (spec D4: `version | vault_id | device_id`) is realized by folding those fields — plus the kyber ciphertext and the peer encapsulation key (X-Wing anti-rebinding) — into the **HKDF `info`** that derives the wrapping KEK. Tamper any of them ⇒ different KEK ⇒ AES-KW unwrap integrity failure. This is stronger than a bolted-on AAD and needs no AEAD.
- **Combiner = HKDF-SHA256 (X-Wing pattern), never XOR.** `KEK = HKDF-SHA256(salt = "omnidrive-hybrid-wrap-v1", ikm = x25519_ss ‖ mlkem_ss, info = transcript)`. The transcript is **length-prefixed** (`u32-BE len ‖ bytes` per field) so concatenation is unambiguous (canonical encoding).
- **Wrapped blob layout:** `kyber_ct (1088) ‖ aes_kw_wrapped_vk (40)` = **1128 bytes**. No separate version byte in the blob — the version lives in the KEK transcript (downgrade-resistant); the storage-level `v2`/`v3` column discriminator is Task 3.
- **Implicit rejection caveat:** ML-KEM decapsulation of a tampered ciphertext does **not** error (FIPS 203 returns a pseudo-random shared secret). The tamper-fail tests therefore pass via the **AES-KW** integrity check downstream, not a decaps error — this is expected and correct.
- **Zeroize** every transient shared-secret / KEK / IKM buffer (`zeroize` is a core dep; `KeyBytes` is already `ZeroizeOnDrop`).

> **Implementer note (`ml-kem` 0.2.3 API):** the exact paths for the `Encapsulate`/`Decapsulate` traits (`ml_kem::kem::{Encapsulate, Decapsulate}` vs crate root), the `Encoded<_>` alias, and the `Ciphertext<MlKem768>` type may differ slightly from the snippets below. Verify against `cargo doc -p ml-kem --no-deps` / docs.rs and adjust the `use` paths and `from_bytes`/`try_from` conversions — but keep every **public function signature and constant** exactly as written. The round-trip + size tests are the compile-and-correctness guard. (Task 1A already confirmed `MlKem768::generate`, `EncodedSizeUser::as_bytes`, and that `Array` derefs to `[u8]`.)

---

### Task 2A — ML-KEM encapsulate / decapsulate byte wrappers

**Files:**
- Modify: `omnidrive-core/src/pqkem.rs`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `pqkem.rs`:

```rust
    #[test]
    fn encapsulate_decapsulate_roundtrip_same_secret() {
        let (ek, dk) = generate_ml_kem_768_keypair();
        let (ct, ss_enc) = ml_kem_encapsulate(&ek).unwrap();
        assert_eq!(ct.len(), ML_KEM_768_CIPHERTEXT_LEN);
        assert_eq!(ss_enc.len(), ML_KEM_768_SHARED_SECRET_LEN);
        let ss_dec = ml_kem_decapsulate(&dk, &ct).unwrap();
        assert_eq!(ss_enc, ss_dec, "encaps and decaps must agree on the shared secret");
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
```

- [ ] **Step 2: Run, expect FAIL (undefined):** `cargo test -p omnidrive-core pqkem`

- [ ] **Step 3: Implement.** Extend the `use` at the top of `pqkem.rs` and add the items above the test module:

```rust
use ml_kem::kem::{Decapsulate, Encapsulate};
use ml_kem::{Ciphertext, Encoded};

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
            Self::InvalidEncapsKeyLen(n) => write!(f, "invalid ML-KEM encapsulation key length: {n}"),
            Self::InvalidDecapsKeyLen(n) => write!(f, "invalid ML-KEM decapsulation key length: {n}"),
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
    // ML-KEM encapsulation is infallible for a well-formed key.
    let (ct, ss) = ek.encapsulate(&mut rng).expect("ML-KEM encapsulation");
    let mut ss_bytes = [0u8; 32];
    ss_bytes.copy_from_slice(&ss);
    Ok((ct.to_vec(), ss_bytes))
}

/// Decapsulates a ciphertext with the local ML-KEM-768 decapsulation key.
/// Returns the 32-byte shared secret. Per FIPS 203 a tampered ciphertext does
/// not error here — it yields a pseudo-random secret (implicit rejection); the
/// caller detects tampering downstream (AES-KW integrity).
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
```

> `ss` (the ML-KEM shared key) derefs to `[u8]`, so `copy_from_slice(&ss)` works. `ct.to_vec()` works because the ciphertext array also derefs to `[u8]`.

- [ ] **Step 4: Run, expect PASS:** `cargo test -p omnidrive-core pqkem` (now 5 tests). `cargo fmt --all`.

- [ ] **Step 5: Commit (do NOT push):**

```bash
git add omnidrive-core/src/pqkem.rs
git commit -m "feat(core): α.B.b ML-KEM-768 encapsulate/decapsulate byte wrappers"
```

---

### Task 2B — Hybrid combiner + wrap/unwrap

**Files:**
- Create: `omnidrive-core/src/hybrid.rs`
- Modify: `omnidrive-core/src/lib.rs` (add `pub mod hybrid;`)

- [ ] **Step 1: Write the failing tests FIRST.** Create `omnidrive-core/src/hybrid.rs` with ONLY this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyBytes;
    use crate::pqkem::generate_ml_kem_768_keypair;

    fn ctx() -> HybridWrapContext<'static> {
        HybridWrapContext { version: HYBRID_WRAP_VERSION, vault_id: "vault-1", device_id: "dev-1" }
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
        blob[0] ^= 0xFF; // flip in the kyber_ct region
        assert!(hybrid_unwrap_vault_key(&x_ss, &dk, &ek, &blob, &ctx()).is_err());
    }

    #[test]
    fn unwrap_rejects_tampered_wrapped_key() {
        let (ek, dk) = generate_ml_kem_768_keypair();
        let x_ss = [0x07u8; 32];
        let vk = KeyBytes::from([0x33u8; 32]);
        let mut blob = hybrid_wrap_vault_key(&x_ss, &ek, &vk, &ctx()).unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0xFF; // flip in the AES-KW wrapped region
        assert!(hybrid_unwrap_vault_key(&x_ss, &dk, &ek, &blob, &ctx()).is_err());
    }

    #[test]
    fn unwrap_rejects_downgraded_version() {
        let (ek, dk) = generate_ml_kem_768_keypair();
        let x_ss = [0x07u8; 32];
        let vk = KeyBytes::from([0x33u8; 32]);
        let blob = hybrid_wrap_vault_key(&x_ss, &ek, &vk, &ctx()).unwrap();
        let bad = HybridWrapContext { version: "v2-x25519", vault_id: "vault-1", device_id: "dev-1" };
        assert!(hybrid_unwrap_vault_key(&x_ss, &dk, &ek, &blob, &bad).is_err());
    }

    #[test]
    fn unwrap_rejects_wrong_device_id() {
        let (ek, dk) = generate_ml_kem_768_keypair();
        let x_ss = [0x07u8; 32];
        let vk = KeyBytes::from([0x33u8; 32]);
        let blob = hybrid_wrap_vault_key(&x_ss, &ek, &vk, &ctx()).unwrap();
        let bad = HybridWrapContext { version: HYBRID_WRAP_VERSION, vault_id: "vault-1", device_id: "dev-OTHER" };
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
```

- [ ] **Step 2: Run, expect FAIL (undefined):** `cargo test -p omnidrive-core hybrid`

- [ ] **Step 3: Implement the module body.** Prepend above the `#[cfg(test)]` block in `hybrid.rs`:

```rust
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
    fn from(e: PqKemError) -> Self { Self::PqKem(e) }
}
impl From<CryptoError> for HybridError {
    fn from(e: CryptoError) -> Self { Self::Crypto(e) }
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
```

- [ ] **Step 4: Register the module.** Add `pub mod hybrid;` to `omnidrive-core/src/lib.rs`.

- [ ] **Step 5: Run, expect PASS:** `cargo test -p omnidrive-core hybrid` (7 tests). `cargo fmt --all`.

- [ ] **Step 6: Commit (do NOT push):**

```bash
git add omnidrive-core/src/hybrid.rs omnidrive-core/src/lib.rs
git commit -m "feat(core): α.B.b hybrid X25519+ML-KEM vault-key wrap/unwrap"
```

---

### Final verification (after Task 2)

- [ ] **Workspace gate** (mirrors pre-push):

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features test-helpers -- -D warnings
cargo build --release --workspace
cargo test -p omnidrive-core
```
Expected: all green. Watch for any new `AsRef`/`Array` ambiguity surfaced by the wider build (as in Task 1's `as_slice()` fix).

- [ ] **DoD confirmation (α.B.b.2 Rust gate):**
  - `encapsulate_decapsulate_roundtrip_same_secret` — encaps/decaps agree (1088 B ct, 32 B ss).
  - `wrap_unwrap_roundtrip` — VK survives hybrid wrap → unwrap (1128-byte blob).
  - tamper-fail set — flipped ciphertext, flipped wrapped key, downgraded version, wrong device_id, wrong x25519 secret, wrong decaps key all reject.
- [ ] **No version bump.** Push (pre-push active, never `--no-verify`).

---

## Task 3 (α.B.b.3): Integration — keygen wiring, hybrid wrap at accept, unwrap selection, e2e

**Scope:** `angeld` only. Wire kyber keygen into post-unlock maintenance; add identity-layer bridges that compute the X25519 shared secret (ECDH stays in `angeld`) and delegate to the core `hybrid` primitives; populate `devices.wrapped_vault_key_kyber` at `accept_device`; add an unwrap selection helper (prefer v3-hybrid, fall back to v2-x25519). DoD = an in-process e2e proving both wraps decrypt to the same Vault Key.

### Context (grounded against current code)
- `vault::run_post_unlock_maintenance` (`vault.rs:849`) runs KDF migration → `identity::ensure_device_keypair` (X25519). We append the kyber keygen here.
- `identity::wrap_vault_key_for_device` / `unwrap_vault_key_from_device` (`identity.rs:265`, `:289`) do X25519 ECDH + AES-KW. The hybrid bridges mirror them: ECDH → `x25519_ss`, then call `omnidrive_core::hybrid::{hybrid_wrap_vault_key, hybrid_unwrap_vault_key}`.
- `api/vault.rs::post_accept_device` (`:334`) wraps the VK for a target device (X25519) and calls `db::set_device_wrapped_vault_key` (`:420`). We add the hybrid wrap alongside it, non-fatally.
- `db::DeviceRecord` (`db.rs:536`) + three `query_as::<_, DeviceRecord>` SELECTs (`db.rs:7306`, `:7345`, `:7377`) need the two kyber columns added (additive; the columns already exist in the schema from Task 1B).
- `identity::open_secret_blob` (Task 1C) unseals the kyber decapsulation key; `db::get_local_device_identity` returns `kyber_public_key` + `encrypted_kyber_private_key` (Task 1B).

---

### Task 3A — wire kyber keygen into post-unlock maintenance

**Files:** Modify `angeld/src/vault.rs` (`run_post_unlock_maintenance`, `:865-870`); test in `vault.rs mod tests`.

- [ ] **Step 1: Write the failing test.** Add to `vault.rs mod tests`:

```rust
    #[tokio::test]
    async fn post_unlock_maintenance_ensures_both_keypairs() -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        let store = VaultKeyStore::new();
        store.unlock(&pool, "pass-123").await?;
        db::upsert_local_device_identity(&pool, "dev-1", "PC", "tok").await?;

        store.run_post_unlock_maintenance(&pool, "pass-123").await?;

        let dev = db::get_local_device_identity(&pool).await?.unwrap();
        assert!(dev.public_key.is_some(), "X25519 pubkey present");
        assert_eq!(
            dev.kyber_public_key.as_deref().map(<[u8]>::len),
            Some(omnidrive_core::pqkem::ML_KEM_768_ENCAPS_KEY_LEN),
            "kyber encaps key generated and persisted"
        );
        assert!(dev.encrypted_kyber_private_key.is_some(), "kyber decaps sealed");
        Ok(())
    }
```

- [ ] **Step 2: Run, expect FAIL:** `cargo test -p angeld --lib post_unlock_maintenance_ensures_both_keypairs` (kyber columns stay NULL — keygen not wired).

- [ ] **Step 3: Implement.** In `run_post_unlock_maintenance`, immediately after the existing X25519 `match` block (`vault.rs:866-869`), add:

```rust
        match crate::identity::ensure_device_kyber_keypair(pool, master_key.as_ref()).await {
            Ok(_) => info!("[DEVICE-KEY] ML-KEM keypair ensured for local device"),
            Err(e) => {
                warn!("[DEVICE-KEY] kyber keypair generation failed (will retry next unlock): {e}")
            }
        }
```

- [ ] **Step 4: Run, expect PASS.** `cargo fmt --all`.

- [ ] **Step 5: Commit:** `git commit -am "feat(vault): α.B.b ensure ML-KEM keypair in post-unlock maintenance"`

---

### Task 3B — DeviceRecord kyber columns + wrapped-kyber setter

**Files:** Modify `angeld/src/db.rs` (struct `:536`; SELECTs `:7306`, `:7345`, `:7377`; new setter after `set_device_wrapped_vault_key` `:7354`); test in `db.rs mod tests`.

- [ ] **Step 1: Write the failing test.** Add to `db.rs mod tests`:

```rust
    #[tokio::test]
    async fn set_and_read_device_wrapped_kyber() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        let vault = get_vault_params(&pool).await.unwrap().unwrap();
        migrate_single_to_multi_user(&pool, &vault.vault_id).await.unwrap();
        let user = list_users(&pool).await.unwrap().pop().unwrap();
        upsert_device(&pool, "dev-x", &user.user_id, "PC", &[9u8; 32], 1).await.unwrap();

        let kyber_pub = vec![0x22u8; 1184];
        let wrapped_kyber = vec![0x44u8; 1128];
        set_device_kyber_public_key(&pool, "dev-x", &kyber_pub).await.unwrap();
        set_device_wrapped_vault_key_kyber(&pool, "dev-x", &wrapped_kyber).await.unwrap();

        let dev = get_device(&pool, "dev-x").await.unwrap().unwrap();
        assert_eq!(dev.kyber_public_key.as_deref(), Some(kyber_pub.as_slice()));
        assert_eq!(dev.wrapped_vault_key_kyber.as_deref(), Some(wrapped_kyber.as_slice()));
    }
```

> If `upsert_device` / `list_users` / `migrate_single_to_multi_user` signatures differ, mirror an existing `db.rs` device test (e.g. near `set_device_wrapped_vault_key` tests in `identity.rs:657`) for the exact setup calls. The assertion target is what matters: the two kyber columns round-trip through `DeviceRecord`.

- [ ] **Step 2: Run, expect FAIL (compile):** `cargo test -p angeld --lib set_and_read_device_wrapped_kyber` — `wrapped_vault_key_kyber` field and `set_device_wrapped_vault_key_kyber` do not exist.

- [ ] **Step 3: Extend `DeviceRecord`.** After `pub enrolled_at: Option<i64>,` (`db.rs:546`) add:

```rust
    pub kyber_public_key: Option<Vec<u8>>,
    pub wrapped_vault_key_kyber: Option<Vec<u8>>,
```

- [ ] **Step 4: Extend all three SELECTs.** In each of the three `query_as::<_, DeviceRecord>` queries (`db.rs:7306`, `:7345`, `:7377`), append the two columns to the column list (before `FROM devices`), e.g.:

```rust
        "SELECT device_id, user_id, device_name, public_key, wrapped_vault_key, \
         vault_key_generation, revoked_at, last_seen_at, created_at, enrolled_at, \
         kyber_public_key, wrapped_vault_key_kyber \
         FROM devices WHERE device_id = ?",
```
(apply the same column addition to the `WHERE user_id = ?` and `WHERE user_id = ? AND revoked_at IS NULL …` variants — the `FROM`/`WHERE`/`ORDER BY` tails stay unchanged).

- [ ] **Step 5: Add the setter.** After `set_device_wrapped_vault_key` (`db.rs` ~`:7370`) add:

```rust
pub async fn set_device_wrapped_vault_key_kyber(
    pool: &SqlitePool,
    device_id: &str,
    wrapped_vault_key_kyber: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devices SET wrapped_vault_key_kyber = ? WHERE device_id = ?")
        .bind(wrapped_vault_key_kyber)
        .bind(device_id)
        .execute(pool)
        .await?;
    Ok(())
}
```

> Note: the `revoke_device` path (`db.rs:7390`) NULLs `wrapped_vault_key` on revoke. For α.B.b.3 we do **not** extend revoke to also NULL `wrapped_vault_key_kyber` — flag it as a follow-up in the report (revoked devices already lose the X25519 wrap; the kyber wrap is dead without the X25519 path being usable, but a tidy revoke should clear both — out of scope here to keep the change surgical).

- [ ] **Step 6: Run, expect PASS.** `cargo fmt --all`.

- [ ] **Step 7: Commit:** `git commit -am "feat(db): α.B.b DeviceRecord kyber columns + set_device_wrapped_vault_key_kyber"`

---

### Task 3C — identity bridges (ECDH + core hybrid) + kyber private reader + selection

**Files:** Modify `angeld/src/identity.rs` (after `unwrap_vault_key_from_device` `:306`); tests in `identity.rs mod tests`.

- [ ] **Step 1: Write the failing tests.** Add to `identity.rs mod tests`:

```rust
    #[test]
    fn hybrid_wrap_unwrap_for_device_roundtrip() {
        let (ek, dk) = omnidrive_core::pqkem::generate_ml_kem_768_keypair();
        let owner_priv = [0x11u8; 32];
        let owner_pub = x25519_dalek::PublicKey::from(
            &x25519_dalek::StaticSecret::from(owner_priv),
        )
        .to_bytes();
        let vault_key = KeyBytes::from([0x33u8; 32]);

        let blob = hybrid_wrap_vault_key_for_device(
            &owner_priv, &owner_pub, &ek, &vault_key, "vault-1", "dev-1",
        )
        .unwrap();
        let out = hybrid_unwrap_vault_key_from_device(
            &owner_priv, &owner_pub, &dk, &ek, &blob, "vault-1", "dev-1",
        )
        .unwrap();
        assert_eq!(out, vault_key);
    }

    #[test]
    fn hybrid_unwrap_for_device_rejects_wrong_vault_id() {
        let (ek, dk) = omnidrive_core::pqkem::generate_ml_kem_768_keypair();
        let owner_priv = [0x11u8; 32];
        let owner_pub = x25519_dalek::PublicKey::from(
            &x25519_dalek::StaticSecret::from(owner_priv),
        )
        .to_bytes();
        let vault_key = KeyBytes::from([0x33u8; 32]);
        let blob = hybrid_wrap_vault_key_for_device(
            &owner_priv, &owner_pub, &ek, &vault_key, "vault-1", "dev-1",
        )
        .unwrap();
        assert!(
            hybrid_unwrap_vault_key_from_device(
                &owner_priv, &owner_pub, &dk, &ek, &blob, "vault-OTHER", "dev-1",
            )
            .is_err()
        );
    }

    #[test]
    fn selection_prefers_hybrid_then_falls_back() {
        let (ek, dk) = omnidrive_core::pqkem::generate_ml_kem_768_keypair();
        let owner_priv = [0x11u8; 32];
        let owner_pub = x25519_dalek::PublicKey::from(
            &x25519_dalek::StaticSecret::from(owner_priv),
        )
        .to_bytes();
        let vk = KeyBytes::from([0x33u8; 32]);

        let wrapped_x = wrap_vault_key_for_device(&owner_priv, &owner_pub, &vk).unwrap();
        let wrapped_h = hybrid_wrap_vault_key_for_device(
            &owner_priv, &owner_pub, &ek, &vk, "vault-1", "dev-1",
        )
        .unwrap();

        // Hybrid available → uses hybrid.
        let via_hybrid = select_and_unwrap_vault_key(
            &owner_priv, &owner_pub, Some(&dk), Some(&ek),
            &wrapped_x, Some(&wrapped_h), "vault-1", "dev-1",
        )
        .unwrap();
        assert_eq!(via_hybrid, vk);

        // No hybrid → falls back to X25519.
        let via_x25519 = select_and_unwrap_vault_key(
            &owner_priv, &owner_pub, Some(&dk), Some(&ek),
            &wrapped_x, None, "vault-1", "dev-1",
        )
        .unwrap();
        assert_eq!(via_x25519, vk);
    }
```

- [ ] **Step 2: Run, expect FAIL (undefined):** `cargo test -p angeld --lib hybrid_wrap_unwrap_for_device_roundtrip`

- [ ] **Step 3: Implement.** Add `use omnidrive_core::hybrid::{HybridWrapContext, HYBRID_WRAP_VERSION, hybrid_unwrap_vault_key, hybrid_wrap_vault_key};` to the top imports of `identity.rs`. After `unwrap_vault_key_from_device` (`identity.rs:306`) add:

```rust
/// Wraps a Vault Key for a target device under the hybrid X25519 + ML-KEM scheme.
/// Computes the X25519 shared secret here (ECDH stays in `angeld`) and delegates the
/// combiner + AES-KW to `omnidrive_core::hybrid`. Returns `kyber_ct ‖ wrapped` (1128 B).
pub fn hybrid_wrap_vault_key_for_device(
    my_private_key: &[u8; 32],
    their_public_key: &[u8; 32],
    their_kyber_encaps_key: &[u8],
    vault_key: &KeyBytes,
    vault_id: &str,
    device_id: &str,
) -> Result<Vec<u8>, IdentityError> {
    validate_x25519_pubkey(their_public_key)?;
    let secret = x25519_dalek::StaticSecret::from(*my_private_key);
    let their_pub = x25519_dalek::PublicKey::from(*their_public_key);
    let shared = secret.diffie_hellman(&their_pub);
    if shared.as_bytes() == &[0u8; 32] {
        return Err(IdentityError::Crypto(
            "ECDH produced all-zero shared secret: low-order point attack rejected".into(),
        ));
    }
    let ctx = HybridWrapContext { version: HYBRID_WRAP_VERSION, vault_id, device_id };
    hybrid_wrap_vault_key(shared.as_bytes(), their_kyber_encaps_key, vault_key, &ctx)
        .map_err(|e| IdentityError::Crypto(format!("hybrid wrap: {e}")))
}

/// Unwraps a Vault Key produced by `hybrid_wrap_vault_key_for_device`. `my_kyber_encaps_key`
/// is the recipient's own encapsulation key (bound into the transcript at wrap time).
pub fn hybrid_unwrap_vault_key_from_device(
    my_private_key: &[u8; 32],
    their_public_key: &[u8; 32],
    my_kyber_decaps_key: &[u8],
    my_kyber_encaps_key: &[u8],
    wrapped_blob: &[u8],
    vault_id: &str,
    device_id: &str,
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
    let ctx = HybridWrapContext { version: HYBRID_WRAP_VERSION, vault_id, device_id };
    hybrid_unwrap_vault_key(
        shared.as_bytes(),
        my_kyber_decaps_key,
        my_kyber_encaps_key,
        wrapped_blob,
        &ctx,
    )
    .map_err(|e| IdentityError::Crypto(format!("hybrid unwrap: {e}")))
}

/// Unwraps the device's Vault Key, preferring the post-quantum hybrid wrap when the
/// kyber ciphertext and the local kyber keypair are all present, else the X25519 wrap.
#[allow(clippy::too_many_arguments)]
pub fn select_and_unwrap_vault_key(
    my_private_key: &[u8; 32],
    their_public_key: &[u8; 32],
    my_kyber_decaps_key: Option<&[u8]>,
    my_kyber_encaps_key: Option<&[u8]>,
    wrapped_x25519: &[u8; WRAPPED_KEY_LEN],
    wrapped_hybrid: Option<&[u8]>,
    vault_id: &str,
    device_id: &str,
) -> Result<KeyBytes, IdentityError> {
    if let (Some(blob), Some(dk), Some(ek)) =
        (wrapped_hybrid, my_kyber_decaps_key, my_kyber_encaps_key)
    {
        return hybrid_unwrap_vault_key_from_device(
            my_private_key, their_public_key, dk, ek, blob, vault_id, device_id,
        );
    }
    unwrap_vault_key_from_device(my_private_key, their_public_key, wrapped_x25519)
}

/// Retrieves and unseals the local device's ML-KEM decapsulation key. Requires the
/// vault to be unlocked (needs `master_key` to derive the identity KEK).
pub async fn get_device_kyber_private_key(
    pool: &SqlitePool,
    master_key: &[u8],
) -> Result<Vec<u8>, IdentityError> {
    let device = db::get_local_device_identity(pool)
        .await?
        .ok_or(IdentityError::NoDeviceIdentity)?;
    let encrypted = device
        .encrypted_kyber_private_key
        .ok_or(IdentityError::Crypto("no encrypted kyber private key stored".into()))?;
    let kek = derive_identity_kek(master_key)?;
    open_secret_blob(&kek, &encrypted)
}
```

- [ ] **Step 4: Run, expect PASS** (all 3 tests). `cargo fmt --all`.

- [ ] **Step 5: Commit:** `git commit -am "feat(identity): α.B.b hybrid wrap/unwrap-for-device bridges + selection + kyber private reader"`

---

### Task 3D — produce the hybrid wrap at `accept_device`

**Files:** Modify `angeld/src/api/vault.rs` (`post_accept_device`, after `:424`). No new test here (covered by 3C unit tests + the 3E e2e); the change is additive wiring.

- [ ] **Step 1: Implement.** In `post_accept_device`, immediately after the `db::set_device_wrapped_vault_key(...)?` call (`api/vault.rs:420-424`), add a non-fatal hybrid wrap (only when the target has published a kyber key):

```rust
    if let Some(kyber_ek) = target_device.kyber_public_key.as_deref() {
        match identity::hybrid_wrap_vault_key_for_device(
            &owner_private,
            &member_pubkey,
            kyber_ek,
            &envelope_key,
            &vault_id,
            &target_device_id,
        ) {
            Ok(wrapped_kyber) => {
                if let Err(e) = db::set_device_wrapped_vault_key_kyber(
                    &state.pool,
                    &target_device_id,
                    &wrapped_kyber,
                )
                .await
                {
                    warn!("hybrid wrap persist failed for {target_device_id}: {e}");
                }
            }
            Err(e) => warn!("hybrid wrap failed for {target_device_id} (X25519 wrap still applied): {e}"),
        }
    }
```

> `warn!` is already imported in `api/vault.rs` (it logs elsewhere); if not, use `tracing::warn!`. `envelope_key` is the same `&KeyBytes` already passed to the X25519 wrap. `target_device.kyber_public_key` is now available via the 3B `DeviceRecord` extension. The hybrid wrap is **best-effort**: a device that has not yet published a kyber key (pre-α.B.b) still gets its X25519 wrap and is unaffected.

- [ ] **Step 2: Build check:** `cargo build -p angeld` (no panic paths added). `cargo fmt --all`.

- [ ] **Step 3: Commit:** `git commit -am "feat(api): α.B.b emit hybrid wrapped vault key at accept_device"`

---

### Task 3E — DoD e2e: solo vault, both wraps decrypt to the same VK

**Files:** Test only — `angeld/src/identity.rs mod tests` (it has `db`, `vault`, and `x25519_dalek` in scope).

- [ ] **Step 1: Write the e2e test.** Add to `identity.rs mod tests`:

```rust
    #[tokio::test]
    async fn e2e_solo_vault_both_wraps_decrypt_to_same_vault_key()
    -> Result<(), Box<dyn std::error::Error>> {
        use secrecy::ExposeSecret;

        let pool = db::init_db("sqlite::memory:").await?;
        let store = crate::vault::VaultKeyStore::new();
        store.unlock(&pool, "pass-123").await?;
        db::upsert_local_device_identity(&pool, "dev-solo", "PC", "tok").await?;

        // Generates both X25519 and ML-KEM keypairs for the local device.
        store.run_post_unlock_maintenance(&pool, "pass-123").await?;

        let master = store.require_master_key().await?;
        let envelope = store.require_envelope_key().await?;
        let vk = KeyBytes::from(<[u8; 32]>::try_from(envelope.expose_secret().as_ref())?);

        let device = db::get_local_device_identity(&pool).await?.unwrap();
        let mut owner_pub = [0u8; 32];
        owner_pub.copy_from_slice(device.public_key.as_ref().unwrap());
        let kyber_ek = device.kyber_public_key.unwrap();

        let owner_priv = get_device_private_key(&pool, master.as_ref()).await?;
        let kyber_dk = get_device_kyber_private_key(&pool, master.as_ref()).await?;

        // Solo vault wraps the VK for itself under BOTH schemes.
        let wrapped_x = wrap_vault_key_for_device(&owner_priv, &owner_pub, &vk)?;
        let wrapped_h = hybrid_wrap_vault_key_for_device(
            &owner_priv, &owner_pub, &kyber_ek, &vk, "vault-solo", "dev-solo",
        )?;

        let vk_x = unwrap_vault_key_from_device(&owner_priv, &owner_pub, &wrapped_x)?;
        let vk_h = hybrid_unwrap_vault_key_from_device(
            &owner_priv, &owner_pub, &kyber_dk, &kyber_ek, &wrapped_h, "vault-solo", "dev-solo",
        )?;

        assert_eq!(vk_x, vk, "X25519 wrap must decrypt to the Vault Key");
        assert_eq!(vk_h, vk, "hybrid wrap must decrypt to the Vault Key");
        assert_eq!(vk_x, vk_h, "both wraps must yield the identical Vault Key");
        Ok(())
    }
```

> `require_envelope_key()` returns the VK guard (same accessor `post_accept_device` uses). If `expose_secret().as_ref()` typing differs, mirror the α.C.b round-trip test (`db.rs`) which does `require_envelope_key().await?.to_vec()` — then `KeyBytes::from(<[u8;32]>::try_from(&vec[..])?)`. Keep the three assertions: both unwraps equal `vk` and equal each other.

- [ ] **Step 2: Run, expect PASS:** `cargo test -p angeld --lib e2e_solo_vault_both_wraps_decrypt_to_same_vault_key`

- [ ] **Step 3: Commit:** `git commit -am "test(identity): α.B.b e2e solo vault — X25519 + hybrid wraps decrypt to same VK"`

---

### Final verification (after Task 3 — closes α.B.b)

- [ ] **Full workspace gate** (mirrors pre-push, all `--all-targets`):

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features test-helpers -- -D warnings
cargo build --release --workspace
cargo test -p omnidrive-core
cargo test -p angeld --lib
```
Expected: all green.

- [ ] **DoD confirmation (phase α.B.b):**
  - `post_unlock_maintenance_ensures_both_keypairs` — kyber keypair generated on unlock.
  - `set_and_read_device_wrapped_kyber` — `wrapped_vault_key_kyber` persists/reads.
  - `hybrid_wrap_unwrap_for_device_roundtrip` + `…rejects_wrong_vault_id` — identity bridge correct + context-bound.
  - `selection_prefers_hybrid_then_falls_back` — unwrap selection (v3 preferred, v2 fallback).
  - **`e2e_solo_vault_both_wraps_decrypt_to_same_vault_key` — the phase DoD: both ciphertexts decrypt to the identical Vault Key.**
- [ ] **No version bump.** Push (pre-push active, never `--no-verify`). Then mark `STATUS.md` §12.5 α.B.b → DONE + tree marker → α.D.a.

**Live SMOKE (separate, does NOT gate code DONE):** real Dell↔Lenovo enroll → joining device unwraps the hybrid-wrapped VK. Requires both machines; run after the Rust gate is green.

---

## Out of scope (do NOT do here)

- Solo-unlock changes (passphrase → KEK → envelope stays as-is).
- Snapshot creation/upload/encryption changes.
- Mobile / WebCrypto.
- Version bump (after phase DoD + optional smoke).
- Widening or overwriting the X25519 `public_key` / `encrypted_private_key` columns or their helpers.
