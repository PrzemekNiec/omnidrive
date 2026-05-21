# α.A.c — Zeroize newtype dla `KeyBytes` (P2-005) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Zamienić `KeyBytes` z `Copy`-able type alias `[u8; 32]` na non-Copy newtype z `#[derive(Zeroize, ZeroizeOnDrop)]`, tak by każda kopia klucza w RAM zerowała się przy dropie (DoD: SMOKE H4 — memdump po lock = 0 trafień known-key).

**Architecture:** Newtype `pub struct KeyBytes(pub(crate) [u8; 32])` w `omnidrive-core/src/crypto.rs` z `Deref<Target=[u8]>` + `AsRef<[u8]>` + `AsRef<[u8;32]>` + `From<[u8;32]>` + redacted `Debug`. Buildery wewnątrz crate'u wypełniają bufor in-place (`KeyBytes([0;32])` → `&mut k.0`) — zero transient plain arrays. Funkcje `&KeyBytes` działają bez zmian dzięki deref-coercion `&KeyBytes → &[u8]`. Call-sites: `*x.expose_secret()` → `.clone()`, literały `[..;32]` → `.into()`.

**Tech Stack:** Rust Edition 2024, `zeroize` 1.x (feature `derive`), `secrecy` 0.10 (`SecretBox<KeyBytes>` działa bez zmian bo newtype: `Zeroize`), `aes-gcm`/`aes-kw`/`argon2`/`hkdf`/`hmac`.

**Spec:** `docs/superpowers/specs/2026-05-21-alpha-A-c-zeroize-keybytes-design.md` (ACCEPTED `eb1be7b`).

**Reguły procesowe (obowiązują każdy task):**
- Pre-push hook = `fmt + clippy --workspace -D warnings`. **Nigdy `--no-verify`.** PUSH tylko gdy workspace zielony.
- Zakaz `git commit --allow-empty`. Jeden atomowy commit = kod + testy.
- Chirurgicznie: dotykaj tylko linii wynikających z zadania. Zero komentarzy w kodzie produkcyjnym (CLAUDE.md).
- 🛡️ Data-integrity: SMOKE wyłącznie na Lenovo, `--no-sync --dry-run`, izolowany SYNC_PATH.

---

## File Structure

| Plik | Odpowiedzialność | Akcja |
|------|------------------|-------|
| `Cargo.toml` (root) | `[workspace.dependencies] zeroize` | Modify |
| `omnidrive-core/Cargo.toml` | jawna zależność `zeroize` | Modify |
| `omnidrive-core/src/crypto.rs` | **definicja** `KeyBytes` newtype + cechy + buildery in-place | Modify (sedno) |
| `omnidrive-core/src/ffi.rs` | `Vec<u8> → [u8;32] → KeyBytes::from` | Modify |
| `angeld/src/vault.rs` | accessory `.clone()` + `derive_cache_key` ret + testy | Modify |
| `angeld/src/downloader.rs` | 5× `.map(.. .clone())` | Modify |
| `angeld/src/packer.rs` | 1× `.clone()` | Modify |
| `angeld/src/migrator.rs` | 2× `.clone()` | Modify |
| `angeld/src/recovery.rs` | return `.into()` + 2 testy | Modify |
| `angeld/src/identity.rs` | `derive_wrapping_key` build + 5 test literałów `.into()` | Modify |
| `angeld/src/disaster_recovery.rs` | return `.into()` | Modify |
| `angeld/src/api/recovery.rs` | **weryfikacja no-op** | Verify only |
| `angeld/src/cache.rs` | **weryfikacja no-op** (borrow-form) | Verify only |
| `STATUS.md` | odhaczenie §12.5 α.A.c (po SMOKE H4) | Modify (Task 4) |

---

## Task 1: Dodaj zależność `zeroize`

**Files:**
- Modify: `Cargo.toml` (root, `[workspace.dependencies]`)
- Modify: `omnidrive-core/Cargo.toml` (`[dependencies]`)

- [ ] **Step 1: Dodaj `zeroize` do workspace dependencies**

