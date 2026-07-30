# P2-003 — Bin/lib dual-compile de-duplikacja — Design

**Data:** 2026-07-30
**Issue:** KNOWN_ISSUES.md P2-003 (Bin `angeld` duplikuje moduły z lib)
**Decyzja:** Opcja A, wariant minimalny (A1) — zatwierdzona przez Przemka 2026-07-30.

---

## 1. Cel

Wyeliminować podwójną kompilację modułów crate’a `angeld`. Dziś `main.rs` (bin) i `lib.rs` (lib) deklarują ten sam zestaw modułów, więc 31 modułów kompiluje się dwa razy. Po zmianie każdy moduł kompiluje się **raz** (w lib), a bin staje się cienkim konsumentem biblioteki.

Zakres to **czysto strukturalny refaktor** — zero zmian zachowania, zero zmian API sieciowego, zero migracji.

## 2. Problem (stan obecny)

- `angeld/src/main.rs` deklaruje `mod xxx;` dla ~40 modułów.
- `angeld/src/lib.rs` deklaruje `pub mod xxx;` dla 31 modułów.
- **Przecięcie (31 modułów) kompiluje się dwukrotnie** — raz jako część `lib angeld`, raz jako część `bin angeld` (w tym `db.rs` 10 649 linii, `smart_sync.rs` 2,2k, `downloader.rs` 1,7k).
- Skutki: ~2× czas kompilacji tych modułów; dwa oddzielne raporty clippy per target (lib-only `clippy --workspace` przepuszczał linty widoczne dopiero przy `--all-targets`); ryzyko driftu, gdyby lib i bin się rozjechały.

**Moduły bin-only (9, tylko w `main.rs`):** `api`, `gc`, `repair`, `scrubber`, `sharing`, `shell_integration`, `shell_state`, `watcher`, `windows_hello`.

## 3. Ground-truth (weryfikacja w kodzie 2026-07-30)

1. **`main.rs` nie eksportuje żadnego `pub` itemu** (`pub fn/struct/enum/const/mod`) — żaden inny moduł nie zależy od symboli z bin roota. Przeniesienie modułów bin-only do lib jest więc wykonalne bez rozplątywania zależności zwrotnych.
2. Moduły bin-only referują wyłącznie `crate::<inny_moduł>` (shared albo inny bin-only). Po przeniesieniu **wszystkich** 9 do lib każde `crate::X` rozwiązuje się wewnątrz tego samego crate’a (lib) — **bez edycji w tych plikach**.
3. Jedyny plik wymagający zmiany referencji to `main.rs` (`crate::…` → `angeld::…`).

## 4. Rozważone warianty (Opcja A z KNOWN_ISSUES)

| Wariant | Opis | Diff | Werdykt |
|---|---|---|---|
| **A1** | 9 modułów bin-only → do `lib.rs`; `main.rs` traci deklaracje `mod`, referuje przez `angeld::`. `run_daemon` zostaje w bin. | Mały: `lib.rs` +9 linii, `main.rs` sweep `crate::`→`angeld::`. | ✅ **WYBRANE** |
| A2 | Bin-only zostają w bin; sweep `crate::<shared>`→`angeld::` w wielu plikach (api/, watcher, repair, scrubber, gc…). | Duży, wielopikowy. | Odrzucone — potrzebne tylko gdyby lib miało być „czyste" pod mobile; mobile konsumuje `omnidrive-core`, nie daemon-lib. |
| A3 | A1 + `run_daemon` przeniesiony do lib, `main.rs` ~30 linii. | Największy. | Odrzucone (YAGNI) — `run_daemon` nie jest duplikowany, brak zysku compile. |

**Uzasadnienie A1:** problem P2-003 to duplikacja kompilacji + niespójny clippy + drift. A1 usuwa to w całości najmniejszym, mechanicznym, weryfikowalnym przez kompilator diffem.

## 5. Konkretne zmiany

1. **`angeld/src/lib.rs`** — dopisać deklaracje 9 modułów bin-only, z zachowaniem obecnego cfg-gatingu z `main.rs`:
   - `pub mod api; pub mod gc; pub mod repair; pub mod scrubber; pub mod sharing; pub mod shell_integration; pub mod shell_state; pub mod watcher;`
   - `windows_hello` — z identycznym `#[cfg(target_os = "windows")]` jak w `main.rs`.
2. **`angeld/src/main.rs`** — usunąć **wszystkie** deklaracje `mod xxx;`; zamienić `use crate::…` oraz inline `crate::…` na `angeld::…`.
3. **Widoczność w lib** — podnieść `mod` → `pub mod` tam, gdzie `main.rs` odwołuje się do modułu prywatnego w lib (np. `win_acl`). Kompilator wskaże każdy taki przypadek (`private module`).
4. **cfg-gating** — `win_session`, `windows_hello`, `win_acl` zachowują dokładnie te same atrybuty `#[cfg(...)]` co dziś; żaden moduł Windows-only nie traci gatingu.

Pliki źródłowe 9 modułów **nie zmieniają lokalizacji ani treści** — zmienia się tylko miejsce deklaracji (`main.rs` → `lib.rs`).

## 6. Kryteria sukcesu (weryfikacja)

Refaktor bez zmiany zachowania — bezpiecznikiem jest istniejąca zielona suita + kompilator. Brak nowych testów.

- `cargo build --release --workspace` → OK.
- `cargo clippy --workspace --all-targets -- -D warnings` → czyste (po zmianie lib i bin dają jeden, spójny set lintów).
- `cargo test -p omnidrive-core` + `cargo test -p angeld` → pełna suita zielona (baseline **angeld 186**).
- Każdy moduł z sekcji 2 pojawia się w grafie kompilacji **dokładnie raz** (weryfikacja: brak modułu zadeklarowanego jednocześnie w `main.rs` i `lib.rs`).
- Pomiar compile-time before/after (`cargo build --release --workspace` z czystego `target`) — oczekiwany spadek; wynik zaraportowany, nie bramkuje.

## 7. Ryzyko i rollback

- **Ryzyko:** niskie. Zmiana czysto strukturalna, w pełni weryfikowana przez kompilator (nierozwiązane referencje = błąd budowania, nie subtelny bug runtime).
- **Rollback:** jeden commit; `git revert` trywialny.

## 8. Poza zakresem

- **Dekompozycja `db.rs`** (P3-001, monolit 10 649 linii) — osobny cykl spec→plan po zamknięciu P2-003.
- **Przeniesienie `run_daemon`/bootstrapu do lib** (wariant A3) — nie realizujemy.
- Jakiekolwiek zmiany logiki modułów, API sieciowego, schematu bazy.
