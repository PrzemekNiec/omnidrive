# γ.c — Soft-delete grace period + Kosz (design)

**Data:** 2026-07-20
**Faza:** γ — Zero Data Loss Hardening
**Typ:** nowa funkcja (backend + Web UI). Greenfield — audyt 2026-07-20 potwierdził zero śladu w kodzie.

---

## Cel

Kasowanie pliku staje się odwracalne przez 7 dni. Usunięty plik trafia do „Kosza", z którego user może go **Przywrócić** albo **Usunąć teraz**; po 7 dniach jest kasowany trwale przez sweeper. DoD roadmapy (STATUS §12.7 γ.c): „usuń plik → 7 dni odzyskiwalny → po 7 dniach gone" + UI Kosz w sidebar.

## Decyzje (z brainstormingu)

- **Trigger:** OBA — delete w Eksploratorze (O:\, watcher) ORAZ delete z Web UI → soft-delete. Odzysk z zakładki „Kosz" w Web UI.
- **Model:** in-place `inodes.deleted_at` (podejście A), NIE osobna tabela. Zgodne 1:1 z roadmapą, minimalny kod, zero przenoszenia danych.
- **Zakres:** pełny — backend + dopracowany glassmorphism Kosz.
- **Layout Kosza:** lista wierszy (wybrane w makiecie).
- **Grace:** 7 dni, stałe (YAGNI, bez konfiguracji).
- **Zakres bytów:** tylko `kind = 'FILE'` (spójnie z obecnymi ścieżkami delete).

## Architektura

### 1. Model danych

- Kolumna `inodes.deleted_at INTEGER NULL` (unix ms). `NULL` = żywy; wartość = soft-deleted w tym momencie. Migracja przez `ensure_column_exists` (istniejący wzorzec, additive).
- Const `SOFT_DELETE_GRACE_MS: i64 = 7 * 24 * 60 * 60 * 1000`.

### 2. Soft-delete (oba triggery)

- `db::soft_delete_inode(pool, inode_id, now_ms) -> Result<bool, sqlx::Error>` → `UPDATE inodes SET deleted_at = ? WHERE id = ? AND kind = 'FILE' AND deleted_at IS NULL`. Zwraca `true` gdy zmieniono wiersz. **Nie dotyka** `chunk_refs`/`file_revisions`/`packs` → dane nienaruszone.
- `watcher.rs::handle_deleted_path` (460): zamiana `delete_file_chunks + delete_inode_record` na `soft_delete_inode`. **Jedyna zmiana w watcherze, czysto DB, zero operacji na plikach** (Święta Zasada). Log `tracing::info!` z inode_id + „soft-delete".
- `api/files.rs::delete_file` (148): analogicznie `soft_delete_inode` zamiast twardego delete. Odpowiedź `{ inode_id, deleted: true }` bez zmian kształtu.

### 3. gc — bez zmian

`gc::get_orphaned_pack_ids` reclaimuje packi bez `pack_locations`. Soft-delete jest **no-op na danych** (chunk_refs/packs/pack_locations zostają), więc gc nie ruszy danych w grace. Reclaim jedzie starą ścieżką dopiero po hard-delete sweepera. Zero zmian w `gc.rs`.

### 4. Blast radius listowań — filtrowanie `deleted_at IS NULL`

**Zasada:** zapytania resolucji ścieżki i listowania plików filtrują soft-deleted; surowy fetch po ID zostaje niefiltrowany (Kosz/sweeper na nim polegają).

Filtrują (dodać `deleted_at IS NULL` / `i.deleted_at IS NULL`):
- `resolve_path` (db.rs:3469) — używa watcher; soft-deleted plik NIE resolvuje się jako żywy.
- `get_inode_by_path` (db.rs:3431) — resolucja + dedup nazw.
- Recursive path listing (db.rs:5480) — file browser (`WHERE i.kind='FILE'` → dodać `AND i.deleted_at IS NULL`).
- Pozostałe listujące `FROM inodes` (5507/5534/5553/5594/5613/5656/8277) — przegląd każdego w planie; filtr tam, gdzie listują żywe pliki/katalogi.

Niefiltrowane (celowo): `get_inode_by_id` (db.rs:3452) — raw fetch; Kosz/restore/sweeper go potrzebują.

Test: soft-deleted plik znika z file-listing/resolve, ale jest widoczny w `list_soft_deleted`.

### 5. Restore

- `db::restore_soft_deleted_inode(pool, inode_id) -> Result<String, sqlx::Error>` → jeśli żywy inode o tej samej `(parent_id, name)` istnieje (kolizja podczas grace — twardo pilnowana przez unique index z γ.1), przemianuj przywracany na `nazwa (restored).ext`, w razie dalszej kolizji `nazwa (restored N).ext` (reuse wzorca `disambiguate_conflict_copy_name`); potem `UPDATE inodes SET deleted_at = NULL, name = <final>`. Zwraca finalną nazwę.
- Re-materializacja placeholdera: po wyczyszczeniu `deleted_at` wyzwolić istniejącą ścieżkę tworzenia placeholdera O:\ (ta sama, którą mount/reconciliation tworzy dla każdego pliku) → plik wraca jako ghost, hydrate-on-open; chunki już są, **zero re-uploadu**. Dokładny punkt wpięcia (funkcja w `smart_sync.rs`/mount) ustalony w planie.

