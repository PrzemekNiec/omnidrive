# γ.c Soft-delete grace + Kosz Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Kasowanie pliku staje się odwracalne przez 7 dni — usunięty plik trafia do „Kosza" (Web UI), skąd można go Przywrócić lub Usunąć teraz; po 7 dniach sweeper kasuje trwale.

**Architecture:** In-place `inodes.deleted_at` (soft-delete flag). Oba triggery (watcher `handle_deleted_path` + API `delete_file`) ustawiają timestamp zamiast twardego delete; dane (chunk_refs/revisions/packs) zostają, więc gc ich nie ruszy w grace. Listowania/resolucja filtrują `deleted_at IS NULL`; raw-by-id zostaje niefiltrowany. Sweeper (periodic task) hard-deletuje wygasłe starą ścieżką. API `/api/trash` + widok Kosz (lista wierszy) w SPA.

**Tech Stack:** Rust, `sqlx` (SQLite), `tokio`, axum, Vanilla JS/HTML/Tailwind (`angeld/static/index.html`).

## Global Constraints

- **Święta Zasada Integralności Danych:** operacje na plikach tylko w obrębie SYNC_PATH; każda mutacja logowana `tracing::info!` z inode_id + typem operacji. Soft-delete NIE dotyka danych; jedyne nowe twarde-delete (sweeper + purge) jawnie logowane.
- **Zakres bytów:** tylko `kind = 'FILE'` (spójnie z obecnymi `delete_file`/`handle_deleted_path`).
- **Grace = 7 dni stałe:** `SOFT_DELETE_GRACE_MS: i64 = 7 * 24 * 60 * 60 * 1000`. Bez konfiguracji (YAGNI).
- **Zero komentarzy w kodzie prod** (CLAUDE.md); `///` tylko gdy WHY nieoczywiste.
- **Bramka końcowa:** `cargo test -p angeld --lib` + `cargo clippy -p angeld --all-targets [-- / --features test-helpers --] -D warnings` + `cargo fmt --check` — wszystko zielone.
- **Bez bumpu wersji** w trakcie tasków (v0.3.28); decyzja o bumpie osobno po DoD.
- Testy w istniejących `#[cfg(test)] mod tests` (`db.rs:8454`, `api/files.rs` jeśli jest — inaczej db-level + manualna weryfikacja API).
- Spec: `docs/superpowers/specs/2026-07-20-gamma-soft-delete-design.md`.

## Referencyjne sygnatury (verbatim z kodu)

```rust
// db.rs
async fn ensure_column_exists(pool: &SqlitePool, table: &str, column: &str, decl: &str) -> Result<(), sqlx::Error>; // db.rs:6623, wywoływane w init_db
pub async fn create_inode(pool: &SqlitePool, parent_id: Option<i64>, name: &str, kind: &str, size: i64) -> Result<i64, sqlx::Error>;
pub async fn create_file_revision(pool: &SqlitePool, inode_id: i64, size: i64, immutable_until: Option<i64>, device_id: Option<&str>, parent_revision_id: Option<i64>, origin: &str, conflict_reason: Option<&str>) -> Result<i64, sqlx::Error>;
pub async fn register_chunk(pool: &SqlitePool, revision_id: i64, chunk_id: &[u8], offset: i64, size: i64) -> Result<i64, sqlx::Error>;
pub async fn get_inode_by_id(pool: &SqlitePool, inode_id: i64) -> Result<Option<InodeRecord>, sqlx::Error>; // NIE filtrować deleted_at
pub async fn get_inode_by_path(pool: &SqlitePool, parent_id: Option<i64>, name: &str) -> Result<Option<InodeRecord>, sqlx::Error>; // db.rs:3431
pub async fn resolve_path(pool: &SqlitePool, path: &str) -> Result<Option<i64>, sqlx::Error>; // db.rs:3469
pub async fn delete_file_chunks(pool: &SqlitePool, inode_id: i64) -> Result<(), sqlx::Error>; // db.rs:4289 (kasuje chunk_refs + file_revisions)
pub async fn delete_inode_record(pool: &SqlitePool, inode_id: i64) -> Result<u64, sqlx::Error>; // db.rs:4318
fn disambiguate_conflict_copy_name(base_name: &str, attempt: usize) -> String; // db.rs:6671, dodaje " [attempt]" przed rozszerzeniem

// InodeRecord: { id: i64, parent_id: Option<i64>, name: String, kind: String, size: i64, mode: Option<i64>, mtime: Option<i64> }

// api/files.rs
async fn delete_file(State(state): State<ApiState>, headers: HeaderMap, Path(inode_id): Path<i64>) -> Result<Json<DeleteFileResponse>, ApiError>; // :148
// acl::require_role(&state.pool, &headers, Role::Member).await?;  ApiError::{NotFound,Conflict,BadRequest}

// watcher.rs
async fn handle_deleted_path(&self, path: &Path, processed_files: &mut HashMap<PathBuf, TrackedFileState>) -> Result<(), WatcherError>; // :460

// main.rs — wzorzec periodic task (token_cleanup_task :809): tokio::spawn { interval(Duration).tick().await loop }
```

