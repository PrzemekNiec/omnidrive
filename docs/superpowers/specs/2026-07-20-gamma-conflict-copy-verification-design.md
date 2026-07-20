# γ.1 — Conflict-copy verification (design)

**Data:** 2026-07-20
**Faza:** γ — Zero Data Loss Hardening
**Typ:** weryfikacja istniejącego feature'u przez testy (charakteryzacja), zero zmian w kodzie produkcyjnym (chyba że test obnaży bug)

---

## Kontekst

Conflict-copy nie jest greenfieldem — rdzeń jest zaimplementowany i wpięty w **lokalną ścieżkę zapisu**:

- `watcher.rs:424` przekazuje `base_revision_id` (rewizja bazowa edycji tego urządzenia) do `packer.pack_file_with_expected_parent`.
- `packer.rs` woła `db::classify_revision_lineage(candidate = expected_parent, current = head)`.
- Klasyfikacja (`db.rs:4002`) zwraca `RevisionLineageRelation`:
  - `Same` — candidate == current → brak konfliktu.
  - `CandidateDescendsFromCurrent` — fast-forward → brak konfliktu.
  - `CurrentDescendsFromCandidate` — lokalna baza jest przestarzała → `reason = "stale_local_base"` → konflikt.
  - `Parallel` — rozjazd gałęzi → `reason = "parallel_local_edit"` → konflikt.
- Przy konflikcie `db::materialize_conflict_copy_from_revision` tworzy nowy inode `nazwa (conflict - device - ts)`, kopiuje chunk refs, zapisuje `conflict_events` z `materialized_inode_id`/`materialized_revision_id`.
- Surface: `db::list_recent_conflicts` → `peer.rs:364` → `MultiDeviceSnapshot.recent_conflicts` (Multi-Device tab).

**Problem:** zero testów integracyjnych. Feature data-safety, którego nie wiemy czy działa — ten sam dług „zbudowane ≠ zweryfikowane" co odłożony Dell smoke.

## Zakres (świadomie ograniczony)

**W zakresie:** lokalna ścieżka konfliktu — klasyfikacja lineage (4-way), materializacja kopii, surface przez `list_recent_conflicts`.

**Poza zakresem (udokumentowany limit, NIE brak testu):** konflikt cross-device. Uzasadnienie: aplikacja rewizji plików między urządzeniami to Faza δ. β.b metadata fetch worker jest **roster-merge-only** (dotyka wyłącznie `devices`/`vault_members`, NIGDY `file_revisions`), a `ingest.rs:340` woła zwykły `pack_file` (bez `expected_parent`). Nie istnieje ścieżka, w której zdalna edycja nadpisuje lokalną rozjezdną edycję → test cross-device dałby fałszywy FAIL na nieistniejącym feature. Pełny CRDT-sync treści = δ.

## Macierz weryfikacji

### Warstwa 1 — klasyfikacja lineage (db-level, `db.rs` `#[cfg(test)]`)

Driver na `sqlite::memory:`, ręczne `create_file_revision` budujące lineage, asercje na `classify_revision_lineage`.

| # | Setup | Oczekiwanie |
|---|---|---|
| T1 | candidate == current | `Same` |
| T2 | candidate potomkiem current (fast-forward) | `CandidateDescendsFromCurrent` |
| T3 | current potomkiem candidate (przestarzała baza) | `CurrentDescendsFromCandidate` |
| T4 | rodzeństwo (wspólny przodek, żadne nie jest przodkiem drugiego) | `Parallel` |

### Warstwa 2 — materializacja (db-level, `db.rs` `#[cfg(test)]`)

Dla scenariuszy T3/T4 wołamy `materialize_conflict_copy_from_revision` i asertujemy:

- (a) powstał nowy inode z nazwą wg `build_conflict_copy_name` (`nazwa (conflict - device - ts)`);
- (b) chunk refs skopiowane z rewizji źródłowej (kopia odtwarzalna) — `copy_chunk_refs`;
- (c) `conflict_events` ma wiersz z ustawionymi `materialized_inode_id`/`materialized_revision_id` i poprawnym `reason`;
- (d) `list_recent_conflicts` zwraca ten event;
- (e) kolizja nazw → drugi konflikt dostaje `disambiguate_conflict_copy_name` (sufiks ` [1]`).

### Warstwa 3 — wiring packera (packer-level, `packer.rs` `#[cfg(test)]`)

Reużywamy harness z istniejących testów packera (`db::init_db("sqlite::memory:")` + `VaultKeyStore::new().unlock` + `Packer::new` + `PackerConfig::new(spool_dir)`).

- **stale base:** pack v1 (`expected_parent=None`) → rev1; pack v2 (`expected_parent=rev1`) → rev2 (head); pack v3 (`expected_parent=rev1`, przestarzały) → `PackResult.conflict_copy_name == Some(...)`, `conflict_events` zawiera `stale_local_base`.
- **parallel:** head = rev_a (parent rev1); podsuwamy divergentny `expected_parent = rev_b` (parent rev1, utworzony przez `create_file_revision`) → `parallel_local_edit`, kopia powstaje.
- **kontrola negatywna:** `Same`/fast-forward → `conflict_copy_name == None` (brak fałszywych kopii).

## Struktura kodu

Testy dodajemy do istniejących modułów `#[cfg(test)] mod tests` w `db.rs` (warstwa 1+2) i `packer.rs` (warstwa 3) — spójnie z obecnym wzorcem. Bez nowego pliku w `tests/`. **Zero zmian w kodzie produkcyjnym.**

Jeśli którykolwiek test FAIL — obnażyliśmy realny bug w feature data-safety. Wtedy: STOP, zgłoszenie Przemkowi, tryb `superpowers:systematic-debugging` przed jakimkolwiek fixem.

## Definition of Done

- Wszystkie testy z warstw 1–3 zielone.
- Bramka: `cargo test -p angeld --lib` + `cargo clippy --all-targets` oba tryby (default + `test-helpers`) + `cargo fmt --check`.
- Spec + wynik odnotowane w STATUS.md (γ.1).
- Bez bumpu wersji (czysto testowa zmiana).

## Poza zakresem / follow-up

- Konflikt cross-device → Faza δ (wymaga ścieżki aplikacji rewizji między urządzeniami).
- Ewentualny cleanup nieaktualnych `#[allow(dead_code)]` na `classify_revision_lineage`/`list_recent_conflicts`/`materialize_*` (są realnie używane) — osobny, chirurgiczny commit, tylko jeśli bramka to potwierdzi.