W `Cargo.toml` (root), w sekcji `[workspace.dependencies]` (po linii `secrecy = ...`):

```toml
zeroize = { version = "1", features = ["derive"] }
```

- [ ] **Step 2: Dodaj jawną zależność w `omnidrive-core`**

W `omnidrive-core/Cargo.toml`, w `[dependencies]` (po `argon2 = "0.5"`, alfabetycznie gdziekolwiek pasuje — np. po `aes-kw`):

```toml
zeroize = { workspace = true, features = ["derive"] }
```

- [ ] **Step 3: Zweryfikuj rozwiązanie zależności**

Run: `cargo check -p omnidrive-core`
Expected: PASS (kompiluje się, dep rozwiązana).

> **Pułapka (spec §7):** jeśli `cargo` rzuci `feature "derive" does not exist for zeroize` — to mało prawdopodobne (derive to kanoniczny feature 1.x), ale w razie czego sprawdź `cargo info zeroize` i użyj nazwy z `[features]` listy. Jeśli zaś instrukcja `["zeroize_derive"]` z DoD przejdzie — zignoruj, `["derive"]` jest poprawne. NIE zmieniaj nic poza nazwą feature.

- [ ] **Step 4: Commit + push**

```bash
git add Cargo.toml omnidrive-core/Cargo.toml Cargo.lock
git commit -m "chore(deps): add zeroize (derive) to omnidrive-core (α.A.c)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
git push
```

Expected: pre-push (fmt + clippy --workspace) PASS — unused dep nie psuje buildu.

---

## Task 2: `KeyBytes` newtype + cechy + buildery in-place + RED-GREEN anchor

**Files:**
- Modify: `omnidrive-core/src/crypto.rs` (linie 1–28 import+def, 122–151 derive_root_keys, 294–334 buildery, 313–322 wrap_key)
- Modify: `omnidrive-core/src/ffi.rs:29,55`
- Test: `omnidrive-core/src/crypto.rs` (`mod tests`)

> ⚠️ Po tym tasku `cargo build -p omnidrive-core` i `cargo test -p omnidrive-core` są **zielone**, ale `cargo build --workspace` jest **CZERWONY** (angeld jeszcze nie zmigorwany). **NIE pushuj po Task 2** — push dopiero po Task 3.

- [ ] **Step 1: Napisz failujące testy zakotwiczające (RED)**

W `omnidrive-core/src/crypto.rs`, w `mod tests` (po `use super::*;`), dodaj:

```rust
    #[test]
    fn keybytes_zeroize_wipes_bytes() {
        use zeroize::Zeroize;
        let mut k = KeyBytes([0xAB; KEY_LEN]);
        k.zeroize();
        assert_eq!(&k[..], [0u8; KEY_LEN].as_slice());
    }

    #[test]
    fn keybytes_is_zeroize_on_drop() {
        fn assert_zod<T: zeroize::ZeroizeOnDrop>() {}
        assert_zod::<KeyBytes>();
    }

    #[test]
    fn keybytes_from_and_deref_round_trip() {
        let raw = [0x11u8; KEY_LEN];
        let k = KeyBytes::from(raw);
        assert_eq!(&k[..], raw.as_slice());
        let r: &[u8] = k.as_ref();
        assert_eq!(r, raw.as_slice());
    }
```

- [ ] **Step 2: Uruchom testy — potwierdź RED**

Run: `cargo test -p omnidrive-core keybytes_`
Expected: FAIL — błąd kompilacji (`KeyBytes` to alias, brak pola `.0`, brak `From`/`zeroize`).

- [ ] **Step 3: Dodaj importy zeroize do `crypto.rs`**

Na górze `omnidrive-core/src/crypto.rs` (po `use std::fmt;`, linia 9), dodaj:

```rust
use std::ops::Deref;
use zeroize::{Zeroize, ZeroizeOnDrop};
```

- [ ] **Step 4: Zamień type alias na newtype + cechy**

Zamień linię 28:

```rust
pub type KeyBytes = [u8; KEY_LEN];
```

na:

```rust
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct KeyBytes(pub(crate) [u8; KEY_LEN]);

impl fmt::Debug for KeyBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KeyBytes([REDACTED])")
    }
}

impl Deref for KeyBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for KeyBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8; KEY_LEN]> for KeyBytes {
    fn as_ref(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

impl From<[u8; KEY_LEN]> for KeyBytes {
    fn from(bytes: [u8; KEY_LEN]) -> Self {
        KeyBytes(bytes)
    }
}
```

> `ChunkId`/`ChunkNonce`/`GcmTag` (linie 29–31) **NIE ruszaj** — zostają aliasami.

- [ ] **Step 5: Buildery — fill in place (`expand_labeled_key`)**

Zamień `expand_labeled_key` (linie 294–300):

```rust
fn expand_labeled_key(input_key_material: &[u8], info: &[u8]) -> Result<KeyBytes, CryptoError> {
    let hkdf = Hkdf::<Sha256>::from_prk(input_key_material).map_err(CryptoError::HkdfPrk)?;
    let mut key = KeyBytes([0u8; KEY_LEN]);
    hkdf.expand(info, &mut key.0)
        .map_err(CryptoError::HkdfExpand)?;
    Ok(key)
}
```

- [ ] **Step 6: Builder `generate_random_key`**

Zamień (linie 305–309):

```rust
pub fn generate_random_key() -> KeyBytes {
    let mut key = KeyBytes([0u8; KEY_LEN]);
    rand::rngs::OsRng.fill_bytes(&mut key.0);
    key
}
```

- [ ] **Step 7: `wrap_key` + `unwrap_key` — `.0` dla Kek, fill in place**

Zamień `wrap_key` (linie 313–322):

```rust
pub fn wrap_key(
    wrapping_key: &KeyBytes,
    plaintext_key: &KeyBytes,
) -> Result<[u8; WRAPPED_KEY_LEN], CryptoError> {
    let kek = Kek::from(wrapping_key.0);
    let mut output = [0u8; WRAPPED_KEY_LEN];
    kek.wrap(plaintext_key, &mut output)
        .map_err(CryptoError::KeyWrap)?;
    Ok(output)
}
```

Zamień `unwrap_key` (linie 325–334):

```rust
pub fn unwrap_key(
    wrapping_key: &KeyBytes,
    wrapped_key: &[u8; WRAPPED_KEY_LEN],
) -> Result<KeyBytes, CryptoError> {
    let kek = Kek::from(wrapping_key.0);
    let mut output = KeyBytes([0u8; KEY_LEN]);
    kek.unwrap(wrapped_key, &mut output.0)
        .map_err(CryptoError::KeyWrap)?;
    Ok(output)
}
```

> `Kek::from(wrapping_key.0)`: `wrapping_key.0` to `[u8;32]` (Copy) — dostęp do `pub(crate)` pola w obrębie crate'u; `kek.wrap(plaintext_key, ..)` przyjmuje `&[u8]` → deref-coercion z `&KeyBytes`.

- [ ] **Step 8: `derive_root_keys` — argon2 fill in place + RootKeys**

Zamień ciało `derive_root_keys` (linie 132–150):

```rust
    let mut master = KeyBytes([0u8; KEY_LEN]);
    argon2
        .hash_password_into(passphrase, &params.salt, &mut master.0)
        .map_err(CryptoError::Argon2)?;

    let vault_key = expand_labeled_key(&master.0, VAULT_KEY_INFO)?;
    let kek = expand_labeled_key(&master.0, KEK_V2_INFO)?;
    let manifest_mac_key = expand_labeled_key(&master.0, MANIFEST_MAC_KEY_INFO)?;
    let lease_mac_key = expand_labeled_key(&master.0, LEASE_MAC_KEY_INFO)?;
    let local_anchor_key = expand_labeled_key(&master.0, LOCAL_ANCHOR_KEY_INFO)?;

    Ok(RootKeys {
        master_key: master,
        vault_key,
        kek,
        manifest_mac_key,
        lease_mac_key,
        local_anchor_key,
    })
```