`unix_timestamp_ms()` helper istnieje (używany w db.rs/disaster_recovery). W testach użyj `SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64`.

---

### Task 1: Schema — `inodes.deleted_at` + stała grace

**Files:**
- Modify: `angeld/src/db.rs` — dodać `ensure_column_exists` w `init_db` (obok innych, ~linia 990) + publiczną stałą `SOFT_DELETE_GRACE_MS`.
- Test: `angeld/src/db.rs` `#[cfg(test)] mod tests`.

**Interfaces:**
- Produces: kolumna `inodes.deleted_at INTEGER` (NULL default), `pub const SOFT_DELETE_GRACE_MS: i64`.

- [ ] **Step 1: Dodaj stałą i migrację kolumny**

W `db.rs` blisko innych stałych (góra pliku) dodaj:
```rust
pub const SOFT_DELETE_GRACE_MS: i64 = 7 * 24 * 60 * 60 * 1000;
```
W `init_db`, w bloku `ensure_column_exists` (po `ensure_column_exists(&pool, "file_revisions", "conflict_reason", "TEXT").await?;`):
```rust
    ensure_column_exists(&pool, "inodes", "deleted_at", "INTEGER").await?;
```

- [ ] **Step 2: Test — kolumna istnieje i domyślnie NULL**

W `mod tests`:
```rust
#[tokio::test]
async fn inodes_deleted_at_defaults_null() -> Result<(), Box<dyn std::error::Error>> {
    let pool = init_db("sqlite::memory:").await?;
    let inode = create_inode(&pool, None, "f.txt", "FILE", 1).await?;
    let deleted_at: Option<i64> =
        sqlx::query_scalar("SELECT deleted_at FROM inodes WHERE id = ?")
            .bind(inode)
            .fetch_one(&pool)
            .await?;
    assert_eq!(deleted_at, None);
    Ok(())
}
```

- [ ] **Step 3: Run**

