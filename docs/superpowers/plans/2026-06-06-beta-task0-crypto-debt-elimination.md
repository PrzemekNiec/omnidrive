# Faza β — Task 0: Crypto Debt Elimination (QG5 findings F-1/F-2/F-3)

> **Dyrektywa:** ZERO DŁUGU TECHNICZNEGO. Przed jakąkolwiek logiką sieciową Fazy β (P1-002 itd.) eliminujemy wszystkie naprawialne findings z formalnego przeglądu QG5 (`docs/superpowers/specs/2026-06-06-crypto-review.md`).
>
> **Tryb:** TDD subagent-driven (implementer → spec-review → code-quality-review per sub-task). Bramka `--all-targets` po całości, push, bez bumpu wersji.
> **Out of scope:** P1-002 (snapshot fetch worker, wstrzymane), P2-001/002/003, P3-002 (te zostają długiem). F-4–F-7 (Info — bez akcji kodowej).

**Cel:** zamknąć F-1 (P2-006), F-2 (P3-003), F-3 (P3-004) trzema chirurgicznymi, w pełni przetestowanymi zmianami — bez regresji istniejących 139 testów.

---

## Pliki

- **F-1:** `angeld/src/db.rs` (`revoke_device` ~`:7405`) + test w `db.rs mod tests`.
- **F-2:** `omnidrive-core/src/crypto.rs` (nowy `decrypt_chunk_v2_verified` po `decrypt_chunk_v2` `:327`) + caller `angeld/src/downloader.rs` (`:1369`) + testy w `crypto.rs mod tests`. **FFI `ffi_decrypt_chunk_v2` i `migrator.rs` NIETKNIĘTE.**
- **F-3:** `angeld/src/vault.rs` (`ensure_vault_config` `:926`) + testy w `vault.rs mod tests` + audyt testów migracji KDF.

Bez nowych zależności. Bez zmian schematu DB. Bez bumpu wersji.

---

## Task 0.1 (F-1 / P2-006) — `revoke_device` NULLuje też hybrydowy wrap

**Problem:** `revoke_device` czyści `wrapped_vault_key` (X25519) i `vault_key_generation`, ale zostawia `wrapped_vault_key_kyber` (v3-hybrid) → zrewokowane urządzenie z kopią DB wciąż odtwarza VK ścieżką hybrydową.

- [ ] **Krok 1 — failing test.** Do `db.rs mod tests`:

```rust
    #[tokio::test]
    async fn revoke_device_nulls_both_wraps() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        let vault = get_vault_params(&pool).await.unwrap().unwrap();
        migrate_single_to_multi_user(&pool, &vault.vault_id).await.unwrap();
        let user = list_users(&pool).await.unwrap().pop().unwrap();
        // setup device with BOTH wraps populated (mirror set_and_read_device_wrapped_kyber)
        // ... insert device "dev-x", set wrapped_vault_key + wrapped_vault_key_kyber ...

        assert!(revoke_device(&pool, "dev-x").await.unwrap());

        let dev = get_device(&pool, "dev-x").await.unwrap().unwrap();
        assert!(dev.revoked_at.is_some());
        assert!(dev.wrapped_vault_key.is_none(), "X25519 wrap cleared");
        assert!(dev.wrapped_vault_key_kyber.is_none(), "hybrid wrap cleared");
    }
```

> Setup urządzenia z oboma wrapami: użyć wzorca z istniejącego testu `set_and_read_device_wrapped_kyber` (`db.rs mod tests`) — direct SQL INSERT do `users`/`devices` + `set_device_wrapped_vault_key` + `set_device_wrapped_vault_key_kyber`. Jeśli `set_device_wrapped_vault_key` ma inną sygnaturę, dopasować do realnej.

- [ ] **Krok 2 — RUN, expect FAIL** (`wrapped_vault_key_kyber` zostaje `Some`): `cargo test -p angeld --lib revoke_device_nulls_both_wraps`
- [ ] **Krok 3 — implementacja.** W `revoke_device` (`db.rs:7407-7410`) dodać `wrapped_vault_key_kyber = NULL` do `SET`:

```rust
        "UPDATE devices SET revoked_at = ?, wrapped_vault_key = NULL, \
         wrapped_vault_key_kyber = NULL, vault_key_generation = NULL \
         WHERE device_id = ? AND revoked_at IS NULL",
```

> `kyber_public_key` NIE jest czyszczony świadomie — to klucz publiczny (encaps), nie sekret ani wrap; spójnie z tym że X25519-revoke zostawia `public_key`. Wrap (`wrapped_vault_key_kyber`) to jedyne, co domyka rewokację.

- [ ] **Krok 4 — RUN, expect PASS.** `cargo fmt --all`.
- [ ] **Krok 5 — commit:** `git commit -am "fix(db): β.0 revoke_device clears hybrid wrapped_vault_key_kyber (F-1)"`

---

## Task 0.2 (F-2 / P3-003) — weryfikacja chunk_id po dekrypcji V2 (parytet z V1)

