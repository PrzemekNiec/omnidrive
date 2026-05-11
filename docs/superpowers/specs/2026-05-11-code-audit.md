# OmniDrive — Code Audit (Faza 0, krok 0.1)

> Data: 2026-05-11 · Wersja: v0.3.23 (commit `bbcc643b0d8042eabc37a671c32811e8d7d36892`)
> Zakres: `angeld/src/`, `omnidrive-core/src/`, oraz przegląd reszty crateów workspace.
> Wynik: lista znalezisk → wpisy w `docs/KNOWN_ISSUES.md` (P3 lub wyżej); ten plik = mapa długu + surowe metryki.

## 1. Raw metrics

### Toolchain status
- `clippy` + `rustfmt`: zainstalowane (stable, up to date — rustc 1.94.0).
- `nightly`: zainstalowany (`nightly-x86_64-pc-windows-msvc`, rustc 1.97.0-nightly 2026-05-10).
- `cargo-udeps`: zainstalowany (v0.1.61, `cargo install cargo-udeps --locked` — OK).

### rustfmt (`cargo fmt --all -- --check`)
- exit code: **1** (kod NIE jest fmt-clean)
- hunków (`Diff in …` linii): **869**
- plików z diffami: **63** distinct
  - `angeld/src/`: ~49 plików (m.in. `db.rs`, `smart_sync.rs`, `downloader.rs`, `onboarding.rs`, `main.rs`, `vault.rs`, `identity.rs`, `uploader.rs`, `packer.rs`, `repair.rs`, wszystkie `api/*.rs`, …)
  - `angeld/tests/`: 8 plików (`common/mod.rs`, wszystkie `e2e_*.rs`)
  - `omnidrive-core/src/`: 4 (`crypto.rs`, `layout.rs`, `lib.rs`)
  - `omnidrive-cli/src/main.rs`, `omnidrive-tray/src/main.rs`, `omnidrive-shell-ext/src/lib.rs`
  - `angeld/src/bin/cfapi_repro.rs`
- → Task 5 Step 3 musi zdecydować: jednorazowy `cargo fmt --all` commit (diff duży ale mechaniczny: 869 hunków / 63 pliki) vs `rustfmt.toml` łagodzący różnice. Brak `rustfmt.toml` obecnie.

### clippy pedantic+nursery (`cargo clippy --workspace --all-targets --all-features -- -W clippy::pedantic -W clippy::nursery`)
- exit code: 0 (to tylko `-W`, nie `-D` — nie failuje)
- warningów ogółem (unikalnych po lokalizacji primary span): **~1332** — z czego:
  - `angeld`: ~1254
  - `omnidrive-core`: ~30
  - `omnidrive-shell-ext`: ~27
  - `omnidrive-cli`: ~18
  - `omnidrive-tray`: ~3
  - (surowy output: angeld lib = 936 warningów + duplikaty w bin/test builds — łącznie ~1396 bloków `warning:` w logu)
- top kategorii (unikalne po primary span):
  1. ~339 — `missing_errors_doc` (`# Errors` section w doc dla fn zwracającej `Result`)
  2. ~180 — `needless_raw_string_hashes` (`r#"…"#` gdzie `r"…"` wystarczy)
  3. ~106 — `doc_markdown` (brak backticków wokół identyfikatorów w docach)
  4. ~56 — `cast_possible_truncation` (`as` cast może obciąć wartość — w tym ~28 na targetach z N-bit pointerami, `usize`→`u32`/`u64`→`usize` itp.)
  5. ~50 — `borrow_as_ptr` (`&x as *const _` zamiast `std::ptr::addr_of!`)
  6. ~44 — `uninlined_format_args` (`format!("{}", x)` → `format!("{x}")`)
  7. ~35 — `missing_const_for_fn` (fn mogłaby być `const`)
  8. ~32 — `option_if_let_else` / `single_match_else` (`if let … else` → `map_or_else`)
  9. ~26 — `cast_precision_loss` (`i64`/`u64` → `f64` traci precyzję)
  10. ~24 — `cast_possible_wrap` (`as` cast może zawinąć — sign change)
  - dalej: `too_many_lines` (~18), `redundant_closure_for_method_calls` (~17), `must_use_candidate` (~16), `future_not_send` (~13 — `Future` nie jest `Send`), `needless_pass_by_value`, `redundant_clone`, `map_unwrap_or`, `match_same_arms`, `struct_field_names`, `significant_drop_tightening`, `ptr_as_ptr`, `semicolon_if_nothing_returned`, `use_self`, …
- **correctness/suspicious warnings: BRAK.** (Sprawdzone: żaden lint z kategorii `clippy::correctness` ani `clippy::suspicious` nie wystąpił. Correctness lints są deny-by-default → byłyby błędami, nie warningami, a clippy zakończył się exit=0; suspicious są warn-by-default → pojawiłyby się w outpucie. Brak ⇒ brak oczywistych bugów wyłapanych statycznie.)

### clippy strict (CI gate: `cargo clippy --workspace -- -D warnings`)
- exit code: **101** → **CI AKTUALNIE CZERWONE** (P2 — `ci.yml` ma dokładnie ten krok).
- 7 błędów, wszystkie w crate `angeld`, wszystkie trywialne (nowe lint domyślnie-warn z rustc/clippy 1.94, których nie było gdy CI ostatnio był zielony):
  - 4× `clippy::collapsible_if`: `angeld/src/disaster_recovery.rs:914`, `disaster_recovery.rs:969`, `downloader.rs:1072`, `smart_sync.rs:834`
  - 3× `clippy::doc_lazy_continuation` (doc list item without indentation): `angeld/src/smart_sync.rs:180`, `smart_sync.rs:848`, `smart_sync.rs:873`