Run: `cargo test -p angeld --lib inodes_deleted_at_defaults_null`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add angeld/src/db.rs
git commit -m "feat(db): γ.c inodes.deleted_at column + SOFT_DELETE_GRACE_MS"
```

---

### Task 2: `soft_delete_inode` + wpięcie obu triggerów

**Files:**
- Modify: `angeld/src/db.rs` — nowa `soft_delete_inode`.
- Modify: `angeld/src/watcher.rs:479-480` — zamiana twardego delete na soft.
- Modify: `angeld/src/api/files.rs:169-170` — zamiana twardego delete na soft.
- Test: `angeld/src/db.rs` `mod tests`.

**Interfaces:**
- Produces: `pub async fn soft_delete_inode(pool: &SqlitePool, inode_id: i64, now_ms: i64) -> Result<bool, sqlx::Error>`.

- [ ] **Step 1: Test — soft_delete ustawia deleted_at, NIE rusza chunk_refs**

```rust
#[tokio::test]
async fn soft_delete_sets_timestamp_and_preserves_chunks() -> Result<(), Box<dyn std::error::Error>> {
    let pool = init_db("sqlite::memory:").await?;
    let inode = create_inode(&pool, None, "f.txt", "FILE", 10).await?;
    let rev = create_file_revision(&pool, inode, 10, None, None, None, "local_write", None).await?;
    register_chunk(&pool, rev, &[7u8; 32], 0, 10).await?;

    let changed = soft_delete_inode(&pool, inode, 1_000).await?;
    assert!(changed);

    let deleted_at: Option<i64> =
        sqlx::query_scalar("SELECT deleted_at FROM inodes WHERE id = ?").bind(inode).fetch_one(&pool).await?;
    assert_eq!(deleted_at, Some(1_000));

    let chunk_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chunk_refs WHERE revision_id = ?").bind(rev).fetch_one(&pool).await?;
    assert_eq!(chunk_count, 1, "soft-delete must not touch chunk_refs");

    let second = soft_delete_inode(&pool, inode, 2_000).await?;
    assert!(!second, "already soft-deleted → no change");
    Ok(())
}
```

- [ ] **Step 2: Run — FAIL (funkcja nie istnieje)**

Run: `cargo test -p angeld --lib soft_delete_sets_timestamp_and_preserves_chunks`
Expected: FAIL (compile error, brak `soft_delete_inode`).

- [ ] **Step 3: Implementacja `soft_delete_inode`**

W `db.rs` (blisko `delete_inode_record`):
```rust
pub async fn soft_delete_inode(
    pool: &SqlitePool,
    inode_id: i64,
    now_ms: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE inodes SET deleted_at = ? WHERE id = ? AND kind = 'FILE' AND deleted_at IS NULL",
    )
    .bind(now_ms)
    .bind(inode_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
```

- [ ] **Step 4: Run — PASS**

Run: `cargo test -p angeld --lib soft_delete_sets_timestamp_and_preserves_chunks`
Expected: PASS.

- [ ] **Step 5: Wpięcie watcher (soft zamiast hard)**

W `watcher.rs::handle_deleted_path` zamień linie 479-480:
```rust
        db::delete_file_chunks(&self.pool, inode_id).await?;
        db::delete_inode_record(&self.pool, inode_id).await?;
        info!("watcher removed {} from sqlite", path.display());
```
na:
```rust
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        db::soft_delete_inode(&self.pool, inode_id, now_ms).await?;
        info!("watcher soft-deleted inode {} ({})", inode_id, path.display());
```

- [ ] **Step 6: Wpięcie API delete_file (soft zamiast hard)**

W `api/files.rs::delete_file` zamień linie 169-170:
```rust
    db::delete_file_chunks(&state.pool, inode_id).await?;
    db::delete_inode_record(&state.pool, inode_id).await?;
```
na:
```rust
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    db::soft_delete_inode(&state.pool, inode_id, now_ms).await?;
    tracing::info!("api soft-deleted inode {}", inode_id);
```

- [ ] **Step 7: Bramka + Commit**

Run: `cargo test -p angeld --lib soft_delete_sets_timestamp_and_preserves_chunks` (PASS) + `cargo clippy -p angeld --all-targets -- -D warnings`.
```bash
git add angeld/src/db.rs angeld/src/watcher.rs angeld/src/api/files.rs
git commit -m "feat(db): γ.c soft_delete_inode + wire watcher & API delete to soft-delete"
```

---

### Task 3: Blast radius — filtrowanie `deleted_at IS NULL` w listowaniach

**Files:**
- Modify: `angeld/src/db.rs` — `resolve_path` (3469), `get_inode_by_path` (3431), recursive path listing (5480, `WHERE i.kind='FILE'`), oraz pozostałe listujące `FROM inodes` (5507/5534/5553/5594/5613/5656/8277 — przejrzeć, filtr gdzie listują żywe pliki/katalogi).
- Test: `angeld/src/db.rs` `mod tests`.

**Interfaces:**
- `get_inode_by_id` (3452) POZOSTAJE niefiltrowany.

- [ ] **Step 1: Test — soft-deleted znika z resolve/path, ale raw-by-id go widzi**

```rust
#[tokio::test]
async fn soft_deleted_excluded_from_lookup_but_visible_by_id() -> Result<(), Box<dyn std::error::Error>> {
    let pool = init_db("sqlite::memory:").await?;
    let inode = create_inode(&pool, None, "gone.txt", "FILE", 1).await?;
    soft_delete_inode(&pool, inode, 1_000).await?;

    assert!(get_inode_by_path(&pool, None, "gone.txt").await?.is_none(), "soft-deleted must not resolve by path");
    assert!(resolve_path(&pool, "/gone.txt").await?.is_none(), "soft-deleted must not resolve as live");
    assert!(get_inode_by_id(&pool, inode).await?.is_some(), "raw by-id must still see soft-deleted");
    Ok(())
}
```

- [ ] **Step 2: Run — FAIL (obecnie soft-deleted nadal resolvuje)**

Run: `cargo test -p angeld --lib soft_deleted_excluded_from_lookup_but_visible_by_id`
Expected: FAIL (asercje `is_none` nie przechodzą).

- [ ] **Step 3: Dodaj filtr do get_inode_by_path (3431)**

Dodaj `AND deleted_at IS NULL` do `WHERE`:
```rust
        WHERE ((parent_id IS NULL AND ? IS NULL) OR parent_id = ?)
          AND name = ?
          AND deleted_at IS NULL
```

- [ ] **Step 4: Dodaj filtr do resolve_path (3469)**

`resolve_path` chodzi po segmentach ścieżki wołając wewnętrznie lookup po `(parent_id, name)`. Znajdź jego wewnętrzne zapytanie `FROM inodes WHERE ... name = ?` i dodaj `AND deleted_at IS NULL` (jeśli `resolve_path` deleguje do `get_inode_by_path`, Step 3 to pokrywa — zweryfikuj czytając 3469-3500).

- [ ] **Step 5: Dodaj filtr do recursive path listing (5480) i pozostałych**

W query 5480 zmień `WHERE i.kind = 'FILE'` na `WHERE i.kind = 'FILE' AND i.deleted_at IS NULL`. Przejrzyj pozostałe `FROM inodes` (5507/5534/5553/5594/5613/5656/8277); w każdym listującym żywe byty dodaj `AND deleted_at IS NULL` (dopasuj alias, np. `i.` lub `child.`+root). NIE zmieniaj zapytań restore/graft/snapshot (2118/1873/2183) ani `get_inode_by_id`.

- [ ] **Step 6: Run — PASS + pełny suite (regresja listowań)**

Run: `cargo test -p angeld --lib soft_deleted_excluded_from_lookup_but_visible_by_id` (PASS) + `cargo test -p angeld --lib` (wszystko green — łapie regresje w path/listing).
Expected: oba PASS.

- [ ] **Step 7: Commit**

```bash
git add angeld/src/db.rs
git commit -m "feat(db): γ.c exclude soft-deleted inodes from path lookup & listings"
```

---

### Task 4: Kosz db queries — list / restore / expired

**Files:**
- Modify: `angeld/src/db.rs` — `list_soft_deleted`, `restore_soft_deleted_inode`, `list_expired_soft_deleted` + struct `SoftDeletedInode`.
- Test: `angeld/src/db.rs` `mod tests`.

**Interfaces:**
- Produces:
  - `pub struct SoftDeletedInode { pub inode_id: i64, pub name: String, pub deleted_at: i64, pub size: i64 }`
  - `pub async fn list_soft_deleted(pool: &SqlitePool) -> Result<Vec<SoftDeletedInode>, sqlx::Error>`
  - `pub async fn restore_soft_deleted_inode(pool: &SqlitePool, inode_id: i64) -> Result<String, sqlx::Error>`
  - `pub async fn list_expired_soft_deleted(pool: &SqlitePool, cutoff_ms: i64) -> Result<Vec<i64>, sqlx::Error>`

- [ ] **Step 1: Testy — list, restore (z disambiguacją), expired**

```rust
#[tokio::test]
async fn list_and_restore_soft_deleted() -> Result<(), Box<dyn std::error::Error>> {
    let pool = init_db("sqlite::memory:").await?;
    let a = create_inode(&pool, None, "a.txt", "FILE", 5).await?;
    soft_delete_inode(&pool, a, 1_000).await?;

    let listed = list_soft_deleted(&pool).await?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].inode_id, a);
    assert_eq!(listed[0].name, "a.txt");
    assert_eq!(listed[0].deleted_at, 1_000);

    let name = restore_soft_deleted_inode(&pool, a).await?;
    assert_eq!(name, "a.txt");
    assert!(get_inode_by_path(&pool, None, "a.txt").await?.is_some(), "restored file resolves again");
    assert!(list_soft_deleted(&pool).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn restore_disambiguates_on_name_collision() -> Result<(), Box<dyn std::error::Error>> {
    let pool = init_db("sqlite::memory:").await?;
    let old = create_inode(&pool, None, "dup.txt", "FILE", 1).await?;
    soft_delete_inode(&pool, old, 1_000).await?;
    create_inode(&pool, None, "dup.txt", "FILE", 1).await?; // live collision

    let name = restore_soft_deleted_inode(&pool, old).await?;
    assert_ne!(name, "dup.txt", "must not collide with live file");
    assert!(name.contains("restored"), "restored name: {name}");
    Ok(())
}

#[tokio::test]
async fn list_expired_returns_only_past_cutoff() -> Result<(), Box<dyn std::error::Error>> {
    let pool = init_db("sqlite::memory:").await?;
    let old = create_inode(&pool, None, "old.txt", "FILE", 1).await?;
    let fresh = create_inode(&pool, None, "fresh.txt", "FILE", 1).await?;
    soft_delete_inode(&pool, old, 1_000).await?;
    soft_delete_inode(&pool, fresh, 9_000).await?;

    let expired = list_expired_soft_deleted(&pool, 5_000).await?;
    assert_eq!(expired, vec![old]);
    Ok(())
}
```

- [ ] **Step 2: Run — FAIL (funkcje nie istnieją)**

Run: `cargo test -p angeld --lib list_and_restore_soft_deleted restore_disambiguates_on_name_collision list_expired_returns_only_past_cutoff`
Expected: FAIL (compile).

- [ ] **Step 3: Implementacja**

W `db.rs`:
```rust
#[derive(Clone, Debug, FromRow)]
pub struct SoftDeletedInode {
    pub inode_id: i64,
    pub name: String,
    pub deleted_at: i64,
    pub size: i64,
}

pub async fn list_soft_deleted(pool: &SqlitePool) -> Result<Vec<SoftDeletedInode>, sqlx::Error> {
    sqlx::query_as::<_, SoftDeletedInode>(
        "SELECT id AS inode_id, name, deleted_at, size \
         FROM inodes WHERE deleted_at IS NOT NULL AND kind = 'FILE' \
         ORDER BY deleted_at DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn restore_soft_deleted_inode(
    pool: &SqlitePool,
    inode_id: i64,
) -> Result<String, sqlx::Error> {
    let inode = get_inode_by_id(pool, inode_id).await?.ok_or(sqlx::Error::RowNotFound)?;
    let mut final_name = inode.name.clone();
    let mut attempt = 1;
    while get_inode_by_path(pool, inode.parent_id, &final_name).await?.is_some() {
        let stem_ext = inode.name.clone();
        final_name = restored_name(&stem_ext, attempt);
        attempt += 1;
    }
    sqlx::query("UPDATE inodes SET deleted_at = NULL, name = ? WHERE id = ?")
        .bind(&final_name)
        .bind(inode_id)
        .execute(pool)
        .await?;
    Ok(final_name)
}

fn restored_name(original: &str, attempt: usize) -> String {
    let (stem, ext) = match original.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() => (s, format!(".{e}")),
        _ => (original, String::new()),
    };
    if attempt == 1 {
        format!("{stem} (restored){ext}")
    } else {
        format!("{stem} (restored {attempt}){ext}")
    }
}

pub async fn list_expired_soft_deleted(
    pool: &SqlitePool,
    cutoff_ms: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT id FROM inodes WHERE deleted_at IS NOT NULL AND deleted_at < ? ORDER BY id ASC",
    )
    .bind(cutoff_ms)
    .fetch_all(pool)
    .await
}
```

- [ ] **Step 4: Run — PASS**

Run: `cargo test -p angeld --lib list_and_restore_soft_deleted restore_disambiguates_on_name_collision list_expired_returns_only_past_cutoff`
Expected: 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add angeld/src/db.rs
git commit -m "feat(db): γ.c list/restore/expired soft-deleted queries"
```

