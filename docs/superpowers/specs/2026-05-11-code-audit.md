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

## 2. Mapa długu (per moduł)

> Wypełnione 2026-05-17 (Task 2). Skupiamy się na gigantach >1000 linii (Task 1 §1 wymieniał 10 plików).

### 2.1 `angeld/src/db.rs` (8592 linii) — **monolit, najpilniejszy kandydat do dekompozycji**

- **Symbol count:** ~150+ `pub async fn` (DB CRUD per tabela).
- **Domeny mieszane w jednym pliku** (z grep `^pub async fn`):
  1. **Ingest jobs** (lines 1327–1535): `create_ingest_job`, `transition_ingest_job`, `update_ingest_progress`, `fail_ingest_job`, `reset_interrupted_ingest_jobs`, `list_ingest_jobs`, `requeue_failed_ingest_job`, `delete_ingest_job`, `retry_ingest_job` — ~210 linii.
  2. **Vault state / config** (lines 1544–2202, 6376–6504): `set_vault_params`, `get_vault_params`, `store_encrypted_vault_key`, `get/insert_wrapped_dek`, `graft_restored_metadata_snapshot` (P1-001 hot zone!), `get/set_vault_config`, `rotate_vault_state`, `rotate_vault_key_only`, `enqueue_deks_for_rewrap`, `get_pending_rewrap_batch`, `complete_rewrap_item`, `fail_rewrap_item`, `get_rewrap_status`, `get_deks_by_generation` — ~600 linii crypto/vault.
  3. **System config / cloud usage** (lines 2203–2364): `get/list/set_system_config_value`, `get_cloud_usage_for_day`, `apply_cloud_usage_delta_with_limits` — ~160 linii.
  4. **Provider config + secrets** (lines 2385–2574): provider CRUD + secrets storage — ~190 linii.
  5. **Device identity + peers** (lines 2575–2838): `get/upsert_local_device_identity`, `store_device_keypair`, `upsert_trusted_peer`, `note_peer_seen`, `update_peer_error`, `list/get_trusted_peer_by_id` — ~260 linii.
  6. **Inodes + revisions** (lines 2839–3514): `create/upsert_inode`, `get_inode_by_path/id`, `resolve_path`, `create_file_revision`, `upsert/list_sync_policies`, `record_metadata_backup_attempt`, `set_pin_state/hydration_state`, `get_current_file_revision`, `promote_revision_to_current`, `classify_revision_lineage` — ~675 linii.
  7. **Chunks + packs + scrub** (lines 3516–4818): pack/shard lifecycle, scrubber state — ~1300 linii.
  8. **Cache** (lines 4086–4269): cache CRUD + LRU — ~180 linii.
  9. **Upload queue + retry** (lines 4943–6099): file_chunks, upload jobs, targets, retry storm tracking — ~1150 linii.
  10. **Internal helpers** (lines 6101–6240): `validate_inode_kind`, `ensure_column_exists`, `is_revision_ancestor`, V2 migration helpers — ~140 linii.
  11. **Shared links** (lines 6562–6797): share token CRUD, password tokens, share_passwords — ~235 linii.
  12. **Users + devices + vault_members** (lines 6812–7140+): multi-user identity — ~330 linii (część w view z grep ucięta).
  13. **Sessions + audit + ACL** (~7140–8000): session tokens, audit log, ACL roles (do potwierdzenia w pełnym czytaniu).
  14. **Tests** (line 7881+ — wszystkie 164 unwrapy w pliku są tu).
- **Smell:** jeden plik, 14+ domen, brak `use crate::db::vault;` style modularności. Każda zmiana w jednym obszarze wymaga przewinięcia całego pliku.
- **Sugestia dekompozycji (do Fazy β/γ — NIE w Fazie 0):**
  ```
  angeld/src/db/
    mod.rs          (init_db, pool helpers, common types)
    ingest.rs       (~210 linii)
    vault_state.rs  (~600 linii) — krytyczne: trzymać razem `graft_restored_metadata_snapshot` + `vault_state` (P1-001 fix Faza α.4 = ten plik)
    system_config.rs (~160 linii)
    providers.rs    (~190 linii)
    device.rs       (~260 linii)
    inodes.rs       (~675 linii)
    packs.rs        (~1300 linii — sam kandydat do dalszego splitu na packs/shards/scrub)
    cache.rs        (~180 linii)
    uploads.rs      (~1150 linii)
    shares.rs       (~235 linii)
    users.rs        (~330 linii)
    sessions.rs     (~?)
  ```
- **Risk dekompozycji:** wszystkie `pub async fn` są używane z `crate::db::*` (sprawdzone wcześniej). Refaktor = bardzo mechaniczny ale duży diff (~8.6k linii). **Nie blokuje v0.4 ale powinien iść przed mobile** (UniFFI exposure łatwiej projektować z modułami niż z monolitem).

