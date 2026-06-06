# OmniDrive Crypto Spec — Envelope Encryption (Format V2)

**Status:** LIVING SPEC (Faza α zaktualizowana)  
**Data:** 2026-04-06 (utworzony), 2026-06-06 (§7 reconcile + §12–§15 dla Fazy α)  
**Autor:** Claude + Przemek  
**Dotyczy:** Phase 0 Crypto Checkpoint, Epic 32.5; §12–§15 = Faza α (α.A–α.C)  

> **Nota α.D.a (2026-06-06):** §1–§11 opisują rdzeń Envelope V2. §12–§15 dodano w fazie α.D.a, aby spec opisywał **cały** stos krypto wdrożony w Fazie α: §12 AAD, §13 auto-lock + zeroize, §14 device key-exchange (X25519 + ML-KEM-768 hybrid), §15 identity grafting. §7 zaktualizowano o podbicie parametrów Argon2id (α.B.a). Towarzyszący formalny przegląd (QG5): `docs/superpowers/specs/2026-06-06-crypto-review.md`.

---

## 1. Streszczenie

Przechodzimy z modelu **"jeden klucz szyfruje wszystko"** (V1) na **Envelope Encryption** (V2), w ktorym kazdy plik/rewizja ma wlasny losowy klucz DEK (Data Encryption Key), a klucz glowny (Vault Key) sluzy wylacznie do opakowywania DEK-ow.

Glowna korzyssc: rotacja klucza glownego i przyszle udostepnianie plikow (Epic 33) nie wymagaja ponownego szyfrowania danych — wystarczy ponownie opakkowac DEK-i.

---

## 2. Obecny model (V1) — co zmieniamy

```
passphrase
    |
    v
Argon2id(passphrase, salt) -> master_key (256-bit)
    |
    +-- HKDF-Expand(master_key, "vault-key-v1")     -> vault_key
    +-- HKDF-Expand(master_key, "manifest-mac-key")  -> manifest_mac_key
    +-- HKDF-Expand(master_key, "lease-mac-key")     -> lease_mac_key
    +-- HKDF-Expand(master_key, "local-anchor-key")  -> local_anchor_key

vault_key uzyty bezposrednio do:
  - HMAC-SHA256(vault_key, plaintext) -> chunk_id
  - HKDF-Expand(vault_key, "chunk-enc-v1") -> chunk_encryption_key
  - AES-256-GCM(chunk_encryption_key, nonce, plaintext) -> ciphertext
```

**Problemy V1:**
- vault_key jest deterministyczny (pochodna passphrase) — kompromitacja hasla = kompromitacja wszystkich danych
- Zmiana hasla wymaga ponownego szyfrowania WSZYSTKICH chunkow
- Brak mozliwosci udostepnienia pojedynczego pliku bez ujawnienia vault_key
- chunk_id = HMAC(vault_key, plaintext) — ujawnia czy dwa urzadzenia maja ten sam plik (deduplication leak)

---

## 3. Nowy model (V2) — Envelope Encryption

### 3.1 Hierarchia kluczy

```
passphrase
    |
    v
Argon2id(passphrase, salt) -> master_key (256-bit)      [KEY DERIVATION KEY]
    |
    +-- HKDF-Expand(master_key, "kek-v2") -> KEK         [KEY ENCRYPTION KEY]
    |
    +-- HKDF-Expand(master_key, "manifest-mac-key")  -> manifest_mac_key  (bez zmian)
    +-- HKDF-Expand(master_key, "lease-mac-key")     -> lease_mac_key     (bez zmian)
    +-- HKDF-Expand(master_key, "local-anchor-key")  -> local_anchor_key  (bez zmian)

KEK (256-bit) -> wrappuje/unwrappuje Vault Key

Vault Key (256-bit, LOSOWY) -> wrappuje/unwrappuje DEK-i

DEK (256-bit, LOSOWY, per-plik) -> szyfruje/deszyfruje chunki tego pliku
```

### 3.2 Generacja i przechowywanie Vault Key

| Aspekt | Decyzja |
|--------|---------|
| Generacja | `OsRng::fill_bytes()` — 256-bit losowy, generowany raz przy tworzeniu vaultu |
| Przechowywanie | `vault_state.encrypted_vault_key` — Vault Key opakowany KEK-iem przez AES-256-KW |
| W pamieci | `SecretBox<KeyBytes>` w `VaultKeyStore` po unlocku |
| Rotacja | Nowy losowy Vault Key -> re-wrap wszystkich DEK (BEZ re-encryption chunkow) |