---

### Task 5: Sweeper — periodyczny hard-delete po grace

**Files:**
- Modify: `angeld/src/main.rs` — nowy periodic task (wzorzec `token_cleanup_task`, ~linia 809).
- Test: `angeld/src/db.rs` `mod tests` (logika sweepera przez list_expired + hard-delete).

**Interfaces:**
- Consumes: `db::list_expired_soft_deleted`, `db::delete_file_chunks`, `db::delete_inode_record`, `db::SOFT_DELETE_GRACE_MS`.

- [ ] **Step 1: Test — hard-delete wygasłych, świeże zostają**

```rust
#[tokio::test]
async fn sweeper_hard_deletes_expired_only() -> Result<(), Box<dyn std::error::Error>> {
    let pool = init_db("sqlite::memory:").await?;
    let old = create_inode(&pool, None, "old.txt", "FILE", 1).await?;
    let rev = create_file_revision(&pool, old, 1, None, None, None, "local_write", None).await?;
    register_chunk(&pool, rev, &[1u8; 32], 0, 1).await?;
    let fresh = create_inode(&pool, None, "fresh.txt", "FILE", 1).await?;
    soft_delete_inode(&pool, old, 1_000).await?;
    soft_delete_inode(&pool, fresh, 9_000).await?;

    for inode_id in list_expired_soft_deleted(&pool, 5_000).await? {
        delete_file_chunks(&pool, inode_id).await?;
        delete_inode_record(&pool, inode_id).await?;
    }

    assert!(get_inode_by_id(&pool, old).await?.is_none(), "expired hard-deleted");
    assert!(get_inode_by_id(&pool, fresh).await?.is_some(), "fresh survives");
    let chunks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunk_refs").fetch_one(&pool).await?;
    assert_eq!(chunks, 0, "expired file chunks reclaimed");
    Ok(())
}
```

