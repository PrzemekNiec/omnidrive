# γ.d — Snapshot upload guard — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Domknąć trzy realne luki w ścieżce metadata-backup/restore: uszkodzony obiekt nie może blokować odzyskiwania (G1), lokalna baza dostaje periodyczny `.bak` nawet przy zalockowanym vaulcie (G2), a zdegradowana baza nie awansuje wskaźnika `latest.db.enc` (G3).

**Architecture:** Wszystkie zmiany produkcyjne mieszczą się w `angeld/src/disaster_recovery.rs`; `angeld/src/main.rs` dokłada jeden argument przy starcie workera. Zero migracji schematu (baseline liczników idzie do istniejącej tabeli `system_config`), zero nowych zależności, zero bumpu wersji. Testy działają w całości offline przez `LocalMetadataBackupStore` — struktura jest prywatna, ale moduł testowy leży w tym samym pliku, więc konstruuje się ją wprost, bez zmiennych środowiskowych (te są procesowo-globalne i powodują flaky testy).

**Tech Stack:** Rust 2024, tokio, sqlx (SQLite), aes-gcm, hkdf, `omnidrive_core::crypto` (`RootKdfParams`, `derive_root_keys`, `KeyBytes`).

**Spec:** `docs/superpowers/specs/2026-07-21-gamma-d-snapshot-guard-design.md` (commit `0accee8`).

## Global Constraints

- **Zero nowych zależności.** `angeld/Cargo.toml` nie ma `chrono` ani `time` — formatowanie znacznika UTC jest ręczne (Task 3).
- **Zero migracji schematu.** Baseline liczników w `system_config` przez istniejące `db::get_system_config_value` / `db::set_system_config_value`.
- **Bez bumpu wersji.** Workspace zostaje na `0.3.28`.
- **Zakaz komentarzy w kodzie produkcyjnym** (CLAUDE.md §3). Dozwolony wyłącznie `///` nad publicznym API, gdy WHY jest nieoczywiste.
- **Chirurgiczne zmiany** (CLAUDE.md §1). Jedyny dozwolony refaktor spoza literalnego zakresu to unifikacja parsera nagłówka w Task 1 — wymuszona przez potrzebę wyciągnięcia parametrów KDF.
- **Zero-Knowledge Rule:** żadnych haseł, kluczy ani master key w logach.
- **Pre-push aktywny** (`fmt` + `clippy --workspace -D warnings`). Nigdy `--no-verify`.
- **Bramka przed każdym commitem:** `cargo test -p angeld --lib` musi być zielony (baseline: 174 testy).
- Każdy task = jeden commit. Zakaz `--allow-empty`.

---

## File Structure

| Plik | Rola w tym planie |
|---|---|
| `angeld/src/disaster_recovery.rs` | Cały kod produkcyjny G1/G2/G3 + testy w `mod tests` na końcu pliku. |
| `angeld/src/main.rs:782-786` | Przekazanie `runtime_paths.db_file_path.clone()` do `start_metadata_backup_worker`. |
| `docs/superpowers/plans/2026-07-21-gamma-d-snapshot-guard.md` | Ten plan — odhaczanie checkboxów w trakcie. |
| `STATUS.md` §12.7 | Reconcile wiersza γ.d po zakończeniu (Task 4). |

---

## Task 1: G1 — restore nie wywraca się na jednym zepsutym obiekcie

**Files:**
- Modify: `angeld/src/disaster_recovery.rs:517-580` (`restore_metadata_from_cloud`)
- Modify: `angeld/src/disaster_recovery.rs:906-1009` (`decrypt_metadata_backup`)
- Modify: `angeld/src/disaster_recovery.rs:1011-1072` (`MetadataBackupParsed`, `parse_metadata_backup`)
- Modify: `angeld/src/disaster_recovery.rs:1079-1103` (`decrypt_metadata_backup_with_master` — destrukturyzacja)
- Modify: `angeld/src/disaster_recovery.rs:1481` (istniejący test `encrypts_snapshot_into_expected_binary_format` — nowa sygnatura)
- Test: `angeld/src/disaster_recovery.rs` `mod tests` (dwa nowe testy)

**Interfaces:**
- Produces: `struct MetadataBackupKeyCache` (`Default`), metoda `fn backup_key(&mut self, passphrase: &str, kdf: &RootKdfParams) -> Result<KeyBytes, DisasterRecoveryError>`; `fn decrypt_metadata_backup(encoded: &[u8], passphrase: &str, cache: &mut MetadataBackupKeyCache) -> Result<Vec<u8>, DisasterRecoveryError>` (dodany trzeci parametr); `MetadataBackupParsed` z nowym polem `kdf: RootKdfParams`.
- Consumes: `parse_metadata_backup`, `derive_metadata_backup_key`, `derive_root_keys`, `RootKdfParams`, `KeyBytes` (już zaimportowane w pliku).

- [ ] **Step 1: Write the failing tests**

Dopisz oba testy na końcu `mod tests` w `angeld/src/disaster_recovery.rs` (przed zamykającym `}` modułu):