### 6. Watcher przy restore

Restore odtwarza **placeholder/ghost** przez normalną ścieżkę mount → inode już istnieje, więc short-circuit content-hash (`watcher.rs:402`, `prev.content_hash == Some(hash)` → skip pack) + istniejący inode zapobiegają nowej rewizji/re-packowi. Zabezpieczenie strukturalne (restore = normalne odtworzenie placeholdera, które watcher toleruje). Zweryfikowane testem/ręcznie w planie.

### 7. Sweeper (hard-delete po grace)

- `db::list_expired_soft_deleted(pool, cutoff_ms) -> Result<Vec<i64>, sqlx::Error>` → inode_id gdzie `deleted_at IS NOT NULL AND deleted_at < cutoff`.
- `start_soft_delete_sweeper(pool)` — periodyczny worker (wzorzec `start_metadata_backup_worker`, tick 1h), dla każdego wygasłego: twardy `delete_file_chunks + delete_inode_record` (istniejąca ścieżka) → gc reclaimuje packi. Log `tracing::info!` z inode_id + „grace expired hard-delete". Spawn w `main.rs` (full-daemon, obok pozostałych workerów).

### 8. API Kosz (nowy moduł `api/trash.rs`)

| Endpoint | Metoda | Rola | Działanie |
|---|---|---|---|
| `/api/trash` | GET | Member | `db::list_soft_deleted(pool)` → `{ items: [{ inode_id, name, original_path, deleted_at, size, days_remaining }] }`. `days_remaining = ceil((deleted_at + GRACE − now)/dzień)`, min 0. |
| `/api/trash/{inode_id}/restore` | POST | Member | `restore_soft_deleted_inode` + re-materializacja placeholdera. Zwraca `{ inode_id, restored_name }`. |
| `/api/trash/{inode_id}/purge` | POST | Member | Twardy delete natychmiast (`delete_file_chunks + delete_inode_record`). Zwraca `{ inode_id, purged: true }`. |

- Konwencje jak `delete_file`: `acl::require_role(Role::Member)`, `ApiError::NotFound` gdy inode nie istnieje.
- `restore`/`purge` na inode który NIE jest soft-deleted → `ApiError::Conflict` („inode is not in trash").
- `original_path` liczona z hierarchii inodów (reuse recursive path CTE albo `resolve` w odwrotną stronę).
- Rejestracja routów w routerze API (`api/mod.rs` lub `files.rs` router — wg tego gdzie osadzimy moduł).

### 9. Web UI — widok Kosz (`angeld/static/index.html`)

- Nowy `nav-item` w `#primaryNav`: `data-view="kosz"`, ikona `delete`, label „Kosz".
- Widok `<div id="view-kosz" data-view="kosz" class="hidden p-8 ...">` + wpis w mapie `VIEWS` (`kosz: { icon:'delete', title:'Kosz' }`).
- JS `loadKosz()` — `GET /api/trash`, render **listy wierszy** (layout A): ikona wg typu · nazwa + oryginalna ścieżka + data usunięcia · rozmiar · badge „pozostałe dni" (progi: `≥4` zielony, `2–3` żółty, `≤1` czerwony) · **Przywróć** (akcent) + **Usuń teraz** (destrukcyjny, `confirm()`). Po akcji reload listy. Wzorzec jak `activateDiagnostykaView`/`refreshDiagnostykaPanels`.
- Empty state: wycentrowana ikona + „Kosz jest pusty. Usunięte pliki trafią tu na 7 dni."
- Bez bulk-actions (empty-trash-all) — YAGNI.

## Testowanie

- **db (unit, `sqlite::memory:`):** soft_delete ustawia deleted_at bez ruszania chunk_refs; listing/resolve wyklucza soft-deleted a raw-by-id nie; restore czyści deleted_at + disambiguuje przy kolizji; list_soft_deleted + days_remaining; list_expired_soft_deleted zwraca tylko wygasłe.
- **API:** trash list/restore/purge happy-path + `Conflict` gdy nie-w-koszu.
- **Sweeper:** wygasły → hard-delete (chunk_refs + inode znikają), nie-wygasły → zostaje.
- **Watcher (integracja):** delete pliku → soft-delete (dane zostają), restore → brak nowej rewizji.
- Bramka: `cargo test -p angeld --lib` + `clippy --all-targets` oba tryby + `fmt --check`.

## Definition of Done

- Soft-delete oba triggery, grace 7d, restore (z disambiguacją), sweeper, API Kosz, widok Kosz w Web UI — działające end-to-end.
- Listing/resolve wyklucza soft-deleted; gc nie rusza danych w grace.
- Testy zielone; bramka green.
- STATUS §12.7 γ.c → DONE. Bump wersji do decyzji (nowa funkcja user-facing → prawdopodobnie tak, ale osobno).

## Poza zakresem / follow-up

- Katalogi (soft-delete tylko FILE; kasowanie katalogów = osobno).
- Bulk „opróżnij kosz".
- Konfigurowalny grace period.
- Cross-device propagacja stanu Kosza (spójne z resztą — cross-device sync = Faza δ).