- [ ] **Step 2: Run — PASS (używa istniejących funkcji + Task 4)**

Run: `cargo test -p angeld --lib sweeper_hard_deletes_expired_only`
Expected: PASS.

- [ ] **Step 3: Spawn sweepera w main.rs**

Po bloku `_token_cleanup_task` (main.rs ~825) dodaj:
```rust
    let sweeper_pool = pool.clone();
    let _soft_delete_sweeper = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            ticker.tick().await;
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let cutoff = now_ms - db::SOFT_DELETE_GRACE_MS;
            match db::list_expired_soft_deleted(&sweeper_pool, cutoff).await {
                Ok(expired) => {
                    for inode_id in expired {
                        if let Err(err) = db::delete_file_chunks(&sweeper_pool, inode_id).await {
                            tracing::warn!("soft-delete sweeper: chunk delete failed for {inode_id}: {err}");
                            continue;
                        }
                        if let Err(err) = db::delete_inode_record(&sweeper_pool, inode_id).await {
                            tracing::warn!("soft-delete sweeper: inode delete failed for {inode_id}: {err}");
                            continue;
                        }
                        tracing::info!("soft-delete sweeper: grace expired, hard-deleted inode {inode_id}");
                    }
                }
                Err(err) => tracing::warn!("soft-delete sweeper query failed: {err}"),
            }
        }
    });
```