- Fix scope: ~10-min mechaniczna poprawka; powinno wejść jako prerequisite Task 5 lub osobny commit przed włączeniem reszty CI.

### cargo-udeps (`cargo +nightly udeps --workspace --all-targets`)
- `angeld` — dev-dependencies: **`mockito`** (nieużywane)
- `omnidrive-core` — dependencies: **`rmp-serde`** (nieużywane)
- `omnidrive-tray` — dependencies: **`winapi`** (nieużywane)
- (Uwaga udeps: możliwe false-positive — `cargo-udeps` nie wykrywa użycia w doc-testach; do potwierdzenia w Task 2.)

### grep hot-spots
- **unwrap/expect — top plików (RAW, włącznie z testami):**
  - 164 `angeld/src/db.rs` · 88 `angeld/src/identity.rs` · 26 `angeld/src/migrator.rs` · 25 `omnidrive-core/src/crypto.rs` · 23 `angeld/src/acl.rs` · 11 `omnidrive-tray/src/main.rs` · 9 `angeld/src/vault.rs` · 9 `angeld/src/recovery.rs` · 3 `angeld/src/sharing.rs` · 3 `angeld/src/cloud_guard.rs` · po 1 w `secure_fs.rs`/`peer.rs`/`main.rs`/`ingest.rs`/`downloader.rs`/`device_identity.rs`/`api/mod.rs`
  - **TOTAL raw: 368**
  - **⚠️ WAŻNE: po odfiltrowaniu kodu testowego (`#[cfg(test)]` tail w pliku) zostaje tylko ~24 unwrap/expect w kodzie produkcyjnym.** W `db.rs` wszystkie 164 są PO linii 7881 (`#[cfg(test)]`); w `identity.rs` wszystkie 88 PO linii 228; `migrator.rs`, `acl.rs` — analogicznie. Z tych ~24 produkcyjnych: 11 w `omnidrive-tray/src/main.rs` (UI binarka — panic akceptowalny przy ładowaniu ikony), 3 w `cloud_guard.rs`, 3 w `sharing.rs`, reszta po 1. Pre-known fakt „~315 w angeld/src" liczył kod testowy — rzeczywista powierzchnia ryzyka jest dużo mniejsza. Szczegółowy triage które z tych ~24 są na hot/IO/crypto path → Task 2 Step 2.
- **panic!/todo!/unimplemented!/unreachable!:** (6 wystąpień, ZERO `todo!`/`unimplemented!`)
  - `angeld/src/downloader.rs:948` — `StorageMode::LocalOnly => unreachable!("local-only handled above")`
  - `angeld/src/main.rs:355` — `panic!("[STARTUP] vault_id consistency check failed: {msg}")` (startup guard — zamierzony fail-fast)
  - `angeld/src/packer.rs:350` — `_ => unreachable!()`
  - `angeld/src/packer.rs:516` — `StorageMode::LocalOnly => unreachable!("local-only packs do not create shards")`
  - `omnidrive-tray/src/main.rs:54` — `.unwrap_or_else(|e| panic!("cannot load icon …"))`
  - `omnidrive-tray/src/main.rs:58` — `.unwrap_or_else(|e| panic!("invalid icon data …"))`
- **TODO/FIXME/HACK/XXX:** **0** (brak w `*.rs` w całym workspace)
- **unsafe blocks per crate** (linie zawierające `unsafe `, w `*/src`):
  - `angeld`: **89** · `omnidrive-shell-ext`: **23** · `omnidrive-core`: **0** · `angelctl`: **0** · `omnidrive-cli`: **0** · `omnidrive-tray`: **0**
  - (angeld `unsafe` ≈ Windows API / cfapi / shell integration; omnidrive-shell-ext ≈ COM/Win32. Krypto-core: zero unsafe — dobrze.)
- **pliki > 1000 linii:**
  - 8592 `angeld/src/db.rs` · 2197 `angeld/src/smart_sync.rs` · 1712 `angeld/src/downloader.rs` · 1293 `angeld/src/onboarding.rs` · 1165 `angeld/src/main.rs` · 1157 `angeld/src/vault.rs` · 1153 `angeld/src/api/onboarding.rs` · 1126 `angeld/src/disaster_recovery.rs` · 1084 `angeld/src/uploader.rs` · 1078 `angeld/src/api/vault.rs`
  - (tuż pod progiem: `repair.rs` 945, `api/maintenance.rs` 858; suma `*/src` plików liczonych = ~38432 linii)
- **#[allow(...)] suppressions (poza `dead_code`, w `angeld/src` + `omnidrive-core/src`):** 6 wystąpień:
  - `angeld/src/acl.rs:25` — `#[allow(clippy::should_implement_trait)]`
  - `angeld/src/cache.rs:146` — `#[allow(clippy::too_many_arguments)]`
  - `angeld/src/db.rs:64` — `#[allow(clippy::should_implement_trait)]`
  - `angeld/src/downloader.rs:551` — `#[allow(clippy::too_many_arguments)]`
  - `angeld/src/onboarding.rs:54` — `#[allow(clippy::should_implement_trait)]`
  - `angeld/src/onboarding.rs:80` — `#[allow(clippy::should_implement_trait)]`
  - (wszystkie wyglądają na uzasadnione: `should_implement_trait` przy fn typu `from_str`/`from_*` które nie są implementacją `FromStr`; `too_many_arguments` przy konstruktorach. Werdykt per pozycja → Task 2 Step 6.)

## 2. Mapa długu (per moduł) — wypełniane w Task 2
## 3. Znaleziska (→ KNOWN_ISSUES.md) — wypełniane w Task 2
## 4. Rekomendacje kolejności (input do Faza α/β) — wypełniane w Task 2
