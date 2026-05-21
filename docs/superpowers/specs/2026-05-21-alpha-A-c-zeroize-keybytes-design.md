# α.A.c — Zeroize newtype dla `KeyBytes` (P2-005) — Design Spec

> **Data:** 2026-05-21
> **Faza:** α (Crypto Hardening) · Grupa A (security hot-fixes) · krok α.A.c
> **Status:** ACCEPTED 2026-05-21 — Przemek ratyfikował obie rekomendacje (`Deref<[u8]>` + `pub(crate)` inner)
> **Punkt startowy:** HEAD `230232a` origin/main, working tree czysty, workspace v0.3.25
> **DoD (STATUS §12.5):** SMOKE H4 — memdump (ProcDump) po `lock` nie znajduje resztek Vault Key / DEK / master key w RAM.

---

## 1. Problem (P2-005)

`omnidrive-core/src/crypto.rs:28` definiuje:

```rust
pub type KeyBytes = [u8; KEY_LEN];   // [u8; 32]
```

`[u8; 32]` jest `Copy` i **nie ma `Drop`**. Konsekwencje:

1. **Niekontrolowane kopie na stosie.** Każde `*secret.expose_secret()` kopiuje 32 bajty klucza do lokalnej zmiennej wywołującego. Po wyjściu ze scope bajty **zostają w pamięci** (brak `Drop` → brak zerowania). Memdump znajduje resztki.
2. **Kanoniczny store JUŻ jest chroniony** — `vault.rs::UnlockedVaultKeys` trzyma klucze w `SecretBox<KeyBytes>`, a `lock()` (`*self.inner.write().await = None`) dropuje `SecretBox`-y, które zerują `[u8;32]` (bo `u8: Zeroize`). **Dziura to nie store — to kopie zwracane przez accessory.**

### Smoking gun (zinwentaryzowane call-sites)

```
vault.rs:71,75,79,85        accessory: *self.x.expose_secret()  → Copy [u8;32] do callera
downloader.rs:288,383,503,678,797   .map(|(_, s)| *s.expose_secret())  → Option<[u8;32]> DEK
packer.rs:218               let dek: KeyBytes = *dek_secret.expose_secret();
migrator.rs:171,539         let dek: KeyBytes = *dek_secret.expose_secret();
recovery.rs:187             let dek_bytes = *dek_before.expose_secret();   (test)
+ ~18 asercji testowych *x.expose_secret() w vault.rs/recovery.rs
```

Naprawa: pozbawić `KeyBytes` cechy `Copy` i nadać mu `Drop`-zeruje-pamięć przez `#[derive(Zeroize, ZeroizeOnDrop)]`. Każda kopia staje się jawnym `.clone()` zwracającym **wartość, która sama się zeruje przy dropie**.

---

## 2. Cele i zakres

### W zakresie

- `KeyBytes`: type alias → newtype z `Zeroize + ZeroizeOnDrop`, **bez `Copy`**.
- Komplet ergonomicznych cech tak, by call-sites zmieniały się chirurgicznie (deref-coercion `&KeyBytes → &[u8]`).
- Naprawa wszystkich miejsc, które przestaną się kompilować (newtype non-Copy łamie build **w 13 plikach**, nie tylko w 5 nazwanych).
- Buildery w `omnidrive-core` zerują/wypełniają bufor **in place** (zero transient plain arrays).
- `zeroize` jako jawna zależność (`[workspace.dependencies]` + `omnidrive-core`).

### Poza zakresem (świadomie — minimum kodu)

- **`ChunkId`, `ChunkNonce`, `GcmTag`** — pozostają zwykłymi aliasami `[u8; N]`. To **nie są sekrety** (HMAC content-address / losowy nonce / GCM auth tag). Rozszerzanie na nie blast-radiusa = gold-plating.
- **Passphrase `SecretString`** — już zarządzane przez `secrecy` (zeroize on drop). Nie ruszamy plumbingu hasła. (55 trafień `expose_secret` ≠ KeyBytes; większość to `SecretString`.)
- **FFI `Vec<u8>`** (`ffi.rs`) — granica UniFFI wymaga `Vec<u8>`; te kopie nie zerują się automatycznie. Mobile = post-v0.4; nie hardenujemy teraz. Buildujemy `[u8;32]` → `KeyBytes::from(...)`, FFI zwraca `k.to_vec()` (akceptowane).
- **Constant-time `PartialEq`** — `derive(PartialEq)` (byte-compare, nie CT). KeyBytes nie jest porównywany w runtime auth-path (jedyne `==` to `ChunkId` mismatch — alias). CT eq (`subtle`) = osobny task jeśli kiedyś potrzebny.
- **`aes-kw Kek` nie zeruje** swojej kopii klucza wewnętrznie — to biblioteka, poza naszą kontrolą; kopia transient, natychmiast konsumowana.