**Flow unlock:**
```
passphrase -> Argon2id -> master_key -> HKDF -> KEK
KEK + encrypted_vault_key -> AES-KW-Unwrap -> Vault Key (w pamieci)
```

**Flow tworzenia vaultu:**
```
1. Generuj losowy Vault Key (256-bit)
2. passphrase -> Argon2id -> master_key -> HKDF -> KEK
3. AES-KW-Wrap(KEK, Vault Key) -> encrypted_vault_key
4. Zapisz encrypted_vault_key + salt + argon2_params do vault_state
```

### 3.3 Generacja i przechowywanie DEK

| Aspekt | Decyzja |
|--------|---------|
| Granulacja | Jeden DEK per inode (plik). Wszystkie rewizje tego samego pliku uzyja tego samego DEK-a. Nowy DEK tylko przy: nowym pliku, explicit key rotation |
| Generacja | `OsRng::fill_bytes()` — 256-bit losowy |
| Wrapping | AES-256-KW (RFC 3394) z Vault Key jako KEK |
| Przechowywanie | Tabela `data_encryption_keys` w SQLite |
| W pamieci | Cache w `HashMap<i64, SecretBox<KeyBytes>>` — unwrap on-demand, evict po timeout |

**Schemat tabeli `data_encryption_keys`:**
```sql
CREATE TABLE data_encryption_keys (
    dek_id        INTEGER PRIMARY KEY AUTOINCREMENT,
    inode_id      INTEGER NOT NULL,
    wrapped_dek   BLOB NOT NULL,           -- 40 bytes (AES-KW output for 32-byte key)
    key_version   INTEGER NOT NULL DEFAULT 1,
    vault_key_gen INTEGER NOT NULL DEFAULT 1,  -- ktora generacja Vault Key
    created_at    INTEGER NOT NULL,
    UNIQUE(inode_id, key_version)
);
```

**Flow szyfrowania pliku (nowy):**
```
1. Czy istnieje DEK dla tego inode_id?
   - NIE: generuj DEK, wrap Vault Key-em, zapisz do data_encryption_keys
   - TAK: pobierz wrapped_dek, unwrap Vault Key-em
2. Dla kazdego chunka:
   chunk_nonce = random 12 bytes (NIE deterministyczny z V1!)
   ciphertext  = AES-256-GCM(DEK, chunk_nonce, plaintext, aad)
3. Zapisz chunk z nonce w ChunkRecordPrefix
```

**Flow deszyfrowania pliku:**
```
1. Pobierz wrapped_dek z data_encryption_keys (by inode_id)
2. Unwrap DEK Vault Key-em
3. Dla kazdego chunka:
   plaintext = AES-256-GCM-Decrypt(DEK, nonce, ciphertext, aad)
```

### 3.4 Dlaczego AES-KW a nie AES-GCM do wrappowania

| Kryterium | AES-256-KW (RFC 3394) | AES-256-GCM |
|-----------|----------------------|-------------|
| Nonce | Nie wymaga | Wymaga 12-byte nonce |
| Rozmiar wyjscia | input + 8 bytes | input + 12 (nonce) + 16 (tag) |
| Nonce reuse risk | Brak (deterministic) | Katastrofalny |
| WebCrypto compat | `wrapKey("raw", ..., "AES-KW")` | `wrapKey("raw", ..., "AES-GCM")` |
| Przeznaczenie | Zaprojektowany do wrappowania kluczy | General-purpose AEAD |

**Decyzja: AES-256-KW** — prostszy, bezpieczniejszy (brak ryzyka nonce reuse), kompatybilny z WebCrypto.

---

## 4. Format V2 — nowy naglownik chunka

### 4.1 ChunkRecordPrefix V2

Obecny `ChunkRecordPrefix` (V1, 80 bytes):
```
[4] record_magic "CHNK"
[1] record_version = 1
[1] flags
[1] compression_algo
[1] reserved_0
[32] chunk_id
[8] plain_len
[8] cipher_len
[12] nonce
[12] reserved_1
--- data follows: [cipher_len] ciphertext + [16] GCM tag ---
```

Nowy `ChunkRecordPrefix` (V2, 80 bytes — ten sam rozmiar!):
```
[4] record_magic "CHNK"
[1] record_version = 2          <-- zmiana
[1] flags
[1] compression_algo
[1] key_wrapping_algo            <-- bylo reserved_0, teraz: 0=legacy, 1=AES-KW
[32] chunk_id
[8] plain_len
[8] cipher_len
[12] nonce                       <-- w V2: random (nie deterministyczny)
[4] dek_id_hint                  <-- z reserved_1: ulatwia lookup DEK w DB
[8] reserved_1                   <-- skrocony reserved
--- data follows: [cipher_len] ciphertext + [16] GCM tag ---
```

