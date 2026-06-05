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

## Task 2 (α.B.b.2) — DEFERRED outline (pure crypto in omnidrive-core)

> **Not yet broken into TDD steps.** Detailed plan (test code + exact signatures) to be written before execution, once Task 1 has landed and the `ml-kem` 0.2 encapsulate/decapsulate + combiner API is pinned against the installed version.

**Scope:** in `omnidrive-core` only — `ml_kem_encapsulate(their_ek) -> (ct, ss)` / `ml_kem_decapsulate(my_dk, ct) -> ss` wrappers; X-Wing-pattern combiner `hybrid_combine(x25519_ss, kyber_ss, transcript) -> KEK` via HKDF-SHA256 (evaluate the `x-wing` crate first; **never XOR**); `hybrid_wrap_vault_key(...) -> kyber_ct || wrapped` and `hybrid_unwrap_vault_key(...) -> VK` using AES-256-KW with AAD binding `vault_id | device_id | "v3-hybrid"`; format discriminator (`v2-x25519` / `v3-hybrid`).

**DoD:** unit round-trip (wrap → unwrap → same VK) + tamper-fail (flipped ciphertext / wrong device / downgraded version all reject). No DB, no integration.

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