```rust
    #[tokio::test]
    async fn restore_falls_back_to_older_snapshot_when_latest_is_corrupt()
    -> Result<(), Box<dyn std::error::Error>> {
        let test_root = env::temp_dir().join(format!(
            "omnidrive-dr-restore-fallback-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir_all(&test_root).await?;

        let passphrase = "restore-fallback-passphrase";
        let kdf_params = RootKdfParams::new(1, vec![0x66; 16], 65_536, 3, 1);
        let master_key = derive_root_keys(passphrase.as_bytes(), &kdf_params)?.master_key;

        let source_db = test_root.join("source.db");
        let source_url = format!(
            "sqlite://{}",
            source_db.to_string_lossy().replace('\\', "/")
        );
        let source_pool = db::init_db(&source_url).await?;
        db::set_vault_params(&source_pool, &[0u8; 16], "test", "vault-restore-fallback").await?;
        source_pool.close().await;
        drop(source_pool);

        let enc_path = test_root.join("source.db.enc");
        encrypt_metadata_snapshot(&source_db, &enc_path, &master_key, &kdf_params).await?;

        let store_root = test_root.join("local_store");
        let metadata_dir = store_root.join("_omnidrive\\system\\metadata");
        let snapshots_dir = metadata_dir.join("snapshots");
        fs::create_dir_all(&snapshots_dir).await?;
        fs::copy(&enc_path, snapshots_dir.join("1700000000000.db.enc")).await?;
        fs::write(
            metadata_dir.join("latest.db.enc"),
            b"corrupted-bytes-not-a-backup",
        )
        .await?;

        let pm = MetadataBackupProviderManager {
            uploaders: Vec::new(),
            download_providers: Vec::new(),
            local_store: Some(LocalMetadataBackupStore { root: store_root }),
        };

        let output_db = test_root.join("restored.db");
        restore_metadata_from_cloud(&pm, passphrase, &output_db, None).await?;

        let restored_url = format!(
            "sqlite://{}",
            output_db.to_string_lossy().replace('\\', "/")
        );
        let restored_pool = db::init_db(&restored_url).await?;
        let vault = db::get_vault_params(&restored_pool)
            .await?
            .expect("restored snapshot must have a vault_state row");
        assert_eq!(vault.vault_id, "vault-restore-fallback");
        restored_pool.close().await;

        let _ = fs::remove_dir_all(&test_root).await;
        Ok(())
    }

    #[tokio::test]
    async fn restore_reports_every_candidate_when_all_are_corrupt()
    -> Result<(), Box<dyn std::error::Error>> {
        let test_root = env::temp_dir().join(format!(
            "omnidrive-dr-restore-all-corrupt-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let store_root = test_root.join("local_store");
        let metadata_dir = store_root.join("_omnidrive\\system\\metadata");
        let snapshots_dir = metadata_dir.join("snapshots");
        fs::create_dir_all(&snapshots_dir).await?;

        fs::write(metadata_dir.join("latest.db.enc"), b"corrupt-latest").await?;
        fs::write(snapshots_dir.join("1700000000000.db.enc"), b"corrupt-one").await?;
        fs::write(snapshots_dir.join("1700000001000.db.enc"), b"corrupt-two").await?;

        let pm = MetadataBackupProviderManager {
            uploaders: Vec::new(),
            download_providers: Vec::new(),
            local_store: Some(LocalMetadataBackupStore { root: store_root }),
        };

        let output_db = test_root.join("restored.db");
        let result = restore_metadata_from_cloud(&pm, "any-passphrase", &output_db, None).await;
        let Err(DisasterRecoveryError::DownloadFailed(errors)) = result else {
            panic!("expected DownloadFailed, got {result:?}");
        };
        assert_eq!(
            errors.len(),
            3,
            "every candidate must be reported, no early abort: {errors:?}"
        );

        let _ = fs::remove_dir_all(&test_root).await;
        Ok(())
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p angeld --lib restore_falls_back_to_older_snapshot_when_latest_is_corrupt restore_reports_every_candidate_when_all_are_corrupt`
Expected: FAIL. Pierwszy test kończy się `Err(InvalidBackupFormat("file too short"))` propagowanym z `restore_metadata_from_cloud` (znak `?` na dekrypcji uszkodzonego `latest.db.enc`); drugi panikuje na `expected DownloadFailed, got Err(InvalidBackupFormat("file too short"))`.

- [ ] **Step 3: Rozszerz `MetadataBackupParsed` i parser o parametry KDF**

Zastąp `struct MetadataBackupParsed` (`:1011-1015`) oraz całe `parse_metadata_backup` (`:1017-1072`):

```rust
struct MetadataBackupParsed {
    kdf: RootKdfParams,
    nonce: [u8; METADATA_BACKUP_NONCE_LEN],
    plaintext: Vec<u8>,
    tag: [u8; METADATA_BACKUP_TAG_LEN],
}

fn parse_metadata_backup(encoded: &[u8]) -> Result<MetadataBackupParsed, DisasterRecoveryError> {
    if encoded.len() < METADATA_BACKUP_HEADER_FIXED_LEN + METADATA_BACKUP_TAG_LEN {
        return Err(DisasterRecoveryError::InvalidBackupFormat("file too short"));
    }

    let magic_end = METADATA_BACKUP_MAGIC.len();
    if &encoded[..magic_end] != METADATA_BACKUP_MAGIC {
        return Err(DisasterRecoveryError::InvalidBackupFormat("magic mismatch"));
    }

    let version = encoded[magic_end];
    if version != METADATA_BACKUP_VERSION {
        return Err(DisasterRecoveryError::InvalidBackupFormat(
            "unsupported backup version",
        ));
    }

    let mut cursor = magic_end + 1;
    let salt_len = u16::from_le_bytes(
        encoded[cursor..cursor + 2]
            .try_into()
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("salt_len"))?,
    ) as usize;
    cursor += 2;

    let parameter_set_version = u32::from_le_bytes(
        encoded[cursor..cursor + 4]
            .try_into()
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("parameter_set_version"))?,
    );
    cursor += 4;
    let memory_cost_kib = u32::from_le_bytes(
        encoded[cursor..cursor + 4]
            .try_into()
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("memory_cost_kib"))?,
    );
    cursor += 4;
    let time_cost = u32::from_le_bytes(
        encoded[cursor..cursor + 4]
            .try_into()
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("time_cost"))?,
    );
    cursor += 4;
    let lanes = u32::from_le_bytes(
        encoded[cursor..cursor + 4]
            .try_into()
            .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("lanes"))?,
    );
    cursor += 4;

    let nonce: [u8; METADATA_BACKUP_NONCE_LEN] = encoded
        .get(cursor..cursor + METADATA_BACKUP_NONCE_LEN)
        .ok_or(DisasterRecoveryError::InvalidBackupFormat("nonce"))?
        .try_into()
        .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("nonce"))?;
    cursor += METADATA_BACKUP_NONCE_LEN;

    let salt = encoded
        .get(cursor..cursor + salt_len)
        .ok_or(DisasterRecoveryError::InvalidBackupFormat("salt"))?
        .to_vec();
    cursor += salt_len;

    if encoded.len() < cursor + METADATA_BACKUP_TAG_LEN {
        return Err(DisasterRecoveryError::InvalidBackupFormat(
            "ciphertext missing",
        ));
    }

    let ciphertext_end = encoded.len() - METADATA_BACKUP_TAG_LEN;
    let plaintext = encoded[cursor..ciphertext_end].to_vec();
    let tag: [u8; METADATA_BACKUP_TAG_LEN] = encoded[ciphertext_end..]
        .try_into()
        .map_err(|_| DisasterRecoveryError::InvalidBackupFormat("tag"))?;

    Ok(MetadataBackupParsed {
        kdf: RootKdfParams::new(
            parameter_set_version,
            salt,
            memory_cost_kib,
            time_cost,
            lanes,
        ),
        nonce,
        plaintext,
        tag,
    })
}
```