**Kluczowe decyzje:**
- **Rozmiar sie nie zmienia** (80 bytes) — backward compatible na poziomie parsowania
- `record_version = 2` — jedyny marker odrozniajacy V1 od V2
- `key_wrapping_algo = 1` — mowi, ze chunk jest zaszyfrowany DEK-iem, a DEK jest w `data_encryption_keys`
- `dek_id_hint` — 4-byte hint (lower 32 bits of dek_id) przyspieszajacy lookup; nieautorytatywny, prawdziwy lookup po `inode_id` z manifestu
- `nonce` — w V2 jest **losowy** (OsRng), nie deterministyczny. Eliminuje chunk content leaking przez powtarzalne nonce

### 4.2 PackHeader V2

Dodajemy pole w `reserved_2` (20 bytes):
```
[4] reserved_2[0..4]  -> vault_key_generation  (u32, big-endian)
[16] reserved_2[4..20] -> reserved (bez zmian)
```

`vault_key_generation` — numer generacji Vault Key. Pozwala urzadzeniu wykryc, ze Vault Key byl zrotowany i DEK-i w tej paczce sa opakowane starsza generacja.

### 4.3 vault_state — nowe kolumny

```sql
ALTER TABLE vault_state ADD COLUMN vault_format_version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE vault_state ADD COLUMN encrypted_vault_key BLOB;  -- NULL for V1
ALTER TABLE vault_state ADD COLUMN vault_key_generation INTEGER NOT NULL DEFAULT 0;
```

- `vault_format_version = 1` — stary model, chunk encryption bezposrednio z vault_key
- `vault_format_version = 2` — envelope encryption, DEK per plik
- `encrypted_vault_key` — Vault Key opakowany KEK-iem (40 bytes AES-KW output)
- `vault_key_generation` — inkrementowane przy kazdej rotacji Vault Key

---

## 5. Backward Compatibility i Migracja

### 5.1 Czytanie

| Daemon wersja | Format w DB / na dysku | Zachowanie |
|---------------|----------------------|------------|
| V2 daemon | V1 chunk (record_version=1) | Uzywa starego flow: vault_key -> chunk_encryption_key -> decrypt. Dziala bez zmian |
| V2 daemon | V2 chunk (record_version=2) | Nowy flow: lookup DEK -> unwrap -> decrypt |
| V1 daemon | V2 chunk | **Odmowa** — `vault_format_version = 2` w bazie, stary daemon nie startuje |

### 5.2 Migracja V1 -> V2

Migracja jest **leniwa** (lazy) + opcjonalny batch:

1. **Lazy:** Nowe pliki sa szyfrowane V2. Stare pliki sa czytane V1.
2. **Batch (opcjonalny):** Background task re-szyfruje stare pliki:
   - Decrypt chunk (stary vault_key flow)
   - Generuj DEK, wrap Vault Key-em
   - Re-encrypt chunk DEK-iem
   - Zapisz nowy chunk z record_version=2
   - Checkpoint po kazdym pliku (resumable)
3. **Finalizacja:** Gdy 100% plikow to V2, ustaw `vault_format_version = 2`

### 5.3 Rollback

Jesli migracja failuje w polowie:
- Pliki juz zmigrowane sa V2 (maja DEK)
- Pliki niezmigrowane sa V1
- Daemon V2 czyta oba formaty — system jest w pelni funkcjonalny
- NIE MA potrzeby rollbacku do V1-only — mixed mode jest stabilny

---

## 6. Przygotowanie pod Zero-Knowledge Sharing (Epic 33)

### 6.1 Share Link = DEK w URL Fragment

```
# Tryb A (LAN): dekryptor serwowany przez daemona, same-origin
http://{lan_ip}:8787/share/{share_id}#{base64url(DEK)}@{daemon_host:port}

# Tryb B (Public, post-v0.3.0): statyczny dekryptor na GitHub Pages
https://skarbiec.app/s/{share_id}#{base64url(DEK)}@{b2_base_url}
```

> `skarbiec.app` = statyczny host (GitHub Pages). Daemon **nie** uczestniczy
> w downloadzie w Trybie B — przeglądarka pobiera chunki bezpośrednio z B2/R2.

- Serwer **nigdy nie widzi DEK** (fragment URI nie jest wysylany do serwera)
- Serwer serwuje zaszyfrowane chunki
- Przegladarka: `crypto.subtle.importKey("raw", DEK)` -> `decrypt("AES-GCM", ...)`

