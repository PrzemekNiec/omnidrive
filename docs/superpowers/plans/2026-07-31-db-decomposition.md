# Dekompozycja `angeld/src/db.rs` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rozbić `angeld/src/db.rs` (10 649 linii) na katalog `angeld/src/db/` z 30 plikami tematycznymi, bez żadnej zmiany zachowania.

**Architecture:** `db/mod.rs` deklaruje podmoduły i re-eksportuje je globem (`pub use inodes::*;`), dzięki czemu wszystkie 912 call-site'ów `db::foo()` poza katalogiem pozostają nietknięte. Przenoszenie idzie falami: każda fala wycina grupę pokrewnych domen z kurczącego się `mod.rs` do plików docelowych i kończy się zielonym `cargo check --all-targets` + `cargo test -p angeld --lib`. Blok testowy zostaje w `mod.rs` do ostatniej fali (kompiluje się przez re-eksporty), potem jest rozdzielany per domena.

**Tech Stack:** Rust Edition 2024, sqlx (runtime `query`/`query_as`, zero compile-time macro), tokio, SQLite.

**Spec:** `docs/superpowers/specs/2026-07-31-db-decomposition-design.md`
**Baza (SHA sprzed refaktoru):** `942a442` — wszystkie numery linii w tym planie odnoszą się do `git show 942a442:angeld/src/db.rs`.

## Global Constraints

- **ZERO zmian zachowania.** Przenoszony kod jest kopiowany dosłownie: identyczny SQL, identyczne sygnatury, identyczna kolejność operacji, identyczne komentarze i docstringi. Jedyne dozwolone modyfikacje to: nagłówek `use` per plik, słowo kluczowe widoczności (`pub(super)`) tam gdzie wskazuje §5 spec-a, oraz `use super::*` → `use crate::db::*` w blokach testowych.
- **ZERO zmian poza `angeld/src/db.rs` i `angeld/src/db/**`.** `git diff --stat` nie może pokazać żadnej innej ścieżki kodu.
- **ZERO nowych testów.** To refaktor — bezpiecznikiem jest kompilator i istniejąca suita.
- **ZERO migracji schematu, ZERO bumpu wersji.** `init_db` przenosi się bez jednego znaku zmiany.
- **Liczniki suity są sztywne:** `cargo test -p omnidrive-core` = **28**, `cargo test -p angeld --lib` = **199**. Każda inna liczba to błąd do zdiagnozowania, nie do zaakceptowania.
- **Postawa lintowa bez zmian:** `#![allow(clippy::too_many_arguments, dead_code)]` zostaje wyłącznie w `db/mod.rs`. Nie wolno dopisywać nowych `#[allow]` w plikach docelowych, żeby uciszyć lint — jeśli lint zapala się na przeniesionym kodzie, to sygnał do weryfikacji poprawności przeniesienia.
- **Bramka przed pushem:** `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings` w obu trybach (default + `--features test-helpers`) + `cargo build --release --workspace` + obie suity. Pre-push hook aktywny, nigdy `--no-verify`.

---

### Task 0: Rename `db.rs` → `db/mod.rs` + skrypty weryfikacyjne

**Files:**
- Rename: `angeld/src/db.rs` → `angeld/src/db/mod.rs`
- Create (scratchpad, NIE w repo): `<scratchpad>/verify_symbols.sh`, `<scratchpad>/verify_bodies.sh`

**Interfaces:**
- Produces: katalog `angeld/src/db/` z jednym plikiem `mod.rs` identycznym co do bajtu z dotychczasowym `db.rs`; dwa skrypty weryfikacyjne używane po każdej fali.

- [ ] **Step 1: Zapisz baseline pliku**

```bash
git show 942a442:angeld/src/db.rs > "$SCRATCH/db_baseline.rs"
wc -l "$SCRATCH/db_baseline.rs"   # oczekiwane: 10649
```

- [ ] **Step 2: Rename przez git**

```bash
mkdir -p angeld/src/db
git mv angeld/src/db.rs angeld/src/db/mod.rs
```

- [ ] **Step 3: Zweryfikuj, że to czysty rename**

Run: `git status --short`
Expected: dokładnie jedna linia `R  angeld/src/db.rs -> angeld/src/db/mod.rs` (100% similarity, zero zmian treści).

- [ ] **Step 4: Napisz skrypt weryfikacji zbioru symboli**

`$SCRATCH/verify_symbols.sh` — porównuje posortowaną listę publicznych sygnatur baseline'u z listą z całego katalogu `db/`:

```bash
#!/usr/bin/env bash
set -euo pipefail
SCRATCH="$(dirname "$0")"
REPO="C:/Users/Przemek/Desktop/aplikacje/omnidrive"

grep -E '^(pub (async )?fn|pub struct|pub enum|pub const|pub type)' \
  "$SCRATCH/db_baseline.rs" | sed 's/[[:space:]]*$//' | sort > "$SCRATCH/sym_before.txt"

cat "$REPO"/angeld/src/db/*.rs \
  | grep -E '^(pub (async )?fn|pub struct|pub enum|pub const|pub type)' \
  | sed 's/[[:space:]]*$//' | sort > "$SCRATCH/sym_after.txt"

if diff -u "$SCRATCH/sym_before.txt" "$SCRATCH/sym_after.txt"; then
  echo "SYMBOLS OK: $(wc -l < "$SCRATCH/sym_before.txt") publicznych sygnatur bez zmian"
else
  echo "SYMBOL DRIFT — patrz diff wyzej"; exit 1
fi
```

