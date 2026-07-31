# Dekompozycja `angeld/src/smart_sync.rs` — design

**Data:** 2026-07-31
**Baza:** HEAD `0639e0c` origin/main, v0.3.28
**Typ:** refaktor mechaniczny — zero zmian zachowania, zero migracji, zero bumpu wersji
**Poprzednik:** `2026-07-31-db-decomposition-design.md` (ta sama metoda ekstraktora)

---

## 1. Stan wyjściowy

`angeld/src/smart_sync.rs` = **2 236 linii** (audyt §2.2 mówił 2 197 — plik urósł), w dwóch wyraźnych warstwach:

| Zakres | Zawartość | Linie |
|---|---|---|
| 1–296 | Publiczne API: `SmartSyncError` (+4 `impl`), `SyncRootStateSnapshot`, `SyncRootRepairReport`, **16 `pub fn`** — każda rozgałęzia `#[cfg(windows)]` → `imp::…` / `#[cfg(not(windows))]` → `UnsupportedPlatform` | ~296 |
| 297–2236 | `#[cfg(windows)] mod imp { … }` — 30 importów, 7 stałych, 2 statiki, 5 struktur, `impl Drop`, **~60 funkcji** w tym 3 callbacki `unsafe extern "system"` + ich `_inner` | ~1 940 |

**Zero testów w pliku.** To istotna różnica względem `db.rs`, gdzie 58 testów stanowiło realną siatkę bezpieczeństwa. Tutaj bezpiecznikiem są wyłącznie: kompilator (kod jest `#[cfg(windows)]`, a budujemy na Windows, więc jest w pełni sprawdzany), `cargo build --release --workspace` oraz maszynowy dowód zero-drift. Suita 199 testów `angeld` nie pokrywa tego modułu i **nie należy jej traktować jako potwierdzenia poprawności tego refaktoru**.

## 2. Korekta oceny ryzyka z audytu

Audyt §2.2 zapisał: *„Risk: zero — wszystkie wewnętrzne fn są `fn` (private), tylko `pub use imp::*` w `mod.rs` musiałby ujawnić poszczególne moduły. Czysto mechaniczne."*

To jest **niedoszacowane w jednym punkcie**: prywatność funkcji nie jest ułatwieniem, tylko źródłem jedynego realnego kosztu tego refaktoru. Dziś wszystkie ~55 prywatnych funkcji `imp` widzą się nawzajem, bo leżą w jednym module. Po rozbiciu na siostrzane pliki każda funkcja wołana spoza swojego nowego modułu musi dostać `pub(super)`. To nadal refaktor mechaniczny, ale **modyfikuje sygnatury**, a nie tylko przenosi bloki — czego przy `db.rs` praktycznie nie było (tam jedna zmiana widoczności na 342 bloki).

Konsekwencja dla weryfikacji: kontrola zero-drift musi jawnie rozróżniać „treść identyczna" od „treść identyczna z dokładnością do dopisanego `pub(super)`", i raportować **pełną listę** podniesionych widoczności, żeby dało się ją przejrzeć jako całość.

## 3. Cel i zakres

**Cel:** rozbić plik na `angeld/src/smart_sync/` z 9 plikami, żaden powyżej ~550 linii.

**W zakresie:** przeniesienie 1:1 wszystkich elementów; `pub(super)` tam i tylko tam, gdzie wymusza to podział; redystrybucja importów per plik; deklaracje modułów i re-eksporty.

**Poza zakresem:** zmiany logiki cfapi, kolejności wywołań COM, obsługi błędów, treści logów; nowe testy; bump wersji; jakakolwiek zmiana w plikach konsumujących `smart_sync::`.

## 4. Docelowa struktura

```
angeld/src/smart_sync/
  mod.rs             ~300   publiczne API bez zmian + `#[cfg(windows)] mod imp;`
  imp/
    mod.rs            ~25   deklaracje podmodułów + re-eksporty dla warstwy publicznej
    registration.rs  ~550   rejestracja/wyrejestrowanie SyncRoot, connect, audit, repair,
                            tożsamość providera, diagnostyka ACL, przygotowanie katalogu
    callbacks.rs     ~430   3 callbacki cfapi + `_inner`, install_hydration_runtime,
                            complete_transfer_*, decode_file_identity, log_callback_panic
    placeholder.rs   ~295   pin state, hydratacja, ewikcja, convert_to_ghost, mark_in_sync
    projection.rs    ~275   projekcja vaulta do SyncRoot, tworzenie placeholderów, flagi
    paths.rs         ~130   normalizacja ścieżek, konwersje wide-string, czas pliku
    state.rs          ~60   stałe providera, CONNECTION_KEY, HYDRATION_CONTEXT,
                            HydrationContext/Request, ComApartmentGuard + Drop
    lifecycle.rs      ~75   dismount_after_lock, mount_after_unlock, dehydrate rekurencyjny