### 6.2 Dlaczego DEK per-plik (nie per-chunk)

- Jeden DEK per plik = jeden secret w URL = prosty link
- Per-chunk DEK wymagalby listy kluczy w URL (nieporecznie)
- Per-plik DEK pozwala na revoke dosteepu do pliku bez wplywu na inne pliki

### 6.3 WebCrypto kompatybilnosc

| Operacja | Algorytm | WebCrypto API |
|----------|----------|---------------|
| DEK wrapping | AES-256-KW | `crypto.subtle.wrapKey("raw", dek, kek, "AES-KW")` |
| Chunk encryption | AES-256-GCM | `crypto.subtle.encrypt("AES-GCM", dek, data)` |
| KDF | Argon2id | **NIE** — brak w WebCrypto. Browser nie odtwarza Vault Key. Dostaje gotowy DEK w URL |

---

## 7. Algorytmy i parametry — podsumowanie

| Element | Algorytm | Parametry |
|---------|----------|-----------|
| KDF (parameter_set 1, legacy) | Argon2id v0x13 | m=64 MiB (65 536 KiB), t=3, p=1, output=256-bit |
| KDF (parameter_set 2, Desktop High Security) | Argon2id v0x13 | **m=256 MiB (262 144 KiB)**, t=3, p=1, output=256-bit (patrz §7.2) |
| KEK derivation | HKDF-SHA256 (Expand-only, PRK=master_key) | info=`"kek-v2"` |
| Vault Key wrapping | AES-256-KW (RFC 3394) | KEK jako wrapping key |
| DEK wrapping | AES-256-KW (RFC 3394) | Vault Key jako wrapping key |
| Chunk encryption | AES-256-GCM | DEK, 12-byte random nonce, 128-bit tag |
| Chunk ID (V2) | HMAC-SHA256(DEK, plaintext) | Deterministyczny per-DEK (nie per-vault) |
| Nonce (V2) | Random | `OsRng`, 12 bytes, per-chunk |

### 7.1 Nonce: Random vs Deterministyczny

**V1:** nonce = HMAC(vault_key, "nonce" || chunk_id)[0..12] — deterministyczny.
Problem: ten sam plik na dwoch urzadzeniach produkuje identyczny ciphertext (leaks equality).

**V2:** nonce = OsRng 12 bytes — losowy.
Ten sam plik na dwoch urzadzeniach produkuje rozny ciphertext. Brak content leaking.

Ryzyko kolizji 12-byte random nonce z jednym DEK: zaniedbywalny przy <2^32 chunkow per plik.

### 7.2 Wersjonowanie parametrów Argon2id i migracja (α.B.a)

Parametry KDF są **wersjonowane** kolumną `vault_config.parameter_set_version` (+ `memory_cost_kib`, `time_cost`, `lanes`, `salt`). Pozwala to podnosić koszt KDF bez utraty zgodności ze starymi vaultami i recovery-keyami.

| parameter_set_version | m (KiB) | t | p | Status |
|---|---|---|---|---|
| 1 | 65 536 (64 MiB) | 3 | 1 | legacy (OWASP floor) |
| 2 | 262 144 (256 MiB) | 3 | 1 | **bieżący — Desktop High Security** |

**Re-key migracja (NIE re-encryption danych)** — wykonywana atomowo przy pierwszym unlocku vaultu o `parameter_set_version < 2` (`vault::run_post_unlock_maintenance` → `migrate_kdf_params_if_needed`):

```
1. Unlock starymi params: passphrase --Argon2id(v1)--> master_v1 --HKDF--> KEK_v1
   KEK_v1 + encrypted_vault_key --AES-KW-Unwrap--> Vault Key  (envelope_key)
2. Re-derive nowymi params: passphrase --Argon2id(v2, nowa sól)--> master_v2 --HKDF--> KEK_v2
3. Re-wrap TEGO SAMEGO Vault Key: AES-KW-Wrap(KEK_v2, Vault Key) --> nowy encrypted_vault_key
4. Re-seal device private key (X25519/ML-KEM) pod KEK tożsamości z master_v2 (patrz §14.4)
5. Zachowaj stary deterministyczny V1 vault_key jako `vault_state.legacy_read_key`
   (sealed pod envelope_key, AAD=vault_id) — aby legacy V1 chunki dało się dalej czytać
6. Wszystko w JEDNEJ tx SQLite (BEGIN IMMEDIATE). Crash w połowie = pełny rollback.
```

