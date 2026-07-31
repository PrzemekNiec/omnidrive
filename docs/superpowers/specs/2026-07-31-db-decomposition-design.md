# Dekompozycja `angeld/src/db.rs` — design

**Data:** 2026-07-31
**Baza:** HEAD `6d834fa` origin/main, v0.3.28
**Typ:** refaktor mechaniczny — zero zmian zachowania, zero migracji schematu, zero bumpu wersji

---

## 1. Problem

`angeld/src/db.rs` = **10 649 linii** w jednym pliku:

| Zakres | Zawartość | Linie |
|---|---|---|
| 1–591 | inner attribute, 6 importów, 3 enumy, ~55 struct rekordów | ~590 |
| 592–1336 | `init_db` — pełen schemat + migracje inline | ~745 |
| 1337–8551 | **238 `pub async fn` + 8 `pub fn`** w ~14 domenach | ~7 215 |
| 8552–10649 | jeden `mod tests` — 58 testów + helpery | ~2 100 |

Konsekwencje: każda zmiana w jednej domenie wymaga nawigacji po całym pliku; plik nie mieści się w kontekście edytora ani modelu; granice domen istnieją tylko w głowie, nie w kodzie. Audyt `docs/superpowers/specs/2026-05-11-code-audit.md §2.1` wskazał ten plik jako **najpilniejszego kandydata do dekompozycji** i zalecił zrobienie tego **przed mobile** (UniFFI łatwiej projektować na modułach niż na monolicie).

Numeracja linii w audycie (8592) jest nieaktualna — plik urósł o ~2 000 linii (γ.b conflict-copy, γ.c soft-delete, β.b graft roster). Podział domenowy z audytu trzyma się jednak kodu i jest tu punktem wyjścia.

**Uwaga do trackera:** dekompozycja `db.rs` nie ma dziś własnego ID w `docs/KNOWN_ISSUES.md` (P3-001 to zamknięty wpis o AAD, P2-003 to zamknięty dual-compile). Wpis zostanie założony przy zamknięciu zadania.

## 2. Cel i zakres

**Cel:** rozbić `db.rs` na katalog `angeld/src/db/` z 30 plikami tematycznymi (29 modułów + `test_support.rs`), tak by żaden nie przekraczał ~800 linii kodu produkcyjnego.

**W zakresie:**
- przeniesienie 1:1 wszystkich typów, funkcji i testów do plików tematycznych
- płaskie re-eksporty w `db/mod.rs` zachowujące dotychczasowe ścieżki wywołań
- rozdzielenie bloku testowego per domena + wspólny `test_support.rs`
- nadanie `pub(super)` prywatnym helperom używanym poza swoją domeną