- [ ] **Step 5: Napisz skrypt weryfikacji ciał funkcji**

`$SCRATCH/verify_bodies.sh` — dzieli oba źródła na bloki top-level (element zaczyna się w kolumnie 0), normalizuje białe znaki, hashuje i porównuje posortowane hashe:

```bash
#!/usr/bin/env bash
set -euo pipefail
SCRATCH="$(dirname "$0")"
REPO="C:/Users/Przemek/Desktop/aplikacje/omnidrive"

hash_blocks() {
  awk '
    /^(pub )?(async )?fn |^(pub )?struct |^(pub )?enum |^impl /{ if (blk != "") print blk; blk=$0; next }
    { if (blk != "") blk = blk "\n" $0 }
    END { if (blk != "") print blk }
  ' RS='\n' ORS='\n\x01' "$@" \
  | tr -d '\r' \
  | while IFS= read -r -d $'\x01' block; do
      printf '%s' "$block" | tr -s '[:space:]' ' ' | sha256sum | cut -d' ' -f1
    done | sort
}

hash_blocks "$SCRATCH/db_baseline.rs" > "$SCRATCH/bodies_before.txt"
hash_blocks "$REPO"/angeld/src/db/*.rs > "$SCRATCH/bodies_after.txt"

comm -3 "$SCRATCH/bodies_before.txt" "$SCRATCH/bodies_after.txt" > "$SCRATCH/bodies_diff.txt"
if [ -s "$SCRATCH/bodies_diff.txt" ]; then
  echo "BODY DRIFT: $(wc -l < "$SCRATCH/bodies_diff.txt") blokow sie rozjechalo"
  exit 1
else
  echo "BODIES OK: $(wc -l < "$SCRATCH/bodies_before.txt") blokow identycznych"
fi
```

Uwaga: skrypt jest narzędziem diagnostycznym, nie wyrocznią. Bloki, które celowo zmieniają widoczność (`fn normalize_policy_path` → `pub(super) fn`) wypadną jako różnica — to oczekiwane i musi być policzone ręcznie przeciw liście z §5 spec-a. Każda inna różnica = błąd.

- [ ] **Step 6: Uruchom bramkę i zacommituj rename**

```bash
cargo check --workspace --all-targets
cargo test -p angeld --lib 2>&1 | tail -5     # oczekiwane: 199 passed
git commit -m "refactor(db): rename db.rs -> db/mod.rs (bez zmian tresci)"
```

---

### Task 1 (Fala F1): `schema.rs` + `graft.rs`

Dwa największe, w pełni samodzielne bloki. Zdejmują ~1 700 linii i od razu robią miejsce.

**Files:**
- Create: `angeld/src/db/schema.rs`, `angeld/src/db/graft.rs`
- Modify: `angeld/src/db/mod.rs`

**Interfaces:**
- Produces: `db::init_db`, `db::graft_restored_metadata_snapshot`, `db::graft_roster_additive`, `db::VaultRestoreApplyReport`, `db::RosterMergeSummary` — wszystkie dostępne pod dotychczasowymi ścieżkami dzięki `pub use`.
- Consumes: `epoch_secs` z `mod.rs` (używany w `graft_roster_additive:2408`).

- [ ] **Step 1: Utwórz `schema.rs`**

