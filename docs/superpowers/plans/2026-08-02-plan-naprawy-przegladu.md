# Plan naprawy znalezisk przeglądu 2026-08 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Zamknąć 143 otwarte znaleziska z przeglądu kodu (`docs/ARCHITECTURE.md`) w siedmiu fazach, zaczynając od przywrócenia uwierzytelnienia API, bez którego żadna inna naprawa nie ma oparcia.

**Architecture:** Naprawy idą fazami ustawionymi według zależności, nie według wagi. Faza 0 stawia „mur uwierzytelnienia" i test, który pilnuje, żeby nikt go nie ominął — dopiero wtedy role w API cokolwiek znaczą i można polegać na `require_role` w kolejnych fazach. Faza 1 przeprowadza przez ten mur klientów. Faza 2 odblokowuje dołączanie urządzeń, bo bez niego smoke β.a na Dellu nie ma sensu. Fazy 3-7 to kolejno: integralność danych, koszt chmury, zepsute funkcje, bezpieczeństwo lokalne i dług.

**Tech Stack:** Rust Edition 2024, Tokio, axum, sqlx/SQLite, aws-sdk-s3, windows-rs, Vanilla JS + Tailwind.

## Global Constraints

- **Święta Zasada Integralności Danych** (`CLAUDE.md`): zero operacji zapisu poza `SYNC_PATH`; przy wątpliwości co do bezpieczeństwa funkcji — zatrzymać się i zapytać.
- **Zero-Knowledge Rule**: nigdy nie logować plaintextowych haseł, DEK-ów, Vault Keys ani tokenów OAuth — `[REDACTED]`.
- **Chirurgiczne zmiany**: każdy hunk w diffie musi wynikać z treści zadania; bez refaktorów przy okazji.
- **Zakaz komentarzy w kodzie produkcyjnym**, poza krótkim `///` nad publicznym API, gdy WHY jest nieoczywiste. Zakaz komentarzy z numerami zadań (`// Z9-01`, `// faza 0`).
- **TDD**: najpierw test na czerwono, potem minimalna implementacja.
- **Jeden commit na zadanie**, bez `--allow-empty`.
- **Pipeline instalatora**: `cargo build --release --workspace` → kopiowanie `target/release/*.exe` do `dist/installer/payload/` → dopiero potem Inno Setup. Podbicie wersji dotyczy **wszystkich** `Cargo.toml` w workspace.
- **Sonda na bazie roboczej**: kopia pliku + `ROLLBACK`, nigdy zapis do oryginału. `sqlite3.exe` nie ma w PATH — używać Pythona (`import sqlite3`).
- Stan wyjściowy: `v0.3.29`, HEAD `09c6b28`, 210 testów lib + 18 integracyjnych zielonych.

---

## Jak czytać ten plan

Przegląd dał **147 pozycji w rejestrze, z czego 4 są już naprawione** (Z4-01, Z6-04, Z6-05, Z6-06), plus **3 pozycje informacyjne `ℹ️`** opisane w rozdziałach 1-3, których rejestr nie obejmuje (Z1-07, Z2-08, Z3-07). Razem **146 rzeczy do rozstrzygnięcia**. Rozpisanie wszystkich od razu w granulacji „krok = 2-5 minut" dałoby dokument na kilka tysięcy linii, który zdezaktualizowałby się przy dwudziestym zadaniu — bo naprawa Fazy 0 zmienia warunki dla Faz 3-7.

Dlatego:

- **§1 Triaż** przypisuje **każdą** ze 146 pozycji do fazy i pakietu roboczego. Nic nie ginie.
- **§2 Faza 0** jest rozpisana w pełnej granulacji TDD — to jest to, co wykonuje się następne.
- **§3-§8** to specyfikacje pakietów roboczych dla Faz 1-7: zakres, pliki, kryterium ukończenia, ryzyko. Każda faza dostaje własny szczegółowy plan (`docs/superpowers/plans/`) w momencie, gdy do niej dochodzimy — pisany już na kodzie po poprzednich fazach.
- **§9** to decyzje, które muszą zapaść przed startem odpowiednich faz. Nie zgaduję ich za Przemka.

Trzy pozycje oznaczam jako **WON'T FIX** z uzasadnieniem — lepiej mieć je zamknięte świadomie niż wiszące.

---

## §1 Triaż — wszystkie 143 otwarte znaleziska

Legenda faz: **F0** mur auth · **F1** klienci · **F2** cross-device · **F3** integralność danych · **F4** chmura i workery · **F5** zepsute funkcje · **F6** bezpieczeństwo lokalne · **F7** dług

| Faza | Pakiet | Znaleziska |
| --- | --- | --- |
| **F0** | WP0.1 Test macierzy uwierzytelnienia | Z10-14 |
| **F0** | WP0.2 Koniec z mintowaniem sesji | Z9-01, Z10-05, Z2-04 |
| **F0** | WP0.3 Bramki na endpointach bez kontroli | Z9-06, Z9-07, Z9-08, Z9-19, Z9-20, Z9-26, Z9-30, Z11-02, Z11-06, Z11-12 |
| **F0** | WP0.4 Wzmocnienie bramek istniejących | Z9-13, Z9-21, Z9-03, Z9-28 |
| **F0** | WP0.5 Anty-CSRF i limitery | Z9-02, Z9-04, Z9-10 |
| **F0** | WP0.6 Tryb testowy poza binarką produkcyjną | Z11-04 |
| **F1** | WP1.1 CLI z uwierzytelnieniem | Z10-01, Z10-04 |
| **F1** | WP1.2 Rozstrzygnięcie klientów powłoki | Z7-01, Z10-06, Z10-09 |
| **F1** | WP1.3 Usunięcie `legacy.html` | Z11-03, Z9-15 |
| **F1** | WP1.4 Sesja bez członkostwa | Z9-24 |
| **F2** | WP2.1 Graft: klucze obce i `pack_deks` | Z8-03, Z8-04, Z11-15 |
| **F2** | WP2.2 Graft: odporność i komunikacja | Z8-10, Z8-12 |
| **F2** | WP2.3 Odwołanie urządzenia musi się udać | Z9-22 |
| **F3** | WP3.1 Transakcyjność krypto | Z3-01, Z3-02, Z2-03 |
| **F3** | WP3.2 Kasowanie, które kasuje | Z11-05, Z6-08, Z2-01, Z2-05 |
| **F3** | WP3.3 Scrubber i wyścigi | Z6-03, Z6-07 |
| **F3** | WP3.4 Cache na NTFS | Z5-01 |
| **F3** | WP3.5 Ingest i polityki | Z4-06 |
| **F3** | WP3.6 CLI restore bez niszczenia bazy | Z10-02 |
| **F3** | WP3.7 Recovery a sesje | Z9-17 |
| **F3** | WP3.8 FK na tabelach tożsamości | Z2-06 |
| **F4** | WP4.1 Workery, które nie giną | Z1-02, Z4-09, Z6-15, Z8-18, Z6-12 |
| **F4** | WP4.2 Ponawianie i kwoty | Z4-07, Z4-08, Z4-11, Z6-01 |
| **F4** | WP4.3 Egress pod kontrolą | Z8-06, Z6-09, Z7-09, Z11-06(N+1) |
| **F4** | WP4.4 Sprzątanie po chmurze | Z8-07, Z6-14, Z6-11, Z6-13, Z7-18 |
| **F4** | WP4.5 Log i przepakowywanie | Z1-01, Z4-04, Z4-05 |
| **F5** | WP5.1 Link share z hasłem | Z11-01, Z9-09, Z9-11 |
| **F5** | WP5.2 LAN Share — decyzja i realizacja | Z9-23, Z11-14 |
| **F5** | WP5.3 Blokada Skarbca, która blokuje | Z7-05, Z11-08, Z10-03, Z10-12, Z10-13 |
| **F5** | WP5.4 Licznik bezczynności | Z7-06 |
| **F5** | WP5.5 Hydratacja bez cichych sukcesów | Z7-07 |
| **F5** | WP5.6 Statusy, które mówią prawdę | Z11-07, Z2-02, Z9-12 |
| **F6** | WP6.1 Named Pipe | Z8-01 |
| **F6** | WP6.2 Zaufanie w mesh LAN | Z8-02 |
| **F6** | WP6.3 Sekrety dostawców pod Vault Key | Z8-05, Z9-27, Z9-25 |
| **F6** | WP6.4 Prawdziwe Windows Hello | Z7-02, Z7-03 |
| **F6** | WP6.5 ACL sync roota | Z7-04, Z7-14 |
| **F6** | WP6.6 Koniec z CDN | Z9-05, Z11-10, Z11-09, Z11-11 |
| **F6** | WP6.7 Higiena plików tymczasowych | Z8-08, Z8-09, Z8-16, Z10-10 |
| **F7** | WP7.1 Martwy kod i flagi | Z3-04, Z3-05, Z3-07, Z4-14, Z6-16, Z8-17, Z1-06, Z10-07, Z10-08, Z11-13 |
| **F7** | WP7.2 Duplikacja | Z1-03, Z1-07, Z2-08, Z8-14, Z9-29, Z6-02, Z7-13 |
| **F7** | WP7.3 Klasyfikacja błędów przez `contains()` | Z6-10, Z8-13, Z7-11 |
| **F7** | WP7.4 Wydajność i drobiazgi | Z4-02, Z2-07, Z8-11, Z8-15, Z9-14, Z9-16, Z9-18, Z9-31, Z10-11, Z10-15 |
| **F7** | WP7.5 Dokumentacja vs kod | Z3-06, Z7-15, Z7-12, Z1-05 |
| **F7** | WP7.6 Windows drobne | Z7-08, Z7-10, Z7-16, Z7-17, Z1-04, Z3-03 |
| — | **WON'T FIX** | Z4-03, Z4-10, Z4-12, Z4-13 |