- [ ] **Step 4: Zastąp `decrypt_metadata_backup` wersją z cache'em derywacji**

Zastąp całe `fn decrypt_metadata_backup` (`:906-1009`) poniższym blokiem (cache + funkcja przepięta na wspólny parser):

```rust
type MetadataBackupKdfKey = (u32, Vec<u8>, u32, u32, u32);

#[derive(Default)]
struct MetadataBackupKeyCache {
    entries: Vec<(MetadataBackupKdfKey, KeyBytes)>,
}

impl MetadataBackupKeyCache {
    fn backup_key(
        &mut self,
        passphrase: &str,
        kdf: &RootKdfParams,
    ) -> Result<KeyBytes, DisasterRecoveryError> {
        let cache_key: MetadataBackupKdfKey = (
            kdf.parameter_set_version,
            kdf.salt.clone(),
            kdf.memory_cost_kib,
            kdf.time_cost,
            kdf.lanes,
        );
        if let Some((_, key)) = self
            .entries
            .iter()
            .find(|(existing, _)| *existing == cache_key)
        {
            return Ok(key.clone());
        }

        let root_keys = derive_root_keys(passphrase.as_bytes(), kdf)
            .map_err(|_| DisasterRecoveryError::BackupDecryptFailed)?;
        let backup_key = derive_metadata_backup_key(&root_keys.master_key)?;
        self.entries.push((cache_key, backup_key.clone()));
        Ok(backup_key)
    }
}

fn decrypt_metadata_backup(
    encoded: &[u8],
    passphrase: &str,
    cache: &mut MetadataBackupKeyCache,
) -> Result<Vec<u8>, DisasterRecoveryError> {
    let MetadataBackupParsed {
        kdf,
        nonce,
        mut plaintext,
        tag,
    } = parse_metadata_backup(encoded)?;

    let metadata_backup_key = cache.backup_key(passphrase, &kdf)?;
    let cipher = Aes256Gcm::new_from_slice(&metadata_backup_key)
        .map_err(|_| DisasterRecoveryError::BackupDecryptFailed)?;

    cipher
        .decrypt_in_place_detached(
            Nonce::from_slice(&nonce),
            &[],
            &mut plaintext,
            aes_gcm::Tag::from_slice(&tag),
        )
        .map_err(|_| DisasterRecoveryError::BackupDecryptFailed)?;

    Ok(plaintext)
}
```

- [ ] **Step 5: Dostosuj `decrypt_metadata_backup_with_master` do nowego pola**

W `decrypt_metadata_backup_with_master` (`:1083-1087`) zamień destrukturyzację, żeby ignorowała `kdf`:

```rust
    let MetadataBackupParsed {
        nonce,
        mut plaintext,
        tag,
        ..
    } = parse_metadata_backup(encoded)?;
```

- [ ] **Step 6: Przepnij obie pętle w `restore_metadata_from_cloud` na `continue`**

W `restore_metadata_from_cloud` dodaj cache tuż pod `let mut errors = Vec::new();` (`:524`):

```rust
    let mut key_cache = MetadataBackupKeyCache::default();
```

W pętli `local_store` zamień `:541`:

```rust
            let plaintext = match decrypt_metadata_backup(&encoded, passphrase, &mut key_cache) {
                Ok(plaintext) => plaintext,
                Err(err) => {
                    errors.push(format!("local-metadata-store {key}: {err}"));
                    continue;
                }
            };
```

W pętli po providerach zamień `:568`:

```rust
            let plaintext = match decrypt_metadata_backup(&encoded, passphrase, &mut key_cache) {
                Ok(plaintext) => plaintext,
                Err(err) => {
                    errors.push(format!("{} {}: {}", provider.provider_name, key, err));
                    continue;
                }
            };
```

- [ ] **Step 7: Zaktualizuj istniejący test do nowej sygnatury**

W teście `encrypts_snapshot_into_expected_binary_format` zamień linię `:1481`:

```rust
        let decrypted = decrypt_metadata_backup(
            &encoded,
            passphrase,
            &mut MetadataBackupKeyCache::default(),
        )?;
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test -p angeld --lib restore_falls_back_to_older_snapshot_when_latest_is_corrupt restore_reports_every_candidate_when_all_are_corrupt encrypts_snapshot_into_expected_binary_format decrypt_with_master`
Expected: PASS — 5 testów zielonych (2 nowe + 1 zaktualizowany + 2 `decrypt_with_master_*`).

- [ ] **Step 9: Bramka i commit**

```bash
cargo fmt --all
cargo clippy -p angeld --all-targets -- -D warnings
cargo test -p angeld --lib
git add angeld/src/disaster_recovery.rs
git commit -m "fix(dr): restore skips undecryptable snapshots instead of aborting"
```
Expected: clippy czysty, suite 176 testów zielona (174 baseline + 2).

