# Dekompozycja `angeld/src/downloader.rs` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans.

**Goal:** Rozbić `angeld/src/downloader.rs` (1 730 linii) na `angeld/src/downloader/` z 8 plikami, bez zmiany zachowania.

**Architecture:** 57% pliku to jeden `impl Downloader` (988 linii, 17 metod). Blok jest dzielony na 4 bloki `impl Downloader` w osobnych modułach (`read`, `pack`, `provider`, `prefetch`) — Rust pozwala na inherent impl w dowolnym module crate'a definiującego typ. Typy i `DownloaderError` zostają w `mod.rs`, wolne funkcje idą do `chunk.rs` i `util.rs`. Metody prywatne wołane między modułami dostają `pub(super)`.

**Spec:** `docs/superpowers/specs/2026-07-31-downloader-decomposition-design.md`
**Baza:** `02c9fb2`.

## Global Constraints

- **ZERO zmian zachowania.** Dozwolone wyłącznie: nagłówek `use` per plik, prefiks `pub(super) `, przeniesienie bloku.
- **ZERO zmian poza `angeld/src/downloader.rs` i `angeld/src/downloader/**`.**
- **ZERO nowych testów, migracji, bumpu wersji, nowych `#[allow]`.**
- **Liczniki:** core **28**, angeld lib **199**; dodatkowo testy e2e konsumujące `Downloader` muszą się kompilować.
- **Bramka przed pushem:** fmt + clippy `--all-targets -D warnings` oba tryby + `build --release --workspace` + obie suity.

---

### Task 0: Narzędzia i dowód bezstratności

- [x] **Step 1:** baseline `downloader_baseline.rs` (1 730 linii)
- [x] **Step 2:** round-trip parsera na całym pliku
- [x] **Step 3:** round-trip parsera na ciele `impl Downloader` (dedent 4 → split → rekonstrukcja) — **OK**
- [x] **Step 4:** inwentarz: 59 bloków top-level, 17 metod w `impl Downloader`
- [ ] **Step 5:** `dl_build.py` — generator z manifestem nazwa→moduł, obsługą podziału `impl` i wyliczaniem `pub(super)`
- [ ] **Step 6:** `dl_verify.py` — kontrola kompletności metod i treści; dozwolone różnice: `pub(super)` oraz przeformatowanie == `rustfmt(baseline)`

### Task 1: Rename i wygenerowanie struktury

- [ ] **Step 1:** `git mv angeld/src/downloader.rs angeld/src/downloader/mod.rs`, potwierdź `R … (100%)`
- [ ] **Step 2:** `dl_build.py` → `mod.rs` + 7 plików
- [ ] **Step 3:** `dl_verify.py` — 17 metod obecnych dokładnie raz, treść zgodna
- [ ] **Step 4:** `cargo check --workspace --all-targets`, przycięcie importów z diagnostyki (część wspólna obu trybów)
- [ ] **Step 5:** Commit

### Task 2: Testy i pełna bramka

- [ ] **Step 1:** rozdzielenie 4 testów do modułów wg asertowanego zachowania
- [ ] **Step 2:** `cargo fmt --all` + ponowna weryfikacja
- [ ] **Step 3:** clippy oba tryby, `build --release --workspace`
- [ ] **Step 4:** core 28, angeld lib 199, kompilacja testów e2e
- [ ] **Step 5:** kontrola zakresu diffu
- [ ] **Step 6:** Commit + push

### Task 3: Dokumentacja

- [ ] **Step 1:** `KNOWN_ISSUES.md` P2-009 zamknięty (rozmiary, lista `pub(super)`, wyniki bramki, zakres pokrycia testami)
- [ ] **Step 2:** `STATUS.md` §12.7b + skreślenie `downloader.rs` z listy długu
- [ ] **Step 3:** pamięć: blok STAN + komenda startowa
- [ ] **Step 4:** Commit + push