---

## 3. Design — newtype `KeyBytes`

```rust
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct KeyBytes(pub(crate) [u8; KEY_LEN]);
```

### Cechy (trait impls)

| Cecha | Powód | Uwaga |
|-------|-------|-------|
| `Zeroize, ZeroizeOnDrop` (derive) | sedno P2-005 — `Drop` zeruje 32 bajty | wymaga `u8: Zeroize` ✓ |
| `Clone` (derive) | świadoma kopia (np. accessory `.clone()`), `RootKeys: Clone` | klon = nowa wartość zerująca się |
| **NIE `Copy`** | wymusza jawne `.clone()` zamiast cichych kopii | `Copy` + `Drop` są wzajemnie wykluczające — to też mechaniczny gwarant |
| `Debug` (manual, **REDACTED**) | zero-knowledge rule (CLAUDE.md) — `[u8;32]` Debug drukuje bajty klucza | `KeyBytes([REDACTED])`; ulepsza też `RootKeys` Debug |
| `PartialEq, Eq` (derive) | testy round-trip (`assert_eq!(unwrapped, vault_key)`), `RootKeys: Eq` | byte-compare (nie CT — patrz §2 out-of-scope) |
| `Deref<Target = [u8]>` | deref-coercion `&KeyBytes → &[u8]` dla `new_from_slice`/`update`/`kek.wrap` bez churnu | **slice, nie `[u8;32]`** — patrz niżej |
| `AsRef<[u8]>` | jawny `.as_ref()` dla `Digest::update(impl AsRef<[u8]>)` itp. | |
| `AsRef<[u8; 32]>` | tam gdzie API chce `&[u8;32]` (np. `Kek::from(*k.as_ref())`) | |
| `From<[u8; 32]>` | konstrukcja z surowej tablicy (buildery, testy `[0x42;32].into()`, ffi) | |

### Decyzja: `Deref<Target = [u8]>`, NIE `[u8; 32]`

Instrukcja dopuszczała `[u8;32] lub [u8]`. Wybieramy **`[u8]`** świadomie:
`Deref<Target = [u8;32]>` pozwoliłby na `let arr = *key;` → `[u8;32]` (Copy) wymovowany na stos = **dokładnie ta niekontrolowana kopia, którą zwalczamy**. `[u8]` jest unsized → `*key` nie da się wymovować, można tylko pożyczyć `&*key = &[u8]`. Dla nielicznych miejsc wymagających `&[u8;32]` mamy `AsRef<[u8;32]>`.

### Decyzja: `pub(crate)` inner, NIE `pub`

Instrukcja proponowała `pub [u8;32]`. Rekomendujemy **`pub(crate)`**:
- Żaden call-site w `angeld` nie potrzebuje `.0` (wszystkie idą przez `From`/`AsRef`/`.clone()`).
- `pub` zostawiłby backdoor `let arr = key.0;` (copy-out) dla `angeld` — crate'u, gdzie żyje większość dziur.
- `pub(crate)` daje buildersom w `omnidrive-core` (i `ffi.rs`, ten sam crate) pełną ergonomię fill-in-place, a `angeld` dalej działa przez publiczne `From`/`AsRef`.

> **RATYFIKOWANE 2026-05-21:** oba powyższe (Deref target `[u8]` + `pub(crate)` visibility) zatwierdzone przez Przemka mimo odejścia od literalnej treści instrukcji — świadomy wybór defensywny fazy α.

### `RootKeys` — bez zmian w derives

`RootKeys` (`#[derive(Clone, Debug, Eq, PartialEq)]`) dalej się kompiluje (newtype ma wszystkie 4). **Nie potrzebuje własnego `ZeroizeOnDrop`** — drop-glue pól uruchamia `Drop` każdego `KeyBytes` → transitive zeroize. Bonus: `Debug` `RootKeys` staje się bezpieczny (redacted pola).