---

## Task 2: G3 — sanity guard wskaźnika `latest.db.enc`

**Files:**
- Modify: `angeld/src/disaster_recovery.rs:614-749` (`upload_metadata_backup` — nowy parametr + dwa miejsca PUT `latest`)
- Modify: `angeld/src/disaster_recovery.rs:751-777` (`run_metadata_backup_now`)
- Test: `angeld/src/disaster_recovery.rs` `mod tests` (dwa nowe testy)

**Interfaces:**
- Consumes: `MetadataBackupProviderManager`, `LocalMetadataBackupStore` z Task 1 (bez zmian).
- Produces: `struct SnapshotHealth { has_vault_state: bool, has_vault_config: bool, inode_count: i64, dek_count: i64 }`; `async fn collect_snapshot_health(pool: &SqlitePool) -> Result<SnapshotHealth, DisasterRecoveryError>`; `fn latest_pointer_may_advance(health: &SnapshotHealth, previous_inode_count: Option<i64>, previous_dek_count: Option<i64>) -> bool`; `upload_metadata_backup(db_pool, provider_manager, enc_file_path, advance_latest: bool)` — czwarty parametr; stałe `LAST_SNAPSHOT_INODE_COUNT_KEY` / `LAST_SNAPSHOT_DEK_COUNT_KEY`.
- Sygnatura `run_metadata_backup_now` **nie zmienia się** — wołają ją `api/auth.rs:389` i `api/maintenance.rs:674`.

- [ ] **Step 1: Write the failing tests**

Dopisz na końcu `mod tests`:

```rust
    #[test]
    fn latest_pointer_advance_decision_table() {
        let healthy = SnapshotHealth {
            has_vault_state: true,
            has_vault_config: true,
            inode_count: 3,
            dek_count: 2,
        };
        assert!(latest_pointer_may_advance(&healthy, Some(1240), Some(87)));

        let no_vault_state = SnapshotHealth {
            has_vault_state: false,
            has_vault_config: true,
            inode_count: 3,
            dek_count: 2,
        };
        assert!(!latest_pointer_may_advance(&no_vault_state, Some(1), Some(1)));

        let no_vault_config = SnapshotHealth {
            has_vault_state: true,
            has_vault_config: false,
            inode_count: 3,
            dek_count: 2,
        };
        assert!(!latest_pointer_may_advance(&no_vault_config, Some(1), Some(1)));

        let emptied_inodes = SnapshotHealth {
            has_vault_state: true,
            has_vault_config: true,
            inode_count: 0,
            dek_count: 2,
        };
        assert!(!latest_pointer_may_advance(&emptied_inodes, Some(1240), Some(87)));

        let emptied_deks = SnapshotHealth {
            has_vault_state: true,
            has_vault_config: true,
            inode_count: 3,
            dek_count: 0,
        };
        assert!(!latest_pointer_may_advance(&emptied_deks, Some(1240), Some(87)));

        let fresh_vault = SnapshotHealth {
            has_vault_state: true,
            has_vault_config: true,
            inode_count: 0,
            dek_count: 0,
        };
        assert!(latest_pointer_may_advance(&fresh_vault, Some(0), Some(0)));
        assert!(latest_pointer_may_advance(&fresh_vault, None, None));
    }

    #[tokio::test]
    async fn degraded_database_uploads_snapshot_without_advancing_latest()
    -> Result<(), Box<dyn std::error::Error>> {
        let test_root = env::temp_dir().join(format!(
            "omnidrive-dr-guard-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir_all(&test_root).await?;

        let db_path = test_root.join("degraded.db");
        let db_url = format!("sqlite://{}", db_path.to_string_lossy().replace('\\', "/"));
        let pool = db::init_db(&db_url).await?;
        db::set_vault_params(&pool, &[0u8; 16], "test", "vault-guard-test").await?;
        db::set_vault_config(&pool, &[0x11u8; 16], 1, 65_536, 3, 1).await?;
        db::set_system_config_value(&pool, LAST_SNAPSHOT_INODE_COUNT_KEY, "1240").await?;
        db::set_system_config_value(&pool, LAST_SNAPSHOT_DEK_COUNT_KEY, "87").await?;

        let store_root = test_root.join("local_store");
        let metadata_dir = store_root.join("_omnidrive\\system\\metadata");
        fs::create_dir_all(&metadata_dir).await?;
        let latest_path = metadata_dir.join("latest.db.enc");
        fs::write(&latest_path, b"last-good-snapshot-bytes").await?;

        let pm = MetadataBackupProviderManager {
            uploaders: Vec::new(),
            download_providers: Vec::new(),
            local_store: Some(LocalMetadataBackupStore {
                root: store_root.clone(),
            }),
        };

        run_metadata_backup_now(&pool, &pm, &[0x42u8; 32]).await?;

        assert_eq!(
            fs::read(&latest_path).await?,
            b"last-good-snapshot-bytes".to_vec(),
            "latest pointer must not advance for a degraded database"
        );

        let snapshots_dir = metadata_dir.join("snapshots");
        let mut entries = fs::read_dir(&snapshots_dir).await?;
        let mut snapshot_count = 0usize;
        while let Some(_entry) = entries.next_entry().await? {
            snapshot_count += 1;
        }
        assert_eq!(
            snapshot_count, 1,
            "the timestamped snapshot must still be uploaded"
        );

        assert_eq!(
            db::get_system_config_value(&pool, LAST_SNAPSHOT_INODE_COUNT_KEY).await?,
            Some("1240".to_string()),
            "baseline must stay at the last good value while the guard is engaged"
        );

        pool.close().await;
        let _ = fs::remove_dir_all(&test_root).await;
        Ok(())
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p angeld --lib latest_pointer_advance_decision_table degraded_database_uploads_snapshot_without_advancing_latest`
Expected: FAIL na etapie kompilacji — `cannot find type SnapshotHealth`, `cannot find function latest_pointer_may_advance`, `cannot find value LAST_SNAPSHOT_INODE_COUNT_KEY`.

