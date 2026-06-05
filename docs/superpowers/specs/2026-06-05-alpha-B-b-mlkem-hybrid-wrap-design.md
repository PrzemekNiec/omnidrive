# α.B.b — ML-KEM-768 Hybrid Vault-Key Wrap (Design / Spec)

> **Status:** ratified design (Przemek 2026-06-04, pre-reprioritization) + grounded against current code 2026-06-05.
> **Plan:** `docs/superpowers/plans/2026-06-05-alpha-B-b-mlkem-hybrid-wrap.md`
> **Phase position:** α.C.a (real X25519 keypair) ✅ + α.C.b (graft identity bundle) ✅ → **α.B.b** → α.D.a (QG5 review).

---

## 1. Problem & Goal

OmniDrive distributes the **Vault Key (VK)** to a new device by wrapping it for that device's public key (Epic 33 multi-device key exchange). Today that wrap is **X25519 ECDH + AES-KW** only (`identity::wrap_vault_key_for_device`). X25519 is vulnerable to *harvest-now-decrypt-later*: an adversary recording the wrapped VK today can recover it once a cryptographically-relevant quantum computer exists.

**Goal:** add a **hybrid** wrap — X25519 **AND** ML-KEM-768 (NIST FIPS 203) **together**, never as a replacement. A device wraps the VK under both; unwrap succeeds if the classical path holds today and remains secure against future quantum attack on the PQ path. For a solo vault (one device, one user) the device wraps the VK *for itself* under both methods, and both ciphertexts must decrypt to the identical VK.

**Non-goal:** α.B.b does **not** touch solo unlock. Solo unlock stays `passphrase → Argon2id → KEK → unwrap envelope` (α.B.a). α.B.b is the device-to-device VK-sharing layer (chicken-and-egg trap — keep the two paths disjoint).

---

## 2. Ratified architectural decisions

| # | Decision | Rationale |
|---|----------|-----------|
| D1 | **Crate `ml-kem = "0.2"` (RustCrypto, pure-Rust, audited).** NOT `pqcrypto-kyber` (C FFI, deprecated). | Whole workspace is pure-Rust RustCrypto (`x25519-dalek`, `aes-gcm`, `aes-kw`, `argon2`, `hkdf`). Consistency, no C toolchain. |
| D2 | **Combiner = X-Wing pattern** (X25519 + ML-KEM-768, published hybrid KEM with proof). If the `x-wing` crate is immature → HKDF-SHA256 following the X-Wing transcript. **NEVER XOR.** | Proven IND-CCA hybrid; `hkdf`+`sha2` already in `omnidrive-core`, zero new dep for the fallback. |
| D3 | **Additive only — overwrite nothing.** X25519 columns/keys from α.C.a stay untouched. ML-KEM lives in NEW columns. | Rejected Gemini idea of widening `public_key`/`encrypted_private_key` to 1184 B — would destroy α.C.a and conflate device identity with KEM-wrap. SQLite BLOB is dynamically typed (no fixed width). |
| D4 | **Format discriminator + AAD.** Version tag (`v2-x25519` / `v3-hybrid`); AAD binds `vault_id` + `device_id` + version (anti-splice / anti-downgrade). | Dovetails with α.D.a crypto-spec §12. |
| D5 | **Unwrap = X25519-default → ML-KEM-failover.** ML-KEM keygen attaches to the **same** post-unlock identity maintenance as α.C.a's X25519 keygen (idempotent). | Backward-compatible; one key-management surface. |
| D6 | **Sub-tasks:** B.b.1 = deps + additive schema + keygen; B.b.2 = pure crypto in `omnidrive-core` (hybrid wrap/unwrap, round-trip + tamper-fail tests); B.b.3 = integration + e2e (both ciphertexts → same VK). | Mirrors α.B.a / α.C.a / α.C.b cadence. |
| D7 | **Egress ≈ +2.3 KB/device** in snapshot/B2. Non-blocking; note for mobile. WebCrypto has no ML-KEM (crypto lives in Rust/UniFFI, not the browser). | |

### Confirmed at this checkpoint (2026-06-05)

- **D-keygen-loc:** ML-KEM keygen + the `ml-kem` dependency live **exclusively in `omnidrive-core`** (`pqkem` module). `angeld::identity` calls the core helper. Crypto stays centralized.
- **D-persist:** dedicated `db::store_kyber_keypair` (+ `db::set_device_kyber_public_key`). **Zero changes** to the X25519 `store_device_keypair` / `set_device_public_key` signatures or call-sites.

### Refinement of D5 discovered against current code

`identity::ensure_device_keypair` (`identity.rs:112-118`) **early-returns** once the X25519 keypair exists. Embedding ML-KEM keygen *inside* it would skip kyber backfill for existing α.C.a devices (they already have X25519). **Therefore:** ML-KEM keygen is a **sibling** function `ensure_device_kyber_keypair`, idempotent on its own kyber columns, called from the same post-unlock maintenance sequence (`vault::run_post_unlock_maintenance`) *alongside* the X25519 one. This preserves the X25519 hot-path early-return and enables kyber backfill. (Wiring into the live unlock path is **B.b.3**; B.b.1 delivers and directly tests the function.)

---

## 3. Key sizes (ML-KEM-768, FIPS 203)

| Artifact | Bytes |
|----------|-------|
| Encapsulation key (public) | 1184 |
| Decapsulation key (secret) | 2400 |
| Ciphertext | 1088 |
| Shared secret | 32 |