**Niezmienniki:** `vault_key_generation` NIE zmienia się (bajty Vault Key identyczne przed/po) → DEK-i, chunki i safety-numbers nietknięte; migracja jest tania (sekundy, tylko re-derive + re-wrap VK). Multi-device z różnymi params = **DECLINE** (per-device params poza zakresem v0.4). Świeży vault tworzony jest na parameter_set 1 i migrowany do 2 przy pierwszym unlocku (patrz nota w §-przeglądzie QG5 — kandydat na uproszczenie: tworzyć od razu na 2).

---

## 8. Nowe crate/dependencies

| Crate | Cel | Uwagi |
|-------|-----|-------|
| `aes-kw` | AES-256-KW (RFC 3394) | Pure Rust, ~100 LOC, auditable |
| `secrecy` | Zerowanie kluczy w pamieci | Juz uzywamy w `vault.rs` |
| `zeroize` | Trait Zeroize dla KeyBytes | Dependency `secrecy`, juz obecne |

Nie dodajemy: `ring`, `openssl` — zostajemy w ekosystemie RustCrypto.

---

## 9. Plan implementacji (Epic 32.5)

Kolejnosc zadan zgodna z `plan.md`:

1. **32.5.1a** — Migracja schematu DB (`vault_format_version`, `encrypted_vault_key`, `data_encryption_keys`)
2. **32.5.1b** — Vault Key generation + AES-KW wrap/unwrap + unlock flow
3. **32.5.1c** — DEK generation + wrap/unwrap
4. **32.5.1d** — Chunk encryption/decryption przez DEK (z backward compat dla V1)
5. **32.5.1e** — Integracja z uploader/downloader
6. **32.5.2a-c** — Migrator V1->V2 (lazy + batch + finalizacja)
7. **32.5.2d** — Vault Key rotation

---

## 11. Session token validation — timing side-channel decision

`validate_user_session` (db.rs) uses a plain SQL `WHERE token = ?` without an
application-level constant-time comparison. This is a **conscious, documented
decision** — not an oversight.

### Why constant-time is not required here

1. **Token entropy:** session tokens are 256-bit values from `OsRng`. An attacker
   needs ~2²⁵⁶ queries to brute-force one by chance; timing information provides
   no shortcut at this bit-length.

2. **Transport:** the daemon listens exclusively on loopback / RFC-1918 LAN (see
   CORS policy in `api/mod.rs`). A remote attacker cannot measure response times
   with the sub-microsecond precision required to distinguish a SQLite row lookup
   from a miss.

3. **Noise floor:** a SQLite `SELECT` with index lookup takes ~1–10 µs. The
   difference between a hit and a miss at the byte-comparison level is ~1–5 ns —
   three orders of magnitude below the query overhead. Even same-machine
   measurements are dominated by OS scheduler jitter.

4. **Comparison site:** the comparison occurs inside SQLite's native C code, not
   in a Rust loop. The timing difference per byte is further masked by SQLite's
   B-tree traversal, page cache, and WAL overhead.

### Contrast with sharing.rs

`validate_share_password` in `sharing.rs` uses `subtle::ConstantTimeEq` because
share passwords are short, user-chosen strings that could be brute-forced offline
if a hash were leaked. Session tokens have neither property.

### What would change this decision

If the daemon ever exposes an HTTP endpoint on a public interface (0.0.0.0 /
non-loopback), this decision must be revisited and `subtle::ConstantTimeEq`
applied before the SQL lookup.

---

## 10. Security considerations

- **Vault Key NIGDY nie opuszcza pamieci w plaintexcie** — na dysku tylko jako `encrypted_vault_key` (AES-KW wrapped)
- **DEK NIGDY nie jest logowany** — `[REDACTED]` w tracing (zgodnie z CLAUDE.md zero-knowledge rule)
- **Passphrase -> master_key jest jednokierunkowy** — Argon2id z soleniem
- **AES-KW jest deterministic** — ten sam KEK + ten sam Vault Key = ten sam ciphertext. To jest OK i zamierzone (brak nonce = brak ryzyka nonce reuse)
- **Random nonce per chunk w V2** — eliminuje content equality leaking miedzy urzadzeniami
- **chunk_id w V2 uzywa DEK nie vault_key** — dwa urzadzenia z roznym DEK dla tego samego pliku produkuja rozne chunk_id (brak cross-device deduplication, ale tez brak leaking)

---

## 12. AAD — Associated Authenticated Data (semantyka, P3-001)

