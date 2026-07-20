# γ.1 Conflict-copy Verification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Udowodnić testami, że istniejący conflict-copy (lokalna ścieżka) działa: klasyfikacja lineage 4-way, materializacja kopii, surface przez `list_recent_conflicts`.

**Architecture:** Testy charakteryzacyjne (nie TDD-red-first — kod produkcyjny JUŻ istnieje). Dwie warstwy: db-level (`db.rs` `#[cfg(test)]`) dla wyczerpującej klasyfikacji + materializacji; packer-level (`packer.rs` `#[cfg(test)]`) dla dowodu wiringu którego używa watcher. Zero zmian w kodzie produkcyjnym.

**Tech Stack:** Rust, `sqlx` (SQLite `sqlite::memory:`), `tokio::test`.

## Global Constraints

- **Kod produkcyjny NIETKNIĘTY.** Jeśli test FAIL — obnażyliśmy realny bug w feature data-safety: STOP, zgłoś Przemkowi, `superpowers:systematic-debugging` PRZED jakimkolwiek fixem. Nie „naprawiaj testu pod kod" i nie „naprawiaj kodu pod test" bez decyzji.
- Testy PASS oczekiwane od razu (charakteryzacja istniejącego zachowania) — odwrotnie niż klasyczne TDD.
- Bramka końcowa: `cargo test -p angeld --lib` + `cargo clippy --all-targets` oba tryby (default + `--features test-helpers`) + `cargo fmt --check` — wszystko zielone.
- Bez bumpu wersji (czysto testowa zmiana, workspace zostaje v0.3.28).
- Nowe testy trafiają do ISTNIEJĄCYCH modułów `#[cfg(test)] mod tests` (`db.rs:8454`, `packer.rs:701`), nie do nowego pliku w `tests/`.
- Zakres LOCAL-ONLY. Konflikt cross-device → Faza δ (patrz spec `docs/superpowers/specs/2026-07-20-gamma-conflict-copy-verification-design.md`, sekcja „Poza zakresem"). NIE pisać testów cross-device.

## Referencyjne sygnatury (z bieżącego kodu, verbatim)

```rust
// db.rs
pub enum RevisionLineageRelation { Same, CandidateDescendsFromCurrent, CurrentDescendsFromCandidate, Parallel }

pub async fn init_db(url: &str) -> Result<SqlitePool, ...>;
pub async fn create_inode(pool: &SqlitePool, parent_id: Option<i64>, name: &str, kind: &str, size: i64) -> Result<i64, sqlx::Error>;
pub async fn create_file_revision(pool: &SqlitePool, inode_id: i64, size: i64, immutable_until: Option<i64>, device_id: Option<&str>, parent_revision_id: Option<i64>, origin: &str, conflict_reason: Option<&str>) -> Result<i64, sqlx::Error>; // ustawia is_current=1, resetuje rodzeństwo
pub async fn register_chunk(pool: &SqlitePool, revision_id: i64, chunk_id: &[u8], offset: i64, size: i64) -> Result<i64, sqlx::Error>;
pub async fn classify_revision_lineage(pool: &SqlitePool, candidate_revision_id: i64, current_revision_id: i64) -> Result<RevisionLineageRelation, sqlx::Error>;
pub async fn materialize_conflict_copy_from_revision(pool: &SqlitePool, source_revision_id: i64, device_id: Option<&str>, device_name: &str, reason: &str) -> Result<(i64, i64, String, i64), sqlx::Error>; // (inode_id, revision_id, name, conflict_id)
pub async fn list_recent_conflicts(pool: &SqlitePool, limit: i64) -> Result<Vec<ConflictEventRecord>, sqlx::Error>;
pub async fn get_chunk_refs_for_revision(pool: &SqlitePool, revision_id: i64) -> Result<Vec<ChunkRefRecord>, sqlx::Error>;

pub struct ConflictEventRecord { pub conflict_id: i64, pub inode_id: i64, pub winning_revision_id: i64, pub losing_revision_id: i64, pub reason: String, pub materialized_inode_id: Option<i64>, pub materialized_revision_id: Option<i64>, pub created_at: i64 }

// packer.rs
impl Packer { pub async fn pack_file_with_expected_parent(&self, inode_id: i64, source_path: impl AsRef<Path>, expected_parent_revision_id: Option<i64>) -> Result<PackResult, PackerError>; }
// PackResult { pub revision_id: Option<i64>, pub conflict_copy_name: Option<String>, ... }
```

---

### Task 1: Klasyfikacja lineage — 4-way matrix (db-level)

**Files:**
- Modify: `angeld/src/db.rs` — dodaj testy do `#[cfg(test)] mod tests` (od linii 8454). Umieść po istniejących helperach/testach.

**Interfaces:**
- Consumes: `init_db`, `create_inode`, `create_file_revision`, `classify_revision_lineage`, `RevisionLineageRelation` (wszystkie dostępne przez `use super::*;` w module testów).
- Produces: nic (testy).

- [ ] **Step 1: Napisz 4 testy klasyfikacji**

Wklej do `mod tests` w `db.rs`:

```rust
#[tokio::test]
async fn lineage_same_when_candidate_equals_current() -> Result<(), Box<dyn std::error::Error>> {
    let pool = init_db("sqlite::memory:").await?;
    let inode = create_inode(&pool, None, "f.txt", "FILE", 10).await?;
    let rev = create_file_revision(&pool, inode, 10, None, None, None, "local_write", None).await?;
    let rel = classify_revision_lineage(&pool, rev, rev).await?;
    assert_eq!(rel, RevisionLineageRelation::Same);
    Ok(())
}

#[tokio::test]
async fn lineage_candidate_descends_from_current_is_fast_forward() -> Result<(), Box<dyn std::error::Error>> {
    let pool = init_db("sqlite::memory:").await?;
    let inode = create_inode(&pool, None, "f.txt", "FILE", 10).await?;
    let current = create_file_revision(&pool, inode, 10, None, None, None, "local_write", None).await?;
    let candidate = create_file_revision(&pool, inode, 10, None, None, Some(current), "local_write", None).await?;
    let rel = classify_revision_lineage(&pool, candidate, current).await?;
    assert_eq!(rel, RevisionLineageRelation::CandidateDescendsFromCurrent);
    Ok(())
}

#[tokio::test]
async fn lineage_current_descends_from_candidate_is_stale_base() -> Result<(), Box<dyn std::error::Error>> {
    let pool = init_db("sqlite::memory:").await?;
    let inode = create_inode(&pool, None, "f.txt", "FILE", 10).await?;
    let candidate = create_file_revision(&pool, inode, 10, None, None, None, "local_write", None).await?;
    let current = create_file_revision(&pool, inode, 10, None, None, Some(candidate), "local_write", None).await?;
    let rel = classify_revision_lineage(&pool, candidate, current).await?;
    assert_eq!(rel, RevisionLineageRelation::CurrentDescendsFromCandidate);
    Ok(())
}

#[tokio::test]
async fn lineage_siblings_are_parallel() -> Result<(), Box<dyn std::error::Error>> {
    let pool = init_db("sqlite::memory:").await?;
    let inode = create_inode(&pool, None, "f.txt", "FILE", 10).await?;
    let base = create_file_revision(&pool, inode, 10, None, None, None, "local_write", None).await?;
    let branch_a = create_file_revision(&pool, inode, 10, None, None, Some(base), "local_write", None).await?;
    let branch_b = create_file_revision(&pool, inode, 10, None, None, Some(base), "local_write", None).await?;
    let rel = classify_revision_lineage(&pool, branch_a, branch_b).await?;
    assert_eq!(rel, RevisionLineageRelation::Parallel);
    Ok(())
}
```

- [ ] **Step 2: Uruchom testy — oczekiwany PASS (charakteryzacja)**

Run: `cargo test -p angeld --lib lineage_ -- --nocapture`
Expected: 4 passed. Jeśli FAIL → realny bug w `classify_revision_lineage`/`is_revision_ancestor` → STOP + systematic-debugging.

- [ ] **Step 3: Commit**

```bash
git add angeld/src/db.rs
git commit -m "test(db): γ.1 revision lineage 4-way classification (conflict-copy verify)"
```

---

### Task 2: Materializacja kopii konfliktu + surface (db-level)

**Files:**
- Modify: `angeld/src/db.rs` — dodaj testy do `#[cfg(test)] mod tests`.

**Interfaces:**
- Consumes: `init_db`, `create_inode`, `create_file_revision`, `register_chunk`, `materialize_conflict_copy_from_revision`, `list_recent_conflicts`, `get_chunk_refs_for_revision`, `ConflictEventRecord`.
- Produces: nic (testy).

- [ ] **Step 1: Napisz test materializacji (chunk refs + conflict_events + surface)**

Wklej do `mod tests` w `db.rs`:

```rust
#[tokio::test]
async fn materialize_conflict_copy_creates_inode_copies_chunks_and_records_event() -> Result<(), Box<dyn std::error::Error>> {
    let pool = init_db("sqlite::memory:").await?;
    let inode = create_inode(&pool, None, "report.txt", "FILE", 20).await?;
    let source = create_file_revision(&pool, inode, 20, None, Some("dev-a"), None, "local_write", None).await?;
    register_chunk(&pool, source, &[1u8; 32], 0, 20).await?;

    let (copy_inode, copy_rev, name, conflict_id) =
        materialize_conflict_copy_from_revision(&pool, source, Some("dev-a"), "Laptop", "parallel_local_edit").await?;

    assert_ne!(copy_inode, inode, "conflict copy must be a distinct inode");
    assert!(name.starts_with("report (conflict - Laptop - "), "unexpected name: {name}");
    assert!(name.ends_with(").txt"), "extension must be preserved: {name}");

    let copied = get_chunk_refs_for_revision(&pool, copy_rev).await?;
    assert_eq!(copied.len(), 1, "chunk refs must be copied so the conflict copy is recoverable");
    assert_eq!(copied[0].size, 20);

    let events = list_recent_conflicts(&pool, 10).await?;
    let event = events.iter().find(|e| e.conflict_id == conflict_id).expect("conflict event surfaced");
    assert_eq!(event.reason, "parallel_local_edit");
    assert_eq!(event.inode_id, inode);
    assert_eq!(event.materialized_inode_id, Some(copy_inode));
    assert_eq!(event.materialized_revision_id, Some(copy_rev));
    Ok(())
}
```

- [ ] **Step 2: Napisz test dezambiguacji nazwy przy kolizji**

Dwie materializacje z tej samej rewizji źródłowej → identyczna nazwa bazowa (ten sam inode + device + `created_at`) → druga musi dostać sufiks ` [1]`.

```rust
#[tokio::test]
async fn materialize_conflict_copy_disambiguates_name_on_collision() -> Result<(), Box<dyn std::error::Error>> {
    let pool = init_db("sqlite::memory:").await?;
    let inode = create_inode(&pool, None, "notes.md", "FILE", 5).await?;
    let source = create_file_revision(&pool, inode, 5, None, Some("dev-a"), None, "local_write", None).await?;
    register_chunk(&pool, source, &[2u8; 32], 0, 5).await?;

    let (_i1, _r1, name1, _c1) =
        materialize_conflict_copy_from_revision(&pool, source, Some("dev-a"), "PC", "stale_local_base").await?;
    let (_i2, _r2, name2, _c2) =
        materialize_conflict_copy_from_revision(&pool, source, Some("dev-a"), "PC", "stale_local_base").await?;

    assert_ne!(name1, name2, "second copy must not collide with the first");
    assert!(name2.contains(" [1]"), "second copy must be disambiguated: {name2}");
    Ok(())
}
```

- [ ] **Step 3: Uruchom testy — oczekiwany PASS**

Run: `cargo test -p angeld --lib materialize_conflict_copy_ -- --nocapture`
Expected: 2 passed. Jeśli chunk-refs assert FAIL → kopia nie jest odtwarzalna (realny data-safety bug). Jeśli dezambiguacja FAIL → brak UNIQUE(parent,name) na `inodes` lub martwa pętla dedup → STOP + systematic-debugging.

- [ ] **Step 4: Commit**

```bash
git add angeld/src/db.rs
git commit -m "test(db): γ.1 conflict copy materialization + surface + name disambiguation"
```

---

### Task 3: Wiring packera — watcher path (packer-level)

**Files:**
- Modify: `angeld/src/packer.rs` — dodaj testy do `#[cfg(test)] mod tests` (od linii 701, obok `splits_into_default_4mb_chunks`).

**Interfaces:**
- Consumes: `db::init_db`, `db::create_inode`, `db::list_recent_conflicts`, `VaultKeyStore::new`/`unlock`, `Packer::new`, `PackerConfig::new`, `Packer::pack_file_with_expected_parent`, `PackResult { revision_id, conflict_copy_name }`. Wzorzec harness = istniejący test `splits_into_default_4mb_chunks` (`packer.rs:707`).
- Produces: nic (testy).

- [ ] **Step 1: Napisz test „stale local base → conflict copy" + kontrolę negatywną**

Wklej do `mod tests` w `packer.rs`. Harness (temp dir, spool, `init_db`, `VaultKeyStore`, `Packer::new`) jak w `splits_into_default_4mb_chunks`.

```rust
#[tokio::test]
async fn stale_local_base_materializes_conflict_copy() -> Result<(), Box<dyn std::error::Error>> {
    let test_root = env::temp_dir().join(format!(
        "omnidrive-packer-conflict-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    let spool_dir = test_root.join("spool");
    let source_path = test_root.join("doc.txt");
    fs::create_dir_all(&spool_dir).await?;

    let pool = db::init_db("sqlite::memory:").await?;
    let inode_id = db::create_inode(&pool, None, "doc.txt", "FILE", 3).await?;
    let vault_keys = VaultKeyStore::new();
    vault_keys.unlock(&pool, "test-passphrase").await?;
    let packer = Packer::new(pool.clone(), vault_keys, PackerConfig::new(&spool_dir))?;

    fs::write(&source_path, b"v1").await?;
    let r1 = packer.pack_file_with_expected_parent(inode_id, &source_path, None).await?;
    let rev1 = r1.revision_id.expect("rev1");
    assert!(r1.conflict_copy_name.is_none(), "first write must not conflict");

    fs::write(&source_path, b"v2x").await?;
    let r2 = packer.pack_file_with_expected_parent(inode_id, &source_path, Some(rev1)).await?;
    assert!(r2.conflict_copy_name.is_none(), "same-base write must not conflict");

    // Trzeci zapis oparty NADAL o rev1 (przestarzała baza — głowa to już rev2).
    fs::write(&source_path, b"v3y").await?;
    let r3 = packer.pack_file_with_expected_parent(inode_id, &source_path, Some(rev1)).await?;
    assert!(r3.conflict_copy_name.is_some(), "stale base must materialize a conflict copy");

    let events = db::list_recent_conflicts(&pool, 10).await?;
    assert!(events.iter().any(|e| e.reason == "stale_local_base"), "stale_local_base event expected");

    let _ = fs::remove_dir_all(&test_root).await;
    Ok(())
}
```

- [ ] **Step 2: Uruchom testy — oczekiwany PASS**

Run: `cargo test -p angeld --lib stale_local_base_materializes_conflict_copy -- --nocapture`
Expected: passed. Jeśli `r3.conflict_copy_name.is_none()` → watcher path NIE wykrywa przestarzałej bazy (realny bug wiringu) → STOP + systematic-debugging.

- [ ] **Step 3: Commit**

```bash
git add angeld/src/packer.rs
git commit -m "test(packer): γ.1 stale-base write materializes conflict copy (watcher wiring)"
```

---

### Task 4: Bramka końcowa + STATUS

**Files:**
- Modify: `STATUS.md` — odnotuj γ.1 jako DONE (sekcja Faza γ / §12.7).

- [ ] **Step 1: Pełna bramka**

Run:
```
cargo fmt --check
cargo clippy -p angeld --all-targets -- -D warnings
cargo clippy -p angeld --all-targets --features test-helpers -- -D warnings
cargo test -p angeld --lib
```
Expected: fmt clean, clippy clean oba tryby, wszystkie testy (w tym 7 nowych) zielone.

- [ ] **Step 2: Odnotuj w STATUS.md**

Dopisz w §12.7 (Faza γ) wiersz: γ.1 conflict-copy verification → DONE (7 testów: 4 lineage + 2 materialize + 1 packer wiring; local-only, cross-device→δ). Bez bumpu.

- [ ] **Step 3: Commit**

```bash
git add STATUS.md
git commit -m "docs(status): γ.1 conflict-copy verification DONE (7 tests, local path proven)"
```

---

## Self-Review

**Spec coverage:**
- Warstwa 1 (4-way matrix T1–T4) → Task 1. ✅
- Warstwa 2 (materializacja: inode + chunk refs + conflict_events + surface + dezambiguacja) → Task 2. ✅
- Warstwa 3 (packer wiring: stale_base + kontrola negatywna) → Task 3. ✅
- **Deviation od specu:** scenariusz „parallel" na warstwie packera przeniesiony do warstwy db (Task 1 `lineage_siblings_are_parallel` + Task 2 materialize z `reason="parallel_local_edit"`). Powód: `create_file_revision` przełącza `is_current`, co uniemożliwia czyste zbudowanie dwóch rodzeństw-z-treścią przez sam packer; a materializacja dla `Parallel` i `CurrentDescendsFromCandidate` idzie tym samym armem `match` (`CurrentDescendsFromCandidate | Parallel =>`), więc packerowy test `stale_base` pokrywa identyczny wiring. Zakres merytoryczny bez uszczerbku.
- Cross-device poza zakresem (spec) → brak zadań, zgodnie z założeniem. ✅

**Placeholder scan:** brak TBD/TODO; każdy krok testowy ma pełny kod i komendę. ✅

**Type consistency:** sygnatury użyte w testach zgodne z sekcją „Referencyjne sygnatury" (verbatim z kodu): `create_file_revision` 8 argumentów, `materialize_conflict_copy_from_revision` zwraca 4-krotkę, `ConflictEventRecord.materialized_*` = `Option<i64>`, `get_chunk_refs_for_revision` → `Vec<ChunkRefRecord>` z polem `.size`. ✅
