# α.C.b — Graft pełen identity bundle (P1-001 + P1-005 fix)

**Data:** 2026-06-04
**Faza:** α.C.b (Crypto Hardening — Identity & device keys)
**Zależność:** α.C.a (real X25519 keypair) ZAMKNIĘTA — `local_device_identity` niesie realny sealed device-priv per urządzenie.
**Cel:** Naprawić split-brain Dell↔Lenovo: po `Join Existing Vault` urządzenie dołączające ma identyczny stan krypto vaultu (EVK, DEK, recovery), więc safety numbers się zgadzają (P1-005) i hydratacja plików działa (P1-001).

---

## 1. Problem (zweryfikowany w kodzie)

Ścieżka unlocku V2 (`vault.rs:215-268`):
```
derive_root_keys(passphrase, vault_config)  →  root_keys.kek
unwrap_key(kek, vault_state.encrypted_vault_key)  →  EVK (envelope / Vault Key)
```
Safety numbers = `SHA256(EVK ‖ user_id)` (`db.rs:1761`, `vault.rs:149-159`). Per-plikowe DEK = `unwrap_key(EVK, data_encryption_keys.wrapped_dek)` (`vault.rs:396-421`).

`graft_restored_metadata_snapshot` (`db.rs:1796`) kopiuje z `vault_state` **tylko** `master_key_salt, argon2_params, vault_id`. Grafuje już `vault_config`, `users`, `devices`, `vault_members`. **Pomija:**

| Brakujące | Skutek |
| --- | --- |
| `vault_state.encrypted_vault_key` | unlock Della wpada w gałąź „brak EVK → wygeneruj świeży gen=1" (`vault.rs:256/268`) → różny EVK → **różne safety numbers (P1-005)** |
| `vault_state.vault_key_generation` | rozjazd generacji DEK vs EVK |
| `vault_state.legacy_read_key` | brak odczytu legacy V1 chunków (artefakt α.B.a) |
| cała `data_encryption_keys` | nawet z dobrym EVK brak per-plikowych DEK → **`aes-gcm fail` przy hydratacji (P1-001)** |
| cała `vault_recovery_keys` | recovery mnemonik nie działa z urządzenia dołączającego |

**Snapshot zawiera wszystkie te dane** — powstaje przez `VACUUM INTO` (`disaster_recovery.rs:323`), czyli pełną kopię DB (potem szyfrowaną przed uploadem). **Fix jest wyłącznie po stronie graftu** — zero zmian w tworzeniu/uploadzie snapshotu.

---

## 2. Zakres (decyzje zatwierdzone 2026-06-04)

**Minimal faithful-replica** — urządzenie dołączające staje się wiernym replikiem stanu krypto vaultu (te same params co źródło). Per-device adaptacyjne KDF params **poza zakresem** (odłożone z α.B.a, osobny przyszły task).

Graft DODATKOWO adoptuje:
1. `vault_state.encrypted_vault_key`, `vault_key_generation`, `legacy_read_key`
2. cała tabela `data_encryption_keys`
3. cała tabela `vault_recovery_keys` (niesie `wrapped_vault_key` per `vault_id` — globalny stan krypto, nie per-device)

**Nietknięte (per-device, świadomie):** `local_device_identity` (sealed device-priv — Dell ma swój z α.C.a; nadpisanie = adopcja cudzej tożsamości urządzenia = błąd krytyczny).

---

## 3. Podejście

**Rozszerzyć `graft_restored_metadata_snapshot` w miejscu**, wg istniejących wzorców (read z `restored_pool` → wipe w liście DELETE → pętla INSERT, wszystko w istniejącej `BEGIN IMMEDIATE` + `foreign_keys=OFF` tx).

**Odrzucone — wholesale-copy** (podmiana całego pliku DB / kopia wszystkich tabel): zgarnęłoby tabele per-device (`local_device_identity`, potencjalnie `provider_configs`). Selektywny graft musi zostać selektywny.

---

## 4. Komponenty zmiany (wszystko w `db.rs::graft_restored_metadata_snapshot`)