AES-256-GCM autentykuje (ale nie szyfruje) opcjonalne **AAD**. OmniDrive używa AAD świadomie i niejednolicie — różne ścieżki mają różne wymagania. Ta sekcja dokumentuje decyzję P3-001 z audytu Fazy 0.

| Ścieżka | Funkcja | AAD | Uzasadnienie |
|---|---|---|---|
| Chunki danych (V1 i V2) | `encrypt_chunk` / `encrypt_chunk_v2` | **`&[]` (puste)** | patrz §12.1 |
| OAuth refresh token | `VaultKeyStore::seal_oauth_token` | **`user_id`** | patrz §12.2 |
| legacy_read_key (α.B.a) | `seal_legacy_read_key` | **`vault_id`** | wiązanie sealowanego V1-vault-key z konkretnym vaultem (anti-splice między bazami) |
| X25519/ML-KEM private key (α.C.a/α.B.b) | `encrypt_private_key` / `seal_secret_blob` | **`&[]` (puste)** | patrz §12.3 |
| Hybrid VK wrap (α.B.b) | `omnidrive_core::hybrid` | brak AEAD-AAD; rola AAD = HKDF-info | patrz §14.3 |

### 12.1 Dlaczego chunki używają pustego AAD (`&[]`)

1. **Integralność jest już zapewniona dwukrotnie:** (a) 128-bitowy tag GCM nad ciphertextem; (b) w V1 i V2 dekryptor weryfikuje `chunk_id == HMAC(klucz, odszyfrowany_plaintext)` — czyli plaintext jest kryptograficznie związany ze swoim identyfikatorem. Podmiana ciphertextu → fail tagu; podmiana mapowania chunk→plik → mismatch chunk_id.
2. **Kompatybilność z WebCrypto (Tryb B sharing, §6.1):** statyczny dekryptor w przeglądarce (GitHub Pages) dostaje DEK we fragmencie URL i odszyfrowuje chunk `crypto.subtle.decrypt({name:"AES-GCM", iv, additionalData: <?>})`. Każde niepuste AAD musiałoby być deterministycznie odtwarzalne po stronie przeglądarki z danych dostępnych w share-linku — co związałoby format chunka z modelem metadanych i utrudniło bezstanowy dekryptor. Puste AAD = dekryptor potrzebuje wyłącznie `(DEK, nonce, ciphertext, tag)`.
3. **Brak realnego wektora, który AAD by tu domknął:** chunki nie są „przenoszone" między kontekstami w sposób, który dałoby się sfałszować bez złamania tagu GCM lub kolizji HMAC chunk_id.

### 12.2 Dlaczego OAuth token używa AAD = `user_id`

Sealowany refresh token leży w bazie współdzielonej przez multi-user vault (Family Cloud, faza δ pod maską). AAD = `user_id` sprawia, że blob zaszyfrowany dla użytkownika A **nie odszyfruje się** podstawiony pod kontekst użytkownika B (klucz wyprowadzany z tego samego envelope key, więc bez AAD blob byłby przenośny między userami). To **cross-user tampering / confused-deputy protection**.

### 12.3 Dlaczego seal klucza prywatnego używa pustego AAD

`encrypt_private_key` (X25519, 60 B) i `seal_secret_blob` (ML-KEM decaps, zmienna długość) trzymają sekret w `local_device_identity` — tabeli **per-device, NIE grafowanej** (§15). Klucz (KEK tożsamości = HKDF(master,"omnidrive-identity-kek-v1")) jest unikatowy per-device i niewspółdzielony, więc nie istnieje „inny kontekst", pod który blob dałoby się podstawić. Integralność at-rest zapewnia tag GCM. Dodatkowe AAD nie domknęłoby żadnego realnego wektora (kandydat na wzmocnienie defense-in-depth: AAD=`device_id` — patrz przegląd QG5, finding informacyjny).

---

## 13. Auto-lock i Zeroize — higiena klucza w pamięci (α.A)

Threat-model §12.1(b): pamięć user-mode procesu jest **świadomie akceptowanym ryzykiem** (malware z uprawnieniami usera może odczytać odblokowany Vault Key z RAM). Mitygacja = minimalizacja okna ekspozycji (auto-lock) + zerowanie kluczy z RAM (Zeroize).

### 13.1 Lock triggers (α.A.a + α.A.b)

| Trigger | Źródło | Mechanizm |
|---|---|---|
| Logout | `api/auth.rs::post_auth_logout` (α.A.a, P1-006) | `vault_keys.lock()` PRZED usunięciem sesji + teardown CF/dysku |
| Idle timeout | `auto_lock.rs` (α.A.b, default 15 min) | wait-free `AtomicU64` last_activity; tick loop; aktywność tylko z realnego inputu (POST /touch) + operacji plikowych (CfApi) — auth-calle NIE resetują timera |
| Windows session lock (Win+L) | observer `WM_WTSSESSION_CHANGE` (α.A.b) | natychmiastowy lock |

