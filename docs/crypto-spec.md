# OmniDrive Crypto Spec — Envelope Encryption (Format V2)

**Status:** DRAFT / RFC  
**Data:** 2026-04-06  
**Autor:** Claude + Przemek  
**Dotyczy:** Phase 0 Crypto Checkpoint, Epic 32.5  

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
https://share.omnidrive.app/{share_id}#{base64url(DEK)}
```

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
| KDF | Argon2id v0x13 | m=64 MiB, t=3, p=1, output=256-bit |
| KEK derivation | HKDF-SHA256 | info=`"kek-v2"` |
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

## 10. Security considerations

- **Vault Key NIGDY nie opuszcza pamieci w plaintexcie** — na dysku tylko jako `encrypted_vault_key` (AES-KW wrapped)
- **DEK NIGDY nie jest logowany** — `[REDACTED]` w tracing (zgodnie z CLAUDE.md zero-knowledge rule)
- **Passphrase -> master_key jest jednokierunkowy** — Argon2id z soleniem
- **AES-KW jest deterministic** — ten sam KEK + ten sam Vault Key = ten sam ciphertext. To jest OK i zamierzone (brak nonce = brak ryzyka nonce reuse)
- **Random nonce per chunk w V2** — eliminuje content equality leaking miedzy urzadzeniami
- **chunk_id w V2 uzywa DEK nie vault_key** — dwa urzadzenia z roznym DEK dla tego samego pliku produkuja rozne chunk_id (brak cross-device deduplication, ale tez brak leaking)