### 2.2 `angeld/src/smart_sync.rs` (2197 linii) — **clean split candidate**

- **Public surface (16 fn):** register/audit/repair/shutdown/unregister sync_root, install_hydration_runtime, dismount/mount/project_vault, evict_unpinned, sync_placeholder_pin, convert_to_ghost, hydrate_placeholder, register_sync_root_public.
- **Wewnątrz `mod imp` (Windows-only):** 75 fn — cfapi callbacks + helpers + lifecycle + COM apartment guard.
- **Sugestia dekompozycji (do Fazy ε — łatwa, brak ryzyka):**
  ```
  angeld/src/smart_sync/
    mod.rs           (public API surface, re-exports)
    registration.rs  (register/audit/repair/shutdown/unregister)
    callbacks.rs     (fetch_data, fetch_placeholders, cancel_fetch_data + _inner)
    projection.rs    (project_vault_to_sync_root, create_projection_placeholder, ensure_placeholder_directory_chain)
    lifecycle.rs     (dismount_after_lock, mount_after_unlock, dehydrate_directory_recursive)
    placeholder.rs   (convert_to_ghost, hydrate_placeholder, dehydrate_placeholder, pin_state, mark_in_sync)
    util.rs          (normalize paths, wide_str, file_time, COM apartment guard)
  ```
- **Risk:** zero — wszystkie wewnętrzne fn są `fn` (private), tylko `pub use imp::*` w `mod.rs` musiałby ujawnić poszczególne moduły. Czysto mechaniczne.

### 2.3 `angeld/src/vault.rs` (1157 linii) — **clean, nic do dekompozycji**

- 56 funkcji, 2 typy (VaultKeyStore, UnlockedVaultKeys), 1 enum (VaultError) + RotationResult/RevocationRotationResult.
- ~440 linii to testy (`#[cfg(test)]` na końcu). Produkcyjny kod ~720 linii.
- Po cleanupie z 2026-05-17 (commit 11b3f3f — usunięcie `set_key_for_tests` + `UnlockedVaultKeys::new`) struktura jest minimalna i logiczna.
- **Werdykt:** ZOSTAWIĆ.

### 2.4 Pozostałe giganty — werdykt skrótowy

| Plik | Linie | Sugestia |
|---|---|---|
| `downloader.rs` | 1712 | Częściowy split: chunk decryption (V1/V2), prefetcher, peer client wrapping, pack cache. Średni risk. **Faza ε.** |
| `onboarding.rs` | 1293 | Już ma logiczny split na flow (Join Existing vs Bootstrap). Zostawić, ale audytować duplikacje. |
| `main.rs` | 1165 | Bootstrap + arg parsing + worker spawn. Duplikacja `mod` deklaracji z lib (P2-003). Po fix P2-003 będzie mniejszy. |
| `api/onboarding.rs` | 1153 | Spec axum routes for onboarding flow — naturalnie duży. Lekkie kandydaci do extract helpers. **Niska priorytetka.** |
| `disaster_recovery.rs` | 1126 | Single concern (DR snapshot restore). Zostawić, ale rozważyć trait `DownloadProvider` w osobnym pliku. |
| `uploader.rs` | 1084 | Analogicznie do `downloader.rs` — single concern, ale obfite. **Średnia priorytetka.** |
| `api/vault.rs` | 1078 | Vault REST endpoints. Lekko splittable per route group. |

## 3. Znaleziska (→ KNOWN_ISSUES.md)

Wpisane 2026-05-17 (Task 2):

| Wpis | Tier | Krótka treść |
|---|---|---|
| **P1-006** | P1 | `/api/auth/logout` nie wywołuje `vault_keys.lock()` — klucze plaintext zostają w RAM po wylogowaniu. Zero-knowledge gap. |
| **P2-003** | P2 | Bin angeld duplikuje 27 modułów z lib (każdy kompilowany 2×; ten audit wykrył dlatego, że lib-only clippy przepuścił 6 lintów). |
| **P2-004** | P2 | Brak auto-lock po idle. 0 matches dla `auto_lock\|idle_timeout` w workspace. |
| **P2-005** | P2 | Brak `Zeroize` impl. `secrecy::SecretBox` zeruje wewnętrznie, ale każda kopia zwrócona z `master_key()` / `vault_key()` zostaje na stosie bez zerowania. |
| **P3-001** | P3 | AAD pusty (`&[]`) na chunk encrypt — świadoma decyzja (WebCrypto compat dla share-link Trybu B), brak udokumentowania w crypto-spec. Doc-only. |
| **P3-002** | P3 (z 2× eskalacją P2) | 23 prod unwrap/expect: 21 idiomatic/UI/post-invariant OK; **2 eskalowane: `peer.rs:159` (reqwest builder) + `ingest.rs:184` (packer init)** — daemon crash na złej config. |