---

## 4. Zmiany w `omnidrive-core/src/crypto.rs`

### Buildery — fill in place (zero transient plain arrays)

```rust
fn expand_labeled_key(ikm: &[u8], info: &[u8]) -> Result<KeyBytes, CryptoError> {
    let hkdf = Hkdf::<Sha256>::from_prk(ikm).map_err(CryptoError::HkdfPrk)?;
    let mut k = KeyBytes([0u8; KEY_LEN]);
    hkdf.expand(info, &mut k.0).map_err(CryptoError::HkdfExpand)?;
    Ok(k)   // przy błędzie k dropuje → zeruje
}

pub fn generate_random_key() -> KeyBytes {
    let mut k = KeyBytes([0u8; KEY_LEN]);
    rand::rngs::OsRng.fill_bytes(&mut k.0);
    k
}

pub fn unwrap_key(wrapping_key: &KeyBytes, wrapped: &[u8; WRAPPED_KEY_LEN]) -> Result<KeyBytes, CryptoError> {
    let kek = Kek::from(*wrapping_key.as_ref());     // AsRef<[u8;32]>
    let mut out = KeyBytes([0u8; KEY_LEN]);
    kek.unwrap(wrapped, &mut out.0).map_err(CryptoError::KeyWrap)?;
    Ok(out)
}
```

### `derive_root_keys` — argon2 fill in place

`master_key` argon2 wypełnia bezpośrednio bufor newtype'a:

```rust
let mut master = KeyBytes([0u8; KEY_LEN]);
argon2.hash_password_into(passphrase, &params.salt, &mut master.0).map_err(CryptoError::Argon2)?;
let vault_key = expand_labeled_key(&master.0, VAULT_KEY_INFO)?;   // &[u8;32] → &[u8]
// ... reszta subkeys z &master.0
Ok(RootKeys { master_key: master, vault_key, kek, manifest_mac_key, lease_mac_key, local_anchor_key })
```

> `expand_labeled_key(ikm: &[u8], ...)` — `&master.0` (`&[u8;32]`) coerces do `&[u8]`. Dostęp do `.0` w obrębie crate'u (pub(crate)).

### `wrap_key` — `Kek::from(*wrapping_key.as_ref())`

`Kek::<Aes256>::from([u8;32])` — `*wrapping_key.as_ref()` (transient copy konsumowany przez `Kek`). `kek.wrap(plaintext_key, ...)` — `plaintext_key: &KeyBytes` → deref do `&[u8]` ✓.

### Funkcje przyjmujące `&KeyBytes`

`chunk_id`, `chunk_nonce`, `chunk_encryption_key`, `encrypt_chunk`, `decrypt_chunk`, `encrypt_chunk_v2`, `decrypt_chunk_v2`, `derive_kek`, `derive_subkey`, `encrypt_secret`, `decrypt_secret` — wszystkie wołają `new_from_slice(key)` / `Nonce::from_slice` / `Mac::update` przyjmujące `&[u8]`. **Deref-coercion `&KeyBytes → &[u8]` załatwia to bez zmian w treści.** Jedyne zmiany: typy zwracane KeyBytes konstruowane przez buildery (wyżej).

### Testy crypto.rs (istniejące — muszą zostać zielone)

`assert_eq!(unwrapped, vault_key)` — wymaga `PartialEq + Debug` ✓ (komunikat błędu pokaże `[REDACTED]` — akceptowane, to round-trip test).

---

## 5. Zmiany w `omnidrive-core/src/ffi.rs`

```rust
// było: let wk: KeyBytes = wrapping_key.try_into()...;
let wk_arr: [u8; KEY_LEN] = wrapping_key.try_into().map_err(|_| OmniCoreError::InvalidKeyLength)?;
let wk = KeyBytes::from(wk_arr);
// crypto::unwrap_key(&wk, ...).map(|k| k.to_vec())   ← to_vec() przez Deref<[u8]> ✓
```

Analogicznie `dek` w `ffi_decrypt_chunk_v2`. `ChunkNonce`/`GcmTag` (`try_into` na aliasy) bez zmian.

---