- [ ] **Step 3: Dodaj stałe, `SnapshotHealth`, zbieranie i decyzję**

Wstaw bezpośrednio nad `pub async fn upload_metadata_backup` (`:614`):

```rust
const LAST_SNAPSHOT_INODE_COUNT_KEY: &str = "last_snapshot_inode_count";
const LAST_SNAPSHOT_DEK_COUNT_KEY: &str = "last_snapshot_dek_count";

struct SnapshotHealth {
    has_vault_state: bool,
    has_vault_config: bool,
    inode_count: i64,
    dek_count: i64,
}

async fn collect_snapshot_health(
    pool: &SqlitePool,
) -> Result<SnapshotHealth, DisasterRecoveryError> {
    let has_vault_state = db::get_vault_params(pool).await?.is_some();
    let has_vault_config = db::get_vault_config(pool).await?.is_some();
    let inode_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inodes")
        .fetch_one(pool)
        .await?;
    let dek_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM data_encryption_keys")
        .fetch_one(pool)
        .await?;

    Ok(SnapshotHealth {
        has_vault_state,
        has_vault_config,
        inode_count,
        dek_count,
    })
}

async fn read_snapshot_counter(
    pool: &SqlitePool,
    config_key: &str,
) -> Result<Option<i64>, DisasterRecoveryError> {
    Ok(db::get_system_config_value(pool, config_key)
        .await?
        .and_then(|value| value.parse::<i64>().ok()))
}

fn latest_pointer_may_advance(
    health: &SnapshotHealth,
    previous_inode_count: Option<i64>,
    previous_dek_count: Option<i64>,
) -> bool {
    if !health.has_vault_state || !health.has_vault_config {
        return false;
    }
    if health.inode_count == 0 && previous_inode_count.unwrap_or(0) > 0 {
        return false;
    }
    if health.dek_count == 0 && previous_dek_count.unwrap_or(0) > 0 {
        return false;
    }
    true
}
```

- [ ] **Step 4: Dodaj parametr `advance_latest` do `upload_metadata_backup`**

Zmień sygnaturę (`:614-618`):

```rust
pub async fn upload_metadata_backup(
    db_pool: &SqlitePool,
    provider_manager: &MetadataBackupProviderManager,
    enc_file_path: &Path,
    advance_latest: bool,
) -> Result<(), DisasterRecoveryError> {
```

W gałęzi `local_store` zamień `match local_store.upload_file(enc_file_path, &snapshot_key).await { ... }` (`:646-674`) na:

```rust
        match local_store.upload_file(enc_file_path, &snapshot_key).await {
            Ok(()) => {
                let latest_result = if advance_latest {
                    local_store
                        .upload_file(enc_file_path, latest_key)
                        .await
                        .map_err(|err| err.to_string())
                } else {
                    Ok(())
                };

                match latest_result {
                    Ok(()) => {
                        successful_uploads += 1;
                        db::update_metadata_backup_status(db_pool, &backup_id, "COMPLETED", None)
                            .await?;
                    }
                    Err(err) => {
                        let error_text = format!("latest pointer update failed: {err}");
                        db::update_metadata_backup_status(
                            db_pool,
                            &backup_id,
                            "FAILED",
                            Some(&error_text),
                        )
                        .await?;
                        warn!(
                            "metadata backup latest pointer update failed for {}: {}",
                            local_store.provider_name(),
                            err
                        );
                    }
                }
            }
            Err(err) => {
                let error_text = err.to_string();
                db::update_metadata_backup_status(db_pool, &backup_id, "FAILED", Some(&error_text))
                    .await?;
            }
        }
```

W pętli po `uploaders` zamień ramię `Ok(_) => match uploader.upload_system_file(enc_file_path, latest_key).await { ... }` (`:700-721`) na:

```rust
            Ok(_) => {
                let latest_result = if advance_latest {
                    uploader
                        .upload_system_file(enc_file_path, latest_key)
                        .await
                        .map(|_| ())
                        .map_err(|err| err.to_string())
                } else {
                    Ok(())
                };

                match latest_result {
                    Ok(()) => {
                        successful_uploads += 1;
                        db::update_metadata_backup_status(db_pool, &backup_id, "COMPLETED", None)
                            .await?;
                    }
                    Err(err) => {
                        let error_text = format!("latest pointer update failed: {err}");
                        db::update_metadata_backup_status(
                            db_pool,
                            &backup_id,
                            "FAILED",
                            Some(&error_text),
                        )
                        .await?;
                        warn!(
                            "metadata backup latest pointer update failed for {}: {}",
                            uploader.provider_name(),
                            err
                        );
                    }
                }
            }
```

- [ ] **Step 5: Wepnij guard w `run_metadata_backup_now`**

Zastąp całe ciało `run_metadata_backup_now` (`:751-777`):