**Pełne uzasadnienia per wpis w `docs/KNOWN_ISSUES.md`.**

### Pozostałe sygnały do TODO (nie eskalowane do KNOWN_ISSUES — wymaga jeszcze decyzji):

- **cargo-udeps false-pos sprawdzenie:** `mockito` (angeld dev), `rmp-serde` (omnidrive-core), `winapi` (omnidrive-tray) — Task 1 wymienił, ale jeszcze nie potwierdzone na 100% w doc-testach. **Action:** uruchomić `cargo +nightly udeps --workspace --all-features` (Task 1 robił bez `--all-features`) → potem decyzja usunąć vs zachować.
- **cast_* warningi przegląd:** 56 `cast_possible_truncation` + 24 `cast_possible_wrap` + 26 `cast_precision_loss`. Audit report Task 1 wymienił że "do sprawdzenia w Task 2: czy któryś w sizing/offsetach crypto/packer → może >P3". **Nie zrobione w tej iteracji** — wymaga targeted grep + read każdego site. **Action:** osobny task w Fazie α/β (read each, decide unsafe-cast guard albo `as` → `try_from?`).
- **Future not Send (13×):** może blokować spawn w niektórych miejscach. Niska prio.
- **`#[allow(...)]` (6 produkcyjnych poza dead_code):** Task 1 wymienił, wszystkie uzasadnione (should_implement_trait dla fn `from_*` nie implementujących `FromStr`, too_many_arguments dla konstruktorów). **Werdykt: zostawić, ale dodać `// reason: ...` komentarz** dla każdego.

## 4. Rekomendacje kolejności (input do Faza α/β/γ)

> **Premise:** Faza 0 audit ujawnił 3 security gaps (P1-006, P2-004, P2-005) których nie planowaliśmy w roadmapie v0.4 Faza α. Te wymagają **wpisania do α** przed Argon2id bump / ML-KEM / X25519 / α.4 graft fix.

### 4.1 Faza α (Crypto Hardening) — propozycja kolejności po audycie

| Krok | Zadanie | Powód kolejności |
|---|---|---|
| **α.0a** | **P1-006 fix: `logout` musi wywołać `vault_keys.lock()`** | Najprostszy fix, największy security ROI. Hot-fix-able do v0.3.24 jeśli zdecydujesz. |
| **α.0b** | **P2-004 fix: auto-lock po idle (config + timer + Windows session-lock hook)** | UI/UX feature + security defense. Wymaga config + Windows event API. Średni effort, wysoki user-visible value. |
| **α.0c** | **P2-005 fix: Zeroize newtype dla `KeyBytes`** | Defense-in-depth. Average effort (refactor type alias + audit ~10 call-sites). Niski regression risk. |
| α.1 | Argon2id parameter bump (3×64MiB → spec-current target) | Już planowane przed audytem. |
| α.2 | ML-KEM (post-quantum readiness) | Planowane. |
| α.3 | X25519 device key exchange | Planowane. |
| **α.4** | **P1-001+P1-005 fix: graft pełen identity bundle** (vault_state crypto + DEK + recovery_keys) | Główny v0.4 multi-device blocker. Lokalizacja: `db.rs::graft_restored_metadata_snapshot` (line 1677-2150) — sekcja "vault state" w mapie §2.1. |
| α.5 | crypto-spec.md update (P3-001 AAD section + α.0a-c security notes) | Doc. |

### 4.2 Faza β (Bug Fixes P1)

- β.0: **P3-002 P2 escalations (2):** refactor `peer.rs::Peer::new` + `ingest.rs::IngestWorker::new` → `Result<Self, E>`. Mały effort.
- β.1: P1-002 snapshot fetch worker (multi-device awareness, plan istnieje)
- β.2: P1-003 Scaleway 403 (IAM policy audit)
- β.3: P1-004 R2 ConnectionReset (rustls/hyper consolidation lub force-close on reset)

### 4.3 Faza γ/ε (architektura)

- γ.0: **P2-003 fix:** decyzja Opcja A/B/C dla bin/lib duplikacji.
- γ.1: db.rs dekompozycja (§2.1) — przed mobile (UniFFI lepiej z modułami).
- ε.0: smart_sync.rs dekompozycja (§2.2) — mechaniczna.

### 4.4 Wpisy doc-only (nie blokujące)

- crypto-spec.md §12 — AAD semantics (P3-001).
- crypto-spec.md §13 — auto-lock policy + zeroize semantics (α.0a-c).
- KNOWN_ISSUES.md → cleanup po α.0a fix (P1-006 → Closed).

