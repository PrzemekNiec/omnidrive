# Faza 0 — QA Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the quality-measurement infrastructure (code-debt audit, manual smoke checklist, performance baseline, complete CI gate) that must exist before any v0.4 feature work — so subsequent phases (α Crypto, β Bug Fixes, γ Zero Data Loss) can be planned and verified against concrete data instead of guesses.

**Architecture:** Mostly investigative + documentation + config work, not feature code — so this plan deviates from the usual write-test→implement→pass cycle. Task 1–2 = code audit (read-only analysis → findings recorded in `docs/KNOWN_ISSUES.md` + a one-shot summary report). Task 3 = a manual smoke checklist doc. Task 4 = a PowerShell perf-measurement script + a baseline doc filled in by running it on the Lenovo dev box. Task 5 = finish the *already-existing* `.github/workflows/ci.yml` (add `cargo fmt --check`, decide `Cargo.lock` policy) + add a local `pre-push` git hook. Task 6 = bookkeeping (STATUS.md §12.4 + KNOWN_ISSUES.md header) + close-out commit.

**Tech Stack:** Rust Edition 2024 workspace (`angeld`, `angelctl`, `omnidrive-cli`, `omnidrive-core`, `omnidrive-tray`, `omnidrive-shell-ext`), `cargo` + `clippy` + `rustfmt` + nightly `cargo-udeps`, GitHub Actions (`windows-latest`), PowerShell 5.1 for perf harness, Markdown for docs. jcodemunch MCP for symbol-level navigation of the giant files.

**Decyzje Przemka (2026-05-11):** jeden plan na całą Fazę 0; CI = GH Actions **i** lokalny pre-push hook; audyt → wpisy w `KNOWN_ISSUES.md` **plus** zbiorczy raport `docs/superpowers/specs/2026-05-11-code-audit.md`.

**Pre-known facts (zebrane 2026-05-11, do potwierdzenia w trakcie):**
- Giganty (`wc -l`): `angeld/src/db.rs` 8592, `smart_sync.rs` 2197, `downloader.rs` 1712, `onboarding.rs` 1293, `main.rs` 1165, `vault.rs` 1157, `api/onboarding.rs` 1153, `disaster_recovery.rs` 1126, `uploader.rs` 1084, `api/vault.rs` 1078, `repair.rs` 945, `api/maintenance.rs` 858, `packer.rs` 774, `api/diagnostics.rs` 731, `identity.rs` 681, `api/files.rs` 664, `watcher.rs` 643, `omnidrive-core/src/crypto.rs` 588, `ingest.rs` 578.
- `.unwrap()` rough count: ~315 w `angeld/src/`, ~25 w `omnidrive-core/src/`.
- `omnidrive-core/src/crypto.rs` ma parametr `aad: &[u8]` w API szyfrowania — `b""` w pliku występuje tylko w teście (linia ~478); pytanie audytowe = czy *call sites* w `angeld` przekazują sensowny AAD czy puste.
- `zeroize` crate NIE jest w żadnym `Cargo.toml` (jest `secrecy = "0.10"`).
- `.github/workflows/ci.yml` JUŻ ISTNIEJE: `runs-on: windows-latest`, kroki `cargo check --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace -- --test-threads=1`. **Brak** kroku `cargo fmt --check` i `--all-features`.
- `Cargo.lock` jest w `.gitignore` (pkt do decyzji — workspace z binarkami zwykle commituje lock).
- Brak `rustfmt.toml` / `clippy.toml` / `rust-toolchain.toml`.
- Testy e2e: `angeld/tests/e2e_*.rs` (`e2e_basic`, `e2e_ingest`, `e2e_reconciliation`, `e2e_recovery`, `e2e_scrubber_repair`, `e2e_shell_recovery`, `e2e_sync`). `e2e_recovery` wymaga `--features test-helpers` (security gate — patrz memory `feedback_e2e_recovery_test.md`). Niektóre e2e zostawiają zwisające subst mounty (memory `feedback_e2e_subst_cleanup.md`).
- Git hooks: tylko pliki `.sample`, brak aktywnych.
- Perf-relevant code (cele SLA z STATUS.md §12.2): `watcher.rs` (P2-001 watcher CPU), `smart_sync.rs` (P2-002 VFS lag), daemon cold start (`main.rs`), daemon RAM idle.

---

## Files Changed

| File | Change |
|------|--------|
| `docs/superpowers/specs/2026-05-11-code-audit.md` | **Create** — zbiorczy raport audytu: mapa długu, lista znalezisk z priorytetami, surowe metryki (clippy/fmt/udeps/unsafe counts). |
| `docs/KNOWN_ISSUES.md` | **Modify** — dodać wpisy P3 (lub wyżej jeśli security/dataloss) z audytu w sekcji `## P3`; zaktualizować nagłówek (`Ostatnia aktualizacja`). |
| `docs/SMOKE_CHECKLIST.md` | **Create** — 30–50-punktowa lista manualnych sprawdzeń do przejścia po każdym buildzie przed Dell smoke. |
| `scripts/perf-baseline.ps1` | **Create** — PowerShell harness mierzący metryki SLA na żywym daemonie (cold start, RAM idle, watcher CPU idle/load, VFS cold/warm fetch). |
| `docs/perf-baseline-2026-05-11.md` | **Create** — tabela zmierzonych wartości (Lenovo) vs cele SLA z STATUS.md §12.2 + komentarz „ile brakuje". |
| `.github/workflows/ci.yml` | **Modify** — dodać krok `cargo fmt --check`; rozważyć `--all-features` w `cargo test`; ewentualnie `actions/checkout` pin. |
| `.gitignore` | **Modify** (warunkowo) — usunąć `Cargo.lock` jeśli decyzja = commitować lock. |
| `Cargo.lock` | **Add to git** (warunkowo) — jeśli decyzja jw. |
| `rustfmt.toml` | **Create** (warunkowo) — jeśli audyt pokaże, że trzeba zafiksować styl przed włączeniem `fmt --check` w CI. |
| `.githooks/pre-push` | **Create** — skrypt uruchamiający `cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace -- --test-threads=1` przed pushem. |
| `scripts/install-git-hooks.ps1` | **Create** — ustawia `git config core.hooksPath .githooks` (żeby hook był wersjonowany w repo). |
| `STATUS.md` | **Modify** — w §12.4 odhaczyć 0.1–0.5 jako DONE; zaktualizować nagłówek „Ostatnia aktualizacja". |