```rust
pub async fn run_metadata_backup_now(
    db_pool: &SqlitePool,
    provider_manager: &MetadataBackupProviderManager,
    master_key: &[u8],
) -> Result<(), DisasterRecoveryError> {
    let health = collect_snapshot_health(db_pool).await?;
    let previous_inode_count = read_snapshot_counter(db_pool, LAST_SNAPSHOT_INODE_COUNT_KEY).await?;
    let previous_dek_count = read_snapshot_counter(db_pool, LAST_SNAPSHOT_DEK_COUNT_KEY).await?;
    let advance_latest =
        latest_pointer_may_advance(&health, previous_inode_count, previous_dek_count);

    if !advance_latest {
        warn!(
            "metadata snapshot sanity guard engaged: latest pointer will NOT advance \
            (vault_state={}, vault_config={}, inodes {:?} -> {}, deks {:?} -> {}). \
            The timestamped snapshot is still uploaded and the previous latest stays recoverable.",
            health.has_vault_state,
            health.has_vault_config,
            previous_inode_count,
            health.inode_count,
            previous_dek_count,
            health.dek_count
        );
    }

    let temp_enc_path = temporary_encrypted_backup_path();
    let create_result =
        create_encrypted_metadata_snapshot(db_pool, &temp_enc_path, master_key).await;

    if let Err(err) = create_result {
        let _ = secure_delete(&temp_enc_path).await;
        return Err(err);
    }

    let upload_result =
        upload_metadata_backup(db_pool, provider_manager, &temp_enc_path, advance_latest).await;
    let cleanup_result = secure_delete(&temp_enc_path).await;

    if let Err(err) = cleanup_result {
        warn!(
            "metadata backup temp cleanup failed for {}: {}",
            temp_enc_path.display(),
            err
        );
    }

    if upload_result.is_ok() && advance_latest {
        db::set_system_config_value(
            db_pool,
            LAST_SNAPSHOT_INODE_COUNT_KEY,
            &health.inode_count.to_string(),
        )
        .await?;
        db::set_system_config_value(
            db_pool,
            LAST_SNAPSHOT_DEK_COUNT_KEY,
            &health.dek_count.to_string(),
        )
        .await?;
    }

    upload_result
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p angeld --lib latest_pointer_advance_decision_table degraded_database_uploads_snapshot_without_advancing_latest`
Expected: PASS — 2 testy zielone.

- [ ] **Step 7: Bramka i commit**

```bash
cargo fmt --all
cargo clippy -p angeld --all-targets -- -D warnings
cargo test -p angeld --lib
git add angeld/src/disaster_recovery.rs
git commit -m "feat(dr): sanity guard keeps latest pointer on last good snapshot"
```
Expected: clippy czysty, suite 178 testów zielona.

---

## Task 3: G2 — periodyczny lokalny `.bak` co 24h

**Files:**
- Modify: `angeld/src/disaster_recovery.rs:31-34` (nowe stałe obok istniejących)
- Modify: `angeld/src/disaster_recovery.rs:247-307` (`start_metadata_backup_worker` — nowy parametr + krok w pętli)
- Modify: `angeld/src/main.rs:782-786` (przekazanie ścieżki bazy)
- Test: `angeld/src/disaster_recovery.rs` `mod tests` (dwa nowe testy)

**Interfaces:**
- Produces: `fn format_utc_compact(time: SystemTime) -> Result<String, DisasterRecoveryError>` (format `YYYYMMDD_HHMMSS`, UTC); `fn civil_from_days(days: i64) -> (i64, u32, u32)`; `fn local_backup_timestamp_suffix<'a>(file_name: &'a str, db_file_name: &str) -> Option<&'a str>`; `async fn run_local_db_backup_if_due(db_pool: &SqlitePool, db_path: &Path, now: SystemTime) -> Result<Option<PathBuf>, DisasterRecoveryError>`; `start_metadata_backup_worker(db_pool, provider_manager, keystore, db_file_path: Option<PathBuf>)` — czwarty parametr.
- Consumes: `sqlite_string_literal` (`:1115`), `SystemTime`/`UNIX_EPOCH` (już zaimportowane `:19`), `Duration` (`:22`).

- [ ] **Step 1: Write the failing tests**

Dopisz na końcu `mod tests`:

```rust
    #[test]
    fn formats_utc_timestamp_for_backup_names() {
        assert_eq!(
            format_utc_compact(UNIX_EPOCH).expect("epoch formats"),
            "19700101_000000"
        );
        assert_eq!(
            format_utc_compact(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
                .expect("known timestamp formats"),
            "20231114_221320"
        );
    }

    #[tokio::test]
    async fn local_db_backup_rotates_and_ignores_manual_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let test_root = env::temp_dir().join(format!(
            "omnidrive-dr-local-bak-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir_all(&test_root).await?;

        let db_path = test_root.join("omnidrive.db");
        let db_url = format!("sqlite://{}", db_path.to_string_lossy().replace('\\', "/"));
        let pool = db::init_db(&db_url).await?;
        db::create_inode(&pool, None, "bak-test.txt", "FILE", 7).await?;

        let manual_path = test_root.join("omnidrive.db.bak.preSmoke-20260604-1422");
        fs::write(&manual_path, b"manual-backup").await?;
        fs::write(test_root.join("omnidrive.db.bak.20200101_000000"), b"old").await?;
        fs::write(test_root.join("omnidrive.db.bak.20210101_000000"), b"older").await?;

        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let first = run_local_db_backup_if_due(&pool, &db_path, now)
            .await?
            .expect("first run must create a backup");
        assert_eq!(
            first.file_name().and_then(|value| value.to_str()),
            Some("omnidrive.db.bak.20231114_221320")
        );
        assert!(fs::try_exists(&first).await?);

        assert!(
            run_local_db_backup_if_due(&pool, &db_path, now + Duration::from_secs(3_600))
                .await?
                .is_none(),
            "a second run within 24h must be a no-op"
        );

        let later = now + Duration::from_secs(25 * 3_600);
        let second = run_local_db_backup_if_due(&pool, &db_path, later)
            .await?
            .expect("a run after 24h must create a backup");
        assert_eq!(
            second.file_name().and_then(|value| value.to_str()),
            Some("omnidrive.db.bak.20231115_231320")
        );

        assert!(
            !fs::try_exists(test_root.join("omnidrive.db.bak.20200101_000000")).await?,
            "retention must drop the oldest backup"
        );
        assert!(fs::try_exists(test_root.join("omnidrive.db.bak.20210101_000000")).await?);
        assert!(fs::try_exists(&first).await?);
        assert!(fs::try_exists(&second).await?);
        assert!(
            fs::try_exists(&manual_path).await?,
            "manual backups outside the YYYYMMDD_HHMMSS pattern must never be rotated away"
        );

        let restored_url = format!("sqlite://{}", second.to_string_lossy().replace('\\', "/"));
        let restored_pool = db::init_db(&restored_url).await?;
        let inode = db::get_inode_by_id(&restored_pool, 1).await?;
        assert!(inode.is_some(), "the backup must be a usable SQLite copy");
        restored_pool.close().await;

        pool.close().await;
        let _ = fs::remove_dir_all(&test_root).await;
        Ok(())
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p angeld --lib formats_utc_timestamp_for_backup_names local_db_backup_rotates_and_ignores_manual_files`
Expected: FAIL na kompilacji — `cannot find function format_utc_compact`, `cannot find function run_local_db_backup_if_due`.