- [ ] **Step 4: Bramka + Commit**

Run: `cargo build -p angeld` (kompiluje) + `cargo clippy -p angeld --all-targets -- -D warnings`.
```bash
git add angeld/src/main.rs angeld/src/db.rs
git commit -m "feat(daemon): γ.c soft-delete sweeper — hard-delete after 7d grace"
```

---

### Task 6: API Kosz — `/api/trash` list / restore / purge

**Files:**
- Modify: `angeld/src/api/files.rs` — 3 handlery + 3 routy w `router()` (obok `delete_file`).
- Test: db-level (Task 4) pokrywa logikę; API smoke manualny w Task 8.

**Interfaces:**
- Consumes: `db::list_soft_deleted`, `db::restore_soft_deleted_inode`, `db::get_inode_by_id`, `db::delete_file_chunks`, `db::delete_inode_record`, `db::resolve_inode_path` (jeśli istnieje — inaczej `original_path` = sama nazwa; patrz Step 2).
- Produces: routy `GET /api/trash`, `POST /api/trash/{inode_id}/restore`, `POST /api/trash/{inode_id}/purge`.

- [ ] **Step 1: Dodaj routy w `router()` (files.rs)**

W builderze routera (po `.route("/api/quota", ...)`):
```rust
        .route("/api/trash", get(list_trash))
        .route("/api/trash/{inode_id}/restore", post(restore_trash))
        .route("/api/trash/{inode_id}/purge", post(purge_trash))
```

- [ ] **Step 2: Handlery**

Dodaj do `files.rs`. `days_remaining` liczone z `deleted_at + GRACE`. `original_path`: użyj istniejącej funkcji ścieżki jeśli jest (grep `resolve_inode_path`/`inode_path`); jeśli brak — zwróć samą `name` (MVP, ścieżka do wzbogacenia osobno).
```rust
#[derive(serde::Serialize)]
struct TrashItem {
    inode_id: i64,
    name: String,
    deleted_at: i64,
    size: i64,
    days_remaining: i64,
}

async fn list_trash(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    acl::require_role(&state.pool, &headers, Role::Member).await?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let items: Vec<TrashItem> = db::list_soft_deleted(&state.pool)
        .await?
        .into_iter()
        .map(|r| {
            let remaining_ms = (r.deleted_at + db::SOFT_DELETE_GRACE_MS) - now_ms;
            let days_remaining = (remaining_ms.max(0) + 86_400_000 - 1) / 86_400_000;
            TrashItem { inode_id: r.inode_id, name: r.name, deleted_at: r.deleted_at, size: r.size, days_remaining }
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn restore_trash(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(inode_id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acl::require_role(&state.pool, &headers, Role::Member).await?;
    let inode = db::get_inode_by_id(&state.pool, inode_id).await?.ok_or(ApiError::NotFound {
        resource: "inode",
        id: inode_id.to_string(),
    })?;
    if inode.deleted_at.is_none() {
        return Err(ApiError::Conflict { message: "inode is not in trash".to_string() });
    }
    let restored_name = db::restore_soft_deleted_inode(&state.pool, inode_id).await?;
    tracing::info!("api restored inode {} from trash as {}", inode_id, restored_name);
    Ok(Json(serde_json::json!({ "inode_id": inode_id, "restored_name": restored_name })))
}

async fn purge_trash(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(inode_id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acl::require_role(&state.pool, &headers, Role::Member).await?;
    let inode = db::get_inode_by_id(&state.pool, inode_id).await?.ok_or(ApiError::NotFound {
        resource: "inode",
        id: inode_id.to_string(),
    })?;
    if inode.deleted_at.is_none() {
        return Err(ApiError::Conflict { message: "inode is not in trash".to_string() });
    }
    db::delete_file_chunks(&state.pool, inode_id).await?;
    db::delete_inode_record(&state.pool, inode_id).await?;
    tracing::info!("api purged inode {} from trash (hard-delete)", inode_id);
    Ok(Json(serde_json::json!({ "inode_id": inode_id, "purged": true })))
}
```
**Uwaga:** `InodeRecord` musi mieć pole `deleted_at`. Jeśli struct nie zawiera `deleted_at`, dodaj `pub deleted_at: Option<i64>` do `InodeRecord` i do SELECT-ów `get_inode_by_id`/`get_inode_by_path` (dodaj kolumnę `deleted_at` w liście SELECT). Zrób to w tym stepie — to część kontraktu restore/purge.