**Poza zakresem (jawnie NIE robimy):**
- jakakolwiek zmiana treści SQL, sygnatur, semantyki, kolejności operacji
- migracje schematu, zmiany w `init_db` poza przeniesieniem
- nowe testy (to refaktor — bezpiecznikiem jest istniejąca suita i kompilator)
- zmiany w plikach konsumujących `db::` (912 call-site'ów zostaje nietkniętych)
- bump wersji, zmiany w instalatorze/payloadzie
- „przy okazji" poprawki lintów, nazw, formatowania przenoszonego kodu

## 3. Decyzje

### D1 — Płaskie API przez `pub use` (zatwierdzone)

`db/mod.rs` deklaruje podmoduły i re-eksportuje je globem:

```rust
pub mod inodes;
pub mod packs;
// …
pub use inodes::*;
pub use packs::*;
```

Call-site pozostaje `db::get_inode_by_path(pool, path)` — **żaden plik poza `db/` nie jest dotykany**. Diff jest w 100% zamknięty w jednym katalogu, co czyni weryfikację „zero drift" wykonalną. Alternatywa (jawne `db::inodes::…`) rozlałaby diff na ~20 plików i kilkaset linii przy zerowym zysku funkcjonalnym — odrzucona.

Kolizje nazw przy globach nie występują: dziś wszystkie symbole żyją w jednej przestrzeni nazw jednego pliku, więc są z definicji unikalne.

### D2 — Testy rozdzielone per submoduł (zatwierdzone)

Każdy plik domenowy dostaje własny `#[cfg(test)] mod tests`. Wspólne helpery (`temp_test_dir`, `build_source_vault`, `USER_FIXTURE`) trafiają do `db/test_support.rs` pod `#[cfg(test)]`.

Testy zmieniają `use super::*` na `use crate::db::*` (+ `use crate::db::test_support::*` tam, gdzie korzystają z helperów) — inaczej widziałyby tylko własną domenę.

Rozdzielenie testów wykonujemy **na końcu**, w ostatniej fali. Do tego czasu blok testowy zostaje w `mod.rs` z `use super::*`, co dzięki re-eksportom (D1) kompiluje się przez cały czas trwania refaktoru.

### D3 — Granularność: 30 plików, cel ≤ ~800 linii kodu produkcyjnego (zatwierdzone)

Podział grubszy (13 plików wg audytu) zostawiłby trzy pliki > 1 000 linii — czyli mniejsze monolity zamiast jednego dużego. Domeny w `db.rs` nie są ciągłe (sync policies w trzech miejscach, chunk queries w czterech), więc przypisanie i tak wymaga decyzji per symbol; przy tej samej pracy warto uzyskać docelową strukturę od razu.

### D4 — Fale tematyczne, ~8 commitów, egzekucja inline (zatwierdzone)

Przenoszenie 8,5 tys. linii przez subagenta to ryzyko cichej modyfikacji treści, której nie wyłapie się w szumie ogromnego diffu. Wykonanie inline, falami; każda fala kończy się zielonym `cargo check --all-targets` + `cargo test -p angeld --lib` i osobnym commitem (bisektowalność).

## 4. Docelowa struktura

```
angeld/src/db/
  mod.rs              ~130   inner attribute, 3 enumy, epoch_secs, SOFT_DELETE_GRACE_MS, deklaracje + re-eksporty
  schema.rs           ~760   init_db, ensure_column_exists (priv)
  graft.rs            ~950   VaultRestoreApplyReport, Restored*, graft_restored_metadata_snapshot, RosterMergeSummary, graft_roster_additive
  uploads.rs          ~740   upload jobs/targets, retry storm, gc_orphan_packs, sync_upload_targets_from_shards
  shards.rs           ~650   pack_shards lifecycle + scrub
  vault_state.rs      ~600   vault params, EVK, wrapped DEK, rotacja, KDF migration, rewrap queue
  packs.rs            ~450   packs CRUD, health/scrub summaries, storage mode
  inodes.rs           ~400   inode CRUD, resolve_path, soft-delete/Kosz
  revisions.rs        ~350   file_revisions, promote, lineage
  chunks.rs           ~350   chunk refs, lookups, locations
  users.rs            ~330   users, vault_members, migrate_single_to_multi_user, backfill UUID
  device_identity.rs  ~300   local_device_identity, keypairy (X25519/Kyber), trusted peers
  projection.rs       ~290   smart_sync_state, pin/hydration, projekcja O:\, eviction
  shares.rs           ~250   shared links, share password tokens
  conflicts.rs        ~230   conflict events, materializacja kopii, naming helpers
  ingest.rs           ~200   ingest jobs
  system_config.rs    ~200   system_config, cloud usage + limity
  providers.rs        ~190   provider configs + secrets
  cache.rs            ~180   cache CRUD + LRU
  sync_policies.rs    ~180   sync policies + matching helpers
  devices.rs          ~170   devices CRUD, wrapy VK, revoke
  migration_v2.rs     ~115   V1→V2 pack migration
  metadata_backup.rs  ~110   metadata backup attempts/status
  sessions.rs         ~125   user sessions
  oauth.rs            ~105   oauth state, encrypted refresh tokens
  invites.rs          ~95    invite codes
  stats.rs            ~90    stats overview, traffic buckets
  recovery_keys.rs    ~70    vault recovery keys
  audit.rs            ~50    audit log
  test_support.rs     (cfg(test)) wspólne helpery testowe
```

Rozmiary to szacunki z realnych zakresów linii w `db.rs`; ostateczne wartości mogą się różnić o kilka procent (importy per plik).

Struct rekordów wędrują **do swojej domeny**, nie do wspólnego `types.rs` — `InodeRecord` obok zapytań o inody, `PackShardRecord` obok operacji na shardach. Kohezja jest wtedy widoczna w drzewie plików, a płaskie re-eksporty i tak wystawiają je pod dotychczasową ścieżką.

## 5. Współdzielone elementy i widoczność

| Element | Docelowo | Widoczność |
|---|---|---|
| `epoch_secs` | `mod.rs` | `pub` (bez zmian — używany w 8 domenach) |
| `SOFT_DELETE_GRACE_MS` | `mod.rs` | `pub` |
| `PackStatus`, `ShardRole`, `StorageMode` (+ `impl`) | `mod.rs` | `pub` — typy przekrojowe (packs, shards, uploads, inodes, projection) |
| `new_user_id` | `users.rs` | `pub` (jedyni konsumenci to `migrate_single_to_multi_user`, `backfill_uuid_user_ids`) |
| `normalize_policy_path` | `sync_policies.rs` | **`pub(super)`** — używany też przez `projection.rs` |
| `ensure_column_exists` | `schema.rs` | prywatny (tylko `init_db`) |
| `validate_inode_kind` | `inodes.rs` | prywatny |
| `path_matches_policy` | `sync_policies.rs` | prywatny |
| `is_revision_ancestor` | `revisions.rs` | prywatny |
| `projection_relative_path` | `projection.rs` | prywatny |
| `restored_name` | `inodes.rs` | prywatny |
| `build_conflict_copy_name`, `disambiguate_conflict_copy_name`, `split_file_name`, `sanitize_conflict_component` | `conflicts.rs` | prywatne |
| `MIGRATION_FAILPOINT` + `set_migration_failpoint` | `vault_state.rs` | jak dziś, pod `#[cfg(feature = "test-helpers")]` |

`pub(super)` (a nie `pub(crate)`) dla `normalize_policy_path` — helper ma pozostać wewnętrzny dla modułu `db`, nie stać się częścią API crate'a.

### Postawa lintowa

`db.rs:1` ma inner attribute `#![allow(clippy::too_many_arguments, dead_code)]` obejmujący cały moduł. Zostaje **wyłącznie w `db/mod.rs`** — poziomy lintów dziedziczą się w dół drzewa modułów, więc obejmą wszystkie podmoduły dokładnie tak jak dziś. Żadnych nowych `#[allow]` rozsiewanych po plikach. Gdyby clippy pokazał inaczej, jest to sygnał do weryfikacji, nie do dopisywania tłumików.

### `#[cfg(feature = "test-helpers")]`

Failpoint migracji KDF jest gated feature'em. Bramka musi lecieć w obu trybach (default + `--features test-helpers`), zgodnie z praktyką repo.

## 6. Weryfikacja „zero drift"

Refaktor bez nowych testów wymaga twardych dowodów, że kod nie zmienił treści. Cztery poziomy:

1. **Kompilator** — `cargo check --workspace --all-targets` po każdej fali. Wychwytuje brakujące importy, zerwane widoczności, literówki w nazwach.
2. **Suita** — `cargo test -p omnidrive-core` = **28**, `cargo test -p angeld --lib` = **199**. Każda liczba inna niż te dwie = błąd, nie „usprawnienie".
3. **Zbiór symboli** — skrypt porównuje posortowaną listę publicznych sygnatur z `git show <base>:angeld/src/db.rs` z listą z konkatenacji `db/*.rs`. Musi być identyczna.
4. **Hash ciał funkcji** — skrypt wycina każdą funkcję (od nagłówka do następnego elementu top-level), normalizuje białe znaki i porównuje posortowane hashe przed/po. To najmocniejszy dowód, że przeniesienie było dosłowne. Skrypt żyje w scratchpadzie, nie w repo.

Dodatkowo: `git diff --stat <base>..HEAD` musi pokazywać **wyłącznie** ścieżki `angeld/src/db.rs` i `angeld/src/db/**`. Jakikolwiek inny plik w diffie = naruszenie zakresu.

## 7. Fale

| Fala | Zawartość | Uzasadnienie kolejności |
|---|---|---|
| **F0** | `git mv db.rs db/mod.rs` | Sam rename, zero zmian treści — git wykryje 100% similarity, kolejne diffy są czytelne |
| **F1** | `schema.rs`, `graft.rs` | Dwa największe, w pełni samodzielne bloki — od razu zdejmują ~1 700 linii |
| **F2** | `vault_state.rs`, `system_config.rs`, `providers.rs`, `migration_v2.rs` | Domena crypto/config, niskie sprzężenie z resztą |
| **F3** | `inodes.rs`, `revisions.rs`, `chunks.rs`, `sync_policies.rs`, `projection.rs`, `conflicts.rs`, `metadata_backup.rs` | Najbardziej rozproszona grupa — wymaga uwagi przy przypisaniu, robiona gdy plik już schudł |
| **F4** | `packs.rs`, `shards.rs`, `cache.rs` | Warstwa storage |
| **F5** | `uploads.rs`, `ingest.rs` | Kolejki |
| **F6** | `users.rs`, `devices.rs`, `device_identity.rs`, `sessions.rs`, `audit.rs`, `invites.rs`, `recovery_keys.rs`, `stats.rs`, `oauth.rs`, `shares.rs` | Tożsamość i reszta ogona — po tej fali `mod.rs` zawiera już tylko re-eksporty i testy |
| **F7** | `test_support.rs` + rozdzielenie 58 testów, finalne czyszczenie `mod.rs` | Testy na końcu, gdy docelowy podział jest znany |

Każda fala = jeden commit z zieloną bramką `check` + `test --lib`. Pełna bramka (fmt + clippy oba tryby + release build + obie suity) przed pushem.

## 8. Ryzyka

| Ryzyko | Mitygacja |
|---|---|
| Ciche zgubienie funkcji przy przenoszeniu | Poziom 3 weryfikacji (zbiór symboli) — złapie brak; kompilator złapie brak używany gdziekolwiek |
| Ciche zmodyfikowanie ciała funkcji | Poziom 4 (hash ciał) |
| Test zgubiony przy rozdzielaniu (F7) | Licznik 199 musi się zgadzać co do jednego; dodatkowo diff nazw testów przed/po |
| Nowe warningi clippy na przeniesionym kodzie | Inner attribute w `mod.rs` zachowuje postawę lintową; bramka `-D warnings` w obu trybach |
| Konflikt z równoległą pracą na `db.rs` | Brak — pracujemy solo na `main`, refaktor idzie w jednej sesji |
| Rozjazd `git mv` (git nie wykryje rename) | F0 to wyłącznie rename bez zmian treści, similarity 100% |

## 9. Definition of Done

- [ ] `angeld/src/db.rs` nie istnieje; istnieje `angeld/src/db/` z 30 plikami, żaden > ~800 linii kodu produkcyjnego
- [ ] `cargo fmt --all --check` czysty
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` czysty w obu trybach (default + `--features test-helpers`)
- [ ] `cargo build --release --workspace` OK
- [ ] `cargo test -p omnidrive-core` = **28**, `cargo test -p angeld --lib` = **199**
- [ ] `git diff --stat` względem bazy pokazuje wyłącznie `angeld/src/db.rs` + `angeld/src/db/**`
- [ ] Zbiór publicznych sygnatur identyczny przed/po (poziom 3)
- [ ] Hashe ciał funkcji identyczne przed/po (poziom 4)
- [ ] Brak bumpu wersji, brak zmian schematu, brak nowych testów
- [ ] Wpis w `docs/KNOWN_ISSUES.md` (nowe ID) + aktualizacja `STATUS.md`
