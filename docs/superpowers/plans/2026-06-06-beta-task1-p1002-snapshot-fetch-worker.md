# Faza β — Task 1: P1-002 Snapshot Fetch Worker (Roster-merge only)

> **Problem (P1-002):** daemon ma snapshot **upload** worker (`start_metadata_backup_worker`) ale NIE ma symetrycznego **fetch** workera. Skutek: gdy urządzenie B dołącza do vaultu, urządzenie A (już działające) nigdy nie pobiera nowszego snapshotu → MultiDevice tab A nie pokazuje B aż do restartu daemona.
>
> **Strategia (ZATWIERDZONA): ROSTER-MERGE ONLY.** Aplikacja pobranego snapshotu jest **ściśle addytywna** — wyłącznie `devices` + `vault_members` (`INSERT OR IGNORE`). **KATEGORYCZNY ZAKAZ** dotykania `data_encryption_keys`, `vault_state`, `vault_recovery_keys`, `local_device_identity`. NIGDY nie wołamy `graft_restored_metadata_snapshot` (robi wipe+copy DEK → data-loss lokalnych kluczy aktywnego urządzenia). Pełna CRDT-sync crypto-state między współaktywnymi urządzeniami = faza δ.
>
> **Tryb:** TDD subagent-driven (implementer → spec-review → code-quality per sub-task). Wzorzec workera: `start_metadata_backup_worker`. Bramka `--all-targets` po całości, push, bez bumpu wersji.

**Święta Zasada Integralności Danych:** każdy sub-task dotykający aplikacji snapshotu MUSI mieć test-strażnik dowodzący, że `data_encryption_keys` (i `vault_state`) są **bajt-w-bajt nietknięte** przed/po. To bramka data-safety całego Taska.

---

## Kontekst (ugruntowany w kodzie)

- **Worker wzorcowy** `disaster_recovery::start_metadata_backup_worker` (`:244`): `interval(TICK)` + `MissedTickBehavior::Skip`, `db::get_last_successful_metadata_backup_at` (last-success guard), `keystore.require_master_key()` (skip gdy locked), `run_metadata_backup_now` (testowalna jednostka), `diagnostics::set_worker_status`. Zwraca `JoinHandle<()>`.
- **Snapshot key:** `_omnidrive/system/metadata/snapshots/{created_at}.db.enc` (timestamp w kluczu → newest-wins po `created_at`).
- **Szyfrowanie:** `encrypt_metadata_snapshot(input, output, master_key, kdf_params)` → nagłówek `MAGIC|ver|salt_len|param_set|m|t|p|nonce|salt|ciphertext|tag`, klucz = `derive_metadata_backup_key(master_key)` = `HKDF-from_prk(master_key)`.
- **Deszyfrowanie (PUŁAPKA):** `decrypt_metadata_backup(encoded, passphrase)` (`:697`) **re-derive'uje** master z `Argon2id(passphrase, embedded_salt, embedded_params)`. Worker **NIE MA passphrase** (nieretainowane — zero-knowledge), ma tylko `master_key`. → patrz T1.3.
- **Provider listing:** `MetadataBackupProviderManager` + `provider.list_snapshot_keys(pool, limit)` / `local_store.list_snapshot_keys(limit)` (oba w `restore_metadata_from_cloud` `:449`). Pobranie+decode → bajty `encoded`.
- **Graft wzorzec (no-ATTACH):** `db::graft_restored_metadata_snapshot` (`:1831`) otwiera snapshot jako osobny `SqlitePool` (`sqlite:...?mode=ro`), czyta przez `RestoredDevice`/`RestoredVaultMember` structs, INSERT do głównej bazy. Istniejący graft robi `DELETE FROM vault_members/devices` + pełny copy — **my tego NIE robimy.**
  - `RestoredDevice` (`:1788`): device_id, user_id, device_name, public_key, wrapped_vault_key, vault_key_generation, revoked_at, last_seen_at, created_at, safety_numbers_verified_at, enrolled_at.
  - `RestoredVaultMember` (`:1802`): user_id, vault_id, role, invited_by, joined_at.
  - INSERT-y wzorcowe: devices `:2147`, vault_members `:2169` (pełne listy kolumn).