- [ ] **Step 3: Bramka**

Run: `cargo build -p angeld` + `cargo clippy -p angeld --all-targets -- -D warnings` + `cargo test -p angeld --lib`.
Expected: kompiluje, clippy czysty, testy green.

- [ ] **Step 4: Commit**

```bash
git add angeld/src/api/files.rs angeld/src/db.rs
git commit -m "feat(api): γ.c /api/trash list/restore/purge endpoints"
```

---

### Task 7: Web UI — widok Kosz (lista wierszy)

**Files:**
- Modify: `angeld/static/index.html` — nav-item, widok, mapa VIEWS, JS `loadKosz`.

**Interfaces:**
- Consumes: `GET /api/trash`, `POST /api/trash/{id}/restore`, `POST /api/trash/{id}/purge`.

- [ ] **Step 1: Nav-item + wpis VIEWS**

W `#primaryNav` (po „Pliki", ~linia 232) dodaj `<a>` wzorowany na sąsiednich (`data-view="kosz"`, ikona Material Symbols `delete`, label „Kosz"). W mapie `VIEWS` (~3479) dodaj `'kosz': { icon: 'delete', title: 'Kosz' }`.

- [ ] **Step 2: Widok + render**

Dodaj `<div id="view-kosz" data-view="kosz" class="hidden p-8 max-w-4xl mx-auto space-y-6 pb-16">` z nagłówkiem „Kosz" + podtytułem „Pliki usuwane trwale po 7 dniach" + kontenerem `<div id="koszList">`. Dodaj JS (wzorzec `activateDiagnostykaView`/`refreshDiagnostykaPanels`):
```javascript
async function loadKosz() {
  const list = document.getElementById('koszList');
  const res = await fetch('/api/trash');
  const data = await res.json();
  if (!data.items || data.items.length === 0) {
    list.innerHTML = '<div class="text-center text-slate-500 py-16">Kosz jest pusty. Usunięte pliki trafią tu na 7 dni.</div>';
    return;
  }
  list.innerHTML = data.items.map(it => {
    const badge = it.days_remaining >= 4 ? 'text-emerald-400 bg-emerald-400/15'
                : it.days_remaining >= 2 ? 'text-amber-400 bg-amber-400/15'
                : 'text-red-400 bg-red-400/15';
    const dni = it.days_remaining === 1 ? '1 dzień' : it.days_remaining + ' dni';
    return `<div class="flex items-center gap-3 bg-white/5 border border-white/10 rounded-xl px-4 py-3">
      <div class="flex-1 min-w-0">
        <div class="text-slate-200 font-semibold truncate">${it.name}</div>
        <div class="text-slate-500 text-xs">usunięto ${new Date(it.deleted_at).toLocaleDateString('pl-PL')}</div>
      </div>
      <span class="text-xs font-semibold px-2 py-1 rounded-full ${badge}">${dni}</span>
      <button onclick="restoreKosz(${it.inode_id})" class="text-sky-400 bg-sky-400/15 rounded-lg px-3 py-1.5 text-xs font-semibold">Przywróć</button>
      <button onclick="purgeKosz(${it.inode_id})" class="text-red-400 border border-red-400/30 rounded-lg px-3 py-1.5 text-xs">Usuń teraz</button>
    </div>`;
  }).join('');
}
async function restoreKosz(id) {
  await fetch(`/api/trash/${id}/restore`, { method: 'POST' });
  loadKosz();
}
async function purgeKosz(id) {
  if (!confirm('Usunąć plik trwale? Tej operacji nie można cofnąć.')) return;
  await fetch(`/api/trash/${id}/purge`, { method: 'POST' });
  loadKosz();
}
```
Podepnij `loadKosz()` w handlerze przełączania widoku dla `data-view="kosz"` (tam gdzie inne widoki mają swoje `activate…`).