> `&master.0` (`&[u8;32]`) coerces do `&[u8]` (param `input_key_material: &[u8]`). `RootKeys` derive(Clone,Debug,Eq,PartialEq) dalej działa (newtype ma wszystkie 4); pola dropują się zerując transitywnie — bez własnego derive na RootKeys.

- [ ] **Step 9: `ffi.rs` — Vec→array→KeyBytes::from**

W `omnidrive-core/src/ffi.rs`, zamień w `ffi_unwrap_key` (linie 29–31):

```rust
    let wk_arr: [u8; 32] = wrapping_key
        .try_into()
        .map_err(|_| OmniCoreError::InvalidKeyLength)?;
    let wk = KeyBytes::from(wk_arr);
```

i `crypto::unwrap_key(&wk, &wk_arr)` (linia 35) → `crypto::unwrap_key(&wk, &wrapped_arr)` — UWAGA: zmienna `wk_arr` w oryginale (linia 32) to `wrapped_key`; **zmień nazwę nowej zmiennej** żeby nie kolidowała. Finalnie linie 29–37:

```rust
    let wk_bytes: [u8; 32] = wrapping_key
        .try_into()
        .map_err(|_| OmniCoreError::InvalidKeyLength)?;
    let wk = KeyBytes::from(wk_bytes);
    let wrapped_arr: [u8; WRAPPED_KEY_LEN] = wrapped_key
        .try_into()
        .map_err(|_| OmniCoreError::InvalidKeyLength)?;
    crypto::unwrap_key(&wk, &wrapped_arr)
        .map(|k| k.to_vec())
        .map_err(|_| OmniCoreError::KeyUnwrap)
```

W `ffi_decrypt_chunk_v2`, zamień `dek` (linie 55–57):

```rust
    let dek_bytes: [u8; 32] = dek
        .try_into()
        .map_err(|_| OmniCoreError::InvalidKeyLength)?;
    let dek_arr = KeyBytes::from(dek_bytes);
```

`crypto::decrypt_chunk_v2(&dek_arr, &nonce_arr, &aad, &ciphertext, &tag_arr)` (linia 64) — bez zmian (`&KeyBytes` param). `nonce_arr`/`tag_arr` (ChunkNonce/GcmTag aliasy) bez zmian.

> `k.to_vec()` (linia 36): `KeyBytes: Deref<[u8]>` → `.to_vec()` ze slice. FFI `Vec<u8>` copy = akceptowane (spec §2).

- [ ] **Step 10: Uruchom testy crypto — GREEN**

Run: `cargo test -p omnidrive-core`
Expected: PASS — 3 nowe anchory + istniejące round-tripy (`wrap_unwrap_round_trip` itd.).

- [ ] **Step 11: Build + clippy core**

Run: `cargo build -p omnidrive-core` then `cargo clippy -p omnidrive-core --all-targets -- -D warnings`
Expected: PASS (core green; workspace jeszcze nie — to OK, fix w Task 3).

- [ ] **Step 12: Commit (BEZ push)**

```bash
git add omnidrive-core/src/crypto.rs omnidrive-core/src/ffi.rs
git commit -m "feat(crypto): KeyBytes ZeroizeOnDrop newtype + anchor tests (α.A.c)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

> **NIE pushuj** — `cargo build --workspace` czerwony (angeld). Push po Task 3.

---

## Task 3: Migracja call-sites w `angeld` (workspace → GREEN)

**Files:** wszystkie zmiany w jednym commicie (atomowa jednostka kompilacji workspace).

> Wzorzec naprawy: `*x.expose_secret()` (deref-copy, nie kompiluje się pod non-Copy) → `x.expose_secret().clone()` (owned newtype, zeruje się przy dropie). Literał `[..; 32]` przypisany do `KeyBytes` → `.into()`. Buildery zwracające KeyBytes z lokalnej `[u8;32]` → `.into()` na returnie.

- [ ] **Step 1: `vault.rs` — accessory (linie 70–86)**

```rust
    fn master_key(&self) -> KeyBytes {
        self.master_key.expose_secret().clone()
    }

    fn vault_key(&self) -> KeyBytes {
        self.vault_key.expose_secret().clone()
    }

    fn envelope_vault_key(&self) -> Option<KeyBytes> {
        self.envelope_vault_key
            .as_ref()
            .map(|k| k.expose_secret().clone())
    }

    fn previous_envelope_vault_key(&self) -> Option<KeyBytes> {
        self.previous_envelope_vault_key
            .as_ref()
            .map(|k| k.expose_secret().clone())
    }
