# α.B.a — Argon2id 2026 Params Bump (Atomic Re-Key on Unlock)

**Status:** RATIFIED 2026-05-25 — proceeding to implementation plan (α.B.a)
**Date:** 2026-05-25
**Phase:** α.B (KDF & wrap upgrades), task `a`
**Scope:** Phase 1 only (atomic, in-memory + DB-metadata key migration). Bulk V1→V2 chunk re-encryption (Phase 2/3) is explicitly out of scope — see [§9 Future Contract](#9-future-contract-αbc).
**Predecessor:** α.A.c (`KeyBytes` `ZeroizeOnDrop`, HEAD `285b913`) — all transient key material here is held in `KeyBytes`/`SecretBox` and zeroized on drop.

---

## 1. Problem & Goal

OmniDrive derives the vault `master_key` via `Argon2id(passphrase, salt, params)`. The shipped default cost is **`parameter_set_version=1`, m=65536 KiB (64 MiB), t=3, p=1** (`angeld/src/vault.rs:17-20`). We are bumping the default to the **Desktop High Security profile: `parameter_set_version=2`, m=262144 KiB (256 MiB), t=3, p=1** — a 4× memory hardening over v1 (64 → 256 MiB); `t` and `p` unchanged. `p=1` (single lane) is retained deliberately for cross-device determinism: the same passphrase must derive the same `master_key` on every device regardless of core count.

Existing vaults were created under v1. When such a vault is unlocked under the new build, the daemon must transparently upgrade the on-disk KDF parameters to v2 — **without** re-encrypting bulk chunk data, **without** changing the random envelope Vault Key (so DEKs and safety numbers are preserved), and with **absolute all-or-nothing safety**: any failure leaves the vault byte-for-byte as it was before the attempt, still fully unlockable under v1.

### 1.1 Goals
- On unlock of a sub-target vault, atomically migrate KDF params v1 → v2.
- Preserve read/write access to **all** existing data (V1 legacy chunks and V2 envelope/DEK chunks) and device identity.
- The DB ends with **exactly one** KDF param-set (the new v2). No param-set history is retained.
- The migration is a fast, local operation (in-memory crypto + DB-metadata writes only). No network, no cloud I/O, no bulk file movement.
- Crash / I/O error / crypto error → full `ROLLBACK`; unlock still succeeds under v1; migration retried on next unlock.

### 1.2 Non-Goals (this spec)
- Bulk re-encryption of legacy V1 chunks to V2 (deferred to α.B.c, §9).
- ML-KEM-768 hybrid wrap (α.B.b).
- Real X25519 keypair generation / multi-device key sync (α.C).
- Changing the random envelope Vault Key or rotating DEKs (that is `rotate_vault_key`, a different operation).

---

## 2. Current Architecture (Audit)

### 2.1 Key tree (`omnidrive-core/src/crypto.rs:157` `derive_root_keys`)
```
passphrase ──Argon2id(salt, params)──▶ master_key
   ├─HKDF "vault-key-v1"────▶ vault_key          (DETERMINISTIC; V1 chunk read fallback)
   ├─HKDF "kek-v2"──────────▶ kek                (AES-KW wraps the random envelope_key)
   ├─HKDF "manifest-mac-v1"─▶ manifest_mac_key    (NO consumers — see §4)
   ├─HKDF "lease-mac-v1"────▶ lease_mac_key       (NO consumers — see §4)
   └─HKDF "local-anchor-v1"─▶ local_anchor_key    (NO consumers — see §4)

envelope_key  (RANDOM, stored as vault_state.encrypted_vault_key = AES-KW(kek, envelope_key))
   └─AES-KW──▶ per-inode DEK ──▶ V2 chunks (packer.rs:266 encrypt_chunk_v2)

device X25519 private key
   └─AES-256-GCM(HKDF(master_key, "identity-kek-v1"))──▶ devices.encrypted_private_key

local cache
   └─derive_cache_key(master_key) ──▶ encrypts rebuildable local cache (cache.rs)
```

### 2.2 Parameter storage (two tables, must stay in sync)
- **`vault_config`** (structured): `salt, parameter_set_version, memory_cost_kib, time_cost, lanes` — **source of truth** read by unlock (`to_root_kdf_params(get_vault_config)`).
- **`vault_state`**: `master_key_salt` BLOB + `argon2_params` TEXT-JSON (`{"mode","parameter_set_version","memory_cost_kib","time_cost","lanes"}`) + `encrypted_vault_key` + `vault_key_generation`.
- The salt is **duplicated** in both tables; a re-key must update both consistently.

### 2.3 Read/write reality
- **Writes are V2-only** (`packer.rs:266` `encrypt_chunk_v2(&dek, …)`). The deterministic `vault_key` is never used for new writes.
- **Reads are V2 with V1 fallback** (`downloader.rs:313` `decrypt_chunk_record(vault_key, dek_option)` — DEK if present, else deterministic `vault_key`).

### 2.4 Existing precedents (patterns to reuse)
- **`api/recovery.rs` passphrase-reset** already performs the lightweight re-wrap: new salt → new KEK → re-wrap the **same** `envelope_key`, keep generation, no DEK re-wrap, no chunk re-encrypt. This is the exact shape of our envelope step. (Note: it does **not** currently re-seal the device key — a latent gap this design corrects for the migration path.)
- **`rotate_for_revocation`** shows `derive_kek(&master_key)` re-derives the KEK without re-running Argon2.
- **`rotate_vault_key`** (`#[allow(dead_code)]`, unwired) is a *full* rotation (new random Vault Key + DEK re-wrap). **Not used here** — it changes safety numbers and is expensive.

### 2.5 Atomicity gap
Today's re-wrap paths call `rotate_vault_state` + `set_vault_config` + per-DEK updates as **separate** statements — a crash mid-sequence leaves an inconsistent vault. This design closes that gap with a single transaction.

---

## 3. Blast-Radius Analysis (the crux)

A params bump changes `master_key`. Every secret derived from or wrapped by `master_key` is affected. Completeness here is a data-integrity requirement (Święta Zasada): omitting one secret = data loss.

| Master-derived secret | How it's protected | Effect of bump | Disposition in migration |
|---|---|---|---|
| `envelope_key` (random VK) | AES-KW(kek) in `vault_state.encrypted_vault_key` | new KEK can't unwrap old blob | **Re-wrap**: unwrap(old_kek) → wrap(new_kek). Value unchanged → DEKs, V2 chunks, safety numbers preserved. |
| device X25519 private key | AES-GCM(identity-kek(master)) in `devices.encrypted_private_key` | new master can't unseal | **Re-seal**: unseal(old_master) → seal(new_master). |
| deterministic `vault_key` (V1) | derived, used to read legacy V1 chunks | value changes → V1 chunks unreadable | **Capture as `legacy_read_key`**: seal the *old* `vault_key` under `envelope_key` (params-independent). V1 stays readable. |
| `manifest_mac_key`, `lease_mac_key`, `local_anchor_key` | derived | value changes | **No-op** — verified zero consumers in `angeld/src`. Nothing persisted is MAC'd by these. |
| `derive_cache_key(master)` | encrypts local cache | value changes → old cache undecryptable | **Invalidate** local cache (rebuildable from cloud; not vault data). Out-of-tx (local files). |

**Conclusion — the complete re-key set written in one atomic SQLite transaction:**
1. Re-wrap `envelope_key` (old KEK → new KEK), generation **unchanged**.
2. Re-seal **every** `devices.encrypted_private_key` reachable on this node (old master → new master). See §7 for the multi-device scope decision.
3. Capture + seal `legacy_read_key` (old deterministic `vault_key` under `envelope_key`).
4. Write new `salt` + v2 params to **both** `vault_config` and `vault_state.argon2_params`.
(No MAC recompute. Local cache invalidation handled separately, post-commit.)

---

## 4. MAC keys are dead weight (finding)

`manifest_mac_key`, `lease_mac_key`, `local_anchor_key` are derived in `derive_root_keys` and returned in `RootKeys`, but a full-repo search finds **no consumers** outside `crypto.rs`. They MAC nothing persistent today. Therefore the migration performs **no MAC recomputation**. If a future feature introduces persisted MACs under these keys, that feature's migration must recompute them; this spec documents the current no-op explicitly so the gap is intentional, not forgotten.

---

## 5. Design — Phase 1 Atomic Migration

### 5.1 Target param-set
A single source of truth constant (e.g. `omnidrive-core`):
```
TARGET_KDF: parameter_set_version = 2, memory_cost_kib = 262144, time_cost = 3, lanes = 1
```
A vault "needs migration" iff its stored `parameter_set_version < TARGET_KDF.parameter_set_version`. The comparison is by version number, not by raw params, so the target can evolve monotonically.

### 5.2 Trigger & idempotency
- The migration runs **after a successful `unlock()`**, as a spawned task (non-blocking UX). Unlock success is never gated on migration success.
- Guard: skip if `stored.parameter_set_version >= TARGET.parameter_set_version` (idempotent — a second unlock after a successful migration is a no-op).
- Concurrency guard: a single-flight flag so two near-simultaneous unlocks don't both migrate. Loser observes the post-commit state and no-ops.
- The migration needs the **old** `master_key` + `envelope_key` (already in memory from the unlock that triggered it) and computes the **new** `master_key` from the same passphrase. The passphrase is available only transiently during unlock; the migration task receives the needed `KeyBytes`/`SecretBox` material captured at unlock time, never the passphrase string beyond what unlock already holds.

### 5.3 Compute phase (in memory, before opening the transaction)
All crypto runs first; the DB transaction only opens once every value is ready, so the transaction window is minimal.
1. `new_salt = random(16)`.
2. `new_master = Argon2id(passphrase, new_salt, TARGET_KDF)`  → derive `new_kek`, and the new identity-kek.
3. `new_encrypted_vault_key = AES-KW(new_kek, envelope_key)`  (envelope_key from unlock; unchanged).
4. For each local `devices` row with `encrypted_private_key`: `priv = unseal(old_master)`, `new_blob = seal(new_master, priv)`. (`priv` held in `SecretBox`, zeroized on drop.)
5. `legacy_read_key_blob = AES-256-GCM( HKDF(envelope_key, "legacy-read-key-v1"), old_vault_key, aad = vault_id )`.
6. Serialize new `argon2_params` JSON for `vault_state`.

If **any** step 1–6 fails (crypto error), abort before touching the DB — the vault is untouched.

### 5.4 Commit phase (one SQLite transaction)
A single `tx = pool.begin()`; every write uses `&mut *tx`; `commit()` only after all succeed; any error → `tx` dropped → automatic `ROLLBACK`.

```
BEGIN
  UPDATE vault_config  SET salt, parameter_set_version=2, memory_cost_kib, time_cost, lanes
  UPDATE vault_state   SET master_key_salt = new_salt,
                           argon2_params    = new_json,
                           encrypted_vault_key = new_encrypted_vault_key
                           -- vault_key_generation UNCHANGED
  UPDATE devices       SET encrypted_private_key = new_blob   (for each migrated device)
  UPDATE vault_state   SET legacy_read_key = legacy_read_key_blob   (new column; see §6)
COMMIT
```
This requires **transaction-accepting** variants of the existing `db::rotate_vault_state` / `db::set_vault_config` / identity re-seal helpers (today they each take `&pool`). The migration calls a single new orchestrator (e.g. `db::migrate_kdf_params_tx(...)`) that performs all writes on one `tx`.

### 5.5 Post-commit
- Update in-memory `UnlockedVaultKeys` to the new `master_key` / derived keys; `envelope_key` is unchanged.
- Invalidate the local cache (it was keyed by the old `derive_cache_key`); the cache is rebuildable, so this is a delete of local cache artifacts, performed **after** commit and outside the transaction. Failure to clear cache is non-fatal (stale cache entries simply miss and re-hydrate).
- Audit-log the migration (`parameter_set_version 1 → 2`), no secrets.

### 5.6 Legacy read path (consumed later by α.B.c and by V1 reads now)
After migration, the deterministic V1 `vault_key` is no longer derivable from the (new) master. To read a V1 chunk, the downloader obtains `legacy_read_key` by: unwrap `envelope_key` (via new KEK) → `HKDF(envelope_key,"legacy-read-key-v1")` → AES-GCM-decrypt `vault_state.legacy_read_key` (aad = vault_id) → the old `vault_key`. This is wired minimally in α.B.a so existing V1 chunks remain readable; α.B.c consumes the same key to re-encrypt them.

---

## 6. Data Model Changes
- **`vault_state.legacy_read_key BLOB NULL`** — new nullable column (additive migration via the existing `ensure_column_exists` pattern, `db.rs:1022`). `NULL` = no legacy key captured (fresh V2-native vault or pre-migration).
- `vault_config` / `vault_state` param columns: **updated in place** (no new rows, no generation bump). End state = single v2 param-set, satisfying the "one param-set only" requirement.
- No schema change to `data_encryption_keys` (DEKs untouched).

---

## 7. Multi-Device Scope Decision (explicit)

The salt lives in a **single shared** `vault_state`/`vault_config` row. A naive bump on device A rewrites that shared salt, so device B — whose `devices.encrypted_private_key` is sealed under the old shared master, and whose private key is **not** present on A — would be unable to unseal its own key after A's bump.

**Decision for α.B.a:** scope to the **single-device (owner) case**. The migration re-seals the device key(s) whose `encrypted_private_key` is present locally. The DoD vault is single-device (§8). The general multi-device model — **per-device KDF param-sets** so each device migrates independently — is deferred to **α.C** (device identity) and recorded here as a contract. The migration MUST detect the multi-device condition (more than one non-revoked device with a private key it cannot re-seal) and, in that case, **decline to migrate** (log + leave vault on v1) rather than strand a peer. This keeps α.B.a data-safe even if run on a multi-device vault.

---

## 8. Failure Handling & Acceptance

### 8.1 Failure matrix
| Failure point | Outcome |
|---|---|
| Crypto error in compute phase (§5.3) | DB never touched. Vault intact on v1. Unlock already succeeded. Retry next unlock. |
| I/O / DB error during transaction (§5.4) | `tx` dropped → `ROLLBACK`. All four updates reverted atomically. Vault intact on v1. Retry next unlock. |
| Crash / power loss mid-transaction | SQLite WAL guarantees the uncommitted tx is discarded on next open. Vault intact on v1. |
| Cache invalidation failure (post-commit) | Non-fatal. Migration already committed (v2). Stale cache misses re-hydrate. |
| Multi-device condition detected | Migration declines (no writes). Vault stays v1, fully functional. |

### 8.2 Acceptance criteria (DoD — to be turned into tests by the writing-plans step, not here)
- e2e: create vault under v1 (e.g. m=19456 test params) → unlock → migration runs → DB shows `parameter_set_version=2`, m=262144/t=3/p=1, single param-set. (Tests may override `TARGET_KDF` to a low-memory value to keep Argon2 fast; the *migration logic* is what's under test, not the specific cost.)
- Post-migration unlock with the **same passphrase** succeeds under v2.
- `envelope_key` value unchanged across migration: existing DEKs unwrap, a V2 chunk written pre-migration decrypts post-migration, safety numbers identical.
- Device private key unseals post-migration (X25519 identity preserved).
- A V1 chunk written pre-migration decrypts post-migration via `legacy_read_key`.
- Injected DB-write failure mid-transaction → `parameter_set_version` still 1, `encrypted_vault_key` unchanged, device key still old-sealed (full rollback); unlock under v1 still works.
- Second unlock after success = no-op (idempotent).

---

## 9. Future Contract (α.B.c)

α.B.c "complete V1→V2 envelope migration" will, as a **background, resumable, egress-throttled** job (never inside a DB transaction), for each inode with V1 chunks: download pack → decrypt with `legacy_read_key` → re-encrypt as V2 (`encrypt_chunk_v2` + per-inode DEK) → upload new V2 pack → **verify** (re-download + decrypt + hash == original) → atomically flip that inode's chunk pointers in one small tx → GC old packs after a grace period. Crash = resume; old data authoritative until each per-inode flip commits. When zero V1 chunks remain, drop `vault_state.legacy_read_key` → final state: V2-only. α.B.a guarantees `legacy_read_key` exists and is correct so α.B.c can rely on it.

---

## 10. Security Notes
- All transient key material (`old_master`, `new_master`, derived KEKs, unsealed device private key, old `vault_key`) is held in `KeyBytes`/`SecretBox` and zeroized on drop (α.A.c; verified by SMOKE H4).
- No plaintext key, passphrase, or `legacy_read_key` is ever logged (`[REDACTED]` rule). Audit log records only the param-set version transition.
- `legacy_read_key` is stored **sealed** under a subkey of the random `envelope_key`, never in plaintext, with `vault_id` as AAD.
- The migration runs only on an already-unlocked vault (the unlock that triggered it authenticated the passphrase).

---

## 11. Ratified Decisions (2026-05-25)
1. **Target params = Desktop High Security: `parameter_set_version=2`, m=262144 KiB (256 MiB), t=3, p=1.** A 4× memory hardening over v1. `p=1` for cross-device determinism (§1). Expected unlock latency ≈ 0.7–1.5 s on a desktop — an accepted, conscious cost for one-time-per-session unlock. A lighter **mobile profile** is deferred to **α.C** (per-device param-sets), not introduced here.
2. **Single-device scope** ratified: multi-device condition → migration **declines** (fail-safe, vault stays v1); per-device param-sets deferred to α.C (§7).
3. **Migration = spawned post-unlock task** ratified (non-blocking UX; failure → log + retry next unlock; never gates unlock success).
4. **`legacy_read_key` = nullable `vault_state` column** ratified (single legacy key suffices — V1 chunks were only ever written under the original deterministic `vault_key`).
5. **Re-key set ratified complete** (§3): `{envelope_key re-wrap, device private key re-seal, legacy_read_key capture+seal, param-set in-place}`; MAC = no-op (zero consumers); local cache = invalidate (rebuildable).
