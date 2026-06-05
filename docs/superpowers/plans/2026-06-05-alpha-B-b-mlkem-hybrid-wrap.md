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

## Task 3 (α.B.b.3) — DEFERRED outline (integration + e2e)

> **Not yet broken into TDD steps.** Detailed plan to be written before execution, after Task 2.

**Scope:** wire `identity::ensure_device_kyber_keypair` into `vault::run_post_unlock_maintenance` alongside the X25519 keygen (the α.C.a sequencing point); populate `devices.wrapped_vault_key_kyber` at device enroll/accept; unwrap selection path (try X25519/v2 default → prefer v3-hybrid when present).

**DoD (phase α.B.b):** e2e — a solo vault produces both an X25519 wrap and a hybrid wrap of the VK; both decrypt to the **same** Vault Key. Live SMOKE (real Dell↔Lenovo enroll) is separate acceptance and does not gate code DONE.

---

## Out of scope (do NOT do here)

- Solo-unlock changes (passphrase → KEK → envelope stays as-is).
- Snapshot creation/upload/encryption changes.
- Mobile / WebCrypto.
- Version bump (after phase DoD + optional smoke).
- Widening or overwriting the X25519 `public_key` / `encrypted_private_key` columns or their helpers.