```

- [ ] **Step 2: `vault.rs` — `derive_cache_key` free fn (linia ~764)**

To wolna funkcja `pub fn derive_cache_key(master_key: &[u8]) -> Result<KeyBytes, VaultError>` budująca `[u8;32]` przez HKDF. Zamień bufor na newtype in-place LUB owiń return `.into()`. Minimalnie — owiń return:

Znajdź `let mut cache_key = [0u8; 32];` (lub analogiczny bufor) + `hkdf.expand(..., &mut cache_key)?;` + `Ok(cache_key)` i zamień końcówkę na:

```rust
    Ok(cache_key.into())
```

> Jeśli funkcja buduje `[u8;32]` i go zwraca — `.into()` na `Ok(...)`. Jeśli już zwraca przez konstruktor, dostosuj. Sprawdź dokładny kształt przy edycji (read linii 764–790).

- [ ] **Step 3: `vault.rs` — testy (linie 834–1105)**

Wszystkie `*x.expose_secret()` w `#[cfg(test)]` → bez `*`, porównuj przez referencję / clone:
- Asercje `assert_eq!(*a.expose_secret(), *b.expose_secret())` → `assert_eq!(a.expose_secret(), b.expose_secret())` (porównanie `&KeyBytes` przez `PartialEq`).
- Asercje `assert_ne!(*a.expose_secret(), master_key)` gdzie `master_key`/`vault_key` to lokalne `KeyBytes` z accessorów → `assert_ne!(a.expose_secret(), &master_key)`.
- Przechwycenia `let dek_a_bytes = *dek_a_before.expose_secret();` → `let dek_a_bytes = dek_a_before.expose_secret().clone();` (potem porównania `assert_eq!(dek_a_after.expose_secret(), &dek_a_bytes)`).

> Mechaniczne. Po edycji kompilator wskaże każde niedopasowanie typu (`&KeyBytes` vs `KeyBytes`).

- [ ] **Step 4: `downloader.rs` (linie 288, 383, 503, 678, 797)**

Każde:
```rust
            .map(|(_, secret)| *secret.expose_secret());
```
→
```rust
            .map(|(_, secret)| secret.expose_secret().clone());
```

> Wynik `Option<KeyBytes>`; dalej `dek: Option<&KeyBytes>` przez `.as_ref()` — bez zmian w sygnaturach (linie 568, 1304).

- [ ] **Step 5: `packer.rs:218`**

```rust
        let dek = dek_secret.expose_secret().clone();
```

> `chunk_id: [u8; 32]` (linie 82, 566, 595) i `vec_to_array_32` (673) **NIE ruszaj** (to ChunkId, nie klucz).

- [ ] **Step 6: `migrator.rs:171,539`**

Oba:
```rust
        let dek = dek_secret.expose_secret().clone();
```

- [ ] **Step 7: `recovery.rs` — return + testy**

- `derive_recovery_key` (linia 75) zwraca KeyBytes z lokalnej tablicy — owiń return `.into()` (read linii 75–82 dla dokładnego kształtu; jeśli buduje `[u8;32]` przez HKDF → `Ok(key.into())` lub `key.into()`).
- Test linia 187: `let dek_bytes = dek_before.expose_secret().clone();`
- Test linia 258: `assert_eq!(dek_after.expose_secret(), &dek_bytes);`