- **Marker:** `db::get_system_config_value` (`:2513`) / `db::set_system_config_value` (`:2543`) — key/value store (używany przez auto-lock).

---

## File Structure

- **Modify** `angeld/src/db.rs` — NEW `graft_roster_additive` + structs reuse; (opcjonalnie) typed marker helpery.
- **Modify** `angeld/src/disaster_recovery.rs` — NEW `decrypt_metadata_backup_with_master`, `run_metadata_fetch_now`, `start_metadata_fetch_worker`.
- **Modify** `angeld/src/main.rs` — spawn fetch worker w 3 trybach uruchomienia (`~:397`, `~:698`, `~:790`).
- Testy: `db.rs mod tests` (T1.1, T1.2), `disaster_recovery.rs mod tests` (T1.3, T1.4, T1.6).

Bez nowych zależności. Bez zmian schematu. Bez bumpu wersji.

---

## T1.1 — `graft_roster_additive` (rdzeń data-safety) [db.rs]

Czysto addytywny merge `devices` + `vault_members` ze snapshotu. **To jest najważniejszy, najbardziej data-safety-krytyczny krok — robimy go pierwszy z najmocniejszymi testami.**

- [ ] **Krok 1 — failing testy** (`db.rs mod tests`):
  1. `graft_roster_additive_adds_missing_device` — target ma vault + owner usera + device A; zbuduj snapshot-DB (osobny plik/`init_db`) z device A **i** B (ten sam owner user_id); `graft_roster_additive(&target, &snap_path)` → `get_device(&target, "B")` zwraca B; A nadal jest.
  2. `graft_roster_additive_does_not_clobber_existing_device` — target ma device A z `revoked_at = Some` i `safety_numbers_verified_at = Some`; snapshot ma device A z `revoked_at = None`. Po grafcie **lokalny A NIETKNIĘTY** (`revoked_at` nadal Some) — `INSERT OR IGNORE` nie nadpisuje.
  3. **🛡️ GUARD `graft_roster_additive_never_touches_dek`** — target ma wpis w `data_encryption_keys` (np. przez `set_wrapped_dek`/insert) + `vault_state`; snapshot zawiera INNE DEK-i i inny `vault_state` (ale TEN SAM vault_id). Po grafcie: liczba i bajty wierszy `data_encryption_keys` lokalnie **identyczne** (snapshotowe DEK-i NIE wczytane); `vault_state.encrypted_vault_key`/`vault_key_generation` **niezmienione**; `vault_recovery_keys` niezmienione. **To bramka Świętej Zasady.**
  4. **🛡️ `graft_roster_additive_rejects_foreign_vault`** — snapshot z INNYM `vault_state.vault_id` niż lokalny → `graft_roster_additive` zwraca `Err(...)` i **NIE wstawia żadnego wiersza** (`devices`/`vault_members` count niezmieniony). Defense-in-depth: izolacja przestrzeni vaultu PRZED jakimkolwiek INSERT-em, niezależnie od tagu GCM.