## 6. Inwentarz call-sites do naprawy (komplet — 13 plików)

| Plik | Linie | Zmiana |
|------|-------|--------|
| `Cargo.toml` (root) | `[workspace.dependencies]` | + `zeroize = { version = "1", features = [<derive>] }` (patrz §7) |
| `omnidrive-core/Cargo.toml` | `[dependencies]` | + `zeroize = { workspace = true, features = [<derive>] }` |
| `omnidrive-core/src/crypto.rs` | 28, 33–85, 122–345 | newtype + traits + buildery in-place + `Kek::from(*..as_ref())` |
| `omnidrive-core/src/ffi.rs` | 29, 55 | `Vec<u8> → [u8;32] → KeyBytes::from` |
| `angeld/src/vault.rs` | 71,75,79,85 | accessory: `*x.expose_secret()` → `x.expose_secret().clone()` |
| `angeld/src/vault.rs` | 764 | `derive_cache_key` return: `[u8;32]` build → `.into()` |
| `angeld/src/vault.rs` | 834–1105 (testy) | `*x.expose_secret()` → `x.expose_secret().clone()` / porównania `&KeyBytes` |
| `angeld/src/downloader.rs` | 288,383,503,678,797 | `*s.expose_secret()` → `s.expose_secret().clone()` (Option<KeyBytes>) |
| `angeld/src/packer.rs` | 218 | `*dek_secret.expose_secret()` → `.clone()` |
| `angeld/src/migrator.rs` | 171,539 | `*dek_secret.expose_secret()` → `.clone()` |
| `angeld/src/recovery.rs` | 75 (ret), 187,258 (testy) | return `.into()` + test `.clone()` / `&` compare |
| `angeld/src/identity.rs` | 177–183 | `derive_wrapping_key`: build `[0u8;32]` + expand + `.into()` |
| `angeld/src/identity.rs` | 383,406,437,464,616 (testy) | `let vk: KeyBytes = [0x42;32];` → `... = [0x42;32].into();` |
| `angeld/src/disaster_recovery.rs` | 440 | `derive_metadata_backup_key` return `.into()` |
| `angeld/src/api/recovery.rs` | 290 | **bez zmian** — `find_map` zwraca owned `KeyBytes` |
| `angeld/src/cache.rs` | 250,280 | **bez zmian** — borrow-form `expose_secret()` → Deref ✓ |

> **api/recovery.rs i cache.rs zweryfikowane jako no-op** — wymienione dla kompletności.

---

## 7. Zależność `zeroize` — pułapka feature flagi

Instrukcja/DoD mówi `features = ["zeroize_derive"]`. W `zeroize` 1.x feature włączający makra to **`"derive"`** (`derive = ["dep:zeroize_derive"]`); przez składnię `dep:` implicit-feature `zeroize_derive` **może nie istnieć** → `cargo` rzuci „feature does not exist".

**Plan:** dodać `features = ["derive"]`; jeśli `cargo check` zaakceptuje `"zeroize_derive"` — zostawić jak w DoD; w przeciwnym razie `"derive"`. Rozstrzygnięcie empiryczne na etapie implementacji (pierwszy `cargo check`).

Wersja: `secrecy = "0.10"` ciągnie `zeroize` 1.x — pinujemy `zeroize = "1"` dla spójności z `SecretBox<KeyBytes>` (newtype: `Zeroize` → `SecretBox` działa bez zmian).

---

## 8. Strategia TDD

Czerwony → zielony → refactor. „Pamięć wyzerowana po dropie" jest niemierzalna w bezpiecznym unit-teście (czytanie zwolnionej pamięci = UB) — rozkładamy DoD na warstwy:

1. **`crypto.rs` — `zeroize()` zeruje bajty (RED-anchor).**
   ```rust
   #[test] fn keybytes_zeroize_wipes_bytes() {
       let mut k = KeyBytes([0xAB; KEY_LEN]);
       k.zeroize();
       assert_eq!(k.as_ref() as &[u8], &[0u8; KEY_LEN][..]);
   }
   ```
   Nie kompiluje się dopóki newtype + `Zeroize` nie istnieją → po implementacji zielony. To dowód logiki zerowania, którą `ZeroizeOnDrop` woła w `Drop`.