**Problem:** `decrypt_chunk_v2` nie rekomputuje `HMAC(DEK, plaintext)` (V1 `decrypt_chunk` to robi). **Nie zmieniamy sygnatury `decrypt_chunk_v2`** — używa jej FFI/share-link (browser nie ma manifestu/expected id). Dodajemy verified-wrapper dla wewnętrznej ścieżki daemona, która MA autorytatywny chunk_id z DB.

- [ ] **Krok 1 — failing testy.** Do `crypto.rs mod tests`:

```rust
    #[test]
    fn decrypt_chunk_v2_verified_roundtrip_ok() {
        let dek = KeyBytes::from([0x11u8; 32]);
        let pt = b"hello envelope v2";
        let enc = encrypt_chunk_v2(&dek, pt, &[]).unwrap();
        let out = decrypt_chunk_v2_verified(
            &dek, &enc.chunk_id, &enc.nonce, &[], &enc.ciphertext, &enc.gcm_tag,
        )
        .unwrap();
        assert_eq!(out, pt);
    }

    #[test]
    fn decrypt_chunk_v2_verified_rejects_wrong_chunk_id() {
        let dek = KeyBytes::from([0x11u8; 32]);
        let enc = encrypt_chunk_v2(&dek, b"payload", &[]).unwrap();
        let wrong = [0xAAu8; 32];
        let err = decrypt_chunk_v2_verified(
            &dek, &wrong, &enc.nonce, &[], &enc.ciphertext, &enc.gcm_tag,
        );
        assert!(matches!(err, Err(CryptoError::ChunkIdMismatch { .. })));
    }
```

> Dopasować typy do realnych: `ChunkId`, `ChunkNonce`, `GcmTag`, `KeyBytes` (jak w istniejących testach `encrypt_chunk_v2`/`decrypt_chunk_v2` w `crypto.rs`). `enc.chunk_id`/`enc.nonce`/`enc.gcm_tag` z `EncryptedChunk`.

- [ ] **Krok 2 — RUN, expect FAIL** (undefined): `cargo test -p omnidrive-core decrypt_chunk_v2_verified`
- [ ] **Krok 3 — implementacja.** Po `decrypt_chunk_v2` (`crypto.rs:327`) dodać:

```rust
/// Like [`decrypt_chunk_v2`] but additionally recomputes `chunk_id = HMAC(dek, plaintext)`
/// and verifies it against `expected_chunk_id` (parity with V1 `decrypt_chunk`). Use this
/// on the daemon read path where the manifest-authoritative chunk_id is known. The plain
/// [`decrypt_chunk_v2`] stays for the FFI / share-link decryptor, which has no manifest.
pub fn decrypt_chunk_v2_verified(
    dek: &KeyBytes,
    expected_chunk_id: &ChunkId,
    nonce: &ChunkNonce,
    aad: &[u8],
    ciphertext: &[u8],
    gcm_tag: &GcmTag,
) -> Result<Vec<u8>, CryptoError> {
    let plaintext = decrypt_chunk_v2(dek, nonce, aad, ciphertext, gcm_tag)?;
    let actual = chunk_id(dek, &plaintext)?;
    if &actual != expected_chunk_id {
        return Err(CryptoError::ChunkIdMismatch { expected: *expected_chunk_id, actual });
    }
    Ok(plaintext)
}
```

- [ ] **Krok 4 — RUN, expect PASS:** `cargo test -p omnidrive-core decrypt_chunk_v2_verified`
- [ ] **Krok 5 — wpiąć w daemon read-path.** `downloader.rs:1369`: zamienić
  `decrypt_chunk_v2(dek, &nonce, &[], ciphertext, &gcm_tag)?`
  na
  `decrypt_chunk_v2_verified(dek, &expected_chunk_id, &nonce, &[], ciphertext, &gcm_tag)?`
  gdzie `expected_chunk_id` = autorytatywny chunk_id z DB (`vec_to_chunk_id(&chunk.chunk_id)?`, już obliczany w tej funkcji `:1323`). Dodać import `decrypt_chunk_v2_verified`. **`migrator.rs:551` NIETKNIĘTE** (re-encrypt round-trip w migracji — chunk_id świeżo policzony, weryfikacja zbędna; pozostaje na plain `decrypt_chunk_v2`).
- [ ] **Krok 6 — `cargo build -p angeld` + `cargo fmt --all`.**
- [ ] **Krok 7 — commit:** `git commit -am "feat(core): β.0 decrypt_chunk_v2_verified + wire daemon read-path (F-2)"`

---

## Task 0.3 (F-3 / P3-004) — świeży vault startuje od razu na parameter_set 2

**Problem:** `ensure_vault_config` tworzy nowy vault z `DEFAULT_*` (param_set 1, m=64MiB) → migrowany do 2 dopiero przy 1. unlocku (okno słabszego KDF + podwójny Argon2id). **Decyzja Przemka:** świeży vault ma od razu startować na `TARGET` (param_set 2, m=256MiB).

- [ ] **Krok 1 — failing testy.** Do `vault.rs mod tests`:

```rust
    #[tokio::test]
    async fn fresh_vault_starts_at_target_param_set() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        bootstrap_local_vault(&pool).await.unwrap();
        let cfg = db::get_vault_config(&pool).await.unwrap().unwrap();
        assert_eq!(cfg.parameter_set_version, i64::from(TARGET_PARAMETER_SET_VERSION));
        assert_eq!(cfg.memory_cost_kib, i64::from(TARGET_MEMORY_COST_KIB));
    }

    #[tokio::test]
    async fn fresh_vault_needs_no_kdf_migration() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        bootstrap_local_vault(&pool).await.unwrap();
        let cfg = db::get_vault_config(&pool).await.unwrap().unwrap();
        assert!(!needs_kdf_migration(cfg.parameter_set_version));
    }
```

- [ ] **Krok 2 — RUN, expect FAIL** (świeży = param_set 1): `cargo test -p angeld --lib fresh_vault_starts_at_target_param_set`
- [ ] **Krok 3 — implementacja.** W `ensure_vault_config` (`vault.rs:932-949`) zamienić `DEFAULT_*` → `TARGET_*` w obu miejscach (`set_vault_config` + `RootKdfParams::new`):

```rust
    db::set_vault_config(
        pool, &salt,
        i64::from(TARGET_PARAMETER_SET_VERSION),
        i64::from(TARGET_MEMORY_COST_KIB),
        i64::from(TARGET_TIME_COST),
        i64::from(TARGET_LANES),
    ).await?;
    Ok((
        RootKdfParams::new(
            TARGET_PARAMETER_SET_VERSION, salt,
            TARGET_MEMORY_COST_KIB, TARGET_TIME_COST, TARGET_LANES,
        ),
        true,
    ))
```

> `DEFAULT_*` constants ZOSTAJĄ (reprezentują legacy v1 param-set; używane przez testy migracji jako punkt startowy „starego" vaulta). Po fixie nie są już używane do tworzenia — to OK (kandydat na rename na `LEGACY_V1_*` w osobnym sprzątaniu, NIE w tym tasku; zero komentarzy-TODO).

- [ ] **Krok 4 — AUDYT testów migracji KDF (KRYTYCZNE — anti-regresja).** Przejrzeć testy w `vault.rs mod tests`, które zakładały że świeży vault = v1 i potem migruje (`migration_*`, `spawn_migration_upgrades_params_to_v2`, `post_unlock_maintenance_migrates_then_generates_keypair`, `migration_is_idempotent`, `migration_preserves_legacy_v1_read_key`, `migration_reseals_device_private_key`, `migration_upgrades_params_and_preserves_envelope`). Te, które polegały na `bootstrap_local_vault`/`ensure_vault_config` dla uzyskania v1, MUSZĄ jawnie tworzyć param_set 1 (np. `db::set_vault_config(pool, &salt, 1, 65536, 3, 1)` przed unlockiem). Cel: migracja v1→v2 nadal pokryta testem; tylko ŹRÓDŁO v1-vaultu w teście jest jawne, nie domyślne. **Jeśli któryś test tworzy v1 niejawnie przez bootstrap → dostosować setup, NIE osłabiać asercji migracji.**
- [ ] **Krok 5 — RUN całość vault:** `cargo test -p angeld --lib vault` — expect PASS (nowe + wszystkie migracyjne zielone).
- [ ] **Krok 6 — `cargo fmt --all`.**
- [ ] **Krok 7 — commit:** `git commit -am "fix(vault): β.0 fresh vault starts at parameter_set 2 (F-3)"`

---

## Bramka końcowa (po Task 0.1–0.3)

- [ ] Pełna bramka (mirror pre-push):

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features test-helpers -- -D warnings
cargo build --release --workspace
cargo test -p omnidrive-core
cargo test -p angeld --lib
```
Expected: wszystko zielone; zero regresji wobec 139 lib + 26 core.

- [ ] **DoD Task 0:**
  - F-1: `revoke_device_nulls_both_wraps` — oba wrapy NULL po revoke.
  - F-2: `decrypt_chunk_v2_verified_roundtrip_ok` + `..._rejects_wrong_chunk_id`; downloader używa verified variant; FFI/migrator nietknięte.
  - F-3: `fresh_vault_starts_at_target_param_set` + `fresh_vault_needs_no_kdf_migration`; testy migracji v1→v2 nadal zielone (jawny v1 setup).
- [ ] **Bez bumpu wersji.** Push (pre-push aktywny, nigdy `--no-verify`).
- [ ] Po DoD: `KNOWN_ISSUES.md` — P2-006/P3-003/P3-004 → **Closed** (Faza β Task 0). Markery: zostają w β; NEXT = P1-002 (roster-merge only).

---

## Sekwencja wykonania
0.1 (F-1, db) → 0.2 (F-2, core+downloader) → 0.3 (F-3, vault + audyt testów migracji) → bramka → push → przeniesienie 3 wpisów do Closed. Subagent-driven: implementer → spec-review → code-quality per sub-task.