**Uzasadnienie WON'T FIX** (do akceptacji Przemka):

- **Z4-03** (providerzy zaszyci pozycyjnie, `EC_2_1` wymaga dokładnie trzech) — to jest świadomy wybór schematu EC 2+1, nie błąd. Zmiana wymagałaby przeprojektowania erasure codingu. Zostaje jako ograniczenie w dokumentacji.
- **Z4-10** (`all_from_env()` wymaga kompletu trzech) — ścieżka `from_env` istnieje wyłącznie dla dev-boxa; produkcja czyta z bazy. Naprawa w F1/WP1.1 dotyczy tylko CLI (Z10-04), reszta zostaje.
- **Z4-12** (`with_webpki_roots()` zamiast magazynu systemu) — dla trzech znanych dostawców S3 z publicznymi certyfikatami webpki jest **bezpieczniejszym** wyborem niż magazyn systemowy, do którego użytkownik może dodać dowolny root. Zostaje świadomie.
- **Z4-13** (`allow_http` z prefiksu endpointu) — po F0 endpoint może ustawić tylko uwierzytelniony administrator; literówka `http://` w polu, które sam wpisał, to jego decyzja. Do rozważenia ostrzeżenie w UI w F7, ale nie zmiana zachowania.

---

## §2 FAZA 0 — Mur uwierzytelnienia

**Dlaczego pierwsza:** `Z9-01` sprawia, że każde `require_role` w projekcie jest dekoracją. Dopóki to stoi, naprawianie czegokolwiek innego w API jest budowaniem na piasku — a naprawy z Faz 3-6 będą polegać na tym, że role działają.

**Kryterium ukończenia fazy:** test `e2e_auth_matrix` zielony; żadne żądanie bez tokenu nie zmienia stanu ani nie zwraca danych Skarbca; `cargo test --workspace` zielony.

**Uwaga o kolejności:** WP0.1 (test) idzie **przed** naprawami, żeby zobaczyć go na czerwono na liście realnych dziur. To jest TDD zastosowane do całej fazy.

### Task 1: Test macierzy uwierzytelnienia (WP0.1, zamyka Z10-14)

**Files:**
- Create: `angeld/tests/e2e_auth_matrix.rs`
- Test: ten sam plik

**Interfaces:**
- Consumes: `common::DaemonHarness` z `angeld/tests/common/mod.rs` (`spawn()`, `unlock()`, `base_url`)
- Produces: `AUTH_MATRIX: &[(Method, &str, Expect)]` — tablica konsumowana wyłącznie w tym pliku; `Expect::{Public, Session, Role}` opisuje oczekiwaną bramkę

- [ ] **Step 1: Napisz test, który wylicza trasy z kodu i porównuje z tablicą**

Ten test jest ważniejszy niż same naprawy — pilnuje, żeby nowa trasa nie weszła bez decyzji o bramce.