```

`imp/mod.rs` re-eksportuje podmoduły globem (`pub(super) use registration::*;` …), dzięki czemu warstwa publiczna w `smart_sync/mod.rs` nadal woła `imp::register_sync_root_public(…)` **bez żadnej zmiany**.

Struktury wędrują do modułu, który je konsumuje: `ExistingSyncRootInfo` → `registration.rs`, `PlaceholderIdentity` → `callbacks.rs`, `HydrationContext`/`HydrationRequest`/`ComApartmentGuard` → `state.rs`.

## 5. Widoczność

Reguła: element zostaje prywatny, jeśli jest używany wyłącznie w swoim nowym module; dostaje `pub(super)`, jeśli używa go moduł siostrzany. `pub(super)` (a nie `pub(crate)`) — zasięg ma nie wyjść poza `imp`. Elementy już `pub` (te wołane przez warstwę publiczną) zostają bez zmian.

Lista podniesionych widoczności jest wyliczana automatycznie z realnych referencji i **w całości raportowana** w commicie oraz w `KNOWN_ISSUES`. Ręczne zgadywanie jest wykluczone.

## 6. Importy

Blok 30 importów `imp` zawiera dwie duże grupy klamrowe (`windows::Win32::Storage::CloudFilters::{…}` ~23 pozycje, `FileSystem::{…}`). Przy redystrybucji rozwijam każdy import do osobnej linii, po czym przycinam iteracyjnie z diagnostyki `cargo check --message-format=json`. Zmiana stylu importów (płaskie zamiast grup klamrowych) jest świadoma i konieczna: przycinanie po numerach linii wymaga, by jeden import = jedna linia.

## 7. Weryfikacja zero-drift

1. **Round-trip parsera** — rekonstrukcja `mod imp` z bloków musi dać baseline co do bajtu. ✅ *już wykonane przed napisaniem tego spec-a.*
2. **Kompletność i treść bloków** — każdy blok baseline'u istnieje w wyniku, treść identyczna albo różniąca się wyłącznie prefiksem `pub(super) `; każda taka różnica trafia na raportowaną listę.
3. **Kompilator** — `cargo check --workspace --all-targets` w obu trybach; ten moduł jest w całości `#[cfg(windows)]`, więc na tej maszynie jest realnie kompilowany.
4. **Zakres diffa** — `git diff` nie może pokazać nic poza `angeld/src/smart_sync.rs` → `angeld/src/smart_sync/**` i `docs/`.

## 8. Ryzyka

| Ryzyko | Mitygacja |
|---|---|
| Brak testów jednostkowych modułu | Świadomie zaakceptowany: refaktor jest czystym przeniesieniem, dowodzonym porównaniem bloków. Poprawność runtime cfapi i tak weryfikuje wyłącznie live smoke, nieosiągalny w tej sesji — **i tak pozostaje po tej zmianie nieprzeprowadzony** |
| Zbyt szerokie podniesienie widoczności | `pub(super)`, nigdy `pub(crate)`; lista wyliczana z referencji, nie z intuicji |
| `unsafe extern "system"` callbacki | Przenoszone jako całe bloki, bez dotykania sygnatur; adres funkcji trafia do tabeli cfapi tak samo jak dziś |
| Statiki `CONNECTION_KEY`/`HYDRATION_CONTEXT` rozjechane na dwa moduły | Oba w `state.rs`, jedno źródło prawdy, `pub(super)` |

## 9. Definition of Done

- [x] `angeld/src/smart_sync.rs` nie istnieje; istnieje `angeld/src/smart_sync/` z 9 plikami, żaden > ~550 linii
- [x] `cargo fmt --all --check` czysty; clippy `--all-targets -D warnings` czysty w obu trybach
- [x] `cargo build --release --workspace` OK; core **28**, angeld lib **199**
- [x] Wszystkie bloki baseline obecne, treść identyczna modulo `pub(super)`; lista podniesień zaraportowana
- [x] `git diff` poza `smart_sync/` i `docs/` pusty
- [x] Wpis w `KNOWN_ISSUES.md` (P2-008) + `STATUS.md` §12.7b