- [ ] **Step 8: `identity.rs` — `derive_wrapping_key` (linie 177–183)**

```rust
fn derive_wrapping_key(shared_secret: &[u8; 32]) -> Result<KeyBytes, IdentityError> {
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret);
    let mut wrapping_key = [0u8; 32];
    hkdf.expand(VAULT_KEY_WRAP_INFO, &mut wrapping_key)
        .map_err(|e| IdentityError::Crypto(format!("HKDF-expand wrapping key: {e}")))?;
    Ok(wrapping_key.into())
}
```

> Lokalny `[u8;32]` jest tu OK (krótkożyjący, w funkcji core-style); konwersja `.into()` na returnie. (Opcjonalnie zeroize lokalu — pomijamy, minimum kodu; funkcja zwraca natychmiast.)

- [ ] **Step 9: `identity.rs` — testy (linie 383, 406, 437, 464, 616)**

Każde `let vault_key: KeyBytes = [0x42u8; 32];` (i `[0xAB; 32]`) →
```rust
        let vault_key: KeyBytes = [0x42u8; 32].into();
```
(odpowiednio `[0xAB; 32].into()`).

- [ ] **Step 10: `disaster_recovery.rs:440` — `derive_metadata_backup_key`**

`pub fn derive_metadata_backup_key(master_key: &[u8]) -> Result<KeyBytes, ...>` budująca `[u8;32]` → owiń return `.into()` (read 440–460 dla kształtu; `Ok(key.into())`).

- [ ] **Step 11: Weryfikacja no-op — `api/recovery.rs` + `cache.rs`**

Nie edytuj. Potwierdź że kompilują się jak są:
- `api/recovery.rs:290` — `find_map` zwraca owned `KeyBytes` (`unwrap_vault_key(...).ok()`), `let envelope_key: KeyBytes = match ...` OK.
- `cache.rs:250,280` — `Aes256Gcm::new_from_slice(cache_key_material.expose_secret())` — `expose_secret(): &KeyBytes` → `&[u8]` deref-coercion OK.

> Jeśli któryś jednak nie kompiluje — zastosuj ten sam wzorzec (`.clone()` / `.into()`).

- [ ] **Step 12: Build całego workspace — GREEN**

Run: `cargo build --release --workspace`
Expected: PASS. Jeśli FAIL — kompilator wskaże pozostałe niedopasowania `KeyBytes` vs `[u8;32]`; zastosuj wzorzec `.clone()`/`.into()`/deref do wskazanego miejsca (nie rozszerzaj scope poza migrację typu).

- [ ] **Step 13: Pełny test suite**

Run: `cargo test --workspace`
Expected: PASS dla wszystkiego oprócz `e2e_recovery` (FAIL pre-existing — wymaga `--features test-helpers`, security gate, NIE regresja; patrz [[feedback_e2e_recovery_test]]). Dla pewności sprawdź też: `cargo test --workspace --features test-helpers` jeśli flaga istnieje w tych crate'ach.

- [ ] **Step 14: Clippy + fmt gate (pre-push lokalnie)**

Run: `cargo clippy --workspace --all-targets -- -D warnings` then `cargo fmt --all -- --check`
Expected: PASS oba. (To dokładnie brama pre-push.)

- [ ] **Step 15: Commit + push (workspace zielony — Task 2 + Task 3 lecą razem)**

```bash
git add angeld/src/vault.rs angeld/src/downloader.rs angeld/src/packer.rs angeld/src/migrator.rs angeld/src/recovery.rs angeld/src/identity.rs angeld/src/disaster_recovery.rs
git commit -m "refactor(crypto): migrate KeyBytes call-sites to ZeroizeOnDrop newtype (α.A.c)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
git push
```

Expected: pre-push (fmt + clippy --workspace) PASS. Po push: commity Task 2 (`feat`) + Task 3 (`refactor`) na origin.

---

## Task 4: SMOKE H4 + zamknięcie (MANUAL — Przemek na Lenovo)