- [ ] **Step 3: Dodaj stałe**

Pod istniejącymi stałymi workerów (`:34`):

```rust
const LOCAL_DB_BACKUP_MIN_INTERVAL: Duration = Duration::from_secs(60 * 60 * 24);
const LOCAL_DB_BACKUP_RETENTION: usize = 3;
```

- [ ] **Step 4: Dodaj formatowanie znacznika i skaner nazw**

Wstaw bezpośrednio nad `pub fn start_metadata_backup_worker` (`:247`):

```rust
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_position + 2) / 5 + 1) as u32;
    let month = if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    } as u32;

    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn format_utc_compact(time: SystemTime) -> Result<String, DisasterRecoveryError> {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| DisasterRecoveryError::InvalidOutputPath("timestamp before unix epoch"))?
        .as_secs();
    let (year, month, day) = civil_from_days((seconds / 86_400) as i64);
    let seconds_of_day = seconds % 86_400;

    Ok(format!(
        "{year:04}{month:02}{day:02}_{:02}{:02}{:02}",
        seconds_of_day / 3_600,
        (seconds_of_day % 3_600) / 60,
        seconds_of_day % 60
    ))
}

fn local_backup_timestamp_suffix<'a>(file_name: &'a str, db_file_name: &str) -> Option<&'a str> {
    let suffix = file_name.strip_prefix(&format!("{db_file_name}.bak."))?;
    if suffix.len() != 15 || suffix.as_bytes()[8] != b'_' {
        return None;
    }
    if !suffix[..8].bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if !suffix[9..].bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    Some(suffix)
}

async fn run_local_db_backup_if_due(
    db_pool: &SqlitePool,
    db_path: &Path,
    now: SystemTime,
) -> Result<Option<PathBuf>, DisasterRecoveryError> {
    let (Some(parent), Some(db_file_name)) = (
        db_path.parent(),
        db_path.file_name().and_then(|value| value.to_str()),
    ) else {
        return Ok(None);
    };
    let Some(cutoff_time) = now.checked_sub(LOCAL_DB_BACKUP_MIN_INTERVAL) else {
        return Ok(None);
    };

    let mut stamps = Vec::new();
    let mut entries = fs::read_dir(parent).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if let Some(stamp) = local_backup_timestamp_suffix(name, db_file_name) {
            stamps.push(stamp.to_string());
        }
    }
    stamps.sort();

    let cutoff = format_utc_compact(cutoff_time)?;
    if let Some(newest) = stamps.last()
        && newest.as_str() >= cutoff.as_str()
    {
        return Ok(None);
    }

    let stamp = format_utc_compact(now)?;
    let backup_path = parent.join(format!("{db_file_name}.bak.{stamp}"));
    if fs::try_exists(&backup_path).await? {
        fs::remove_file(&backup_path).await?;
    }

    let sql = format!("VACUUM INTO '{}'", sqlite_string_literal(&backup_path));
    sqlx::query(&sql).execute(db_pool).await?;

    stamps.push(stamp);
    stamps.sort();
    while stamps.len() > LOCAL_DB_BACKUP_RETENTION {
        let stale = stamps.remove(0);
        let stale_path = parent.join(format!("{db_file_name}.bak.{stale}"));
        if let Err(err) = fs::remove_file(&stale_path).await {
            warn!(
                "local database backup rotation failed for {}: {}",
                stale_path.display(),
                err
            );
        }
    }

    Ok(Some(backup_path))
}
```

- [ ] **Step 5: Wepnij krok w worker**

Zmień sygnaturę `start_metadata_backup_worker` (`:247-251`):

```rust
pub fn start_metadata_backup_worker(
    db_pool: SqlitePool,
    provider_manager: Arc<MetadataBackupProviderManager>,
    keystore: Arc<VaultKeyStore>,
    db_file_path: Option<PathBuf>,
) -> JoinHandle<()> {
```

Wstaw krok bezpośrednio po `ticker.tick().await;` (`:258`), przed pobraniem `last_success`:

```rust
            if let Some(db_path) = db_file_path.as_deref() {
                match run_local_db_backup_if_due(&db_pool, db_path, SystemTime::now()).await {
                    Ok(Some(path)) => info!("local database backup created: {}", path.display()),
                    Ok(None) => {}
                    Err(err) => warn!("local database backup failed: {err}"),
                }
            }
```

- [ ] **Step 6: Przekaż ścieżkę bazy z `main.rs`**

W `angeld/src/main.rs:782-786` zamień wywołanie:

```rust
    let metadata_backup_worker = start_metadata_backup_worker(
        pool.clone(),
        metadata_backup_provider_manager.clone(),
        Arc::new(vault_keys.clone()),
        runtime_paths.db_file_path.clone(),
    );
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p angeld --lib formats_utc_timestamp_for_backup_names local_db_backup_rotates_and_ignores_manual_files`
Expected: PASS — 2 testy zielone.

- [ ] **Step 8: Bramka i commit**

```bash
cargo fmt --all
cargo clippy -p angeld --all-targets -- -D warnings
cargo test -p angeld --lib
git add angeld/src/disaster_recovery.rs angeld/src/main.rs
git commit -m "feat(dr): periodic local database backup with retention"
```
Expected: clippy czysty (także dla targetu `bin`, bo zmieniło się `main.rs`), suite 180 testów zielona.

---

## Task 4: Bramka pełna, dokumentacja i smoke