Wspólny teardown: `lock_flow::force_lock_and_dismount` (lock vault + audit + CF dismount + unmount dysku O:). Lock zeruje `VaultKeyStore` (Vault Key, master, DEK cache) — kolejne operacje wymagają ponownego unlocku.

### 13.2 Zeroize KeyBytes (α.A.c, P2-005)

`KeyBytes` to newtype `pub struct KeyBytes([u8; 32])` z `#[derive(Zeroize, ZeroizeOnDrop)]` (NIE alias) + `Debug` zredukowany do `[REDACTED]` + non-`Copy` (by uniknąć cichych kopii na stosie). Buildery wypełniają klucz **in-place** (`&mut k.0`) — np. `derive_root_keys` pisze wynik Argon2id wprost do `master.0`, eliminując transient plaintext-array. `expose_secret().clone()` zwraca `KeyBytes`, więc transientne klony też mają `ZeroizeOnDrop`.

**Dowód (SMOKE H4, Lenovo):** memdump procesu daemona po `IdleTimeout` lock — known Vault Key: `before.dmp` = 1 trafienie (kontrola), `after.dmp` = **0 trafień** → klucz wyzerowany z RAM po locku.

---

## 14. Device key-exchange — wrap Vault Key dla urządzeń (α.C.a + α.B.b)

> **Rozdział kura-jajko:** to jest warstwa **multi-device key-exchange** (udostępnienie Vault Key nowemu urządzeniu, Epic 33). Jest ROZŁĄCZNA z solo-unlockiem (passphrase → KEK → VK, §3.2). Solo-unlock nie używa niczego z §14.

### 14.1 Tożsamość urządzenia (α.C.a)

Każde urządzenie ma parę kluczy:
- **X25519** — public (32 B) w `devices.public_key`; private (32 B) sealed AES-256-GCM pod KEK tożsamości w `local_device_identity.encrypted_private_key` (60 B: nonce‖ct‖tag).
- **ML-KEM-768** (α.B.b, FIPS 203) — encapsulation key (1184 B) w `devices.kyber_public_key`; decapsulation key (2400 B) sealed pod tym samym KEK tożsamości w `local_device_identity.encrypted_kyber_private_key`.

KEK tożsamości = `HKDF-SHA256-Expand(PRK=master_key, info="omnidrive-identity-kek-v1")`. Keypair generowany idempotentnie w `run_post_unlock_maintenance` (`ensure_device_keypair` X25519, `ensure_device_kyber_keypair` ML-KEM — sibling, backfill dla starych urządzeń). Generacja ML-KEM = non-fatal (warn+retry przy następnym unlocku).

### 14.2 Wrap V2 — X25519 (klasyczny, α.C.a)

```
1. shared = ECDH(my_x25519_private, their_x25519_public)
   guard: shared == [0;32] -> REJECT (low-order point attack)
2. wrapping_key = HKDF-SHA256-Expand(PRK=shared, info="vault-key-wrap-v1")
3. wrapped = AES-256-KW(wrapping_key, Vault Key)   -> 40 B
   -> devices.wrapped_vault_key   (dyskryminator: "v2-x25519")
```

### 14.3 Wrap V3 — hybrid X25519 + ML-KEM-768 (post-quantum, α.B.b)

Cel: **harvest-now-decrypt-later resistance**. Hybrid = X25519 **I** ML-KEM RAZEM (nie zamiana) — złamanie wymaga złamania OBU. ECDH liczone w `angeld`; combiner + AES-KW w `omnidrive_core::hybrid` (core wolny od `x25519-dalek`).

```
1. x25519_ss = ECDH(my_x25519_private, their_x25519_public)   [+ low-order guard]
2. (kyber_ct, mlkem_ss) = ML-KEM-768.Encapsulate(their_kyber_encaps_key)
                          kyber_ct = 1088 B, mlkem_ss = 32 B
3. KEK = HKDF-SHA256(
       salt = "omnidrive-hybrid-wrap-v1",
       ikm  = x25519_ss ‖ mlkem_ss,                 // X-Wing pattern, NIGDY XOR
       info = TLV[ version | vault_id | device_id | kyber_ct | their_kyber_ek ]  )
4. wrapped = AES-256-KW(KEK, Vault Key)   -> 40 B
5. blob = kyber_ct (1088) ‖ wrapped (40)  = 1128 B
   -> devices.wrapped_vault_key_kyber   (dyskryminator: "v3-hybrid")
```