### 4.1 `vault_state` — rozszerzyć read + apply
- `RestoreVaultRecord` + SELECT z snapshotu: dodać `encrypted_vault_key: Option<Vec<u8>>`, `vault_key_generation: Option<i64>`, `legacy_read_key: Option<Vec<u8>>`.
- Apply: rozszerzyć INSERT…ON CONFLICT (obie gałęzie `local_vault` Some/None) o 3 kolumny. **Kluczowe:** EVK + generation + legacy_read_key zawsze z **remote** (źródła), spójne z grafowanym remote `vault_config` (= ta sama para KEK↔wrapped).
- **Backward-compat V1:** `Option` `None` (snapshot sprzed α.B.a) → bind `NULL`, nie wybucha; `post_join_existing` fallback jak dziś.

### 4.2 `data_encryption_keys` — nowy read + wipe + insert
- Read z snapshotu: `dek_id, inode_id, wrapped_dek, key_version, vault_key_gen, created_at` (`unwrap_or_default` — V1 snapshot bez tabeli nie blokuje).
- Dopisać `DELETE FROM data_encryption_keys` do listy wipe (przed inode'ami, FK-bezpiecznie przy `foreign_keys=OFF`).
- Pętla INSERT zachowuje `dek_id` (PK), `vault_key_gen`, `key_version` 1:1.

### 4.3 `vault_recovery_keys` — nowy read + wipe + insert
- Read: `id, vault_id, wrapped_vault_key, vk_generation, created_at, created_by, revoked_at` (`unwrap_or_default`).
- `DELETE FROM vault_recovery_keys` do listy wipe.
- Pętla INSERT 1:1 (zachowuje `revoked_at`, więc revoke historia spójna).

---

## 5. Korektność / Święta Zasada Integralności

- **Spójna para:** `vault_config` (źródło KEK) + `encrypted_vault_key` z **tego samego snapshotu** → unlock unwrapuje właściwy EVK → safety numbers match.
- **All-or-nothing:** wszystko w istniejącej `BEGIN IMMEDIATE TRANSACTION` — crash w połowie = rollback, baza nietknięta.
- **Re-graft / idempotencja:** destrukcyjny overwrite zamierzony — naprawia świeży EVK gen=1 jeśli urządzenie zdążyło odpalić unlock przed joinem.
- **Snapshot/upload bez zmian** (VACUUM INTO już wszystko niesie).
- **`local_device_identity` nietknięte** — chroni pracę α.C.a.
- **Izolacja ścieżek:** zmiana dotyczy wyłącznie DB graftu w trybie join; brak operacji na plikach poza `SYNC_PATH`.

---

## 6. Testy (DoD bramka)

**Rust e2e** (`#[cfg(feature="test-helpers")]`, w `tests/` angeld lub module graftu):
1. Zbuduj źródłowy vault (init + unlock → EVK), zapisz ≥1 inode + ≥1 wrapped DEK + ≥1 recovery key.
2. `create_metadata_snapshot` (VACUUM INTO) do pliku snapshotu.
3. Graft do świeżego, niezależnego DB (symulacja urządzenia dołączającego).
4. Asercje:
   - (a) `encrypted_vault_key`, `vault_key_generation`, `data_encryption_keys`, `vault_recovery_keys` w docelowym DB == źródło (bajt-w-bajt);
   - (b) unlock docelowego DB tym samym hasłem → EVK identyczny ze źródłem;
   - (c) `safety_numbers(user)` docelowy == źródłowy;
   - (d) DEK round-trip — `unwrap_key(EVK, wrapped_dek)` na docelowym się udaje.
5. Test backward-compat: snapshot V1 (bez EVK/DEK/recovery) → graft nie wybucha, pola `NULL`/puste.

**Live akceptacja (osobno, nie blokuje bramki kodu):** SMOKE C3 (safety numbers identyczne Dell↔Lenovo) + D7 (Dell hydrate → SHA256 match z Lenovo). Wymaga Della — robione po zielonej bramce Rust.

---

## 7. Poza zakresem (NIE robić tutaj)

- Bump wersji (po DoD + opcjonalnie live smoke).
- Per-device adaptacyjne KDF params (odłożone z α.B.a — osobny task).
- α.B.b ML-KEM hybrid wrap (decyzje w pamięci `project_alpha_bb_mlkem_design`).
- Zmiany w tworzeniu/uploadzie/szyfrowaniu snapshotu.
- Graft `local_device_identity` (per-device, celowo pominięte).
- Decomposition `db.rs` (osobny task fazy γ).
