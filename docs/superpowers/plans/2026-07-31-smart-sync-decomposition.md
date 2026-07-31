# Dekompozycja `angeld/src/smart_sync.rs` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Kroki oznaczone `- [x]`.

**Goal:** Rozbić `angeld/src/smart_sync.rs` (2 236 linii) na `angeld/src/smart_sync/` z 9 plikami, bez żadnej zmiany zachowania.

**Architecture:** Warstwa publiczna (16 `pub fn` + typy błędów) zostaje nietknięta w `smart_sync/mod.rs`. Wnętrze `mod imp` (1 940 linii) rozjeżdża się na `smart_sync/imp/{registration,callbacks,placeholder,projection,paths,state,lifecycle}.rs`, spięte globami w `imp/mod.rs` — dzięki czemu wywołania `imp::foo(…)` z warstwy publicznej nie zmieniają się. Prywatne funkcje używane przez moduł siostrzany dostają `pub(super)`.

**Tech Stack:** Rust Edition 2024, windows-rs (cfapi / CloudFilters), tokio, sqlx.

**Spec:** `docs/superpowers/specs/2026-07-31-smart-sync-decomposition-design.md`
**Baza:** `0639e0c` — numery linii odnoszą się do `git show 0639e0c:angeld/src/smart_sync.rs`.

## Global Constraints

- **ZERO zmian zachowania.** Bloki kopiowane dosłownie. Jedyne dozwolone modyfikacje: nagłówek `use` per plik oraz prefiks `pub(super) ` wymuszony podziałem.
- **ZERO zmian poza `angeld/src/smart_sync.rs` i `angeld/src/smart_sync/**`.**
- **ZERO nowych testów, zero migracji, zero bumpu wersji.**
- **Liczniki suity sztywne:** core **28**, angeld lib **199** — mają się nie zmienić (ten moduł nie ma testów, więc każda zmiana licznika = regresja gdzie indziej).
- **Postawa lintowa bez zmian.** Żadnych nowych `#[allow]`. Import potrzebny tylko pod flagą → `#[cfg(feature = …)]`, nie tłumik.
- **Bramka przed pushem:** fmt + clippy `--all-targets -D warnings` oba tryby + `build --release --workspace` + obie suity. Pre-push aktywny, nigdy `--no-verify`.

---

### Task 0: Narzędzia i baseline

- [x] **Step 1:** `git show 0639e0c:angeld/src/smart_sync.rs > $SCRATCH/smart_sync_baseline.rs` (2 236 linii)
- [x] **Step 2:** Round-trip parsera — rekonstrukcja `mod imp` z bloków identyczna z baseline co do bajtu. **Bez zielonego round-tripu nie wolno przenosić niczego.**
- [x] **Step 3:** Inwentarz bloków: warstwa zewnętrzna + wnętrze `imp` z rozmiarami.

*(Kroki 1–3 wykonane przed napisaniem planu: round-trip OK, inwentarz = 79 bloków w `imp`.)*

- [x] **Step 4:** `ss_build.py` — generator plików z manifestu nazwa→moduł, z automatycznym wyliczeniem `pub(super)` z realnych referencji między modułami.
- [x] **Step 5:** `ss_verify.py` — kontrola kompletności i treści bloków; różnica wyłącznie w prefiksie `pub(super) ` jest dozwolona i **raportowana imiennie**, każda inna = błąd.

### Task 1: Rename + wygenerowanie struktury

**Files:** `angeld/src/smart_sync.rs` → `angeld/src/smart_sync/mod.rs` + `smart_sync/imp/*.rs`

- [x] **Step 1:** `git mv angeld/src/smart_sync.rs angeld/src/smart_sync/mod.rs`; potwierdź `R … (100%)` w `git status --short`
- [x] **Step 2:** Uruchom `ss_build.py` — zapisuje `mod.rs` (warstwa publiczna + `#[cfg(windows)] mod imp;`) oraz 8 plików w `imp/`
- [x] **Step 3:** `ss_verify.py` — komplet bloków, treść identyczna modulo `pub(super)`
- [x] **Step 4:** `cargo check --workspace --all-targets`; brakujące importy uzupełnia iteracyjne przycinanie (`prune_lines.py`), nie ręczne zgadywanie
- [x] **Step 5:** Commit

```bash
git add angeld/src/smart_sync
git commit -m "refactor(smart_sync): rozbij monolit na smart_sync/ + imp/"
```

### Task 2: Przycięcie importów i pełna bramka

- [x] **Step 1:** `prune_lines.py` — usuwa importy zbędne w **obu** trybach (część wspólna, nie suma), do zbieżności
- [x] **Step 2:** `cargo fmt --all`, potem ponowny `ss_verify.py` (fmt nie ma prawa ruszyć treści bloków)
- [x] **Step 3:** `cargo clippy --workspace --all-targets -- -D warnings` oraz to samo z `--features test-helpers`
- [x] **Step 4:** `cargo build --release --workspace`
- [x] **Step 5:** `cargo test -p omnidrive-core` = 28; `cargo test -p angeld --lib` = 199
- [x] **Step 6:** Kontrola zakresu: `git diff --stat` poza `smart_sync/` i `docs/` musi być pusty
- [x] **Step 7:** Commit + `git push origin main`

### Task 3: Dokumentacja

- [x] **Step 1:** `KNOWN_ISSUES.md` — wpis **P2-008** od razu zamknięty (wzorzec P2-007): rozmiary plików, pełna lista podniesień `pub(super)`, wyniki bramki, jawna adnotacja o braku testów jednostkowych modułu
- [x] **Step 2:** `STATUS.md` §12.7b — wiersz P2-008, aktualizacja listy pozostałego długu (skreślenie `smart_sync.rs`)
- [x] **Step 3:** Pamięć: blok STAN + komenda startowa następnej sesji (Faza δ albo `downloader.rs` 1 712)
- [x] **Step 4:** Commit + push