Przenieś z `mod.rs` (linie baseline'u):
- `init_db` — 592–1336
- `ensure_column_exists` — 6714–6735 (prywatny, jedyny konsument to `init_db`)

Nagłówek pliku:

```rust
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::str::FromStr;
```

- [ ] **Step 2: Utwórz `graft.rs`**

Przenieś z `mod.rs`:
- `VaultRestoreApplyReport` — 97–106 (wraz z atrybutami nad linią 97)
- 16 struktur `Restored*` — 1662–1843 (`RestoredInode`, `RestoredRevision`, `RestoredSyncPolicy`, `RestoredSmartSyncState`, `RestoredMetadataBackup`, `RestoredPack`, `RestoredPackShard`, `RestoredPackLocation`, `RestoredChunkRef`, `RestoredConflictEvent`, `RestoredProviderConfig`, `RestoredUser`, `RestoredDevice`, `RestoredVaultMember`, `RestoredDek`, `RestoredRecoveryKey`)
- `graft_restored_metadata_snapshot` — 1844–2477
- `RosterMergeSummary` — 2478–2482
- `graft_roster_additive` — 2483–2612

Nagłówek pliku:

```rust
use crate::db::epoch_secs;
use serde::Serialize;
use sqlx::{FromRow, Row, SqlitePool};
```

- [ ] **Step 3: Podłącz moduły w `mod.rs`**

Na górze `mod.rs`, tuż pod inner attribute:

```rust
pub mod graft;
pub mod schema;

pub use graft::*;
pub use schema::*;
```

- [ ] **Step 4: Bramka fali**

```bash
cargo check --workspace --all-targets
cargo test -p angeld --lib 2>&1 | tail -5     # 199 passed
bash "$SCRATCH/verify_symbols.sh"
```

Jeśli `check` zgłasza brakujące importy — dopisz brakujący `use` do nagłówka nowego pliku. Jeśli zgłasza cokolwiek innego (nieznana nazwa, zła widoczność, niezgodność typów), to znak, że przeniesienie nie było dosłowne — cofnij i przenieś ponownie.

- [ ] **Step 5: Commit**

```bash
git add angeld/src/db/
git commit -m "refactor(db): wydziel schema.rs i graft.rs"
```

---

### Task 2 (Fala F2): `vault_state.rs`, `system_config.rs`, `providers.rs`, `migration_v2.rs`

Domena crypto/config — niskie sprzężenie z resztą.

**Files:**
- Create: `angeld/src/db/vault_state.rs`, `angeld/src/db/system_config.rs`, `angeld/src/db/providers.rs`, `angeld/src/db/migration_v2.rs`
- Modify: `angeld/src/db/mod.rs`

**Interfaces:**
- Produces: `db::get_vault_params`, `db::set_vault_config`, `db::migrate_kdf_params_tx`, `db::set_migration_failpoint`, `db::apply_cloud_usage_delta_with_limits`, `db::upsert_provider_config`, `db::get_v1_packs_for_migration` i pozostałe wymienione niżej.

- [ ] **Step 1: Utwórz `vault_state.rs`**

Przenieś: `VaultRecord` 86–96, `VaultConfigRecord` 107–117, `set_vault_params` 1548–1573, `get_vault_params` 1574–1586, `store_encrypted_vault_key` 1587–1606, `WrappedDekRecord` 1607–1616, `get_wrapped_dek` 1617–1633, `insert_wrapped_dek` 1634–1661, `get_vault_config` 2614–2626, `set_vault_config` 2627–2665, `get_all_wrapped_deks` 6945–6956, `update_wrapped_dek` 6957–6977, `rotate_vault_state` 6978–7003, `rotate_vault_key_only` 7004–7021, `KdfMigrationWrites` 7022–7033, `MIGRATION_FAILPOINT` + `set_migration_failpoint` 7034–7043 (**zachowaj oba `#[cfg(feature = "test-helpers")]`**), `get_legacy_read_key` 7044–7051, `migrate_kdf_params_tx` 7052–7098, `RewrapQueueItem` 7099–7108, `enqueue_deks_for_rewrap` 7109–7126, `get_pending_rewrap_batch` 7127–7152, `complete_rewrap_item` 7153–7161, `fail_rewrap_item` 7162–7179, `get_rewrap_status` 7180–7195, `get_deks_by_generation` 7196–7212.

- [ ] **Step 2: Utwórz `system_config.rs`**

Przenieś: `SystemConfigRecord` 262–270, `CloudUsageDailyRecord` 500–508, `CloudUsageDelta` 509–515, `CloudUsageApplyResult` 516–527, `get_system_config_value` 2666–2682, `list_system_config` 2683–2695, `set_system_config_value` 2696–2728, `get_last_applied_roster_snapshot_at` 2729–2739, `set_last_applied_roster_snapshot_at` 2740–2752, `get_cloud_usage_for_day` 2753–2769, `apply_cloud_usage_delta_with_limits` 2770–2871.

- [ ] **Step 3: Utwórz `providers.rs`**

Przenieś: `ProviderConfigRecord` 271–287, `ProviderSecretRecord` 288–296, `get_provider_config` 2872–2900, `list_provider_configs` 2901–2927, `upsert_provider_config` 2928–2991, `delete_provider_config` 2992–3003, `get_provider_secret` 3004–3025, `upsert_provider_secret` 3026–3061.

- [ ] **Step 4: Utwórz `migration_v2.rs`**

Przenieś: `V1PackForMigration` 6830–6845, `get_v1_packs_for_migration` 6846–6881, `count_all_packs` 6882–6889, `count_v1_packs` 6890–6902, `mark_pack_migrated_v2` 6903–6933, `finalize_vault_format_v2` 6934–6944.

- [ ] **Step 5: Podłącz moduły i uruchom bramkę fali**

Dopisz `pub mod` + `pub use` dla czterech nowych modułów, potem:

```bash
cargo check --workspace --all-targets
cargo check --workspace --all-targets --features test-helpers
cargo test -p angeld --lib 2>&1 | tail -5     # 199 passed
bash "$SCRATCH/verify_symbols.sh"
```

Drugie `check` z feature'em jest tu obowiązkowe — `set_migration_failpoint` jest za flagą i domyślny build go nie kompiluje.

- [ ] **Step 6: Commit**

```bash
git add angeld/src/db/
git commit -m "refactor(db): wydziel vault_state, system_config, providers, migration_v2"
```

---

### Task 3 (Fala F3): `inodes.rs`, `revisions.rs`, `chunks.rs`, `sync_policies.rs`, `projection.rs`, `conflicts.rs`, `metadata_backup.rs`

Najbardziej rozproszona grupa — te domeny są w baseline poprzeplatane, więc każdy symbol trzeba wziąć osobno.

**Files:**
- Create: `angeld/src/db/inodes.rs`, `revisions.rs`, `chunks.rs`, `sync_policies.rs`, `projection.rs`, `conflicts.rs`, `metadata_backup.rs`
- Modify: `angeld/src/db/mod.rs`

**Interfaces:**
- Produces: `db::get_inode_by_path`, `db::soft_delete_inode`, `db::promote_revision_to_current`, `db::get_file_chunk_locations`, `db::find_sync_policy_for_path`, `db::get_active_files_for_projection`, `db::materialize_conflict_copy_from_revision`, `db::record_metadata_backup_attempt` i pozostałe niżej.
- Consumes: `pub(super) normalize_policy_path` z `sync_policies.rs` — używany przez `projection.rs::projection_relative_path`.

- [ ] **Step 1: Utwórz `inodes.rs`**

Przenieś: `InodeRecord` 118–130, `FileInventoryRecord` 156–167, `create_inode` 3359–3384, `upsert_inode` 3385–3436, `get_inode_by_path` 3437–3458, `get_inode_by_id` 3459–3475, `resolve_path` 3476–3496, `delete_inode_record` 4325–4338, `soft_delete_inode` 4339–4354, `SoftDeletedInode` 4355–4361, `list_soft_deleted` 4362–4371, `restore_soft_deleted_inode` 4372–4395, `restored_name` 4396–4407 (prywatny), `list_expired_soft_deleted` 4408–4420, `validate_inode_kind` 6705–6713 (prywatny).

- [ ] **Step 2: Utwórz `revisions.rs`**

Przenieś: `FileRevisionRecord` 141–155, `RevisionLineageRelation` 297–305, `create_file_revision` 3497–3560, `get_current_file_revision` 3876–3895, `get_storage_mode_for_inode` 3896–3910, `get_file_revision` 3911–3931, `list_file_revisions` 3932–3949, `get_referencing_inode_ids_for_pack` 3950–3971, `promote_revision_to_current` 3972–4015, `classify_revision_lineage` 4016–4036, `is_revision_ancestor` 6792–6829 (prywatny).

- [ ] **Step 3: Utwórz `chunks.rs`**

Przenieś: `ChunkRecord` 131–140, `ChunkLookupRecord` 306–318, `FileChunkLocation` 340–351, `register_chunk` 4037–4060, `copy_chunk_refs` 4061–4083, `get_chunk_lookup_by_chunk_id` 4251–4295, `delete_file_chunks` 4296–4324, `get_file_chunk_locations` 5876–5913, `get_revision_chunk_locations_in_range` 5914–5959, `get_chunk_locations_for_revision` 7356–7391, `ChunkRefRecord` 7392–7401, `get_chunk_refs_for_revision` 7461–7475.

- [ ] **Step 4: Utwórz `sync_policies.rs`**

Przenieś: `SyncPolicyRecord` 178–187, `upsert_sync_policy` 3561–3604, `list_sync_policies` 3605–3622, `set_sync_policy_type_for_path` 3623–3667, `find_sync_policy_for_path` 3862–3875, `normalize_policy_path` 6736–6744 — **zmień `fn` na `pub(super) fn`** (jedyna dozwolona zmiana widoczności w tej fali), `path_matches_policy` 6745–6760 (prywatny).

- [ ] **Step 5: Utwórz `projection.rs`**

Przenieś: `ProjectionFileRecord` 168–177, `SmartSyncStateRecord` 188–196, `SmartSyncEvictionRecord` 197–204, `ensure_smart_sync_state` 3668–3691, `get_smart_sync_state` 3692–3708, `set_pin_state` 3820–3840, `set_hydration_state` 3841–3861, `get_active_files_for_projection` 5613–5672, `get_active_file_for_projection_by_inode` 5673–5735, `list_unpinned_hydrated_files_for_eviction` 5736–5793, `projection_relative_path` 5794–5825 (prywatny), `get_inode_path` 5826–5849.

Nagłówek musi zawierać `use crate::db::sync_policies::normalize_policy_path;`.

- [ ] **Step 6: Utwórz `conflicts.rs`**

Przenieś: `ConflictEventRecord` 249–261, `create_conflict_event` 4084–4119, `materialize_conflict_copy_from_revision` 4120–4200, `attach_conflict_materialization` 4201–4224, `list_recent_conflicts` 4225–4250, `build_conflict_copy_name` 6761–6768, `disambiguate_conflict_copy_name` 6769–6773, `split_file_name` 6774–6780, `sanitize_conflict_component` 6781–6791 (cztery ostatnie prywatne).

- [ ] **Step 7: Utwórz `metadata_backup.rs`**

Przenieś: `MetadataBackupRecord` 327–339, `record_metadata_backup_attempt` 3709–3755, `update_metadata_backup_status` 3756–3778, `get_last_successful_metadata_backup_at` 3779–3793, `list_recent_metadata_backups` 3794–3819.

- [ ] **Step 8: Podłącz moduły i uruchom bramkę fali**

```bash
cargo check --workspace --all-targets
cargo test -p angeld --lib 2>&1 | tail -5     # 199 passed
bash "$SCRATCH/verify_symbols.sh"
```

- [ ] **Step 9: Commit**

```bash
git add angeld/src/db/
git commit -m "refactor(db): wydziel inodes, revisions, chunks, sync_policies, projection, conflicts, metadata_backup"
```

---

### Task 4 (Fala F4): `packs.rs`, `shards.rs`, `cache.rs`

**Files:**
- Create: `angeld/src/db/packs.rs`, `angeld/src/db/shards.rs`, `angeld/src/db/cache.rs`
- Modify: `angeld/src/db/mod.rs`

**Interfaces:**
- Produces: `db::create_pack`, `db::resolve_pack_status`, `db::register_pack_shard`, `db::get_next_shards_for_scrub`, `db::upsert_cache_entry` i pozostałe niżej.
- Consumes: `PackStatus`, `ShardRole`, `StorageMode` z `mod.rs`.

- [ ] **Step 1: Utwórz `packs.rs`**

Przenieś: `PackRecord` 352–368, `VaultHealthSummary` 456–463, `ScrubStatusSummary` 464–474, `ScrubErrorRecord` 475–483, `ActiveStorageModeSummary` 484–493, `OrphanedPackSummary` 494–499, `create_pack` 4421–4485, `update_pack_status` 4486–4506, `get_pack` 4507–4532, `find_pack_by_plaintext_hash` 4533–4574, `get_orphaned_pack_ids` 4575–4596, `get_next_degraded_pack` 4597–4623, `get_vault_health_summary` 4624–4641, `get_scrub_status_summary` 4642–4662, `list_scrub_errors` 4663–4687, `get_physical_usage_for_provider` 4872–4892, `get_active_storage_mode_summaries` 4893–4935, `get_orphaned_pack_summary` 4936–4965, `delete_pack_metadata` 5049–5086, `resolve_pack_status` 5387–5391, `resolve_pack_status_for_mode` 5392–5421, `list_active_packs` 5422–5453, `get_desired_storage_mode_for_pack` 5454–5612, `pack_requires_healthy` 5850–5875.

- [ ] **Step 2: Utwórz `shards.rs`**

Przenieś: `PackShardRecord` 369–389, `ScrubShardRecord` 390–406, `PackShardSummary` 407–416, `register_pack_shard` 4966–5014, `get_pack_shards` 5015–5048, `get_incomplete_pack_shards` 5087–5121, `mark_pack_shard_in_progress` 5122–5144, `mark_pack_shard_completed` 5145–5167, `requeue_pack_shard` 5168–5206, `mark_pack_shard_failed` 5207–5232, `mark_pack_shard_permanently_failed` 5233–5257, `get_next_shards_for_scrub` 5258–5292, `update_shard_verification_status` 5293–5337, `reset_in_progress_pack_shards` 5338–5352, `summarize_pack_shards` 5353–5386.

- [ ] **Step 3: Utwórz `cache.rs`**

Przenieś: `CacheEntryRecord` 205–221, `CacheStatusSummary` 319–326, `get_cache_entry` 4688–4716, `upsert_cache_entry` 4717–4780, `touch_cache_entry` 4781–4797, `list_cache_entries_by_lru` 4798–4827, `get_total_cache_size` 4828–4839, `get_cache_status_summary` 4840–4856, `delete_cache_entry` 4857–4871.

- [ ] **Step 4: Podłącz moduły i uruchom bramkę fali**

```bash
cargo check --workspace --all-targets
cargo test -p angeld --lib 2>&1 | tail -5     # 199 passed
bash "$SCRATCH/verify_symbols.sh"
```

- [ ] **Step 5: Commit**

```bash
git add angeld/src/db/
git commit -m "refactor(db): wydziel packs, shards, cache"
```

---

### Task 5 (Fala F5): `uploads.rs`, `ingest.rs`

**Files:**
- Create: `angeld/src/db/uploads.rs`, `angeld/src/db/ingest.rs`
- Modify: `angeld/src/db/mod.rs`

**Interfaces:**
- Produces: `db::get_next_upload_job`, `db::gc_orphan_packs`, `db::sync_upload_targets_from_shards`, `db::create_ingest_job`, `db::get_pack_ids_for_inode` i pozostałe niżej.

- [ ] **Step 1: Utwórz `uploads.rs`**

Przenieś: `UploadJob` 417–425, `UploadTargetRecord` 426–443, `PackDownloadTarget` 444–455, `queue_pack_for_upload` 5960–5979, `get_next_upload_job` 5980–6017, `mark_upload_job_completed` 6018–6033, `get_upload_job_by_pack_id` 6034–6051, `ensure_upload_targets` 6052–6077, `get_incomplete_upload_targets` 6078–6109, `mark_upload_target_in_progress` 6110–6133, `mark_upload_target_completed` 6134–6171, `requeue_upload_target` 6172–6213, `mark_upload_target_failed` 6214–6243, `GcOrphanReport` 6244–6253, `gc_orphan_packs` 6254–6336, `RetryStormTargetRecord` 6337–6347, `list_retry_storm_targets` 6348–6375, `UploadTargetSyncReport` 6376–6388, `sync_upload_targets_from_shards` 6389–6435, `mark_upload_target_permanently_failed` 6436–6463, `has_incomplete_upload_targets` 6464–6483, `requeue_upload_job` 6484–6511, `mark_upload_job_failed` 6512–6527, `reset_in_progress_upload_targets` 6528–6543, `get_upload_targets_for_job` 6544–6574, `list_recent_upload_jobs` 6575–6594, `get_pending_upload_queue_size` 6595–6609, `get_latest_upload_error` 6610–6627, `get_latest_upload_target_for_provider` 6628–6659, `get_completed_pack_targets` 6660–6690, `reset_in_progress_upload_jobs` 6691–6704.

- [ ] **Step 2: Utwórz `ingest.rs`**

Przenieś: `IngestJobRow` 1337–1349, `create_ingest_job` 1350–1368, `get_next_pending_ingest_job` 1369–1380, `transition_ingest_job` 1381–1400, `update_ingest_progress` 1401–1415, `fail_ingest_job` 1416–1433, `reset_interrupted_ingest_jobs` 1434–1447, `list_ingest_jobs` 1448–1458, `get_ingest_job` 1459–1473, `requeue_failed_ingest_job` 1474–1489, `delete_ingest_job` 1490–1497, `delete_failed_ingest_job` 1498–1506, `get_pack_ids_for_inode` 1507–1525, `retry_ingest_job` 1526–1539.

- [ ] **Step 3: Podłącz moduły i uruchom bramkę fali**

```bash
cargo check --workspace --all-targets
cargo test -p angeld --lib 2>&1 | tail -5     # 199 passed
bash "$SCRATCH/verify_symbols.sh"
```

- [ ] **Step 4: Commit**

```bash
git add angeld/src/db/
git commit -m "refactor(db): wydziel uploads i ingest"
```

---

### Task 6 (Fala F6): tożsamość i ogon — 10 plików

Po tej fali `mod.rs` zawiera już wyłącznie: inner attribute, enumy, `epoch_secs`, `SOFT_DELETE_GRACE_MS`, deklaracje modułów, re-eksporty i blok testowy.

**Files:**
- Create: `angeld/src/db/users.rs`, `devices.rs`, `device_identity.rs`, `sessions.rs`, `audit.rs`, `invites.rs`, `recovery_keys.rs`, `stats.rs`, `oauth.rs`, `shares.rs`
- Modify: `angeld/src/db/mod.rs`

**Interfaces:**
- Produces: `db::create_user`, `db::create_device`, `db::get_local_device_identity`, `db::create_user_session`, `db::insert_audit_log`, `db::consume_invite_code`, `db::insert_recovery_key`, `db::get_stats_overview`, `db::create_oauth_state`, `db::create_shared_link` i pozostałe niżej.

- [ ] **Step 1: Utwórz `users.rs`**

Przenieś: `UserRecord` 528–538, `VaultMemberRecord` 556–565, `new_user_id` 12–14, `create_user` 7476–7499, `get_user` 7500–7509, `list_users` 7510–7518, `update_user_display_name` 7519–7531, `delete_user` 7532–7541, `add_vault_member` 7709–7730, `get_vault_member` 7731–7748, `count_vault_members` 7749–7755, `list_vault_members` 7756–7768, `update_vault_member_role` 7769–7784, `remove_vault_member` 7785–7799, `migrate_single_to_multi_user` 7940–8022, `ensure_local_device_in_vault` 8023–8074, `verify_vault_device_binding` 8075–8104, `backfill_uuid_user_ids` 8105–8163.

- [ ] **Step 2: Utwórz `devices.rs`**

Przenieś: `DeviceRecord` 539–555, `create_device` 7542–7564, `get_device` 7565–7579, `set_device_safety_verified` 7580–7592, `get_device_safety_verified_at` 7593–7604, `list_devices_for_user` 7605–7619, `set_device_wrapped_vault_key` 7620–7636, `set_device_wrapped_vault_key_kyber` 7637–7650, `get_active_devices_for_user` 7651–7666, `revoke_device` 7667–7680, `set_device_public_key` 7681–7696, `touch_device_last_seen` 7697–7708.

- [ ] **Step 3: Utwórz `device_identity.rs`**

Przenieś: `LocalDeviceIdentityRecord` 222–235, `TrustedPeerRecord` 236–248, `get_local_device_identity` 3062–3078, `upsert_local_device_identity` 3079–3119, `update_local_device_name` 3120–3138, `store_device_keypair` 3139–3157, `store_kyber_keypair` 3158–3176, `set_device_kyber_public_key` 3177–3190, `upsert_trusted_peer` 3191–3242, `note_peer_seen` 3243–3288, `update_peer_error` 3289–3310, `list_trusted_peers` 3311–3332, `get_trusted_peer_by_id` 3333–3358.

- [ ] **Step 4: Utwórz `sessions.rs`**

Przenieś: `SESSION_TTL_SECONDS` 8164–8167, `UserSession` 8168–8176, `generate_session_token` 8177–8184, `create_user_session` 8185–8222, `validate_user_session` 8223–8238, `renew_user_session` 8239–8256, `delete_user_session` 8257–8265, `delete_user_sessions_for_user` 8266–8277, `cleanup_expired_sessions` 8278–8289.

- [ ] **Step 5: Utwórz `audit.rs`**

Przenieś: `AuditLogRecord` 566–579, `insert_audit_log` 7800–7828, `list_audit_logs` 7829–7846.

- [ ] **Step 6: Utwórz `invites.rs`**

Przenieś: `InviteCodeRecord` 580–591, `create_invite_code` 7847–7872, `get_invite_code` 7873–7885, `is_invite_code_valid` 7886–7897, `consume_invite_code` 7898–7908, `list_invite_codes` 7909–7921, `delete_invite_code` 7922–7939.

- [ ] **Step 7: Utwórz `recovery_keys.rs`**

Przenieś: `RecoveryKeyRecord` 8290–8299, `insert_recovery_key` 8300–8322, `list_active_recovery_keys` 8323–8337, `revoke_all_recovery_keys` 8338–8356.

- [ ] **Step 8: Utwórz `stats.rs`**

Przenieś: `StatsOverview` 8357–8361, `get_stats_overview` 8362–8376, `count_active_devices` 8377–8383, `record_traffic` 8384–8413, `TrafficBucket` 8414–8420, `get_traffic_buckets` 8421–8446.

- [ ] **Step 9: Utwórz `oauth.rs`**

Przenieś: `create_oauth_state` 8447–8465, `get_and_delete_oauth_state` 8466–8487, `delete_expired_oauth_states` 8488–8498, `store_encrypted_refresh_token` 8499–8512, `get_encrypted_refresh_token` 8513–8527, `users_with_plaintext_refresh_token` 8528–8540, `clear_plaintext_refresh_token` 8541–8551.

- [ ] **Step 10: Utwórz `shares.rs`**

Przenieś: `SharedLinkRecord` 7213–7228, `SharePasswordToken` 7229–7235, `is_shared_link_valid` 7236–7256, `create_shared_link` 7257–7289, `get_shared_link` 7290–7302, `list_shared_links` 7303–7312, `list_shared_links_for_inode` 7313–7326, `revoke_shared_link` 7327–7335, `increment_shared_link_download_count` 7336–7346, `delete_shared_link` 7347–7355, `create_share_password_token` 7402–7425, `validate_share_password_token` 7426–7446, `cleanup_expired_share_tokens` 7447–7460.

- [ ] **Step 11: Podłącz moduły i uruchom bramkę fali**

```bash
cargo check --workspace --all-targets
cargo test -p angeld --lib 2>&1 | tail -5     # 199 passed
bash "$SCRATCH/verify_symbols.sh"
bash "$SCRATCH/verify_bodies.sh"
```

`verify_bodies.sh` uruchamiamy tu po raz pierwszy na pełnym zestawie — cały kod produkcyjny jest już rozdzielony. Oczekiwana jedyna różnica: `normalize_policy_path` (zmiana `fn` → `pub(super) fn`). Cokolwiek więcej = błąd do zdiagnozowania.

- [ ] **Step 12: Commit**

```bash
git add angeld/src/db/
git commit -m "refactor(db): wydziel users, devices, device_identity, sessions, audit, invites, recovery_keys, stats, oauth, shares"
```

---

### Task 7 (Fala F7): rozdzielenie testów + finalne `mod.rs`

**Files:**
- Create: `angeld/src/db/test_support.rs`
- Modify: `angeld/src/db/mod.rs` + wszystkie 28 plików domenowych (dopisanie bloków `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `test_support::temp_test_dir`, `test_support::build_source_vault`, `test_support::USER_FIXTURE` — dostępne dla testów we wszystkich podmodułach.

- [ ] **Step 1: Wypisz inwentarz testów**

```bash
grep -n "async fn \|    fn \|#\[tokio::test\]\|#\[test\]\|#\[cfg(feature" angeld/src/db/mod.rs | sed -n '/8552/,$p'
```

Zapisz listę 58 nazw testów do `$SCRATCH/tests_before.txt` (posortowaną) — posłuży za dowód kompletności po rozdzieleniu:

```bash
grep -oE 'async fn [a-z0-9_]+|fn [a-z0-9_]+' angeld/src/db/mod.rs \
  | sed 's/^async //' | sort -u > "$SCRATCH/tests_before.txt"
```

- [ ] **Step 2: Utwórz `test_support.rs`**

Przenieś tam wspólne helpery z bloku testowego (baseline 8556–…): `USER_FIXTURE`, `temp_test_dir`, `build_source_vault` oraz każdy inny helper używany przez więcej niż jeden test. Cały plik pod `#[cfg(test)]`:

```rust
#![cfg(test)]

use crate::db::*;
```

W `mod.rs` dopisz `#[cfg(test)] mod test_support;` — moduł jest wewnętrzny dla `db`, bez `pub use`.

- [ ] **Step 3: Rozdziel testy per domena**

Dla każdego z 58 testów: przypisz go do pliku, którego funkcje testuje (test wołający `soft_delete_inode`/`restore_soft_deleted_inode` → `inodes.rs`; test graftu → `graft.rs`; test `revoke_device_nulls_both_wraps` → `devices.rs`; testy KDF/rewrap → `vault_state.rs` itd.). Jeśli test dotyka dwóch domen, decyduje ta, której zachowanie jest asertowane.

W każdym pliku docelowym dopisz na końcu:

```rust
#[cfg(test)]
mod tests {
    use crate::db::*;
    use crate::db::test_support::*;

    // przeniesione testy tej domeny — dosłownie, bez zmian ciał
}
```

Zachowaj wszystkie `#[cfg(feature = "test-helpers")]` na testach z baseline 9519/9524/9539/9572 — trafiają do `vault_state.rs`.

- [ ] **Step 4: Wyczyść `mod.rs`**

Po przeniesieniu wszystkich testów `mod.rs` ma zawierać wyłącznie: inner attribute, importy potrzebne dla `epoch_secs`, `SOFT_DELETE_GRACE_MS`, trzy enumy z `impl`, `epoch_secs`, deklaracje `pub mod` + `#[cfg(test)] mod test_support;` oraz `pub use`. Docelowo ~130 linii.

- [ ] **Step 5: Weryfikacja kompletności testów**

```bash
cat angeld/src/db/*.rs | grep -oE 'async fn [a-z0-9_]+|fn [a-z0-9_]+' \
  | sed 's/^async //' | sort -u > "$SCRATCH/tests_after.txt"
diff "$SCRATCH/tests_before.txt" "$SCRATCH/tests_after.txt"
cargo test -p angeld --lib 2>&1 | tail -5     # MUSI byc dokladnie 199 passed
```

- [ ] **Step 6: Commit**

```bash
git add angeld/src/db/
git commit -m "refactor(db): rozdziel testy per submodul + test_support"
```

---

### Task 8: Pełna bramka, weryfikacja zakresu i push

**Files:** brak zmian w kodzie (poza ewentualnym `cargo fmt`).

- [ ] **Step 1: Formatowanie**

```bash
cargo fmt --all
git diff --stat        # jesli cokolwiek sie zmienilo, zacommituj osobno
cargo fmt --all --check
```

- [ ] **Step 2: Clippy w obu trybach**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features test-helpers -- -D warnings
```

- [ ] **Step 3: Build release**

```bash
cargo build --release --workspace
```

- [ ] **Step 4: Obie suity**

```bash
cargo test -p omnidrive-core 2>&1 | tail -5    # 28 passed
cargo test -p angeld --lib 2>&1 | tail -5      # 199 passed
```

- [ ] **Step 5: Weryfikacja zero-drift i zakresu**

```bash
bash "$SCRATCH/verify_symbols.sh"
bash "$SCRATCH/verify_bodies.sh"
git diff --stat 942a442..HEAD -- . ":(exclude)angeld/src/db" ":(exclude)angeld/src/db.rs"
```

Ostatnia komenda MUSI zwrócić pustkę — dowód, że refaktor nie dotknął niczego poza katalogiem `db/`. Dodatkowo sprawdź rozmiary:

```bash
wc -l angeld/src/db/*.rs | sort -n
```

Żaden plik nie powinien przekraczać ~1 100 linii łącznie z testami (`graft.rs` będzie największy).

- [ ] **Step 6: Push**

```bash
git push origin main
```

Pre-push hook (fmt + clippy) musi przejść. Nigdy `--no-verify`. Jeśli hook zgłasza błąd — napraw przyczynę i pushuj ponownie.

---

### Task 9: Dokumentacja i pamięć

**Files:**
- Modify: `docs/KNOWN_ISSUES.md`, `STATUS.md`
- Modify: `<memory>/project_next_session_plan.md`, `<memory>/MEMORY.md`

- [ ] **Step 1: Wpis P2-007 w `KNOWN_ISSUES.md`**

Dodaj do sekcji `## Closed` (zadanie zamykamy w tym samym commicie, w którym je zakładamy — analogicznie do P2-003):

```markdown
### P2-007 — `db.rs` monolit 10 649 linii (dekompozycja)

- **Wykryto:** audyt `docs/superpowers/specs/2026-05-11-code-audit.md §2.1` (2026-05-17), wskazany jako najpilniejszy kandydat do dekompozycji przed mobile.
- **Symptom:** jeden plik, 14+ domen, 238 `pub async fn`, blok testowy 2 100 linii. Każda zmiana wymaga nawigacji po całości; plik nie mieści się w kontekście edytora.
- **Status:** ✅ CLOSED 2026-07-31. Rozbity na `angeld/src/db/` (30 plików). Płaskie re-eksporty w `mod.rs` — 912 call-site'ów `db::` nietkniętych. Testy rozdzielone per submoduł + `test_support.rs`. ZERO zmian zachowania (weryfikacja: zbiór publicznych sygnatur + hashe znormalizowanych ciał funkcji identyczne). Suita: core 28, angeld lib 199. Spec `docs/superpowers/specs/2026-07-31-db-decomposition-design.md`, plan `…/plans/2026-07-31-db-decomposition.md`.
```

Zaktualizuj też nagłówek pliku (`Ostatnia aktualizacja`, `Aktualna wersja: v0.3.28`).

- [ ] **Step 2: Wpis w `STATUS.md`**

Odnotuj zadanie w sekcji postępu (tabela §12.7 lub odpowiednik), zgodnie z konwencją używaną dla P2-003.

- [ ] **Step 3: Aktualizacja pamięci**

W `project_next_session_plan.md`: nowy blok STAN na górze (SHA, liczba plików, wyniki bramki, lista odchyleń od planu jeśli były) + komenda startowa następnej sesji. Kandydaci na NEXT: **Faza δ (Multi-User Closure)** albo **dekompozycja `smart_sync.rs`** (audyt §2.2 — 2 197 linii, zero ryzyka, wszystkie wewnętrzne fn prywatne). Zaktualizuj też linię w `MEMORY.md`.

- [ ] **Step 4: Commit i push**

```bash
git add docs/KNOWN_ISSUES.md STATUS.md
git commit -m "docs: P2-007 CLOSED — dekompozycja db.rs"
git push origin main
```