```rust
// angeld/tests/e2e_auth_matrix.rs
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Expect {
    /// Celowo publiczne — udokumentowane w §9.5 ARCHITECTURE.md.
    Public,
    /// Wymaga dowolnej ważnej sesji.
    Session,
    /// Wymaga sesji z rolą w wskazanym vaulcie.
    Role,
}

const AUTH_MATRIX: &[(&str, &str, Expect)] = &[
    ("GET", "/api/vault/status", Expect::Public),
    ("POST", "/api/unlock", Expect::Public),
    ("POST", "/api/unlock/windows-hello", Expect::Public),
    ("GET", "/api/unlock/hello-available", Expect::Public),
    ("POST", "/api/vault/join", Expect::Public),
    ("POST", "/api/recovery/restore", Expect::Public),
    ("GET", "/api/recovery/status", Expect::Public),
    ("GET", "/api/onboarding/status", Expect::Public),
    ("GET", "/api/health", Expect::Public),
    ("GET", "/api/diagnostics/health", Expect::Public),
    ("POST", "/api/onboarding/setup-provider", Expect::Role),
    ("POST", "/api/onboarding/complete", Expect::Role),
    ("POST", "/api/onboarding/reset", Expect::Role),
    ("DELETE", "/api/onboarding/provider/backblaze-b2", Expect::Role),
    ("POST", "/api/providers/backblaze-b2/test", Expect::Role),
    ("POST", "/api/vault/add-device", Expect::Role),
    ("POST", "/api/vault/rotate-key", Expect::Role),
    ("GET", "/api/transfers", Expect::Role),
    ("GET", "/api/multidevice/status", Expect::Role),
    ("GET", "/api/storage/cost", Expect::Role),
    ("GET", "/api/ingest", Expect::Role),
    ("GET", "/api/stats/overview", Expect::Role),
    ("POST", "/api/maintenance/repair-shell", Expect::Role),
];

/// Wyciąga wszystkie literały tras z `angeld/src/api/*.rs`.
fn routes_declared_in_source() -> BTreeSet<String> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/api");
    let mut found = BTreeSet::new();
    for entry in std::fs::read_dir(dir).expect("api dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read source");
        for line in src.lines() {
            let Some(rest) = line.trim().strip_prefix(".route(\"") else {
                continue;
            };
            let Some(end) = rest.find('"') else { continue };
            found.insert(rest[..end].to_string());
        }
    }
    found
}

#[test]
fn every_declared_route_has_an_entry_in_the_matrix() {
    let declared = routes_declared_in_source();
    let covered: BTreeSet<String> = AUTH_MATRIX
        .iter()
        .map(|(_, path, _)| normalize_path(path))
        .collect();
    let missing: Vec<&String> = declared.difference(&covered).collect();
    assert!(
        missing.is_empty(),
        "trasy bez wpisu w AUTH_MATRIX (dopisz je razem z decyzją o bramce): {missing:?}"
    );
}

/// `/api/onboarding/provider/backblaze-b2` -> `/api/onboarding/provider/{provider_name}`
fn normalize_path(path: &str) -> String {
    let known = [
        ("/api/onboarding/provider/", "{provider_name}"),
        ("/api/providers/", "{provider_name}"),
    ];
    for (prefix, placeholder) in known {
        if let Some(rest) = path.strip_prefix(prefix) {
            let tail = rest.split_once('/').map(|(_, t)| format!("/{t}")).unwrap_or_default();
            return format!("{prefix}{placeholder}{tail}");
        }
    }
    path.to_string()
}
```

- [ ] **Step 2: Uruchom i zobacz, że nie kompiluje się / nie przechodzi**

Run: `cargo test --test e2e_auth_matrix every_declared_route -- --nocapture`
Expected: FAIL — lista tras bez wpisu w macierzy (spodziewane ~40 pozycji). Uzupełnij `AUTH_MATRIX` o wszystkie brakujące trasy, nadając każdej `Expect` zgodnie z triażem. Powtarzaj aż PASS.

- [ ] **Step 3: Dopisz test, który sprawdza zachowanie na żywym daemonie**

```rust
mod common;
use common::DaemonHarness;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_public_routes_reject_requests_without_a_token()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;

    let mut offenders = Vec::new();
    for (method, path, expect) in AUTH_MATRIX {
        if *expect == Expect::Public {
            continue;
        }
        let resp = h.request_without_token(method, path).await?;
        if resp.status != 401 && resp.status != 403 {
            offenders.push(format!("{method} {path} -> {}", resp.status));
        }
    }

    assert!(
        offenders.is_empty(),
        "endpointy osiagalne bez tokenu:\n{}",
        offenders.join("\n")
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vault_status_never_returns_a_session_token()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let body = h.get_json("/api/vault/status").await?;
    assert!(
        body.get("session_token").is_none(),
        "/api/vault/status nie moze wystawiac tokenu: {body}"
    );
    Ok(())
}
```

- [ ] **Step 4: Dodaj `request_without_token` do harnessu**

```rust
// angeld/tests/common/mod.rs — dopisz w impl DaemonHarness
pub async fn request_without_token(
    &self,
    method: &str,
    path: &str,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    http_request_raw(method, &format!("{}{}", self.base_url, path), None, None).await
}
```

- [ ] **Step 5: Uruchom testy i zapisz listę czerwonych**

Run: `cargo test --test e2e_auth_matrix -- --nocapture`
Expected: FAIL z listą endpointów osiągalnych bez tokenu **oraz** FAIL na `vault_status_never_returns_a_session_token`. Ta lista to zakres WP0.2-WP0.4 — zapisz ją, będzie kryterium ukończenia.

- [ ] **Step 6: Commit**

```bash
git add angeld/tests/e2e_auth_matrix.rs angeld/tests/common/mod.rs
git commit -m "test(api): macierz uwierzytelnienia endpointow (czerwona)"
```

### Task 2: `/api/vault/status` przestaje wystawiać token (WP0.2, zamyka Z9-01, Z10-05)

**Files:**
- Modify: `angeld/src/api/vault.rs:141-178`
- Modify: `angeld/static/index.html:4015-4033`
- Test: `angeld/tests/e2e_auth_matrix.rs` (test z Task 1, Step 3)

**Interfaces:**
- Consumes: `Expect::Public` dla `/api/vault/status` z Task 1
- Produces: `GET /api/vault/status` zwraca `{unlocked, initialized, members_count, multi_user}` — **bez** `session_token`

Konsekwencja dla UI: dashboard otwarty przy odblokowanym Skarbcu nie dostanie już sesji za darmo. Pokazuje ekran odblokowania i prosi o hasło. To jest zamierzone — konsola Skarbca ma wymagać hasła, a tray i tak potrzebuje wyłącznie pola `unlocked`.

- [ ] **Step 1: Uruchom istniejący test, żeby zobaczyć czerwony**

Run: `cargo test --test e2e_auth_matrix vault_status_never_returns -- --nocapture`
Expected: FAIL — `session_token` obecny w odpowiedzi.

- [ ] **Step 2: Usuń mintowanie z handlera**

```rust
// angeld/src/api/vault.rs
async fn get_vault_status(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let unlocked = state.vault_keys.require_key().await.is_ok();
    let initialized = db::get_vault_config(&state.pool)
        .await
        .ok()
        .flatten()
        .is_some();

    let members_count = match db::get_vault_params(&state.pool).await {
        Ok(Some(vault)) => db::count_vault_members(&state.pool, &vault.vault_id)
            .await
            .unwrap_or(0),
        _ => 0,
    };

    Json(serde_json::json!({
        "unlocked": unlocked,
        "initialized": initialized || unlocked,
        "members_count": members_count,
        "multi_user": members_count > 1,
    }))
}
```

- [ ] **Step 3: Uruchom test**

Run: `cargo test --test e2e_auth_matrix vault_status_never_returns -- --nocapture`
Expected: PASS

- [ ] **Step 4: Popraw dashboard, żeby prosił o hasło zamiast liczyć na token**

```javascript
// angeld/static/index.html — w bloku startowym, zamiast gałęzi `if (data && data.unlocked)`
          return fetch('/api/vault/status', { headers: { 'Accept': 'application/json' } })
            .then(r => r.ok ? r.json() : Promise.reject())
            .then(data => {
              VAULT_STATE.unlocked = Boolean(data && data.unlocked);
              if (VAULT_STATE.sessionToken) {
                startDashboard();
                return;
              }
              showLockScreen();
              if (lsInp) setTimeout(() => lsInp.focus(), 100);
            });
```

- [ ] **Step 5: Sprawdź ręcznie, że tray dalej działa**

Run: `cargo build --release --workspace` a następnie uruchom `target/release/angeld.exe` i `target/release/omnidrive-tray.exe`.
Expected: ikona zasobnika przechodzi z `Locked` na `Synced` po odblokowaniu przez dashboard; sonda `SELECT COUNT(*) FROM user_sessions` na kopii bazy nie rośnie w czasie bezczynności.

- [ ] **Step 6: Commit**

```bash
git add angeld/src/api/vault.rs angeld/static/index.html
git commit -m "fix(api): /api/vault/status nie wystawia juz tokenu sesji (Z9-01)"
```

### Task 3: Sprzątanie wygasłych sesji (WP0.2, zamyka Z2-04)

**Files:**
- Modify: `angeld/src/main.rs` (obok istniejącego `_token_cleanup_task`, ~linia 768)
- Test: `angeld/src/db/sessions.rs` (test jednostkowy)

**Interfaces:**
- Consumes: `db::cleanup_expired_sessions(&pool) -> Result<u64, sqlx::Error>` (istnieje, brak wywołujących)
- Produces: brak nowego API

- [ ] **Step 1: Napisz test, że funkcja kasuje wygasłe i zostawia ważne**

```rust
// angeld/src/db/sessions.rs — w module tests
#[tokio::test]
async fn cleanup_removes_only_expired_sessions() -> Result<(), Box<dyn std::error::Error>> {
    let pool = crate::db::init_db("sqlite::memory:").await?;
    crate::db::create_user(&pool, "u-1", "U", None, "local", None).await?;
    create_user_session(&pool, "live", "u-1", "dev-a", SESSION_TTL_SECONDS).await?;
    create_user_session(&pool, "dead", "u-1", "dev-a", 0).await?;

    let removed = cleanup_expired_sessions(&pool).await?;

    assert_eq!(removed, 1);
    assert!(validate_user_session(&pool, "live").await?.is_some());
    assert!(validate_user_session(&pool, "dead").await?.is_none());
    Ok(())
}
```

- [ ] **Step 2: Uruchom test**

Run: `cargo test -p angeld cleanup_removes_only_expired_sessions`
Expected: PASS albo FAIL wskazujący na sygnaturę — jeśli FAIL, dopasuj wywołanie do istniejącej sygnatury w `db/sessions.rs`, nie zmieniaj funkcji.

- [ ] **Step 3: Podepnij sprzątanie do istniejącego zadania okresowego**

```rust
// angeld/src/main.rs — w istniejącym _token_cleanup_task, po cleanup_expired_share_tokens
            match db::cleanup_expired_sessions(&cleanup_pool).await {
                Ok(count) if count > 0 => {
                    tracing::debug!("cleaned up {count} expired user sessions");
                }
                Err(err) => {
                    tracing::warn!("failed to clean up user sessions: {err}");
                }
                _ => {}
            }
```

- [ ] **Step 4: Uruchom pełne testy**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add angeld/src/main.rs angeld/src/db/sessions.rs
git commit -m "fix(db): sprzataj wygasle sesje co 5 minut (Z2-04)"
```

### Task 4: Bramki na endpointach diagnostyki, statystyk i ingestu (WP0.3, zamyka Z9-06, Z9-07, Z9-26, Z11-06/auth)

**Files:**
- Modify: `angeld/src/api/diagnostics.rs` (9 handlerów)
- Modify: `angeld/src/api/stats.rs` (3 handlery)
- Modify: `angeld/src/api/maintenance.rs:764` (`get_ingest_jobs`)
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Consumes: `acl::require_role(&pool, &headers, Role::Viewer) -> Result<AuthorizedCaller, ApiError>`
- Produces: handlery przyjmują dodatkowy parametr `headers: HeaderMap`

Wyjątki, które zostają publiczne (są w macierzy jako `Expect::Public`): `/api/health` i `/api/diagnostics/health` — używa ich harness testowy do wykrycia gotowości API i tray do stwierdzenia, czy daemon żyje. Nie zwracają danych Skarbca.

- [ ] **Step 1: Uruchom test macierzy**

Run: `cargo test --test e2e_auth_matrix non_public_routes_reject -- --nocapture`
Expected: FAIL z listą zawierającą `/api/transfers`, `/api/multidevice/status`, `/api/storage/cost`, `/api/stats/*`, `/api/ingest`.

- [ ] **Step 2: Dodaj bramkę do każdego z nich**

Wzorzec, powtórzony dla każdego handlera z listy (przykład na `get_transfers`):

```rust
async fn get_transfers(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<TransferResponse>>, ApiError> {
    acl::require_role(&state.pool, &headers, Role::Viewer).await?;

    let jobs = db::list_recent_upload_jobs(&state.pool, 50).await?;
```

Do zmiany, wszystkie na `Role::Viewer`: `get_transfers`, `get_diagnostics_overview`, `get_shell_state`, `get_sync_root_state`, `get_restore_state`, `get_storage_cost`, `get_multidevice_status` (`diagnostics.rs`); `get_stats_overview`, `get_stats_traffic`, `get_stats_system` (`stats.rs`); `get_ingest_jobs` (`maintenance.rs`).

`get_shell_state` i `get_sync_root_state` nie mają dziś `State<ApiState>` — dodaj je razem z `headers`.

Dodaj import w `stats.rs`:

```rust
use crate::acl::{self, Role};
use axum::http::HeaderMap;
```

- [ ] **Step 3: Uruchom test**

Run: `cargo test --test e2e_auth_matrix non_public_routes_reject -- --nocapture`
Expected: wymienione trasy znikają z listy naruszeń.

- [ ] **Step 4: Popraw dashboard, żeby wysyłał token do tych paneli**

W `angeld/static/index.html` wszystkie wywołania powyższych endpointów muszą używać istniejącego helpera nagłówków (`vaultAuthHeaders()`), a nie gołego `{ Accept: 'application/json' }`. Znajdź je: `grep -n "api/transfers\|api/storage/cost\|api/stats/\|api/ingest\|api/multidevice\|api/diagnostics/" angeld/static/index.html`.

- [ ] **Step 5: Uruchom pełne testy**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add angeld/src/api/diagnostics.rs angeld/src/api/stats.rs angeld/src/api/maintenance.rs angeld/static/index.html
git commit -m "fix(api): rola Viewer na diagnostyce, statystykach i ingescie (Z9-06, Z9-07, Z9-26)"
```

### Task 5: Bramki na operacjach zmieniających stan (WP0.3, zamyka Z9-08, Z9-19, Z9-20, Z11-02, Z11-12)

**Files:**
- Modify: `angeld/src/api/onboarding.rs` (`post_setup_provider`, `post_complete_onboarding`, `post_reset_onboarding`, `delete_provider`, `post_test_provider`)
- Modify: `angeld/src/api/maintenance.rs:532` (`post_repair_shell`)
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Produces: te endpointy wymagają `Role::Admin`

**Pułapka do rozwiązania w tym zadaniu:** kreator onboardingu woła `setup-provider` i `complete` **zanim** istnieje jakakolwiek sesja. Rozwiązanie: wymagaj roli **tylko wtedy, gdy onboarding jest już zakończony**. W trakcie kreatora (`onboarding_state != COMPLETED`) endpointy zostają otwarte — Skarbiec nie ma jeszcze czego chronić.

- [ ] **Step 1: Napisz test obu ścieżek**

```rust
// angeld/tests/e2e_auth_matrix.rs
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn setup_provider_is_open_during_onboarding_and_gated_after()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;

    let body = serde_json::json!({
        "provider_name": "backblaze-b2",
        "endpoint": "http://127.0.0.1:1",
        "region": "eu-central-003",
        "bucket": "test"
    });

    let during = h.post_json_without_token("/api/onboarding/setup-provider", &body).await?;
    assert_ne!(during.status, 401, "w trakcie kreatora endpoint musi byc otwarty");

    h.unlock().await?;
    h.post("/api/onboarding/complete").await?;

    let after = h.post_json_without_token("/api/onboarding/setup-provider", &body).await?;
    assert_eq!(
        after.status, 401,
        "po zakonczeniu onboardingu endpoint musi wymagac sesji; got {}",
        after.status
    );
    Ok(())
}
```

- [ ] **Step 2: Uruchom test**

Run: `cargo test --test e2e_auth_matrix setup_provider_is_open -- --nocapture`
Expected: FAIL — po zakończeniu onboardingu status inny niż 401.

- [ ] **Step 3: Dodaj helper bramki zależnej od stanu onboardingu**

```rust
// angeld/src/api/onboarding.rs
async fn require_admin_after_onboarding(
    state: &ApiState,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let completed = db::get_system_config_value(&state.pool, SYSTEM_CONFIG_ONBOARDING_STATE)
        .await?
        .is_some_and(|value| value.eq_ignore_ascii_case(OnboardingState::Completed.as_str()));
    if completed {
        acl::require_role(&state.pool, headers, Role::Admin).await?;
    }
    Ok(())
}
```

Wywołaj je jako pierwszą instrukcję w `post_setup_provider`, `post_complete_onboarding`, `post_reset_onboarding`, `delete_provider` i `post_test_provider`; każdy z tych handlerów dostaje dodatkowy parametr `headers: HeaderMap`.

`post_repair_shell` w `maintenance.rs` dostaje zwykłe `acl::require_role(&state.pool, &headers, Role::Admin).await?` — nie ma związku z kreatorem, więc bez helpera. Handler musi przyjąć `State<ApiState>` i `headers`.

- [ ] **Step 4: Uruchom testy**

Run: `cargo test --test e2e_auth_matrix -- --nocapture`
Expected: PASS na obu testach z tego zadania.

- [ ] **Step 5: Commit**

```bash
git add angeld/src/api/onboarding.rs angeld/src/api/maintenance.rs angeld/tests/e2e_auth_matrix.rs
git commit -m "fix(api): rola Admin na operacjach dostawcow i repair-shell po onboardingu (Z9-08, Z9-19, Z9-20, Z11-02, Z11-12)"
```

### Task 6: Rotacja hasła wymaga starego hasła (WP0.4, zamyka Z9-21)

**Files:**
- Modify: `angeld/src/api/vault.rs:1040-1087`
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Produces: `RotateKeyRequest { old_passphrase: SecretString, new_passphrase: SecretString }`

- [ ] **Step 1: Napisz test**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rotate_key_rejects_wrong_old_passphrase()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;

    let resp = h
        .post_json(
            "/api/vault/rotate-key",
            serde_json::json!({
                "old_passphrase": "nie-to-haslo",
                "new_passphrase": "nowe-haslo-1234"
            }),
        )
        .await?;

    assert_eq!(
        resp.status, 400,
        "rotacja ze zlym starym haslem musi byc odrzucona; got {} body={}",
        resp.status, resp.body
    );
    Ok(())
}
```

- [ ] **Step 2: Uruchom test**

Run: `cargo test --test e2e_auth_matrix rotate_key_rejects_wrong_old -- --nocapture`
Expected: FAIL — 200, rotacja przeszła.

- [ ] **Step 3: Dodaj weryfikację, wzorem `post_change_password`**

```rust
#[derive(serde::Deserialize)]
struct RotateKeyRequest {
    old_passphrase: SecretString,
    new_passphrase: SecretString,
}

async fn post_rotate_key(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<RotateKeyRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Admin).await?;

    if req.new_passphrase.expose_secret().is_empty() {
        return Err(ApiError::BadRequest {
            code: "empty_passphrase",
            message: "new_passphrase must not be empty".into(),
        });
    }

    let valid = state
        .vault_keys
        .verify_passphrase(&state.pool, req.old_passphrase.expose_secret())
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?;
    if !valid {
        return Err(ApiError::BadRequest {
            code: "wrong_passphrase",
            message: "current passphrase is incorrect".to_string(),
        });
    }

    state
        .vault_keys
        .rotate_vault_key(&state.pool, req.new_passphrase.expose_secret())
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?;
```

Reszta handlera (audyt + `spawn_post_rotation_backup`) bez zmian.

- [ ] **Step 4: Uruchom test**

Run: `cargo test --test e2e_auth_matrix rotate_key_rejects_wrong_old -- --nocapture`
Expected: PASS

- [ ] **Step 5: Sprawdź, czy dashboard woła ten endpoint z nowym polem**

Run: `grep -n "rotate-key" angeld/static/index.html`
Jeśli wywołanie istnieje, dopisz `old_passphrase` z pola formularza; jeśli nie istnieje, nic nie rób.

- [ ] **Step 6: Commit**

```bash
git add angeld/src/api/vault.rs angeld/tests/e2e_auth_matrix.rs
git commit -m "fix(api): rotate-key wymaga starego hasla (Z9-21)"
```

### Task 7: `add-device` za bramką i z kompletem kontroli (WP0.4, zamyka Z9-03, Z9-28)

**Files:**
- Modify: `angeld/src/api/vault.rs:585-741`
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Produces: `post_add_device` wymaga `Role::Admin`; `try_auto_wrap_vault_key` sprawdza `enrolled_at`, `revoked_at` i zerowy klucz publiczny

- [ ] **Step 1: Napisz test**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn add_device_requires_admin_and_never_wraps_for_unenrolled()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;

    let body = serde_json::json!({
        "user_id": "u-obcy",
        "device_id": "dev-obcy",
        "device_name": "Obce",
        "public_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    });

    let anon = h.post_json_without_token("/api/vault/add-device", &body).await?;
    assert_eq!(anon.status, 401, "add-device bez tokenu musi byc odrzucone");

    let authed = h.post_json("/api/vault/add-device", body).await?;
    assert!(
        !authed.body.contains("wrapped_vault_key\":\""),
        "nieznane urzadzenie nie moze dostac owinietego klucza: {}",
        authed.body
    );
    Ok(())
}
```

- [ ] **Step 2: Uruchom test**

Run: `cargo test --test e2e_auth_matrix add_device_requires_admin -- --nocapture`
Expected: FAIL na pierwszej asercji.

- [ ] **Step 3: Dodaj bramkę i przenieś kontrole z `post_accept_device`**

```rust
async fn post_add_device(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<AddDeviceRequest>,
) -> Result<Json<AddDeviceResponse>, ApiError> {
    acl::require_role(&state.pool, &headers, Role::Admin).await?;

    let vault_id = db::get_vault_params(&state.pool)
```

oraz w `try_auto_wrap_vault_key`, przed wywołaniem `wrap_vault_key_for_device`:

```rust
    let target = db::get_device(&state.pool, target_device_id).await.ok()??;
    if target.revoked_at.is_some() || target.enrolled_at.is_none() {
        return None;
    }
    if target_pub == [0u8; 32] {
        return None;
    }
```

- [ ] **Step 4: Uruchom testy**

Run: `cargo test --test e2e_auth_matrix add_device -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add angeld/src/api/vault.rs angeld/tests/e2e_auth_matrix.rs
git commit -m "fix(api): add-device za rola Admin i z kontrolami z accept-device (Z9-03, Z9-28)"
```

### Task 8: Podniesienie roli dla weryfikacji urządzeń i owiniętych kluczy (WP0.4, zamyka Z9-13, Z9-30)

**Files:**
- Modify: `angeld/src/api/vault.rs:1143` (`post_verify_device`), `:471` (`get_my_wrapped_key`)
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Produces: `post_verify_device` wymaga `Role::Admin`; `get_my_wrapped_key` zwraca wyłącznie klucz **własnego** urządzenia wywołującego

- [ ] **Step 1: Napisz test**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrapped_key_endpoint_only_serves_the_calling_device()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let resp = h.get_raw("/api/vault/my-wrapped-key?device_id=cudze-urzadzenie").await?;
    assert_eq!(
        resp.status, 403,
        "pytanie o cudze urzadzenie musi byc odrzucone; got {} body={}",
        resp.status, resp.body
    );
    Ok(())
}
```

- [ ] **Step 2: Uruchom test**

Run: `cargo test --test e2e_auth_matrix wrapped_key_endpoint -- --nocapture`
Expected: FAIL — 404 zamiast 403.

- [ ] **Step 3: Zawęź handler do urządzenia wywołującego**

```rust
async fn get_my_wrapped_key(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<WrappedKeyResponse>, ApiError> {
    let caller = acl::require_role(&state.pool, &headers, Role::Viewer).await?;

    let device_id = params.get("device_id").ok_or(ApiError::BadRequest {
        code: "missing_device_id",
        message: "device_id query parameter is required".to_string(),
    })?;

    if device_id != &caller.device_id {
        return Err(ApiError::Forbidden {
            message: "can only fetch the wrapped key of the calling device".to_string(),
        });
    }
```

W `post_verify_device` zmień `Role::Viewer` na `Role::Admin`.

- [ ] **Step 4: Uruchom testy**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add angeld/src/api/vault.rs angeld/tests/e2e_auth_matrix.rs
git commit -m "fix(api): my-wrapped-key tylko dla wlasnego urzadzenia, verify-device dla Admina (Z9-13, Z9-30)"
```

### Task 9: Anty-CSRF na endpointach bez ciała (WP0.5, zamyka Z9-02/CSRF)

**Files:**
- Create: `angeld/src/api/local_guard.rs`
- Modify: `angeld/src/api/mod.rs` (rejestracja modułu)
- Modify: `angeld/src/api/auth.rs:411` (`post_windows_hello_unlock`)
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Produces: `pub(super) fn require_local_intent(headers: &HeaderMap) -> Result<(), ApiError>` — odrzuca żądanie bez nagłówka `X-OmniDrive-Local: 1`

Mechanizm: nietypowy nagłówek wymusza w przeglądarce preflight CORS, którego daemon nie obsłuży dla obcego origin. Strona WWW nie wykona więc tego żądania w ogóle. Nie chroni to przed lokalnym procesem — to jest zadanie WP6.4 (prawdziwe Windows Hello). Tutaj zamykamy wyłącznie drive-by z przeglądarki.

- [ ] **Step 1: Napisz test**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn windows_hello_unlock_requires_local_intent_header()
-> Result<(), Box<dyn std::error::Error>> {
    let h = DaemonHarness::spawn().await?;
    let resp = h.request_without_token("POST", "/api/unlock/windows-hello").await?;
    assert_eq!(
        resp.status, 403,
        "POST bez naglowka X-OmniDrive-Local musi byc odrzucony; got {} body={}",
        resp.status, resp.body
    );
    Ok(())
}
```

- [ ] **Step 2: Uruchom test**

Run: `cargo test --test e2e_auth_matrix windows_hello_unlock_requires -- --nocapture`
Expected: FAIL — 404 (brak poświadczenia) albo 200.

- [ ] **Step 3: Napisz strażnika**

```rust
// angeld/src/api/local_guard.rs
use axum::http::HeaderMap;

use super::error::ApiError;

const LOCAL_INTENT_HEADER: &str = "x-omnidrive-local";

/// Wymusza preflight CORS: przeglądarka nie wyśle tego nagłówka z obcego origin
/// bez zgody serwera, więc żądanie drive-by nie dojdzie do handlera.
pub(super) fn require_local_intent(headers: &HeaderMap) -> Result<(), ApiError> {
    let present = headers
        .get(LOCAL_INTENT_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim() == "1");

    if present {
        Ok(())
    } else {
        Err(ApiError::Forbidden {
            message: "missing local intent header".to_string(),
        })
    }
}
```

W `angeld/src/api/mod.rs` dopisz `mod local_guard;`. W `auth.rs` dodaj `headers: HeaderMap` do `post_windows_hello_unlock` i jako pierwszą instrukcję `super::local_guard::require_local_intent(&headers)?;`.

- [ ] **Step 4: Uruchom test**

Run: `cargo test --test e2e_auth_matrix windows_hello_unlock_requires -- --nocapture`
Expected: PASS

- [ ] **Step 5: Dodaj nagłówek w dashboardzie**

```javascript
// angeld/static/index.html — w lockScreenWindowsHelloUnlock
        const res = await fetch('/api/unlock/windows-hello', {
          method: 'POST',
          headers: { 'Accept': 'application/json', 'X-OmniDrive-Local': '1' },
        });
```

- [ ] **Step 6: Commit**

```bash
git add angeld/src/api/local_guard.rs angeld/src/api/mod.rs angeld/src/api/auth.rs angeld/static/index.html angeld/tests/e2e_auth_matrix.rs
git commit -m "fix(api): naglowek local-intent na unlock/windows-hello (Z9-02 CSRF)"
```

### Task 10: Zapamiętywanie hasła staje się opcją (WP0.5, zamyka Z9-02/silent-store)

**Files:**
- Modify: `angeld/src/api/auth.rs:66-70` i `:331-334`
- Modify: `angeld/src/api/settings.rs` (nowy endpoint)
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Produces: klucz `system_config` = `windows_hello_enabled` (`"0"` domyślnie); `POST /api/settings/windows-hello {enabled: bool}` wymaga `Role::Admin`

- [ ] **Step 1: Napisz test, że domyślnie nic się nie zapisuje**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unlock_does_not_store_passphrase_unless_opted_in()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let resp = h.get_json("/api/unlock/hello-available").await?;
    assert_eq!(
        resp["available"].as_bool(),
        Some(false),
        "bez wlaczenia opcji haslo nie moze trafic do Credential Managera"
    );
    Ok(())
}
```

- [ ] **Step 2: Uruchom test**

Run: `cargo test --test e2e_auth_matrix unlock_does_not_store -- --nocapture`
Expected: FAIL — `available: true`.

- [ ] **Step 3: Obwaruj zapis flagą**

```rust
// angeld/src/api/auth.rs — w post_unlock, zamiast bezwarunkowego store_passphrase
    let hello_enabled = db::get_system_config_value(&state.pool, "windows_hello_enabled")
        .await
        .ok()
        .flatten()
        .is_some_and(|value| value == "1");
    if hello_enabled
        && let Err(err) = windows_hello::store_passphrase(request.passphrase.expose_secret())
    {
        warn!("[UNLOCK] windows_hello store failed (non-fatal): {err}");
    }
```

Ten sam warunek w `post_change_password`. Dodaj endpoint w `settings.rs`:

```rust
#[derive(Deserialize)]
struct WindowsHelloRequest {
    enabled: bool,
}

async fn post_windows_hello_setting(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<WindowsHelloRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acl::require_role(&state.pool, &headers, Role::Admin).await?;
    db::set_system_config_value(
        &state.pool,
        "windows_hello_enabled",
        if req.enabled { "1" } else { "0" },
    )
    .await?;
    if !req.enabled {
        let _ = windows_hello::clear_stored_credential();
    }
    Ok(Json(serde_json::json!({ "enabled": req.enabled })))
}
```

Jeśli `windows_hello::clear_stored_credential` nie istnieje, dopisz ją w `angeld/src/windows_hello.rs` wzorem `store_passphrase`, wołając `CredDeleteW`.

- [ ] **Step 4: Uruchom testy**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add angeld/src/api/auth.rs angeld/src/api/settings.rs angeld/src/windows_hello.rs angeld/tests/e2e_auth_matrix.rs
git commit -m "feat(auth): zapamietanie hasla w Credential Managerze jako opcja, domyslnie wylaczona (Z9-02)"
```

### Task 11: Limiter na `/api/unlock` i `verify-password` (WP0.5, zamyka Z9-04, Z9-10)

**Files:**
- Modify: `angeld/src/api/mod.rs` (`ApiState` + nowy limiter)
- Modify: `angeld/src/api/auth.rs:43` (`post_unlock`)
- Modify: `angeld/src/api/sharing.rs:339` (`verify_share_password`)
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Consumes: `RecoveryRateLimiter::{check, record_failure, record_success}` — istniejący typ, wielokrotnie użyty
- Produces: `ApiState.unlock_limiter: Arc<RecoveryRateLimiter>`

- [ ] **Step 1: Napisz test**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_wrong_passphrase_is_rate_limited()
-> Result<(), Box<dyn std::error::Error>> {
    let h = DaemonHarness::spawn().await?;
    let mut last = 0u16;
    for _ in 0..5 {
        let resp = h
            .post_json_without_token("/api/unlock", &serde_json::json!({ "passphrase": "zle" }))
            .await?;
        last = resp.status;
    }
    assert_eq!(
        last, 429,
        "po serii bledow /api/unlock musi zwrocic 429; got {last}"
    );
    Ok(())
}
```

- [ ] **Step 2: Uruchom test**

Run: `cargo test --test e2e_auth_matrix repeated_wrong_passphrase -- --nocapture`
Expected: FAIL — 400 za każdym razem.

- [ ] **Step 3: Podłącz limiter**

W `ApiState` dodaj `unlock_limiter: Arc<RecoveryRateLimiter>` i zainicjuj w `ApiServer::run` przez `Arc::new(RecoveryRateLimiter::new())`. W `post_unlock` dodaj `ConnectInfo(addr): ConnectInfo<SocketAddr>` i:

```rust
    let ip = addr.ip();
    if let Err(retry_after) = state.unlock_limiter.check(ip) {
        return Err(ApiError::TooManyRequests {
            retry_after_secs: retry_after,
            message: format!("too many unlock attempts — retry after {retry_after}s"),
        });
    }
```

Na ścieżce błędu `unlock` wywołaj `state.unlock_limiter.record_failure(ip)` i dopisz audyt:

```rust
    if let Ok(Some(vault)) = db::get_vault_params(&state.pool).await {
        let _ = db::insert_audit_log(
            &state.pool,
            &vault.vault_id,
            "vault_unlock_failed",
            None,
            None,
            None,
            None,
            Some(&format!(r#"{{"ip":"{ip}"}}"#)),
        )
        .await;
    }
```

Na ścieżce sukcesu `state.unlock_limiter.record_success(ip)`. Ten sam wzorzec zastosuj w `verify_share_password` (osobny limiter `share_limiter` w `ApiState`).

- [ ] **Step 4: Uruchom testy**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add angeld/src/api/mod.rs angeld/src/api/auth.rs angeld/src/api/sharing.rs angeld/tests/e2e_auth_matrix.rs
git commit -m "fix(api): limiter i audyt nieudanych prob na /api/unlock i verify-password (Z9-04, Z9-10)"
```

### Task 12: Tryb testowy znika z binarki produkcyjnej (WP0.6, zamyka Z11-04)

**Files:**
- Modify: `angeld/src/main.rs:110, 320, 382, 968`
- Modify: `angeld/src/diagnostics.rs` (nowy wariant statusu)
- Modify: `angeld/tests/common/mod.rs` (harness startuje z feature flagą)
- Modify: `angeld/Cargo.toml` (feature `test-helpers` obejmuje tryb e2e)
- Test: `angeld/tests/e2e_basic.rs:81-85`

**Interfaces:**
- Produces: `WorkerStatus::NotStarted` z `as_str() == "not_started"`

- [ ] **Step 1: Napisz test, że diagnostyka nie kłamie**

```rust
// angeld/tests/e2e_basic.rs — zamień asercje na statusach nieuruchomionych workerow
            assert_eq!(health.worker_statuses.repair, "not_started");
            assert_eq!(health.worker_statuses.scrubber, "not_started");
            assert_eq!(health.worker_statuses.gc, "not_started");
            assert_eq!(health.worker_statuses.watcher, "not_started");
            assert_eq!(health.worker_statuses.metadata_backup, "not_started");
```

- [ ] **Step 2: Uruchom test**

Run: `cargo test --test e2e_basic happy_path -- --nocapture`
Expected: FAIL — otrzymano `idle`.

- [ ] **Step 3: Dodaj wariant statusu i przestań kłamać**

W `angeld/src/diagnostics.rs` dodaj do `WorkerStatus` wariant `NotStarted` z `as_str()` zwracającym `"not_started"`, i ustaw go jako wartość początkową zamiast `Idle`. W `main.rs:382` zamień pętlę ustawiającą `Idle` na ustawianie `NotStarted` dla workerów, które w tym trybie nie startują.

- [ ] **Step 4: Schowaj flagę za feature**

```rust
// angeld/src/main.rs
#[cfg(feature = "test-helpers")]
fn is_e2e_test_mode() -> bool {
    env_flag("OMNIDRIVE_E2E_TEST_MODE")
}

#[cfg(not(feature = "test-helpers"))]
fn is_e2e_test_mode() -> bool {
    false
}
```

W `angeld/tests/common/mod.rs` nic nie zmieniaj — `cargo test` buduje binarkę z features testowymi, więc zmienna dalej działa w testach. Sprawdź to w kroku 5.

- [ ] **Step 5: Uruchom testy i zweryfikuj build produkcyjny**

Run: `cargo test --workspace` (Expected: PASS) oraz `cargo build --release --workspace` a potem uruchomienie `target/release/angeld.exe` z `OMNIDRIVE_E2E_TEST_MODE=1` — Expected: daemon startuje **wszystkie** workery, zmienna jest ignorowana.

- [ ] **Step 6: Commit**

```bash
git add angeld/src/main.rs angeld/src/diagnostics.rs angeld/tests/e2e_basic.rs angeld/Cargo.toml
git commit -m "fix(daemon): tryb e2e tylko za feature test-helpers, status not_started zamiast falszywego idle (Z11-04)"
```

### Task 13: Zamknięcie Fazy 0

- [ ] **Step 1: Uruchom komplet testów**

Run: `cargo test --workspace`
Expected: PASS, w tym `e2e_auth_matrix` w całości.

- [ ] **Step 2: Sprawdź, że macierz nie ma już naruszeń**

Run: `cargo test --test e2e_auth_matrix non_public_routes_reject -- --nocapture`
Expected: PASS, lista naruszeń pusta.

- [ ] **Step 3: Odhacz znaleziska w rejestrze**

W `docs/ARCHITECTURE.md` zmień wagę na ✅ i dopisz „**NAPRAWIONE** `<sha>`" dla: Z2-04, Z9-01, Z9-02, Z9-03, Z9-04, Z9-06, Z9-07, Z9-08, Z9-10, Z9-13, Z9-19, Z9-20, Z9-21, Z9-26, Z9-28, Z9-30, Z10-05, Z10-14, Z11-02, Z11-04, Z11-12.

- [ ] **Step 4: Commit**

```bash
git add docs/ARCHITECTURE.md
git commit -m "docs(architecture): faza 0 zamknieta, 21 znalezisk naprawionych"
```

---

## §3 FAZA 1 — Klienci przez mur

**Wejście:** Faza 0 zamknięta. **Wyjście:** każdy klient, który zostaje w repo, potrafi się uwierzytelnić; klienci, którzy zostają usunięci, są usunięci.

| Pakiet | Zakres | Kryterium ukończenia |
| --- | --- | --- |
| **WP1.1** Z10-01, Z10-04 | `omnidrive-cli` dostaje `--api-token` / `OMNIDRIVE_API_TOKEN` oraz komendę `omnidrive login`, która pyta o hasło i woła `/api/unlock`, zapisując token w pliku `%LOCALAPPDATA%\OmniDrive\cli-session` z ACL tylko dla użytkownika. `recovery restore` przestaje wymagać kompletu env — czyta konfigurację dostawcy z bazy przez `MetadataBackupProviderManager::from_onboarding_db_all`. | `omnidrive ls` i `omnidrive pin` działają po `omnidrive login`; test integracyjny wywołujący obie komendy przeciw harnessowi |
| **WP1.2** Z7-01, Z10-06, Z10-09 | **Decyzja D1 (§9)**: albo instalator wgrywa i rejestruje `omnidrive_shell_ext.dll` i usuwamy wariant rejestrowy z `shell_integration.rs`, albo odwrotnie. Przy wariancie DLL: litera dysku z `OMNIDRIVE_DRIVE_LETTER` zamiast twardego `O:\`. | Jedno menu kontekstowe, działające, z testem `e2e_shell_recovery` rozszerzonym o sprawdzenie pozycji menu |
| **WP1.3** Z11-03, Z9-15 | Usunięcie `static/legacy.html` i trasy `/legacy` z `api/mod.rs`. To 2258 linii martwego, nieuwierzytelnionego kodu — utrzymywanie go kosztuje więcej niż daje. | `grep -r legacy angeld/` pusty poza historią gita |
| **WP1.4** Z9-24 | `extract_session` w `settings.rs` i `require_session` w `auto_lock.rs` zaczynają sprawdzać członkostwo w vaulcie (nowy helper `acl::require_member_session`). Callback Google przestaje tworzyć użytkownika, jeśli w vaulcie jest już właściciel, a konto Google nie jest jego członkiem. | Test: sesja z konta bez `vault_members` dostaje 403 na `/api/settings/restart-daemon` |

---

## §4 FAZA 2 — Cross-device

**Wejście:** Faza 1 zamknięta. **Wyjście:** „Join Existing Vault" działa end-to-end na Dellu — to jest warunek smoke'u β.a.

| Pakiet | Zakres | Kryterium ukończenia |
| --- | --- | --- |
| **WP2.1** Z8-03, Z8-04, Z11-15 | `graft_restored_metadata_snapshot`: wyłączyć FK **przed** `BEGIN` (albo dopisać `DELETE FROM user_sessions` do listy kasowanych — decyzja **D2**, §9); dopisać kopiowanie tabeli `pack_deks` po `packs`; zawęzić fallback w `dek_for_pack` tak, żeby przy wielu DEK-ach na inode **nie zgadywał** i nie zapisywał zgadywanki, tylko zwracał błąd. | Test e2e: plik **12 MiB** (3 chunki) → migawka → graft na czystej bazie → `restore_file` odtwarza bajt w bajt. Ten test musi być czerwony przed naprawą. |
| **WP2.2** Z8-10, Z8-12 | `r_vault_config` przestaje być `unwrap_or(None)` — brak `vault_config` w migawce to twardy błąd grafta z komunikatem. Kreator w kroku „join" dostaje ostrzeżenie, że lokalne metadane zostaną skasowane, z checkboxem potwierdzenia. | Test: graft migawki bez `vault_config` zwraca `Err`; wizard nie pozwala kliknąć dalej bez zaznaczenia |
| **WP2.3** Z9-22 | `post_revoke_device` i `post_remove_member`: nieudana `rotate_for_revocation` przestaje być `warn!` — zwraca 500 z jasnym komunikatem, a `revoked_at` jest wycofywane w transakcji. Odwołanie albo się udaje w całości, albo wcale. | Test: wstrzyknięty błąd rotacji → 500 → `revoked_at` nadal `NULL` |

---

## §5 FAZA 3 — Integralność danych

**Uwaga:** ta faza dotyka ścieżek, które mogą stracić dane użytkownika. Każdy pakiet zaczyna się od sondy na **kopii** bazy roboczej i kończy testem odtwarzającym scenariusz utraty.

| Pakiet | Zakres | Kryterium ukończenia |
| --- | --- | --- |
| **WP3.1** Z3-01, Z3-02, Z2-03 | `rotate_vault_key` w jednej transakcji (nowy EVK + re-wrap wszystkich DEK-ów + `vault_config`); uszkodzony `encrypted_vault_key` (zła długość) daje błąd zamiast cichego wygenerowania nowego; `backfill_uuid_user_ids` przywraca `FK = ON` na tym samym połączeniu przed zwrotem do puli. | Test: przerwanie rotacji w połowie zostawia bazę odszyfrowywalną starym hasłem |
| **WP3.2** Z11-05, Z6-08, Z2-01, Z2-05 | `purge_trash` kolejkuje obiekty do skasowania w chmurze przed usunięciem metadanych; ujednolicenie dwóch definicji sieroty; soft-delete przestaje blokować odtworzenie pliku w podkatalogu; projekcja pojedynczego inode'a respektuje `deleted_at`. | Sonda: po `purge` nie ma obiektu w mocku S3; test odtworzenia pliku w podkatalogu zielony |
| **WP3.3** Z6-03, Z6-07 | `get_next_shards_for_scrub` pomija shardy w stanie `PENDING`/`IN_PROGRESS`; osłona `status != 'UPLOADING'` obejmuje gałąź `LocalOnly`, a `pack_locations` dostaje FK do `packs`. | Sonda na bazie roboczej: świeży `PENDING` nie trafia do kolejki scrubbera |
| **WP3.4** Z5-01 | Klucz cache przestaje zawierać `:` — zamiana na kodowanie, które nie tworzy alternatywnych strumieni NTFS, plus migracja istniejących wpisów. | Sonda NTFS: `dir /r` w katalogu cache nie pokazuje strumieni |
| **WP3.5** Z4-06 | `ingest.rs:391` ocenia pack regułami wynikającymi z jego polityki, nie zawsze `EC_2_1`. | Test: plik z polityką `STANDARD` przechodzi Inbox upload do `COMPLETED` |
| **WP3.6** Z10-02 | `omnidrive recovery restore` pisze do pliku obok bazy i wymaga jawnego `--apply`, a przy `--apply` woła `graft_restored_metadata_snapshot`, nie surowe `fs::write`. Odmawia działania, gdy daemon nasłuchuje na porcie API. | Test: bez `--apply` baza nietknięta; z `--apply` przy żywym daemonie komenda kończy się błędem |
| **WP3.7** Z9-17 | Po `recovery/restore` kasowane są wszystkie sesje i poświadczenie DPAPI. | Test: token sprzed odzyskania przestaje działać |
| **WP3.8** Z2-06 | FK na `shared_links(inode_id)` i `user_sessions(user_id)` — po Fazie 0 sesje są sprzątane, więc FK nie zablokuje grafta. | Migracja przechodzi na kopii bazy roboczej |

---

## §6 FAZA 4 — Chmura, koszt i workery

| Pakiet | Zakres | Kryterium ukończenia |
| --- | --- | --- |
| **WP4.1** Z1-02, Z4-09, Z6-15, Z8-18, Z6-12 | Wszystkie workery do `tokio::select!` w `main.rs`; pętle `run()` przestają kończyć się na pierwszym błędzie SQLite (log + backoff + `continue`); `run_pipe_server` z retry; `repair_pack` zapisuje wynik także przy `Healthy`. | Test: wstrzyknięty błąd SQLite nie zabija workera; śmierć workera podnosi status `Error` w diagnostyce |
| **WP4.2** Z4-07, Z4-08, Z4-11, Z6-01 | `mark_*_failed` inkrementuje `attempts`; `is_retryable()` przestaje uznawać 403/404 za przejściowe; jedna warstwa ponawiania zamiast trzech; wyłącznik chmury da się zresetować bez restartu daemona. | Sonda: pack zablokowany kwotą nie wraca częściej niż co backoff |
| **WP4.3** Z8-06, Z6-09, Z7-09, Z11-06 | `run_metadata_fetch_now` przekazuje `pool` do `cloud_guard`; DEEP verify losuje shard zamiast brać `batch_index == 0`; `CANCEL_FETCH_DATA` faktycznie anuluje pobranie; `/api/storage/cost` liczy backlog jednym zapytaniem zamiast N+1. | Sonda: dobowy egress scrubbera poniżej limitu |
| **WP4.4** Z8-07, Z6-14, Z6-11, Z6-13, Z7-18 | Sprzątanie porzuconych multipartów bez flagi; kasowanie pobranych shardów po naprawie; `request_checksum_calculation` we wszystkich klientach S3; `reset_in_progress_pack_shards` zawężony do własnych shardów; eksmisja cache'u podpięta. | Spool nie rośnie po serii napraw |
| **WP4.5** Z1-01, Z4-04, Z4-05 | Rotacja `angeld.log` po rozmiarze; watcher nie przepakowuje całego watch roota po restarcie; watcher startuje po onboardingu bez restartu. | Log nie przekracza limitu; restart nie tworzy nowych rewizji |

---

## §7 FAZA 5 — Funkcje, które nie działają

| Pakiet | Zakres | Kryterium ukończenia |
| --- | --- | --- |
| **WP5.1** Z11-01, Z9-09, Z9-11 | `ApiError` dla share dostaje pola kontraktowe (`requires_password`, `reason`) — albo klient przestaje ich oczekiwać i czyta `message`. Licznik pobrań liczy sesję pobrania, nie ostatni chunk. Token share przenosi się do nagłówka `x-share-token`. | Test e2e: link z hasłem → prompt → poprawne hasło → plik odszyfrowany |
| **WP5.2** Z9-23, Z11-14 | **Decyzja D3 (§9)**: albo daemon serwuje share po HTTPS z certyfikatem self-signed (i instalator dodaje go do magazynu zaufania), albo tryb LAN Share zostaje wycofany z dokumentacji i UI. Przy wariancie HTTPS — Service Worker zaczyna działać i znika buforowanie w RAM. | Link LAN otwiera się na drugim urządzeniu albo UI nie obiecuje, że to możliwe |
| **WP5.3** Z7-05, Z11-08, Z10-03, Z10-12, Z10-13 | `post_vault_lock` woła `lock_flow::force_lock_and_dismount`; teardown przestaje być detached — API czeka na wynik i raportuje błędy; tray i deinstalator wołają `POST /api/settings/restart-daemon` zamiast `taskkill /F`; restart czeka na zwolnienie portu. | Test: po `POST /api/vault/lock` żaden plik w sync roocie nie jest zhydratowany |
| **WP5.4** Z7-06 | Jedna definicja „bezczynny" dla UI i pętli tick; praca w Eksploratorze dotyka licznika. | Test: hydratacja pliku przez cfapi przesuwa `remaining_seconds` |
| **WP5.5** Z7-07 | Hydratacja bez dostawców zwraca błąd do `cldflt.sys`, nie `STATUS_SUCCESS` z zerem bajtów. | Test: odczyt pliku przy braku dostawców daje błąd I/O, nie pusty plik |
| **WP5.6** Z11-07, Z2-02, Z9-12 | `provider_connection_status` zwraca `FAILED` przy trwałej awarii; `/api/stats/overview` liczy pliki niezależnie od wielkości liter; `post_vault_join` przestaje połykać błędy i nie konsumuje zaproszenia przy niepowodzeniu. | Tray pokazuje ikonę błędu przy awarii dostawcy |

---

## §8 FAZA 6 — Bezpieczeństwo lokalne · FAZA 7 — Dług

**Faza 6** (do wykonania po β.a smoke, bo zmienia zachowanie na maszynie produkcyjnej):

| Pakiet | Zakres |
| --- | --- |
| **WP6.1** Z8-01 | Named Pipe: DACL zawężony do SID interaktywnego użytkownika; `GetNamedPipeClientProcessId` + weryfikacja, że klient to `explorer.exe` albo podpisany `omnidrive_shell_ext.dll`; retry przy zajętej nazwie. |
| **WP6.2** Z8-02 | Peer: `trusted = 1` wyłącznie po wyzwaniu podpisanym kluczem urządzenia z tabeli `devices`; ogłoszenie UDP przestaje nieść `vault_id` w jawnej postaci (skrót z solą). |
| **WP6.3** Z8-05, Z9-27, Z9-25 | Sekrety dostawców i token OAuth pieczętowane Vault Key zamiast DPAPI; `snapshot-local` ogranicza ścieżkę do katalogu runtime. |
| **WP6.4** Z7-02, Z7-03 | Prawdziwe Windows Hello przez `Windows.Security.Credentials.UI` zamiast samego DPAPI; bufor po `CryptUnprotectData` zerowany i zwalniany; hasło w `SecretString`. |
| **WP6.5** Z7-04, Z7-14 | Usunięcie ACE `Authenticated Users` z sync roota (po teście na żywo, §7.7 ARCHITECTURE.md); obserwator WTS reaguje na przełączenie użytkownika i rozłączenie RDP. |
| **WP6.6** Z9-05, Z11-10, Z11-09, Z11-11 | Tailwind, jdenticon i Inter serwowane lokalnie z binarki; CSP na `/`; token OAuth przenosi się z `localStorage` do pamięci; Service Worker z zasięgiem `/sw-download/`. |
| **WP6.7** Z8-08, Z8-09, Z8-16, Z10-10 | `secure_delete` na wszystkich ścieżkach sprzątania łącznie z sidecarami WAL; migawka roster-fetch do katalogu runtime zamiast `%TEMP%`; szyfrowanie lokalnych kopii `.bak`; rotacja logu rozszerzenia powłoki. |

**Faza 7** — 44 pozycje długu (WP7.1-WP7.6 z §1). Do zrobienia hurtem, jednym przebiegiem po każdym module, bez osobnego planu: usunięcie martwego kodu i `#![allow(dead_code)]`, deduplikacja, zamiana `contains()` na typowane błędy, drobiazgi wydajnościowe i uzgodnienie dokumentacji z kodem.

---

## §9 Decyzje przed startem

Te cztery rzeczy muszą zapaść, zanim odpowiednie fazy ruszą. Nie zgaduję ich.

- **D1 (przed F1/WP1.2) — który klient powłoki zostaje?** DLL (`omnidrive-shell-ext`) jest lepszy technicznie: sześć pozycji, `catch_unwind` wszędzie, komunikacja przez pipe. Wariant rejestrowy (`shell_integration.rs`) jest zainstalowany, ale nie działa (Z7-01). **Rekomendacja: zostaje DLL**, instalator go wgrywa i rejestruje, `shell_integration.rs` znika. Koszt: rejestracja wymaga uprawnień administratora przy instalacji.
- **D2 (przed F2/WP2.1) — jak wyłączyć FK w graftcie?** Wariant A: przenieść `PRAGMA foreign_keys = OFF` przed `BEGIN` (działa, ale wyłącza FK dla całego połączenia na czas grafta). Wariant B: dopisać `DELETE FROM user_sessions` i `DELETE FROM invite_codes` do listy kasowanych tabel i zostawić FK włączone. **Rekomendacja: wariant B** — graft i tak kasuje tożsamości, a włączone FK złapią kolejny taki błąd.
- **D3 (przed F5/WP5.2) — LAN Share: HTTPS czy wycofanie?** HTTPS z self-signed wymaga dodania certyfikatu do magazynu zaufania na każdym urządzeniu odbiorcy — to psuje obietnicę „wyślij link i gotowe". **Rekomendacja: wycofać tryb A z UI i dokumentacji**, zostawić tryb B (publiczny link, deszyfrowanie po stronie odbiorcy z GH Pages), i usunąć `OMNIDRIVE_SHARE_HOST` z opisu jako „for LAN sharing".
- **D4 (przed F6) — kiedy Faza 6?** Zmienia zachowanie na maszynie produkcyjnej (ACL sync roota, DPAPI → Vault Key, pipe). **Rekomendacja: po smoke'u β.a na Dellu**, żeby nie mieszać dwóch źródeł zmian w jednej weryfikacji.

---

## §10 Kolejność, wersje i smoke

```
F0 (1-2 sesje)  → bump 0.3.30 → cargo test --workspace
F1 (1 sesja)    → bump 0.3.31
F2 (1 sesja)    → bump 0.3.32 → instalator → SMOKE β.a na Dellu   ← kamień milowy
F3 (2 sesje)    → bump 0.4.0-rc1
F4 (2 sesje)    → bump 0.4.0-rc2
F5 (2 sesje)    → bump 0.4.0-rc3 → SMOKE pełny
F6 (2 sesje)    → bump 0.4.0
F7 (1 sesja)    → bump 0.4.1
```

Po każdej fazie: `cargo build --release --workspace`, kopiowanie binarek do `dist/installer/payload/`, podbicie wersji we **wszystkich** `Cargo.toml`, odhaczenie znalezisk w `docs/ARCHITECTURE.md`.

**Smoke β.a jest po F2, nie wcześniej** — przed naprawą Z8-03 i Z8-04 dołączenie Della do Skarbca albo się nie uda, albo uda się i pliki będą nieodszyfrowywalne. Uruchamianie go teraz spaliłoby sesję na diagnozowaniu czegoś, co już jest zdiagnozowane.

---

## §11 Self-review

**Pokrycie:** wszystkie 150 identyfikatorów z `docs/ARCHITECTURE.md` mają przypisanie — 4 jako już naprawione, 4 jako WON'T FIX z uzasadnieniem, 142 rozdzielone na 38 pakietów roboczych w siedmiu fazach. Sprawdzone skryptem porównującym zbiór ID w planie ze zbiorem ID w rejestrze; pierwsze przejście wykazało trzy braki (Z1-07, Z2-08, Z3-07 — pozycje informacyjne `ℹ️` spoza tabeli rejestru), dopisane do WP7.1 i WP7.2.

**Placeholdery:** Faza 0 nie zawiera „TBD", „dodaj obsługę błędów" ani „podobnie jak zadanie N" — każdy krok ma kod albo dokładną komendę. Fazy 1-7 są świadomie na poziomie pakietów roboczych, a nie kroków; §„Jak czytać ten plan" mówi to wprost i wskazuje, że każda faza dostanie własny szczegółowy plan.

**Spójność typów:** `Expect` (Task 1) używane w Task 5, 7, 8, 9, 11. `require_local_intent` (Task 9) zwraca `Result<(), ApiError>` i jest wołane jako `?`. `WorkerStatus::NotStarted` (Task 12) ma `as_str() == "not_started"` i tego samego łańcucha używa test w Step 1. `RecoveryRateLimiter` (Task 11) to istniejący typ z `api/mod.rs:45` — nie tworzę nowego.

**Znane ryzyko planu:** Task 2 zmienia zachowanie UI (dashboard zacznie pytać o hasło przy otwartym Skarbcu). Jeśli to okaże się nie do przyjęcia, alternatywą jest token bootstrapowy zapisywany do pliku w katalogu runtime z ACL tylko dla użytkownika i odczytywany przez tray, który podaje go dashboardowi — ale to większa zmiana i nie należy do Fazy 0.