**Files:**
- Modify: `STATUS.md:708` (drzewko γ) i `STATUS.md:716` (wiersz tabeli γ.d)
- Modify: `docs/superpowers/plans/2026-07-21-gamma-d-snapshot-guard.md` (odhaczone checkboxy)

**Interfaces:**
- Consumes: wyniki Tasków 1-3. Nic nie produkuje dla kolejnych tasków.

- [ ] **Step 1: Pełna bramka workspace**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features test-helpers -- -D warnings
cargo build --release --workspace
cargo test -p angeld --lib
cargo test -p omnidrive-core
```
Expected: wszystko zielone; angeld 180 testów (174 baseline + 6 nowych), omnidrive-core 28.

- [ ] **Step 2: Zaktualizuj STATUS.md §12.7**

W drzewku (`STATUS.md:708`) zamień wiersz γ.d na:

```
└── γ.d — Snapshot upload guard        ✅ DONE — spec-premisa moot (append-only + advance-on-success); domknięte 3 realne luki: restore-fallback, lokalny .bak/24h, sanity guard latest
```

W tabeli (`STATUS.md:716`) zamień komórkę stanu wiersza **γ.d** na:

```
✅ **DONE 2026-07-21.** Spec `docs/superpowers/specs/2026-07-21-gamma-d-snapshot-guard-design.md`, plan `…/plans/2026-07-21-gamma-d-snapshot-guard.md`. Premisa specowa („nie nadpisuj dobrego snapshotu") potwierdzona jako **już spełniona strukturalnie**: klucz timestampowany (append-only), `latest.db.enc` awansuje wyłącznie po udanym uploadzie snapshotu, 0 sukcesów → `NoSuccessfulUploads` bez przesunięcia markera. Audyt wyciągnął 3 inne luki i te zostały domknięte: **G1** `restore_metadata_from_cloud` przerywał całe odzyskiwanie na pierwszym nieodszyfrowywalnym obiekcie (a `latest.db.enc` jest kandydatem pierwszym) → teraz `continue` + cache derywacji Argon2id per parametry KDF; **G2** periodyczny `omnidrive.db.bak.YYYYMMDD_HHMMSS` co 24h z retencją 3, wykonywany PRZED `require_master_key` (działa przy zalockowanym vaulcie, gdy snapshot chmurowy w ogóle nie powstaje), rotacja ignoruje ręczne `.bak.preSmoke-*`; **G3** sanity guard — brak `vault_state`/`vault_config` albo spadek `inodes`/`data_encryption_keys` do zera przy niezerowym baseline blokuje wyłącznie awans `latest`, timestampowany snapshot leci normalnie, baseline w `system_config` nie przesuwa się do czasu powrotu liczników. 6 testów (2 restore-fallback, 2 guard, 2 lokalny `.bak`), suite angeld **180** green.
```

- [ ] **Step 3: Commit dokumentacji**

```bash
git add STATUS.md docs/superpowers/plans/2026-07-21-gamma-d-snapshot-guard.md
git commit -m "docs(status): γ.d snapshot upload guard DONE"
```

- [ ] **Step 4: Smoke na Lenovo (akcja Przemka, nie bramkuje kodu)**

Uruchom daemona z `target/release` na realnej bazie i zweryfikuj:
1. W katalogu żywej bazy pojawia się `omnidrive.db.bak.YYYYMMDD_HHMMSS` (pierwszy tick workera, ≤1h od startu; log `local database backup created:`).
2. Ręczne `omnidrive.db.bak.preSmoke-*` / `.preCleanup-*` nadal istnieją.
3. Normalny backup dalej awansuje wskaźnik — log `metadata backup worker uploaded a fresh recovery snapshot` **bez** linii `metadata snapshot sanity guard engaged`.

Wynik smoke dopisz do STATUS.md §12.7 jako osobny commit.

---

## Self-Review

**Pokrycie speca:**

| Wymaganie speca | Task |
|---|---|
| G1 `continue` zamiast `?` w obu pętlach | Task 1, Step 6 |
| G1 cache derywacji per parametry KDF | Task 1, Steps 4 |
| G1 unifikacja parsera nagłówka | Task 1, Steps 3+5 |
| G1 test fallbacku + test braku wczesnego abortu | Task 1, Step 1 (testy 1-2 speca) |
| G2 `.bak` przed `require_master_key` | Task 3, Step 5 |
| G2 `Option<PathBuf>` z `main.rs`, `:memory:` → brak `.bak` | Task 3, Steps 5-6 |
| G2 UTC `YYYYMMDD_HHMMSS`, retencja 3, ochrona plików ręcznych | Task 3, Steps 4+1 (test 5 speca) |
| G3 kryterium drop-do-zera + struktura | Task 2, Step 3 |
| G3 blokada tylko awansu `latest`, snapshot leci dalej | Task 2, Step 4 |
| G3 status `COMPLETED`, baseline bez przesunięcia | Task 2, Steps 4-5 |
| G3 tabelka decyzji + test integracyjny | Task 2, Step 1 (testy 3-4 speca) |
| Smoke G2 | Task 4, Step 4 |
| DoD: pełna bramka | Task 4, Step 1 |

**Spójność typów:** `MetadataBackupKeyCache` (Task 1) używana wyłącznie w Task 1; `SnapshotHealth` / `latest_pointer_may_advance` / `LAST_SNAPSHOT_*_KEY` zdefiniowane i konsumowane w Task 2; `run_local_db_backup_if_due` / `format_utc_compact` zdefiniowane i konsumowane w Task 3. Sygnatura `run_metadata_backup_now` niezmieniona → `api/auth.rs:389` i `api/maintenance.rs:674` nietknięte. Jedyna zmiana sygnatury dotykająca `main.rs` to `start_metadata_backup_worker` (Task 3, Step 6).

**Uwaga wykonawcza:** `angeld` kompiluje się dualnie (lib + bin, P2-003), więc `clippy --all-targets` musi być czysty także dla targetu `bin` po zmianie `main.rs`.