2. **Static trait assertion — `ZeroizeOnDrop` gwarantowany.**
   ```rust
   #[test] fn keybytes_is_zeroize_on_drop() {
       fn assert_zod<T: zeroize::ZeroizeOnDrop>() {}
       assert_zod::<KeyBytes>();
   }
   ```
   Compile-time dowód, że `Drop` zeruje.

3. **Ergonomia/round-trip (regression).** `From<[u8;32]>` → `AsRef`/`Deref` round-trip; istniejące `wrap_unwrap_round_trip` i pozostałe testy crypto/vault/identity/recovery **muszą zostać zielone** (gwarant chirurgiczności).

4. **SMOKE H4 (manual, DoD gate).** ProcDump memdump procesu `angeld` PO `lock` (idle/Win+L/logout) → grep wzorca znanego klucza w dumpie = **0 trafień**. Recipe w §9. To jest właściwy dowód behawioralny, poza unit-testami.

> Każdy atomowy krok: kod produkcyjny + test w jednym commicie (zakaz `--allow-empty`). Pre-push (fmt + clippy --workspace -D warnings) — nigdy `--no-verify`.

---

## 9. SMOKE H4 — procedura akceptacyjna (DoD)

1. **Build:** `cargo build --release -p angeld` (H4 nie zależy od `TEST_MIN`, więc release OK; ale do idle-triggera wygodniej DEBUG + `OMNIDRIVE_AUTO_LOCK_TEST_MIN=1` — recipe α.A.b).
2. Uruchom daemon, **odblokuj** Skarbiec znanym hasłem (znamy więc Vault Key/master deterministycznie z salt+params, albo dumpujemy DEK z DB-wrapped → unwrap referencyjny).
3. **Zablokuj** (idle / Win+L / `POST /api/vault/lock`).
4. `procdump -ma <pid_angeld> dump_after_lock.dmp`.
5. Szukaj w dumpie bajtów known-key (master_key / vault_key / przykładowy DEK). **PASS = 0 trafień.**
6. (Kontrola pozytywna — opcjonalnie) dump PRZED lock powinien zawierać klucz → potwierdza, że metoda wyszukiwania działa.

> 🛡️ Data-integrity: SMOKE na Lenovo (dev box), `--no-sync --dry-run` + e2e mode, izolowany SYNC_PATH. Bez egress.

---

## 10. Ryzyka i pułapki

- **Blast radius = 13 plików.** Mitygacja: zmiany mechaniczne (`*x.expose_secret()` → `.clone()`, `[..;32]` → `.into()`); kompilator wskaże każde miejsce; pełny `cargo build --release --workspace` przed twierdzeniem „done".
- **`Kek::from` kopiuje** klucz do struktury `aes-kw` (nie zeruje). Transient, natychmiast konsumowany; poza kontrolą (biblioteka). Akceptowane.
- **Feature flaga `zeroize_derive` vs `derive`** — §7, rozstrzygane empirycznie.
- **`Debug` redacted psuje czytelność asercji** — komunikat `assert_eq!` pokaże `[REDACTED]`. Akceptowane (round-trip testy; debugujesz inaczej). Zero-knowledge > test ergonomics.
- **`derive_root_keys` / buildery** — fill-in-place eliminuje transient plain array; przy każdym wczesnym return (`?`) `KeyBytes` dropuje → zeruje. Brak ścieżki, w której surowy klucz zostaje un-zeroized w tej funkcji.

---

## 11. Definition of Done

- [ ] `zeroize` jako jawna zależność (workspace + omnidrive-core), `cargo check` zielony.
- [ ] `KeyBytes` = newtype `ZeroizeOnDrop`, non-Copy, z kompletem cech z §3.
- [ ] `ChunkId`/`ChunkNonce`/`GcmTag` nietknięte (aliasy).
- [ ] `cargo build --release --workspace` zielony (wszystkie 13 plików skompilowane).
- [ ] Unit testy §8 (1–3) zielone; cały dotychczasowy suite zielony.
- [ ] `clippy --workspace -D warnings` + `fmt` zielone (pre-push przechodzi).
- [ ] **SMOKE H4 PASS** na Lenovo (memdump po lock = 0 trafień known-key).
- [ ] STATUS §12.5 wiersz α.A.c odhaczony; bump v0.3.25 → v0.3.26 **dopiero po** SMOKE H4.