**Decyzje krytyczne:**
- **Combiner = HKDF (X-Wing), NIGDY XOR.** XOR shared-secretów nie jest IND-CCA-bezpieczny przy adwersaryjnym jednym z wejść.
- **„AAD" bez AEAD:** AES-KW (RFC 3394) nie ma parametru AAD. Wiązanie anti-splice/anti-downgrade (`version|vault_id|device_id`) + anti-rebinding (`kyber_ct|ek`, wzorzec X-Wing) realizuje **HKDF-info**. Podmiana któregokolwiek pola ⇒ inny KEK ⇒ porażka integralności AES-KW przy unwrap. Transkrypt jest **length-prefixed (TLV)** — konkatenacja jest jednoznaczna (kanoniczne kodowanie, brak ambiguity injection).
- **Wersja w transkrypcie, nie w blobie:** brak osobnego bajtu wersji w blobie → downgrade-resistant (zmiana `version` w kontekście zmienia KEK). Dyskryminator `v2`/`v3` żyje na poziomie kolumny DB.
- **Implicit rejection (FIPS 203):** ML-KEM dekapsulacja sfałszowanego ciphertextu NIE zwraca błędu — zwraca pseudolosowy shared-secret. Wykrycie fałszerstwa następuje **downstream** na integralności AES-KW (inny mlkem_ss ⇒ inny KEK ⇒ unwrap fail). To jest oczekiwane i poprawne.
- **Zeroize:** każdy transient ss/KEK/IKM zerowany (`zeroize`); `KeyBytes` jest `ZeroizeOnDrop`.

Rozmiary ML-KEM-768: encaps 1184 B, decaps 2400 B, ciphertext 1088 B, shared-secret 32 B.

### 14.4 Selekcja przy unwrap

`identity::select_and_unwrap_vault_key`: jeśli dostępne SĄ jednocześnie hybrydowy blob + lokalny ML-KEM decaps + encaps → użyj **v3-hybrid**; w przeciwnym razie fallback do **v2-x25519**. Pozwala starym (X25519-only) i nowym urządzeniom współistnieć. Re-seal kluczy prywatnych przy migracji KDF (§7.2 krok 4) trzyma decaps/private spójne z aktualnym master.

### 14.5 Zakres α.B.b vs. przyszłość

DoD α.B.b (zrealizowane): solo vault wrapuje VK dla siebie pod OBOMA schematami → oba deszyfrują na ten sam VK (`e2e_solo_vault_both_wraps_decrypt_to_same_vault_key`). Produkcyjny wrap przy `accept_device` jest best-effort (gdy target opublikował kyber key). Pełne wpięcie `select_and_unwrap_vault_key` w onboarding + NULLowanie hybrydowego wrapu przy `revoke_device` = follow-up (patrz przegląd QG5).

---

## 15. Identity grafting — odtworzenie stanu krypto na nowym urządzeniu (α.C.b)

Domyka P1-001/P1-005 (Dell↔Lenovo split-brain: różne EVK ⇒ różne safety-numbers + DEK nie unwrapuje się). `db::graft_restored_metadata_snapshot` (tx `BEGIN IMMEDIATE`) kopiuje z pobranego snapshotu pełen bundle tożsamości vaultu:

| Grafowane | Dlaczego |
|---|---|
| `vault_state.encrypted_vault_key` + `vault_key_generation` + `legacy_read_key` | dołączające urządzenie wyprowadza **TEN SAM** Vault Key ⇒ identyczne safety-numbers (P1-005) |
| tabela `data_encryption_keys` (wipe+copy, `dek_id` verbatim) | dołączające urządzenie unwrapuje istniejące DEK ⇒ czyta istniejące pliki (P1-001) |
| tabela `vault_recovery_keys` (wipe+copy, `id`+`revoked_at` verbatim) | recovery działa identycznie na każdym urządzeniu |

**Świadomie NIE grafowane:** `local_device_identity` (X25519/ML-KEM private + KEK tożsamości) — to własność **per-device** (α.C.a). Każde urządzenie ma własną tożsamość; graft kopiuje stan *vaultu*, nie *urządzenia*. To rozdziela §14 (klucze urządzenia) od §15 (stan vaultu).

DoD α.C.b (zrealizowane in-process): joined EVK == source EVK + safety-numbers identyczne + grafted DEK unwrapuje ten sam plaintext. Live SMOKE Dell↔Lenovo = osobna akceptacja.