- [ ] **Krok 2 — RUN, expect FAIL (undefined):** `cargo test -p angeld --lib graft_roster_additive`
- [ ] **Krok 3 — implementacja.** Po `graft_restored_metadata_snapshot` dodać:
```rust
pub struct RosterMergeSummary { pub devices_added: u64, pub members_added: u64 }

/// Additively merges ONLY the `devices` and `vault_members` rosters from a
/// decrypted snapshot into the live DB (INSERT OR IGNORE — existing rows are
/// never overwritten). Deliberately does NOT touch data_encryption_keys,
/// vault_state, vault_recovery_keys, or local_device_identity — applying a
/// peer snapshot on an ACTIVE device must never clobber local crypto state
/// (data-loss). Used by the periodic snapshot fetch worker (P1-002).
///
/// Rejects the snapshot (error, no rows written) if its `vault_state.vault_id`
/// differs from `expected_vault_id` — vault-namespace isolation enforced before
/// any INSERT, not relying on the GCM tag alone (defense-in-depth).
pub async fn graft_roster_additive(
    pool: &SqlitePool,
    snapshot_db_path: &Path,
    expected_vault_id: &str,
) -> Result<RosterMergeSummary, sqlx::Error> {
    // open snapshot as separate RO pool (no ATTACH — Windows lock-safe), mirror graft_restored_metadata_snapshot
    // STEP 0 (BEFORE any roster read/insert): read snapshot vault_state.vault_id;
    //   if it != expected_vault_id -> return Err (foreign vault rejected). NO INSERT.
    // read r_devices (RestoredDevice) + r_vault_members (RestoredVaultMember) — REUSE existing SELECTs
    // for each: INSERT OR IGNORE INTO devices (...) / vault_members (...) — count rows_affected
    // NO DELETE. NO other tables.
}
```
Reuse `RestoredDevice`/`RestoredVaultMember` (move out of `graft_restored_metadata_snapshot`'s body to module scope if needed, or duplicate the SELECT — implementer decides minimal-diff). Use `INSERT OR IGNORE` with the EXACT column lists from `:2147`/`:2169`. Sum `rows_affected()` for the summary. The vault_id mismatch error should be a distinct, log-friendly variant (e.g. `sqlx::Error::Protocol("snapshot vault_id mismatch ...")` mirroring the existing "missing vault_state row" pattern at `:1861`).
> **PK do ON CONFLICT:** devices PK = `device_id`; vault_members PK = `(user_id, vault_id)` — `INSERT OR IGNORE` honoruje istniejące PK/UNIQUE bez nadpisania. Precondition (single-user-multi-device): owner `users` row już lokalnie istnieje (nowe wiersze to peer DEVICES tego samego ownera) → FK satysfakcjonowane; jeśli `vault_members.invited_by` wskazuje brakującego usera, IGNORE nie failuje przy foreign_keys=OFF (kontekst jak istniejący graft).
- [ ] **Krok 4 — RUN, expect PASS** (3 testy, w tym GUARD). `cargo fmt --all`.
- [ ] **Krok 5 — commit:** `git commit -am "feat(db): β.1 graft_roster_additive — additive devices+members merge, no DEK touch (P1-002)"`

---

## T1.2 — marker ostatnio zaaplikowanego snapshotu [db.rs]

Newer-wins na poziomie SELEKCJI snapshotu + idempotencja workera.

- [ ] **Krok 1 — failing test** `roster_snapshot_marker_round_trips`: `set_last_applied_roster_snapshot_at(&pool, 1234)` → `get_last_applied_roster_snapshot_at(&pool)` == `Some(1234)`; domyślnie `None`.
- [ ] **Krok 2 — RUN, expect FAIL.**
- [ ] **Krok 3 — implementacja:** cienkie typed helpery nad `get/set_system_config_value` z kluczem `"last_applied_roster_snapshot_at"` (i64 jako string). 
- [ ] **Krok 4 — RUN, expect PASS.** `cargo fmt --all`.
- [ ] **Krok 5 — commit:** `git commit -am "feat(db): β.1 last_applied_roster_snapshot marker (P1-002)"`

---

## T1.3 — `decrypt_metadata_backup_with_master` [disaster_recovery.rs]

Worker ma `master_key`, NIE passphrase. Snapshot single-user-multi-device szyfrowany pod `HKDF(master)` gdzie master jest identyczny na wszystkich urządzeniach (wspólne passphrase+salt+params z graftu vault_state). Wariant deszyfrujący kluczem master, bez Argon2 re-derivation.

- [ ] **Krok 1 — failing testy** (`disaster_recovery.rs mod tests`, mirror istniejącego testu encrypt/decrypt `:1097`):
  1. `decrypt_with_master_roundtrip` — `encrypt_metadata_snapshot(.., master, params)` → `decrypt_metadata_backup_with_master(&encoded, master.as_ref())` == plaintext.
  2. `decrypt_with_master_rejects_wrong_master` — inny master → `Err(BackupDecryptFailed)` (tag GCM fail).
- [ ] **Krok 2 — RUN, expect FAIL (undefined).**
- [ ] **Krok 3 — implementacja.** Skopiuj parsowanie nagłówka z `decrypt_metadata_backup` (`:697`) — magic/version/salt_len/params/nonce/salt/ciphertext/tag — ale ZAMIAST `derive_root_keys(passphrase, ...)` użyj `derive_metadata_backup_key(master_key)` wprost (params/salt z nagłówka są wtedy ignorowane dla derywacji klucza; nadal parsowane by znaleźć offsety nonce/ciphertext). Wspólny helper parsujący można wyodrębnić (DRY) lub zduplikować minimalnie — implementer decyduje (preferencja: mały prywatny `parse_backup_header(encoded) -> (nonce, salt, params, ct, tag)` używany przez oba). `///` docstring OK.
- [ ] **Krok 4 — RUN, expect PASS.** `cargo fmt --all`.
- [ ] **Krok 5 — commit:** `git commit -am "feat(recovery): β.1 decrypt_metadata_backup_with_master (P1-002)"`

---

## T1.4 — `run_metadata_fetch_now` + `start_metadata_fetch_worker` [disaster_recovery.rs]

- [ ] **Krok 1 — failing integration test** `fetch_now_merges_peer_device_additively` (`disaster_recovery.rs mod tests`), in-process z LOCAL store (jak testy restore):
  - Zbuduj device A pool (vault + owner + device A + 1 wpis DEK).
  - Zbuduj "peer" snapshot DB (owner + device A + device B), zaszyfruj `encrypt_metadata_snapshot(.., master, params)` i wgraj do local store pod kluczem `_omnidrive/system/metadata/snapshots/{created_at}.db.enc` (reuse helperów upload local store).
  - `run_metadata_fetch_now(&poolA, &provider_manager_localonly, master.as_ref()).await?` → zwraca `Some(summary{devices_added:1})`.
  - **Assert:** `get_device(&poolA,"B")` = Some; **GUARD:** `data_encryption_keys` poolA identyczne (1 wpis, bajty niezmienione); marker == created_at snapshotu.
  - Drugie wywołanie `run_metadata_fetch_now` → `Ok(None)` (marker równy newest → no-op, idempotencja).
- [ ] **Krok 2 — RUN, expect FAIL.**
- [ ] **Krok 3 — implementacja.**
  - `run_metadata_fetch_now(pool, provider_manager, master_key) -> Result<Option<RosterMergeSummary>, DisasterRecoveryError>`: odczytaj **lokalny** `vault_id` (`db::get_vault_params(pool)` → `vault_id`); zlistuj klucze snapshotów (local store + providery), wyznacz **newest** po `created_at` z klucza; jeśli `created_at <= marker` → `Ok(None)`; pobierz `encoded`, `decrypt_metadata_backup_with_master(&encoded, master_key)` (błąd deszyfracji = warn+`Ok(None)`, NIE advance marker), zapisz plaintext do temp `.db` (reuse `write_plaintext_snapshot_if_valid` + `snapshot_has_vault_state_row`), `db::graft_roster_additive(pool, &temp, &local_vault_id)` (**mismatch vault_id = warn+`Ok(None)`, NIE advance marker** — obcy vault odrzucony), `set_last_applied_roster_snapshot_at(pool, created_at)`, audit-log wpis (devices_added), secure-delete temp. Zwróć `Some(summary)`.
  - `start_metadata_fetch_worker(db_pool, provider_manager, keystore) -> JoinHandle<()>`: mirror `start_metadata_backup_worker` — `interval(METADATA_FETCH_WORKER_TICK)`, `MissedTickBehavior::Skip`, własny MIN_INTERVAL (np. 1h per STATUS β.b), `keystore.require_master_key()` (skip gdy locked), `run_metadata_fetch_now`, `diagnostics::set_worker_status(WorkerKind::MetadataFetch?, ...)` (dodać wariant enuma jeśli trzeba — albo reuse istniejącego; implementer sprawdzi `WorkerKind`).
- [ ] **Krok 4 — RUN, expect PASS.** `cargo fmt --all`.
- [ ] **Krok 5 — commit:** `git commit -am "feat(recovery): β.1 metadata fetch worker — run_metadata_fetch_now + loop (P1-002)"`

---

## T1.5 — spawn w main.rs (3 tryby uruchomienia)

- [ ] **Krok 1 — implementacja.** Tam gdzie `start_metadata_backup_worker(...)` jest wołane (`main.rs ~:781`) + spawn-sites (`~:397/:698/:790`), dodać `start_metadata_fetch_worker(pool.clone(), metadata_backup_provider_manager.clone(), Arc::new(vault_keys.clone()))` i dorzucić jego `JoinHandle` do `tokio::select!`/task-set tak jak `metadata_backup_task`. Provider manager współdzielony (Arc). Sprawdzić wszystkie 3 tryby (pełny daemon, oraz pozostałe ścieżki run) — dodać tylko tam, gdzie backup worker już żyje.
- [ ] **Krok 2 — `cargo build -p angeld`** + `cargo fmt --all`.
- [ ] **Krok 3 — commit:** `git commit -am "feat(daemon): β.1 spawn metadata fetch worker in all run modes (P1-002)"`

> Brak nowego testu (orkiestracja spawnu) — pokryte przez T1.4 unit + T1.6 e2e. Worker w testach domyślnie nie wstaje (testy wołają `run_metadata_fetch_now` bezpośrednio).

---

## T1.6 — DoD e2e (konsolidacja) [disaster_recovery.rs mod tests]

- [ ] **Krok 1 — test** `e2e_existing_device_learns_peer_via_fetch_without_dek_loss` (jeśli nie pokryte w pełni przez T1.4, dodać scenariusz „dwa cykle + revoke-survival"):
  - Pełny scenariusz P1-002: device A (z DEK + zrewokowanym starym device) ; peer snapshot dodaje device B ; `run_metadata_fetch_now` ; **assert:** B widoczny, A's DEK niezmienione, A's revoke-state zachowane, marker advanced, drugi tick no-op.
- [ ] **Krok 2 — RUN, expect PASS.**
- [ ] **Krok 3 — commit:** `git commit -am "test(recovery): β.1 e2e fetch learns peer device, DEK intact (P1-002)"`

---

## Bramka końcowa

- [ ] Pełna bramka (mirror pre-push):
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features test-helpers -- -D warnings
cargo build --release --workspace
cargo test -p omnidrive-core
cargo test -p angeld --lib
```
- [ ] **DoD Task 1 (P1-002):**
  - `graft_roster_additive` — dodaje peer device addytywnie; **NIGDY** nie modyfikuje `data_encryption_keys`/`vault_state` (GUARD test zielony); nie nadpisuje istniejących wierszy.
  - `decrypt_metadata_backup_with_master` — deszyfracja kluczem master (worker bez passphrase).
  - `run_metadata_fetch_now` — newest-wins, idempotentny (marker), best-effort (skip na lock/decrypt-fail), audit.
  - fetch worker wpięty w main.rs (3 tryby).
  - e2e: aktywne urządzenie uczy się peera bez utraty lokalnych DEK.
- [ ] **Bez bumpu wersji.** Push (pre-push aktywny, nigdy `--no-verify`).
- [ ] Po DoD: `KNOWN_ISSUES` P1-002 → Closed (β.b). STATUS §12.6 β.b → DONE. **Live SMOKE Dell↔Lenovo (Dell join → Lenovo widzi Della po ≤1 tick) = osobna akceptacja operacyjna, NIE bramkuje DONE kodu.**

---

## Granice (NIE w tym Tasku)
- ŻADNEGO `graft_restored_metadata_snapshot` w ścieżce workera (wipe+copy DEK = data-loss).
- Brak sync `data_encryption_keys`/`vault_state`/`vault_recovery_keys` między współaktywnymi urządzeniami (CRDT → faza δ).
- Brak zmian w upload workerze, share-linkach, FFI, P1-003/004 (snapshot redundancy = β.c).
- Brak UI (MultiDevice tab już renderuje `devices` — fetch tylko zasila tabelę).