> **Nie dla subagenta implementacyjnego.** To gate akceptacyjny DoD wykonywany na produkcyjnym Lenovo wg 🛡️ data-integrity.

- [ ] **Step 1: Build i uruchom daemon (recipe α.A.b)**

DEBUG build dla wygody idle-triggera:
```
cargo build -p angeld
$env:OMNIDRIVE_E2E_TEST_MODE=1; $env:OMNIDRIVE_AUTO_LOCK_TEST_MIN=1; $env:OMNIDRIVE_DRY_RUN=1
target\debug\angeld.exe --no-sync --dry-run
```
Odblokuj Skarbiec znanym hasłem testowego vaulta.

- [ ] **Step 2: (kontrola pozytywna) dump PRZED lock**

`procdump -ma <pid> before.dmp` — potwierdź, że metoda wyszukiwania known-key DZIAŁA (klucz JEST w RAM przed lockiem). Known-key = Vault Key / DEK odtworzony offline z hasła+salt+params LUB z DB-wrapped → referencyjny unwrap.

- [ ] **Step 3: Zablokuj + dump PO lock**

Wymuś lock (idle ~1 min / Win+L / `POST /api/vault/lock`). Po locku:
`procdump -ma <pid> after.dmp`

- [ ] **Step 4: Grep known-key w `after.dmp`**

Szukaj bajtów known-key (master_key / vault_key / przykładowy DEK) w dumpie.
**PASS = 0 trafień** w `after.dmp` (przy ≥1 trafieniu w `before.dmp`).

- [ ] **Step 5: Odhacz STATUS §12.5**

Zaznacz wiersz α.A.c jako DONE + przesuń marker „◄── JESTEŚMY TU" na α.B.a. Commit `docs(status): α.A.c DONE — SMOKE H4 PASS`.

- [ ] **Step 6: Bump wersji (osobny release commit, PO H4)**

Workspace v0.3.25 → v0.3.26 (6 Cargo.toml + Cargo.lock) + commit `chore(release): bump workspace v0.3.26 [close α.A.c zeroize KeyBytes]`. (Zgodnie z DoD: NIE bumpować w trakcie, dopiero po H4.)

---

## Self-Review (writing-plans)

**1. Spec coverage:**
- Spec §3 newtype + cechy → Task 2 Step 4 ✓
- Spec §4 buildery in-place + Kek::from + derive_root_keys → Task 2 Steps 5–8 ✓
- Spec §5 ffi.rs → Task 2 Step 9 ✓
- Spec §6 inwentarz 13 plików → Task 3 Steps 1–11 (wszystkie pokryte; api/recovery + cache jako verify) ✓
- Spec §7 feature flaga → Task 1 Step 3 pułapka ✓
- Spec §8 TDD warstwy 1–3 → Task 2 Steps 1–2 (anchory), warstwa 4 (SMOKE H4) → Task 4 ✓
- Spec §9 SMOKE H4 → Task 4 ✓
- Spec §11 DoD checklist → rozłożone na Task 1–4 ✓

**2. Placeholder scan:** Brak „TBD/TODO". Kroki wymagające odczytu dokładnego kształtu (vault.rs:764 derive_cache_key, recovery.rs:75, disaster_recovery.rs:440) mają jawną instrukcję „read linii X–Y dla kształtu" + konkretny wzorzec `.into()` — to nie placeholder, to świadome dostosowanie do funkcji, której pełnej treści nie cytujemy (są to proste HKDF-buildery zwracające `KeyBytes`).

**3. Type consistency:** `KeyBytes` newtype, `pub(crate) [u8; KEY_LEN]`, `.0` dostęp in-crate, `From<[u8;KEY_LEN]>`, `Deref<Target=[u8]>`, `AsRef<[u8]>`+`AsRef<[u8;KEY_LEN]>` — spójne w Task 2 (def) i Task 3 (użycie `.clone()`/`.into()`). Commit messages spójne z konwencją (`chore(deps)`/`feat(crypto)`/`refactor(crypto)`).

**Wynik:** brak luk. Plan gotowy.