---

## Task 1: Audit tooling baseline — zebranie surowych metryk

> Cel: jednym ciągiem uruchomić wszystkie narzędzia statyczne i zebrać surowy output. To jest start kroku 0.1 ORAZ dostarcza danych potrzebnych Task 5 (CI fmt step) i Task 2 (triage). **Nic nie naprawiamy w tym tasku — tylko mierzymy.**

**Files:**
- Create: `docs/superpowers/specs/2026-05-11-code-audit.md` (szkielet + sekcja „Raw metrics")

- [ ] **Step 1: Upewnij się, że toolchain ma potrzebne komponenty**

Run:
```bash
rustup component add clippy rustfmt
rustup toolchain install nightly --component miri 2>/dev/null; rustup toolchain install nightly
cargo install cargo-udeps --locked || echo "udeps install failed — odnotuj w raporcie, pomiń Step 5"
```
Expected: clippy + rustfmt obecne; nightly zainstalowany; `cargo-udeps` zainstalowany lub jawnie odnotowany jako pominięty.

- [ ] **Step 2: `cargo fmt --check` — czy kod jest fmt-clean?**

Run: `cargo fmt --all -- --check > .audit_fmt.txt 2>&1; echo "exit=$?"`
Zapisz: liczbę plików z diffami i `exit` code do raportu (sekcja „Raw metrics → rustfmt"). NIE uruchamiaj `cargo fmt` (bez `--check`) — formatowanie to osobna decyzja (Task 5 Step 3).
Expected: albo `exit=0` (czysto — wtedy CI fmt step jest bezpieczny), albo lista plików (wtedy Task 5 musi zdecydować: zafiksować styl jednym commitem czy dodać `rustfmt.toml` łagodzący różnice).

- [ ] **Step 3: `cargo clippy` — pełny tryb, wszystkie targety**

Run: `cargo clippy --workspace --all-targets --all-features -- -W clippy::pedantic -W clippy::nursery 2>&1 | tee .audit_clippy.txt | tail -5`
Zapisz do raportu: liczbę `warning:` linii ogółem, top 10 najczęstszych lintów (`grep -oP 'clippy::[a-z_]+' .audit_clippy.txt | sort | uniq -c | sort -rn | head`), oraz wszystkie warningi o kategorii `correctness` / `suspicious` (te są ważne — potencjalne bugi, nie styl).
Expected: lista lintów; każdy `correctness`/`suspicious` warning → kandydat na wpis w KNOWN_ISSUES (Task 2).

- [ ] **Step 4: `cargo clippy` — strict (to co ma być w CI)**

Run: `cargo clippy --workspace -- -D warnings 2>&1 | tail -10; echo "exit=$?"`
Zapisz `exit` do raportu. To jest dokładnie krok który JEST już w `ci.yml` — jeśli `exit≠0`, CI jest aktualnie czerwone i to jest P2 (blokuje sens CI gate).
Expected: `exit=0` (CI green na clippy) — jeśli nie, odnotuj wszystkie błędy do naprawienia (mogą wpaść do Task 5 jako prerequisite lub osobny commit).

- [ ] **Step 5: `cargo +nightly udeps` — nieużywane zależności**

Run: `cargo +nightly udeps --workspace --all-targets 2>&1 | tee .audit_udeps.txt | tail -20` (pomiń jeśli Step 1 nie zainstalował udeps)
Zapisz do raportu listę unused deps per crate.
Expected: lista (może być pusta); każda unused dep → wpis P3 (cleanup).

- [ ] **Step 6: Surowe greby — debt hot-spots**

Run:
```bash
echo "=== unwrap/expect per file (top 25) ===" && grep -rn '\.unwrap()\|\.expect(' angeld/src omnidrive-core/src angelctl/src omnidrive-cli/src omnidrive-tray/src --include='*.rs' | awk -F: '{print $1}' | sort | uniq -c | sort -rn | head -25
echo "=== panic!/todo!/unimplemented!/unreachable! ===" && grep -rn 'panic!\|todo!\|unimplemented!\|unreachable!' --include='*.rs' angeld/src omnidrive-core/src angelctl/src omnidrive-cli/src omnidrive-tray/src omnidrive-shell-ext/src
echo "=== TODO/FIXME/HACK/XXX ===" && grep -rn 'TODO\|FIXME\|HACK\|XXX' --include='*.rs' angeld/src omnidrive-core/src angelctl/src omnidrive-cli/src omnidrive-tray/src omnidrive-shell-ext/src
echo "=== unsafe blocks count per crate ===" && for c in angeld omnidrive-core angelctl omnidrive-cli omnidrive-tray omnidrive-shell-ext; do echo -n "$c: "; grep -rn 'unsafe ' $c/src --include='*.rs' | wc -l; done
echo "=== files > 1000 lines ===" && find angeld/src omnidrive-core/src angelctl/src omnidrive-cli/src omnidrive-tray/src omnidrive-shell-ext/src -name '*.rs' -exec wc -l {} + | sort -rn | awk '$1>1000'
echo "=== #[allow(...)] suppressions ===" && grep -rn '#\[allow(' --include='*.rs' angeld/src omnidrive-core/src | grep -v 'dead_code' | head -30
```
Zapisz cały output do raportu, sekcja „Raw metrics".
Expected: konkretne liczby i listy plików — to surowy materiał do triage w Task 2.

- [ ] **Step 7: Utwórz szkielet raportu i zapisz surowe metryki**

Create `docs/superpowers/specs/2026-05-11-code-audit.md`:
```markdown
# OmniDrive — Code Audit (Faza 0, krok 0.1)

> Data: 2026-05-11 · Wersja: v0.3.23 (commit <pełny SHA z `git rev-parse HEAD`>)
> Zakres: `angeld/src/`, `omnidrive-core/src/`, oraz przegląd reszty crateów workspace.
> Wynik: lista znalezisk → wpisy w `docs/KNOWN_ISSUES.md` (P3 lub wyżej); ten plik = mapa długu + surowe metryki.

## 1. Raw metrics

### rustfmt (`cargo fmt --all -- --check`)
- exit code: <…>
- plików z diffami: <…>

### clippy pedantic+nursery (`cargo clippy --workspace --all-targets --all-features -- -W clippy::pedantic -W clippy::nursery`)
- warningów ogółem: <…>
- top 10 lintów: <…>
- correctness/suspicious warnings: <lista lub „brak">

### clippy strict (CI gate: `cargo clippy --workspace -- -D warnings`)
- exit code: <…>  (jeśli ≠0 → CI aktualnie czerwone)

### cargo-udeps
- <lista unused deps per crate lub „nie uruchomiono — powód">

### grep hot-spots
- unwrap/expect — top plików: <…>
- panic!/todo!/unimplemented!/unreachable!: <…>
- TODO/FIXME/HACK/XXX: <…>
- unsafe blocks per crate: <…>
- pliki > 1000 linii: <…>
- #[allow(...)] suppressions: <…>

## 2. Mapa długu (per moduł) — wypełniane w Task 2
## 3. Znaleziska (→ KNOWN_ISSUES.md) — wypełniane w Task 2
## 4. Rekomendacje kolejności (input do Faza α/β) — wypełniane w Task 2
```

- [ ] **Step 8: Commit**

```bash
rm -f .audit_fmt.txt .audit_clippy.txt .audit_udeps.txt
git add docs/superpowers/specs/2026-05-11-code-audit.md
git commit -m "docs(audit): Faza 0.1 — raw metrics baseline (clippy/fmt/udeps/grep)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 2: Audit triage — mapa długu + wpisy w KNOWN_ISSUES.md + raport

> Cel: zamienić surowe metryki z Task 1 na konkretne, priorytetyzowane znaleziska. To dokończenie kroku 0.1. **Nadal nic nie naprawiamy** — tylko katalogujemy. Przemek zatwierdza priorytety.

**Files:**
- Modify: `docs/superpowers/specs/2026-05-11-code-audit.md` (sekcje 2–4)
- Modify: `docs/KNOWN_ISSUES.md` (sekcja `## P3`, nagłówek)

- [ ] **Step 1: Mapowanie gigantów przez jcodemunch**

Dla każdego pliku > 1000 linii (lista z Task 1 Step 6): `mcp__jcodemunch__get_file_outline(path=…)`. Dla `db.rs` (8592 linii) dodatkowo `mcp__jcodemunch__get_symbols` żeby zobaczyć ile odrębnych obszarów odpowiedzialności tam jest.
W raporcie sekcja 2 — tabela: `plik | linie | główne obszary odpowiedzialności | czy kandydat do split | sugerowany podział`.
Expected: tabela. `db.rs` niemal na pewno = kandydat (sugestia: rozbić per domena — migrations, vault_state, devices/users/members, inodes, packs, object_locations, upload_jobs, audit, …). `smart_sync.rs` już zaznaczony do dekompozycji w P2-002.

- [ ] **Step 2: Triage `unwrap()`/`expect()` — które są na hot/IO/crypto paths?**

Nie wszystkie 315 unwrapów to bug. Przejrzyj top pliki z Task 1 Step 6. Klasyfikacja:
- **P1/P2 kandydat:** `unwrap()` na wyniku I/O (plik, sieć, DB), na deserializacji danych z chmury, w worker loopach (uploader/downloader/scrubber/repair/watcher/ingest) — panic = padnięcie workera = potencjalny dataloss/zawieszenie sync.
- **P3:** `unwrap()` na rzeczach logicznie niemożliwych do failowania (np. `Regex::new` na literałach, `Mutex::lock` gdzie nie ma poisoningu w grę), w kodzie startup/config gdzie panic jest akceptowalny.
- **Ignoruj:** `unwrap()` w `#[cfg(test)]` i w `tests/`.
W raporcie sekcja 3: lista konkretnych `plik:linia` dla P1/P2 kandydatów (nie wszystkich — reprezentatywne + count). Zbiorczo: jeden wpis P3 „~N unwrap() do przeglądu/zamiany na `?`+kontekst, szczegóły w audit report §3".
Expected: konkretne `file:line` dla najgroźniejszych, plus agregat.

- [ ] **Step 3: Crypto AAD audit — czy call sites przekazują sensowny AAD?**

`mcp__jcodemunch__search_text(query="encrypt_secret")` i `decrypt_secret`, `wrap_dek`/`unwrap_dek`, `encrypt_chunk_v2` — znajdź wszystkie wywołania w `angeld/`. Dla każdego: jaki AAD jest przekazywany? Porównaj z `docs/crypto-spec.md` — spec definiuje jakie AAD powinny być (np. `user_id`, `vault_id`, `inode_id`, `chunk_index`). Każdy call site z `b""` lub niezgodny z spec = wpis. **Priorytet: P1 jeśli spec wymaga AAD a kod daje pusty** (osłabia authenticated encryption, ryzyko substitution attack między ciphertextami) — Przemek/crypto review (Faza α.5) potwierdza ostateczny priorytet.
W raporcie sekcja 3: tabela `call site (file:line) | funkcja | AAD przekazany | AAD wg crypto-spec | zgodność | priorytet`.
Expected: tabela. Jeśli wszystkie zgodne — świetnie, odnotuj „AAD OK" i zamknij wątek. Jeśli nie — wpisy w KNOWN_ISSUES + flaga do Faza α.5.

- [ ] **Step 4: Key zeroization audit**

`grep -rn 'KeyBytes\|SecretString\|secrecy::\|Zeroizing' --include='*.rs' angeld/src omnidrive-core/src` — gdzie żyją klucze w pamięci (KEK, Vault Key, DEK, OAuth tokeny)? `secrecy::SecretString` zeruje swój bufor na drop. Ale: gołe `[u8; 32]` / `Vec<u8>` trzymające klucze? Klucze kopiowane do `String`/`Vec` przy (de)serializacji? `previous_envelope_key` w DB?
W raporcie sekcja 3: lista miejsc gdzie klucz materializuje się jako nie-zeroizowany typ. **Priorytet P2** (defense-in-depth — nie dataloss, ale memory-disclosure surface). Rekomendacja: dodać `zeroize` crate + `ZeroizeOnDrop` lub `Zeroizing<Vec<u8>>` wrappery. Decyzja o implementacji = Faza α (crypto hardening).
Expected: lista + rekomendacja crate `zeroize`.

- [ ] **Step 5: Auto-lock po idle — czy istnieje?**

`mcp__jcodemunch__search_text(query="auto_lock")` / `idle` / `lock_timeout` w `angeld/`. Sprawdź `vault.rs`, `api/vault.rs`, `api/settings.rs` — czy jest jakikolwiek mechanizm który po N minutach bezczynności robi `vault lock` (czyści Vault Key z pamięci)? Threat model w STATUS.md §12.1 / `docs/crypto-spec.md` — czy auto-lock jest tam wymagany?
W raporcie sekcja 3: stan faktyczny (jest/nie ma) + wpis. **Priorytet P2/P3** (UX-security — laptop zostawiony odblokowany; nie dataloss). Jeśli brak → wpis „brak auto-lock po idle, rekomendacja: konfigurowalny timeout, default np. 30 min, po nim `vault lock`". Implementacja = Faza δ lub osobny mini-feature.
Expected: jednoznaczna odpowiedź + ew. wpis.

- [ ] **Step 6: panic!/todo!/unimplemented!/unreachable! + #[allow] — przegląd po jednym**

Lista z Task 1 Step 6. Dla każdego `todo!()`/`unimplemented!()`: czy to dead code path czy realnie osiągalny? Osiągalny = P1 (crash). Dla każdego `#[allow(...)]` (poza `dead_code`): czy uzasadniony? Nieuzasadnione tłumienie clippy = P3.
W raporcie sekcja 3.
Expected: lista z werdyktem per pozycja.

- [ ] **Step 7: Wpisz znaleziska do KNOWN_ISSUES.md**

W `docs/KNOWN_ISSUES.md` sekcja `## P3 — Drobne UX / kosmetyka` (i wyżej jeśli triage dał P1/P2 — wtedy do odpowiedniej sekcji) — dodaj wpisy w formacie zgodnym z istniejącymi (Wykryto / Symptom / Hipoteza / Impact / Fix scope / Status). Każdy wpis odsyła do `docs/superpowers/specs/2026-05-11-code-audit.md §3` po szczegóły. Zaktualizuj nagłówek pliku: `Ostatnia aktualizacja: 2026-05-11`.
Przykładowe wpisy (dostosuj do faktycznych znalezisk):
```markdown
### P3-001 — db.rs monolit (8592 linie)
- **Wykryto:** Faza 0.1 code audit (2026-05-11)
- **Symptom:** `angeld/src/db.rs` = 8592 linie, ~N odrębnych obszarów odpowiedzialności w jednym pliku
- **Impact:** trudność utrzymania, ryzyko regresji przy edycji (cały plik nie mieści się w kontekście), wolny rebuild incremental
- **Fix scope:** rozbić na moduł `db/` per domena (migrations, vault_state, identity, inodes, packs, object_locations, upload_jobs, audit) — patrz audit report §2 po sugerowany podział. Bez zmian zachowania, tylko move + `pub use`.
- **Status:** OPEN. Kandydat do Faza β.5 (razem z dekompozycją smart_sync.rs) albo osobnego refactor-batcha.
```
(plus: P3 unwrap aggregate, P3 udeps cleanup, ew. P1/P2 dla AAD/zeroize/auto-lock/todo!() wedle triage)

- [ ] **Step 8: Dokończ raport — sekcja 4 (rekomendacje kolejności)**

W `docs/superpowers/specs/2026-05-11-code-audit.md §4`: krótka lista „co z tego idzie do której fazy v0.4" — np. AAD → α.5 (crypto review), zeroize → α, db.rs/smart_sync split → β.5, unwrap hot-path → β, auto-lock → δ. To input dla planów kolejnych faz.

- [ ] **Step 9: Checkpoint z Przemkiem**

Pokaż Przemkowi listę nowych wpisów w KNOWN_ISSUES.md z proponowanymi priorytetami. Przemek zatwierdza („OK P3" / „to jest P1" / „skip"). Skoryguj priorytety wg jego decyzji. **Nie commituj przed jego OK** (workflow z nagłówka KNOWN_ISSUES.md).

- [ ] **Step 10: Commit**

```bash
git add docs/KNOWN_ISSUES.md docs/superpowers/specs/2026-05-11-code-audit.md
git commit -m "docs(audit): Faza 0.1 — triage + KNOWN_ISSUES P3 entries + audit report

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 3: docs/SMOKE_CHECKLIST.md — manualna checklista po buildzie

> Cel: krok 0.2. Lista 30–50 sprawdzeń którą przechodzimy po KAŻDYM buildzie zanim damy instalator na Della — żeby smoke test nie był „z głowy" (jak v0.3.23, gdzie safety-numbers mismatch wyszedł dopiero przy ręcznym porównaniu). Źródła do skompilowania listy: STATUS.md §12.4 QG1, sekcja „asercje" w STATUS.md (te „12 asertów" wspominane w starych notatkach), KNOWN_ISSUES.md (każdy zamknięty bug → regression check), CLAUDE.md (zasady integralności danych).

**Files:**
- Create: `docs/SMOKE_CHECKLIST.md`

- [ ] **Step 1: Zbierz źródła**

Przeczytaj: STATUS.md sekcje o smoke testach / QG1 / „asercje", KNOWN_ISSUES.md sekcja `## Closed` (każdy fix = kandydat na regression check), `feedback_*` w pamięci (`feedback_e2e_subst_cleanup.md`, `feedback_cfapi_directories.md` — known gotchas do sprawdzenia). Wypisz surową listę kandydatów na punkty.

- [ ] **Step 2: Napisz `docs/SMOKE_CHECKLIST.md`**

Struktura (wypełnij konkretami — to NIE jest placeholder, każdy punkt ma być wykonywalny przez człowieka klikającego w UI / curla):
```markdown
# OmniDrive — Smoke Checklist

> Przejść po KAŻDYM buildzie przed wydaniem instalatora na maszynę testową (Dell).
> Format: `[ ]` = nie sprawdzone, `[x]` = OK, `[!]` = FAIL (→ wpis w KNOWN_ISSUES.md, build NIE idzie dalej).
> Wersja buildu: ____  ·  Data: ____  ·  Maszyna: ____

## A. Build & instalacja
- [ ] `cargo build --release --workspace` przeszło bez błędów
- [ ] Świeże binarki skopiowane do `dist/installer/payload/` (angeld, angelctl, omnidrive, omnidrive-tray, omnidrive_shell_ext.dll) — sprawdź timestampy
- [ ] `payload/static/` zsynchronizowane z `angeld/static/`
- [ ] Wersja podbita we wszystkich `Cargo.toml` + `installer/omnidrive.iss` (memory: feedback_version_bump)
- [ ] Instalator `OmniDrive-Setup-X.Y.Z.exe` wygenerowany
- [ ] Instalator uruchamia się, instaluje bez błędu UAC/Defender
- [ ] Po instalacji daemon wstaje (sprawdź `curl http://127.0.0.1:8787/api/diagnostics` → pełny JSON)

## B. Onboarding — nowy vault
- [ ] Wizard otwiera się, wszystkie kroki przeklikalne
- [ ] Utworzenie nowego vaulta: hasło + providery (B2/R2/Scaleway) → COMPLETED
- [ ] Dysk O:\ pojawia się w Eksploratorze
- [ ] `vault lock` → O:\ znika / staje się niedostępny; `vault unlock` hasłem → wraca

## C. Onboarding — Join Existing Vault (multi-device)
- [ ] Na drugiej maszynie: Join Existing → wpisanie hasła → graft przechodzi (status COMPLETED)
- [ ] **`GET /api/vault/safety-numbers` na OBU maszynach — `key_generation`, `mnemonic`, `safety` IDENTYCZNE** (← to wyłapuje P1-001/P1-005; jeśli różne — build FAIL)
- [ ] MultiDevice tab pokazuje OBA urządzenia na obu maszynach
- [ ] Plik wgrany na maszynie A → otwierany z O:\ na maszynie B → checksum match (← P1-001 hydration)
- [ ] `members_count` w `/api/vault/status` zgodny na obu

## D. Upload / Download / Sync
- [ ] Wrzucenie pliku do SYNC_PATH → pojawia się w O:\, status „synced"
- [ ] Plik < 1 MB i plik > 100 MB — oba uploadują się, downloadują, checksum match
- [ ] Watcher: modyfikacja pliku → re-upload; brak modyfikacji → BRAK re-uploadu (sprawdź logi daemona przez ~2 min — nie ma pakowania w pętli)
- [ ] Usunięcie pliku → propaguje się
- [ ] `metadata-backup status` — ile providerów zielonych (cel ≥2/3; znane: P1-003 Scaleway 403, P1-004 R2 ConnReset — odnotuj jeśli nadal)

## E. UI / Dashboard
- [ ] Wszystkie zakładki sidebara otwierają poprawny widok (Przegląd, Pliki, Skarbiec, MultiDevice, Audyt, Chmura, Diagnostyka, Ustawienia)
- [ ] Zakładka Diagnostyka: 3 karty live pokazują dane (nie „ERROR"); przyciski akcji odpalają z confirm-dialogiem
- [ ] „Wyloguj" działa (po join-existing też)
- [ ] Gramatyka PL: brak skrótów „mies." / „MB/s" / „sek." (CLAUDE.md)
- [ ] Statusy mają wagi OK/WARN/FAILED

## F. Recovery / Maintenance
- [ ] `POST /api/maintenance/gc-orphans` — odpala, nie crashuje
- [ ] `POST /api/maintenance/retry-storms` — zwraca dane, kafelek alertu działa
- [ ] „Wymuś rotację klucza" → nowy `latest.db.enc` uploadowany; po rotacji unlock starym hasłem nadal działa
- [ ] Zmiana hasła → unlock nowym hasłem działa, starym nie

## G. Stabilność
- [ ] Power-cycle: reboot maszyny → daemon autostartuje → O:\ wraca po unlock
- [ ] `cargo test --workspace -- --test-threads=1` — pass (na maszynie buildującej)
- [ ] Brak zwisających subst mountów po testach e2e (memory: feedback_e2e_subst_cleanup — sprawdź `subst` bez argumentów)
- [ ] Daemon RAM idle w taskmgr — odnotuj wartość (cel < 200 MB)

## H. Zero-Knowledge sanity
- [ ] `grep` w logach daemona: ZERO plaintextowych haseł / kluczy / tokenów (powinno być `[REDACTED]`)
- [ ] Wszystkie operacje plikowe poza SYNC_PATH? — sprawdź logi `tracing::info!` z pełnymi ścieżkami (CLAUDE.md zasada izolacji)
```
(docelowo 30–50 punktów; dostosuj liczby po przejrzeniu STATUS.md — może być więcej w sekcji C/D)

- [ ] **Step 3: Commit**

```bash
git add docs/SMOKE_CHECKLIST.md
git commit -m "docs: Faza 0.2 — SMOKE_CHECKLIST.md (manual post-build checklist)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 4: Perf baseline — harness + pomiar na Lenovo

> Cel: krok 0.3. Bez liczb nie wiemy jak daleko jesteśmy od SLA §12.2 — i nie da się zaplanować β.4 (watcher CPU) ani ε (VFS lag). Wynik: skrypt `scripts/perf-baseline.ps1` (powtarzalny) + `docs/perf-baseline-2026-05-11.md` z tabelą. **Uwaga bezpieczeństwa (CLAUDE.md):** harness operuje WYŁĄCZNIE na dedykowanym folderze testowym (SYNC_PATH / `.tmp_perf/`), nigdy na realnych plikach użytkownika; watcher zostaje w trybie kontrolowanym.

**Files:**
- Create: `scripts/perf-baseline.ps1`
- Create: `docs/perf-baseline-2026-05-11.md`

- [ ] **Step 1: Napisz `scripts/perf-baseline.ps1`**

Skrypt (PowerShell 5.1; mierzy i wypisuje tabelę markdown). Zakres pomiarów = cele z STATUS.md §12.2:
```powershell
# scripts/perf-baseline.ps1 — OmniDrive performance baseline harness
# UWAGA: operuje tylko na $PerfDir; NIE dotyka realnych plików użytkownika.
param(
    [string]$ApiBase = "http://127.0.0.1:8787",
    [string]$PerfDir = "$PSScriptRoot\..\.tmp_perf",
    [string]$VaultDrive = "O:"
)
$ErrorActionPreference = "Stop"
function Time-Ms([scriptblock]$b){ $sw=[Diagnostics.Stopwatch]::StartNew(); & $b | Out-Null; $sw.Stop(); [math]::Round($sw.Elapsed.TotalMilliseconds) }

# 1. Daemon cold start: stop angeld, start, poll /api/diagnostics until 200
#    (zakłada że daemon jest uruchamiany przez `angelctl` lub `target\release\angeld.exe`)
# 2. Daemon RAM idle: po 60s idle → (Get-Process angeld).WorkingSet64 / 1MB
# 3. Watcher CPU idle: 60s sampling (Get-Counter '\Process(angeld)\% Processor Time') → avg
# 4. Watcher CPU load: wygeneruj 100 zmian/min w $PerfDir przez 60s, sampluj CPU → avg
# 5. VFS cold fetch: wgraj plik 10MB i 100MB (do SYNC_PATH), poczekaj aż "synced",
#    `vault lock` + `unlock` (czyści cache), zmierz czas Get-Content pierwszych bajtów z $VaultDrive
# 6. VFS warm open: drugi odczyt tego samego pliku → czas
# Output: tabela markdown: Metryka | Zmierzone | Cel SLA §12.2 | Δ | Status(OK/WARN/FAIL)
```
(pełna implementacja — engineer pisze konkretny PowerShell wg powyższego planu; każdy pomiar w osobnej funkcji, wynik do `$results` hashtable, na końcu `ConvertTo-Markdown`-style tabela do stdout. Jeśli któryś pomiar wymaga ręcznej akcji — wypisz instrukcję i `Read-Host` NIE używaj; raczej parametr `-SkipManual`.)

- [ ] **Step 2: Uruchom na Lenovo i zapisz wyniki**

Run (na Lenovo, z żywym daemonem v0.3.23): `pwsh scripts/perf-baseline.ps1 | Tee-Object docs/perf-baseline-2026-05-11.md`
Następnie ręcznie dopisz do `docs/perf-baseline-2026-05-11.md` nagłówek + komentarz:
```markdown
# OmniDrive — Performance Baseline

> Data: 2026-05-11 · Maszyna: Lenovo ThinkPad (dev box) · Wersja: v0.3.23 · Dane: <ile plików/GB w vault>
> Cele SLA: STATUS.md §12.2. Status: OK = spełnione, WARN = blisko, FAIL = łamie SLA → wpis P2 w KNOWN_ISSUES.

<tabela ze skryptu>

## Komentarz
- Watcher CPU idle: <zmierzone> vs cel <1% — <ile brakuje / czy P2-001 potwierdzony liczbowo>
- VFS cold fetch 100MB: <zmierzone> vs cel <10s — <czy P2-002 potwierdzony>
- ...
- Wnioski do Faza β.4 / Faza ε: <...>
```
Jeśli któraś metryka = FAIL → upewnij się że odpowiedni wpis w KNOWN_ISSUES.md (P2-001/P2-002) ma teraz konkretne liczby zamiast „subiektywna obserwacja".

- [ ] **Step 3: Sprzątanie po pomiarze**

Run: `Remove-Item -Recurse -Force .tmp_perf -ErrorAction SilentlyContinue` (jest gitignorowane przez `.tmp_*` — patrz Clean Ark). Upewnij się że żaden plik testowy nie został w SYNC_PATH ani na O:\.

- [ ] **Step 4: Commit**

```bash
git add scripts/perf-baseline.ps1 docs/perf-baseline-2026-05-11.md
git commit -m "perf: Faza 0.3 — perf-baseline harness + Lenovo baseline measurements

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 5: Dokończ CI + lokalny pre-push hook

> Cel: krok 0.4. `ci.yml` już istnieje (check + clippy -D warnings + test --test-threads=1 na windows-latest). Czego brakuje: `cargo fmt --check`, decyzja o `Cargo.lock`, ew. `--all-features` w teście, oraz lokalny pre-push hook (decyzja Przemka: oba).

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `.gitignore` (warunkowo)
- Add to git: `Cargo.lock` (warunkowo)
- Create: `rustfmt.toml` (warunkowo)
- Create: `.githooks/pre-push`
- Create: `scripts/install-git-hooks.ps1`

- [ ] **Step 1: Decyzja `Cargo.lock`**

Workspace ma binarki (`angeld`, `angelctl`, `omnidrive-cli`, `omnidrive-tray`, `omnidrive-shell-ext`) → konwencja Rusta: **commitować `Cargo.lock`** (powtarzalne buildy, CI cache działa poprawnie, łatwiej diagnozować „u mnie działa"). `ci.yml` już używa `hashFiles('**/Cargo.lock')` w kluczu cache — bez commitowanego locka cache jest mniej skuteczny.
Jeśli decyzja = commit (rekomendowane):
```bash
# usuń linię Cargo.lock z .gitignore (Edit), potem:
git add -f Cargo.lock .gitignore
```
Jeśli Przemek woli zostawić lock ignorowany — pomiń, odnotuj w commit message.

- [ ] **Step 2: Sprawdź czy `cargo fmt --check` przejdzie**

Run: `cargo fmt --all -- --check; echo "exit=$?"` (powinno być znane z Task 1 Step 2).
- Jeśli `exit=0` → przejdź do Step 4 (dodaj fmt step do CI, bez `rustfmt.toml`).
- Jeśli `exit≠0` → Step 3.

- [ ] **Step 3 (warunkowo): Zafiksuj formatowanie LUB dodaj rustfmt.toml**

Opcja A (preferowana jeśli diff niewielki): `cargo fmt --all` → osobny commit `style: cargo fmt --all (Faza 0.4 — przed włączeniem fmt --check w CI)`. Sprawdź `cargo build --workspace` po fmt (czasem fmt rozbija makra — rzadko, ale sprawdź).
Opcja B (jeśli diff ogromny / kontrowersyjny): utwórz `rustfmt.toml` z ustawieniami zbliżonymi do obecnego stylu (np. `max_width`, `use_small_heuristics`), żeby `--check` przechodził bez wielkiego diffa. Mniej idealne — odłóż pełny reformat na osobny batch.
Decyzja A vs B = wg rozmiaru diffa z Step 2; przy wątpliwości zapytaj Przemka.

- [ ] **Step 4: Dodaj `cargo fmt --check` do `ci.yml`**

W `.github/workflows/ci.yml`, w jobie `check`, dodaj komponent `rustfmt` i krok PRZED `cargo check`:
```yaml
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt

      - name: cargo fmt --check
        run: cargo fmt --all -- --check
```
(reszta jobu bez zmian)

- [ ] **Step 5 (decyzja): `--all-features` w `cargo test`?**

STATUS.md QG3 mówi `cargo test --workspace --all-features`. To włączy feature `test-helpers` → `e2e_recovery` faktycznie się wykona (memory: bez tego feature test failuje jako security gate). Sprawdź lokalnie: `cargo test --workspace --all-features -- --test-threads=1; echo "exit=$?"`.
- Jeśli `exit=0` → zmień w `ci.yml` krok testu na `cargo test --workspace --all-features -- --test-threads=1`.
- Jeśli `exit≠0` (jakiś test failuje z `--all-features`) → NIE dodawaj `--all-features` teraz; odnotuj failujące testy jako wpis w KNOWN_ISSUES (prawdopodobnie P2 — blokuje QG3) i zostaw `ci.yml` jak jest. Naprawa = osobne zadanie.

- [ ] **Step 6: Utwórz `.githooks/pre-push`**

Create `.githooks/pre-push` (bash — działa z Git for Windows):
```bash
#!/usr/bin/env bash
# Pre-push: szybki gate przed wysłaniem na origin. Pomiń: `git push --no-verify`.
set -e
echo "[pre-push] cargo fmt --check..."
cargo fmt --all -- --check
echo "[pre-push] cargo clippy -D warnings..."
cargo clippy --workspace -- -D warnings
echo "[pre-push] cargo test --workspace..."
cargo test --workspace -- --test-threads=1
echo "[pre-push] OK"
```
Make executable: `chmod +x .githooks/pre-push` (i `git update-index --chmod=+x .githooks/pre-push` przy add).

- [ ] **Step 7: Utwórz `scripts/install-git-hooks.ps1`**

Create `scripts/install-git-hooks.ps1`:
```powershell
# Ustawia wersjonowane git hooki z .githooks/. Uruchom raz po klonie repo.
git config core.hooksPath .githooks
Write-Host "core.hooksPath = .githooks  (hooki: $(Get-ChildItem .githooks -Name | Where-Object { $_ -notlike '*.sample' }))"
```
Uruchom go raz teraz: `pwsh scripts/install-git-hooks.ps1`. Dodaj wzmiankę w `CLAUDE.md` lub `STATUS.md` workflow, że po klonie trzeba odpalić ten skrypt (lub w README jeśli istnieje).

- [ ] **Step 8: Zweryfikuj cały gate lokalnie**

Run: `cargo fmt --all -- --check && cargo clippy --workspace -- -D warnings && cargo test --workspace -- --test-threads=1; echo "exit=$?"`
Expected: `exit=0`. Jeśli nie — napraw zanim commitujesz (albo, jeśli to znany pre-existing fail udokumentowany w KNOWN_ISSUES, odnotuj w commit message i upewnij się że CI też to odzwierciedla — ale celem Fazy 0 jest zielony gate).

- [ ] **Step 9: Commit**

```bash
git add .github/workflows/ci.yml .githooks/pre-push scripts/install-git-hooks.ps1
# warunkowo: .gitignore Cargo.lock rustfmt.toml
git commit -m "ci: Faza 0.4 — add fmt --check to CI + local pre-push hook + commit Cargo.lock

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
git push origin main   # ← test: pre-push hook powinien się odpalić i przejść
```
Expected: pre-push hook uruchamia się, przechodzi, push idzie. CI na GitHub: workflow `CI` zielony (sprawdź `gh run list --limit 1` jeśli `gh` dostępne).

---

## Task 6: Bookkeeping — zamknięcie Fazy 0

> Cel: odhaczyć Fazę 0 w STATUS.md, zaktualizować nagłówki, podsumować.

**Files:**
- Modify: `STATUS.md` (§12.4 + nagłówek)

- [ ] **Step 1: Odhacz §12.4 w STATUS.md**

W STATUS.md §12.4 tabela — każdy podkrok 0.1–0.5 oznacz jako ✅ DONE z linkiem do artefaktu:
- 0.1 → `docs/superpowers/specs/2026-05-11-code-audit.md` + N wpisów w KNOWN_ISSUES.md
- 0.2 → `docs/SMOKE_CHECKLIST.md`
- 0.3 → `docs/perf-baseline-2026-05-11.md` + `scripts/perf-baseline.ps1`
- 0.4 → `.github/workflows/ci.yml` (+ fmt) + `.githooks/pre-push`
- 0.5 → ✅ (commit `8a8e028`, push 2026-05-11)
Zaktualizuj nagłówek STATUS.md: `Ostatnia aktualizacja: 2026-05-11`. Jeśli §12 ma jakiś marker „aktualna faza" — przesuń na „Faza α — Crypto Hardening *(następna)*".

- [ ] **Step 2: Update memory**

Zaktualizuj `project_next_session_plan.md` (memory): Faza 0 DONE → następny krok = `/superpowers:writing-plans` dla Fazy α (Crypto Hardening), seed = znaleziska z audit report §4 (AAD, zeroize, Argon2id params, X25519 placeholders, ML-KEM, graft identity bundle = α.4/P1-001+P1-005). Zaktualizuj `MEMORY.md` hook. Zaktualizuj `project_overview.md` jeśli trzeba.

- [ ] **Step 3: Commit**

```bash
git add STATUS.md
git commit -m "docs: Faza 0 DONE — QA Foundation complete (audit + smoke checklist + perf baseline + CI gate)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
git push origin main
```

- [ ] **Step 4: Podsumowanie dla Przemka**

Wypisz: co zrobione (5 artefaktów), ile wpisów dodano do KNOWN_ISSUES.md i z jakimi priorytetami, czy CI jest zielone, czy perf baseline pokazał FAILe vs SLA, i co jest następnym krokiem (Faza α plan). Zapytaj czy ruszamy z planem Fazy α czy robimy przerwę.

---

## Self-Review (wykonane przy pisaniu planu)

**1. Spec coverage (STATUS.md §12.4):** 0.1 → Task 1+2 ✅ · 0.2 → Task 3 ✅ · 0.3 → Task 4 ✅ · 0.4 → Task 5 ✅ · 0.5 → już zrobione, odhaczane w Task 6 ✅. Dodatkowo: raport audytu (decyzja Przemka) → Task 1+2 ✅; lokalny pre-push hook (decyzja Przemka) → Task 5 ✅.

**2. Placeholder scan:** Task 1, 3, 5 mają konkretne komendy i treść plików. Task 4 Step 1 (perf-baseline.ps1) i Task 4 Step 2 (treść doc) są opisane proceduralnie a nie pełnym kodem — to świadome: dokładny PowerShell zależy od tego jak na danej maszynie uruchamia się/zatrzymuje daemon (`angelctl` vs surowy exe vs usługa), czego plan nie może z góry znać; engineer ma jasny zakres pomiarów (6 metryk = 6 funkcji) i format wyjścia (tabela markdown vs SLA §12.2). Task 2 z natury jest investigative — „placeholdery" tam (`<…>` w szablonie raportu) to pola do wypełnienia danymi, nie pominięty kod.

**3. Type/path consistency:** Ścieżki spójne: `docs/superpowers/specs/2026-05-11-code-audit.md`, `docs/SMOKE_CHECKLIST.md`, `docs/perf-baseline-2026-05-11.md`, `scripts/perf-baseline.ps1`, `scripts/install-git-hooks.ps1`, `.githooks/pre-push`, `.github/workflows/ci.yml`. Komendy testowe spójne (`cargo test --workspace -- --test-threads=1` wszędzie, zgodnie z istniejącym `ci.yml`). Numeracja KNOWN_ISSUES (P3-001…) — engineer kontynuuje od ostatniego istniejącego numeru w pliku (sprawdzić w Task 2 Step 7).

**Uwaga wykonawcza:** Tasks 1→2→3→5→6 można robić w jednej sesji (read-only + docs + config). **Task 4 (perf baseline) wymaga maszyny Lenovo z żywym daemonem i danymi** — może iść osobno/równolegle; nie blokuje Task 5/6 jeśli §12.4 0.3 odhaczy się z dopiskiem „measurements pending Lenovo run". Token budget (memory `feedback_token_budget.md`): checkpoint z Przemkiem po każdym Tasku.