- [ ] **Step 3: Manualna weryfikacja (build + smoke)**

Run: `cargo build --release -p angeld`. Uruchom daemon z `target/release`, otwórz `http://127.0.0.1:8787`, odblokuj vault. Usuń plik testowy w O:\ (w SYNC_PATH!) → sprawdź że pojawia się w „Kosz" z badge dni. Kliknij Przywróć → plik wraca do O:\ i znika z Kosza. Usuń ponownie → Usuń teraz → znika trwale.
**Święta Zasada:** operuj tylko na plikach testowych w SYNC_PATH.

- [ ] **Step 4: Commit**

```bash
git add angeld/static/index.html
git commit -m "feat(ui): γ.c Kosz view — soft-deleted files list, restore & purge"
```

---

### Task 8: Restore O:\ re-projekcja + bramka + STATUS

**Files:**
- Modify: `angeld/src/api/files.rs` (restore_trash) — wyzwolenie re-projekcji placeholdera jeśli potrzebne.
- Modify: `STATUS.md` §12.7.

- [ ] **Step 1: Zweryfikuj re-projekcję O:\ przy restore**

W smoke z Task 7 Step 3 sprawdź, czy po Przywróć plik **fizycznie wraca do O:\** (nie tylko do listy „Pliki" w Web UI). Jeśli TAK (reconciliation/projection sam re-projektuje po wyczyszczeniu `deleted_at`) — restore jest kompletny, przejdź do Step 2. Jeśli NIE — znajdź w `smart_sync.rs` funkcję projekcji placeholdera per-inode (`create_projection_placeholder` :1140 lub entrypoint reconciliation) i wywołaj ją z `restore_trash` dla przywróconego inode (przez `ApiState`, jeśli daemon eksponuje handle do smart-sync; inaczej dodaj lekki trigger). Dopisz krok jako mini-TDD jeśli pojawi się testowalna jednostka.

- [ ] **Step 2: Pełna bramka**

Run:
```
cargo fmt --check
cargo clippy -p angeld --all-targets -- -D warnings
cargo clippy -p angeld --all-targets --features test-helpers -- -D warnings
cargo test -p angeld --lib
```
Expected: wszystko zielone (w tym nowe testy γ.c).

- [ ] **Step 3: STATUS §12.7 γ.c → DONE**

W tabeli §12.7 zmień wiersz γ.c na `✅` z opisem: soft-delete oba triggery + grace 7d + restore(disambiguacja) + sweeper + `/api/trash` + widok Kosz. Zaktualizuj drzewko (`γ.c ... ✅ DONE`).

- [ ] **Step 4: Commit**

```bash
git add STATUS.md angeld/src/api/files.rs
git commit -m "docs(status): γ.c soft-delete + Kosz DONE"
```

---

## Self-Review

**Spec coverage:**
- Model `inodes.deleted_at` + grace const → Task 1. ✅
- Soft-delete oba triggery → Task 2. ✅
- gc bez zmian (soft-delete no-op) → strukturalnie, brak zadania (nic do zmiany). ✅
- Blast radius listowań → Task 3. ✅
- Restore (disambiguacja) → Task 4 (db) + Task 6 (API) + Task 8 (O:\ re-projekcja). ✅
- Sweeper → Task 5. ✅
- API `/api/trash` list/restore/purge → Task 6. ✅
- Web UI Kosz (layout A, badge progi, empty state, bez bulk) → Task 7. ✅
- Cross-device / katalogi / bulk / konfig grace = poza zakresem (spec). ✅

**Placeholder scan:** Task 8 Step 1 zawiera warunkową ścieżkę (re-projekcja) — to celowa weryfikacja empiryczna cfapi (Święta Zasada, nie fabrykuję niesprawdzonego wywołania), nie placeholder wymagania. Reszta kroków ma pełny kod + komendy.

**Type consistency:** `soft_delete_inode(pool, inode_id, now_ms) -> bool`, `SoftDeletedInode {inode_id,name,deleted_at,size}`, `restore_soft_deleted_inode -> String`, `list_expired_soft_deleted(pool, cutoff) -> Vec<i64>`, `SOFT_DELETE_GRACE_MS: i64` — spójne między Task 4/5/6. `InodeRecord.deleted_at: Option<i64>` dodane w Task 6 Step 2 (kontrakt restore/purge). ⚠️ Zależność: Task 6 wymaga `InodeRecord.deleted_at` — jeśli implementer robi taski poza kolejnością, ta modyfikacja structa jest w Task 6.