Sealed decapsulation-key blob at rest = `nonce(12) || AES-256-GCM(ct+tag)` = `12 + 2400 + 16 = 2428` bytes.

---

## 4. Data model (additive)

**`local_device_identity`** (mirrors the X25519 `encrypted_private_key` / `public_key` pair):
- `+ encrypted_kyber_private_key BLOB` — sealed ML-KEM decapsulation key (2428 B), local device only.
- `+ kyber_public_key BLOB` — ML-KEM encapsulation key (1184 B), local device's own copy.

**`devices`** (vault roster):
- `+ kyber_public_key BLOB` — peer device's encapsulation key (NULL until that device enrolls a kyber key).
- `+ wrapped_vault_key_kyber BLOB` — VK wrapped via the PQ path (NULL until B.b.2 populates it).

Migration uses the established idempotent patterns: `ALTER TABLE … ADD COLUMN … BLOB` (wrapped in `let _ =`) for `local_device_identity` (`db.rs:744-749` style) and `ensure_column_exists(...)` for `devices` (`db.rs:1151-1154` style).

> **Why store the local encap key separately** even though FIPS-203 decap keys embed the encap key: avoids re-parsing the secret to read the public part, and mirrors the existing X25519 storage exactly. Cheap (1184 B), consistent, and keeps the secret blob opaque.

---

## 5. Key management at rest

The ML-KEM decapsulation key is sealed identically to the X25519 private key: **AES-256-GCM under the identity KEK** = `HKDF-SHA256(master_key, info="omnidrive-identity-kek-v1")` (`identity::derive_identity_kek`, reused). Random 12-byte nonce per seal. Reusing the same KEK for two different plaintexts (X25519 priv, ML-KEM decap) under GCM with independent random nonces is safe and standard.

The existing `encrypt_private_key`/`decrypt_private_key` are **hard-locked to 32-byte keys** (signature `&[u8; 32]`, asserts `plaintext.len() == 32`). The 2400-byte decap key needs **variable-length** seal/open helpers: new `seal_secret_blob(kek, &[u8]) -> Vec<u8>` / `open_secret_blob(kek, &[u8]) -> Vec<u8>` in `identity.rs` (same `nonce || ct` scheme, no length lock). The X25519 32-byte helpers are left untouched (surgical).

---

## 6. Crypto contract (informative — implemented across B.b.1 → B.b.3)

```
Wrap (B.b.2, source device wrapping VK for target device):
  x25519_ss  = ECDH(my_x25519_priv, their_x25519_pub)
  (kyber_ct, kyber_ss) = ML-KEM-768.Encapsulate(their_kyber_pub)
  KEK_hybrid = X-Wing-combine(x25519_ss, kyber_ss, transcript)        # HKDF-SHA256, never XOR
  wrapped    = AES-256-KW(KEK_hybrid, VK)                             # AAD binds vault_id|device_id|"v3-hybrid"
  store: devices.wrapped_vault_key_kyber = kyber_ct || wrapped

Unwrap (B.b.2, target device):
  x25519_ss  = ECDH(my_x25519_priv, their_x25519_pub)
  kyber_ss   = ML-KEM-768.Decapsulate(my_kyber_priv, kyber_ct)
  KEK_hybrid = X-Wing-combine(x25519_ss, kyber_ss, transcript)
  VK         = AES-256-KW-Unwrap(KEK_hybrid, wrapped)

Selection (B.b.3): try X25519-only (v2) first; if a v3-hybrid wrap is present, prefer it.
```

B.b.1 builds **only** keygen + storage. No encapsulation, no combiner, no AES-KW of the VK.

---

## 7. Scope boundaries

**In α.B.b.1 (this checkpoint's detailed task):**
1. `ml-kem = "0.2"` dependency in `omnidrive-core`.
2. `omnidrive-core::pqkem` — `generate_ml_kem_768_keypair()` + size constants + tests.
3. Additive schema (2 `local_device_identity` cols + 2 `devices` cols) + struct fields + SELECT.
4. `db::store_kyber_keypair` + `db::set_device_kyber_public_key`.
5. `identity`: variable-length `seal_secret_blob`/`open_secret_blob` + `ensure_device_kyber_keypair` (idempotent, sealed under identity KEK, persists, non-fatal `devices` update).

**Deferred to α.B.b.2** (pure crypto, own detailed plan before execution): X-Wing/HKDF combiner, ML-KEM encapsulate/decapsulate wrappers, hybrid `wrap`/`unwrap` of the VK, format discriminator + AAD, round-trip + tamper-fail unit tests in `omnidrive-core`.

**Deferred to α.B.b.3** (integration + e2e): wire `ensure_device_kyber_keypair` into `vault::run_post_unlock_maintenance`; populate `devices.wrapped_vault_key_kyber` at enroll/accept; unwrap selection (X25519-default → hybrid); **DoD e2e: solo vault produces both an X25519 and a hybrid wrap → both decrypt to the same VK.**

**Out of scope for the whole phase:** solo-unlock changes, snapshot upload/encryption changes, mobile/WebCrypto, version bump (after DoD + optional smoke).

---

## 8. Definition of Done (phase)

`STATUS.md §12.5 α.B.b`: e2e test — vault with hybrid wrap → unlock → assert both ciphertexts decrypt to the same VK. Per-step gates: `cargo fmt --all` + `cargo clippy --workspace --all-targets [-- / --features test-helpers --] -D warnings` + `cargo build --release --workspace` + `cargo test`. Pre-push hook active — never `--no-verify`. No version bump until DoD.
