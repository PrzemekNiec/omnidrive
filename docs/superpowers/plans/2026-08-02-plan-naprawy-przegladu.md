# Plan naprawy znalezisk przeglądu 2026-08 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Zamknąć 143 otwarte znaleziska z przeglądu kodu (`docs/ARCHITECTURE.md`) — 142 naprawą, jedno (Z4-13) świadomą decyzją o pozostawieniu zachowania — zaczynając od przywrócenia uwierzytelnienia API, bez którego żadna inna naprawa nie ma oparcia.

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
- **Testy integracyjne wymagają `--features test-helpers`.** `cargo test` **nie** włącza features niedomyślnych, a `angeld/Cargo.toml:78` ma `test-helpers = []` poza `default`. Router już dziś warunkuje `auto_lock::test_routes()` tym feature'em (`api/mod.rs:279-282`). Każda komenda `Run:` w tym planie ma tę flagę — pominięcie jej daje zielone testy, które nie zbudowały tego, co miały sprawdzić.
- **axum 0.8**: parametry tras zapisujemy `{nazwa}`, nie `:nazwa`.
- Stan wyjściowy: `v0.3.29`, HEAD `d9c4057`, 210 testów lib + 18 integracyjnych zielonych.

## Fakty zweryfikowane w kodzie przed pisaniem tego planu

Poprzednia wersja planu zakładała kilkanaście sygnatur bez sprawdzenia. Sprawdzone i **potwierdzone** (można na nich budować bez ponownego oglądania):

`ApiError::{BadRequest{code:&'static str, message:String}, Unauthorized{message}, Forbidden{message}, TooManyRequests{retry_after_secs:u64, message}, Internal{message}}` (`api_error.rs:7-46`) · `AuthorizedCaller{user_id, device_id, vault_id, role}` (`acl.rs:46`) · `verify_passphrase(&self, pool, &str) -> Result<bool, VaultError>` (`vault.rs:514`) · `rotate_vault_key(&self, pool, &str) -> Result<RotationResult, VaultError>` (`vault.rs:546`) · `try_auto_wrap_vault_key(state, target_device_id, target_public_key: &[u8], vault_id) -> Option<(String,i64,String)>` (`api/vault.rs:685`) · `db::get_device(pool, &str) -> Result<Option<DeviceRecord>, sqlx::Error>` (`db/devices.rs:47`) · `db::create_user` (6 argumentów, `db/users.rs:35`) · `db::insert_audit_log` (8 argumentów, `db/audit.rs:21`) · `db::count_vault_members` (`db/users.rs:141`) · `db::get/set_system_config_value` (`db/system_config.rs:39,69`) · `SYSTEM_CONFIG_ONBOARDING_STATE` + `OnboardingState::Completed.as_str() == "COMPLETED"` (`onboarding.rs:29,50`) · `RecoveryRateLimiter::{check(ip)->Result<(),u64>, record_failure, record_success}` (`api/mod.rs:57-86`) · `secrecy 0.10` z feature `serde`, więc `SecretString: Deserialize` działa · serwer startuje przez `into_make_service_with_connect_info::<SocketAddr>()` (`api/mod.rs:324`), więc `ConnectInfo` jest dostępne · `db::cleanup_expired_sessions` używa `WHERE expires_at <= ?` (`db/sessions.rs:124`), więc sesja z TTL 0 **jest** kasowana.

Sprawdzone i **obalające** wcześniejsze założenia (stąd zmiany w zadaniach niżej):

1. `acl::require_role` bez nagłówka `Authorization` zwraca **401**, a nie 403 (`acl.rs:64` → `extract_session_or_401`). 403 pojawia się dopiero przy ważnym tokenie bez członkostwa lub bez roli.
2. Harness ma `spawn/unlock/post_json/get_json/post/get_raw/health/connect_db/shutdown` i typ `HttpResponse{status: u16, body: String}`, ale **nie ma** `http_request_raw` ani żadnego wariantu „bez tokenu" poza `get_raw`, które już teraz idzie bez tokenu (`tests/common/mod.rs:211`).
3. `http_post_raw` **zawsze** wysyła `Content-Type: application/json` i ciało (`mod.rs:293`). Handler z ekstraktorem `Json<T>` odrzuci puste/niepełne ciało kodem 415/422 **zanim** wykona się cokolwiek w ciele funkcji — więc bramka wpisana jako „pierwsza instrukcja handlera" nigdy nie zdąży zwrócić 401. To wywraca konstrukcję testu macierzy i jest powodem, dla którego Zadanie 2 wprowadza ekstraktory.
4. Router składa się z `.merge()` modułowych `routes()`, bez `.nest()` — ścieżki w źródłach są absolutne (`api/mod.rs:284-314`), więc porównywanie ich z macierzą ma sens. Ale część wywołań `.route(` jest **wielolinijkowa** (`onboarding.rs:39,48,54`, `mod.rs:290`), więc skaner „po liniach" gubi m.in. `/api/onboarding/provider/{provider_name}` i `/api/providers/{provider_name}/test` — czyli dokładnie dwie trasy, które ta faza ma zamknąć.
5. `diagnostics.rs` ma **9** tras, nie 12 jak twierdzi Z9-06 (`diagnostics.rs:145-153`). Rejestr do poprawienia.
6. Tray odpytuje trzy endpointy: `/api/vault/status`, `/api/health` i **`/api/ingest`** (`omnidrive-tray/src/main.rs:118,139,153`). Zamknięcie `/api/ingest` rolą (Z9-26) psuje wykrywanie nieudanych zadań w trayu — patrz Zadanie 6.
7. `get_hello_available` nie przyjmuje `State` (`api/auth.rs:404`), więc nie ma jak odczytać flagi z bazy; a `windows_hello` używa stałej nazwy poświadczenia `"OmniDrive/VaultPassphrase"` (`windows_hello.rs:24`), wspólnej dla wszystkich instancji na koncie użytkownika — test uruchomiony na Lenovo czytałby i kasował **prawdziwe** poświadczenie Przemka.
8. `RecoveryRateLimiter` blokuje na **30 s po pierwszej** nieudanej próbie (`api/mod.rs:63-68`). Wpięcie go bez zmian pod `/api/unlock` znaczy: jedna literówka w haśle = pół minuty czekania.

---

## Jak czytać ten plan

Przegląd dał **147 pozycji w rejestrze, z czego 4 są już naprawione** (Z4-01, Z6-04, Z6-05, Z6-06), plus **3 pozycje informacyjne `ℹ️`** opisane w rozdziałach 1-3, których rejestr nie obejmuje (Z1-07, Z2-08, Z3-07). Razem **146 rzeczy do rozstrzygnięcia**. Rozpisanie wszystkich od razu w granulacji „krok = 2-5 minut" dałoby dokument na kilka tysięcy linii, który zdezaktualizowałby się przy dwudziestym zadaniu — bo naprawa Fazy 0 zmienia warunki dla Faz 3-7.

Dlatego:

- **§1 Triaż** przypisuje **każdą** ze 146 pozycji do fazy i pakietu roboczego. Nic nie ginie.
- **§2 Faza 0** jest rozpisana w pełnej granulacji TDD — to jest to, co wykonuje się następne.
- **§3-§8** to specyfikacje pakietów roboczych dla Faz 1-7: zakres, pliki, kryterium ukończenia, ryzyko. Każda faza dostaje własny szczegółowy plan (`docs/superpowers/plans/`) w momencie, gdy do niej dochodzimy — pisany już na kodzie po poprzednich fazach.
- **§9** to decyzje, które muszą zapaść przed startem odpowiednich faz. Nie zgaduję ich za Przemka.

Jedna pozycja (Z4-13) zostaje **WON'T FIX** co do zachowania — lepiej mieć ją zamkniętą świadomie niż wiszącą. Trzy pozostałe, które poprzednia wersja planu zamykała tym samym stemplem, wracają do faz: uzasadnienia okazały się błędne przy sprawdzeniu (§1).

---

## §1 Triaż — wszystkie 143 otwarte znaleziska

Legenda faz: **F0** mur auth · **F1** klienci · **F2** cross-device · **F3** integralność danych · **F4** chmura i workery · **F5** zepsute funkcje · **F6** bezpieczeństwo lokalne · **F7** dług

| Faza | Pakiet | Znaleziska |
| --- | --- | --- |
| **F0** | WP0.1 Test macierzy uwierzytelnienia | Z10-14 |
| **F0** | WP0.2 Koniec z mintowaniem sesji | Z9-01, Z10-05, Z2-04 |
| **F0** | WP0.3 Bramki na endpointach bez kontroli | Z9-06, Z9-07, Z9-08, Z9-19, Z9-20, Z9-26, Z9-30, Z11-02, Z11-12 |
| **F0** | WP0.4 Wzmocnienie bramek istniejących | Z9-13, Z9-21, Z9-03, Z9-28 |
| **F0** | WP0.5 Anty-CSRF i limitery | Z9-02, Z9-04, Z9-10 |
| **F0** | WP0.6 Tryb testowy poza binarką produkcyjną | Z11-04 |
| **F1** | WP1.1 CLI z uwierzytelnieniem | Z10-01, Z10-04 |
| **F1** | WP1.2 Rozstrzygnięcie klientów powłoki | Z7-01, Z10-06, Z10-09 |
| **F1** | WP1.3 Usunięcie `legacy.html` | Z11-03, Z9-15 |
| **F1** | WP1.4 Sesja bez członkostwa | Z9-24 |
| **F1** | WP1.5 Tray z tożsamością | (nowe, bez ID w rejestrze) |
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
| **F4** | WP4.2 Ponawianie, kwoty i transport | Z4-07, Z4-08, Z4-11, Z6-01, Z4-12, Z4-13(sygnał), Z4-10(część) |
| **F4** | WP4.3 Egress pod kontrolą | Z8-06, Z6-09, Z7-09, Z11-06 |
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
| **F7** | WP7.4 Wydajność i drobiazgi | Z4-02, Z2-07, Z8-11, Z8-15, Z9-14, Z9-16, Z10-11, Z10-15 |
| **F7** | WP7.5 Dokumentacja vs kod | Z3-06, Z7-15, Z7-12, Z1-05, Z4-03, Z9-18 |
| **F7** | WP7.6 Windows drobne | Z7-08, Z7-10, Z7-16, Z7-17, Z1-04, Z3-03 |
| — | **WON'T FIX** | Z4-03, Z4-10, Z4-12, Z4-13 |

**WON'T FIX — poprzednie uzasadnienia były błędne, oto poprawione.** Żadna z tych czterech pozycji nie broni się w pierwotnej wersji; dwie zmieniają status na „do zrobienia".

- **Z4-03 — NIE jest WON'T FIX, wchodzi do F7/WP7.5.** Poprzednie uzasadnienie („świadomy wybór schematu EC 2+1, zmiana wymagałaby przeprojektowania erasure codingu") odpowiada na pytanie, którego znalezisko nie zadaje. Nikt nie kwestionuje 2+1. Problem jest taki, że `SHARD_PROVIDERS` to **stała kompilacji z nazwami dostawców**, a dostawcy są konfigurowalni w bazie (`ARCHITECTURE.md` §6.2 mówi to wprost). Zamiana `const [&str; 3]` na deterministycznie uporządkowany odczyt z `provider_configs` nie dotyka kodera. §6.2 podaje też koszt bieżący: to jest powód, dla którego zaległość „Scaleway IAM" blokuje więcej, niż wygląda. Zakres: sam wybór nazw, bez zmiany arności EC.
- **Z4-10 — częściowo WON'T FIX, reszta do F7/WP7.5.** Teza „`from_env` istnieje wyłącznie dla dev-boxa" jest sprzeczna z §4b.5: to jest ścieżka, której używa `--no-onboarding`. Po naprawie CLI w WP1.1 (Z10-04) zostaje właśnie ona. Do zrobienia zostaje jedna rzecz: `OMNIDRIVE_ALLOW_EMPTY_UPLOADERS` ma dawać **tylu uploaderów, ilu skonfigurowano**, a nie zero. Wymóg kompletu trzech w `from_env` zostaje świadomie (to jest tryb dev).
- **Z4-12 — NIE jest WON'T FIX, wchodzi do F4/WP4.2.** Argument „użytkownik może dodać dowolny root do magazynu systemowego" opisuje przeciwnika, który na własnej maszynie może po prostu podmienić binarkę — nic nie chroni. Realny koszt jest odwrotny: `webpki-roots` to **wkompilowana migawka**, która się starzeje. Rotacja CA u dowolnego z trzech dostawców albo firmowy proxy TLS wywraca upload w sposób nienaprawialny konfiguracją, u wszystkich instalacji naraz, do czasu wydania nowej wersji. Kierunek: korzenie systemowe jako domyślne, `webpki` jako fallback, plus jeden `warn!` przy rozjeździe.
- **Z4-13 — zostaje WON'T FIX co do zachowania, ale sygnał jest obowiązkowy (F4/WP4.2).** Poprzednie uzasadnienie („po F0 endpoint ustawia tylko uwierzytelniony administrator, literówka to jego decyzja") pomija to, co znalezisko faktycznie mówi: degradacja do HTTP następuje **bez ostrzeżenia i bez wpisu w logu** (§4b.5). Administrator nie decyduje o czymś, czego nie widzi. Zostawiamy automatyczne wyprowadzanie `allow_http` z prefiksu, ale dokładamy `warn!` przy starcie klienta i widoczny znacznik „transport nieszyfrowany" w statusie dostawcy. To dwie linie, nie zmiana architektury.

**Korekty do wniesienia w `docs/ARCHITECTURE.md`** (rejestr, nie kod — robione razem z Zadaniem 15):

| Pozycja | Korekta |
| --- | --- |
| Z9-06 | „12 handlerów" → **9** (`diagnostics.rs:145-153`). |
| Z11-06 | Zawęzić do samego N+1. Połowa „bez bramki" jest w całości zawarta w Z9-06, bo `/api/storage/cost` mieszka w `diagnostics.rs`. Stąd jedno ID nie może stać w dwóch fazach. |
| Z10-05 | Nie jest osobnym znaleziskiem — to następstwo Z9-01. Po Zadaniu 4 znika bez własnej naprawy. Zejść do przypisu pod Z9-01. |
| Z9-15 | Scalić z Z11-03 (ten sam plik, jedna akcja: usunięcie w WP1.3). |
| Z11-13 | Scalić z Z10-08 (ten sam artefakt; `required-features` zamyka oba). |
| Z1-07, Z8-14 | Scalić z Z6-02 jako lista miejsc wywołania jednej wady („konfiguracja czytana z env na gorącej ścieżce"). |
| Z1-02, Z4-09, Z6-15, Z8-18 | Przeciąć inaczej: to są **dwie** wady (`?` w ciele pętli zabija workera; worker poza nadzorem), rozsypane na cztery moduły. WP4.1 i tak traktuje je łącznie. |
| Wagi | Do decyzji **D5** niżej. |

---

## §2 FAZA 0 — Mur uwierzytelnienia

**Dlaczego pierwsza:** `Z9-01` sprawia, że każde `require_role` w projekcie jest dekoracją. Dopóki to stoi, naprawianie czegokolwiek innego w API jest budowaniem na piasku — a naprawy z Faz 3-6 będą polegać na tym, że role działają.

**Kryterium ukończenia fazy:** test `e2e_auth_matrix` zielony; żadne żądanie bez tokenu nie zmienia stanu ani nie zwraca danych Skarbca poza jawnie wymienioną listą publiczną; `cargo test --workspace --features test-helpers` zielony.

**Uwaga o kolejności:** test macierzy (Zadanie 3) idzie **przed** naprawami, żeby zobaczyć go na czerwono na liście realnych dziur. Poprzedzają go dwa zadania czysto techniczne, bez których ten test nie ma jak działać: harness nie umie wysłać żądania bez tokenu (Zadanie 1), a bramka wpisana w ciało handlera nigdy nie zdąży odpowiedzieć 401 (Zadanie 2).

**Decyzja architektoniczna tej fazy: bramka jest ekstraktorem, nie pierwszą linijką handlera.** W axumie wszystkie ekstraktory muszą się powieść, zanim uruchomi się ciało funkcji. Handler z `Json<T>` odrzuci żądanie bez ciała kodem 415/422 i do `acl::require_role` nigdy nie dojdzie. Żądanie bez uwierzytelnienia dostawałoby więc „422 Unprocessable Entity" zamiast „401", a test macierzy raportowałby każdy endpoint POST jako dziurę — na zawsze, także po naprawie. `AuthorizedCaller` jako `FromRequestParts` rozwiązuje to u źródła: bramka biegnie na nagłówkach, przed dotknięciem ciała. Efekt uboczny jest korzystny — z handlerów znika powtarzana linijka `acl::require_role(&state.pool, &headers, …)`, a rola staje się widoczna w sygnaturze.

### Task 1: Harness umie wysłać żądanie bez tokenu (przygotowanie)

**Files:**
- Modify: `angeld/tests/common/mod.rs`

**Interfaces:**
- Consumes: `parse_http_url`, `HttpResponse{status: u16, body: String}` (istnieją)
- Produces: `http_request_raw(method, url, body: Option<&Value>, token: Option<&str>)` oraz metody `DaemonHarness::{request_without_token, request_with_token}`

Powód: w harnessie nie ma `http_request_raw` (plan poprzednio go zakładał), nie ma żadnego wariantu DELETE, a `http_post_raw` **zawsze** dokleja `Content-Type` i ciało — czyli nie da się nim odtworzyć żądania, które przychodzi z przeglądarki albo z `curl`a bez ciała. Bez tego macierz testuje coś innego, niż deklaruje.

- [ ] **Step 1: Dopisz uogólnioną funkcję i przepnij na nią istniejące dwie**

```rust
// angeld/tests/common/mod.rs
#[allow(dead_code)]
pub async fn http_request_raw(
    method: &str,
    url: &str,
    body: Option<&serde_json::Value>,
    token: Option<&str>,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    let (host_port, path) = parse_http_url(url)?;
    let auth = match token {
        Some(t) => format!("Authorization: Bearer {t}\r\n"),
        None => String::new(),
    };
    let body_text = body.map(|b| b.to_string());
    let framing = match &body_text {
        Some(text) => format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            text.len()
        ),
        None => String::new(),
    };

    let mut stream = TcpStream::connect(host_port.as_str()).await?;
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host_port}\r\n{auth}Connection: close\r\n{framing}\r\n{}",
        body_text.unwrap_or_default()
    );
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let response_str = String::from_utf8(response)?;
    let status: u16 = response_str
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1).map(str::to_string))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no status code"))?
        .parse()?;
    let body = response_str
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();

    Ok(HttpResponse { status, body })
}
```

`http_get_raw` i `http_post_raw` stają się jednolinijkowymi opakowaniami na powyższą (`http_request_raw("GET", url, None, token)` / `("POST", url, Some(body), token)`). To nie jest refaktor przy okazji — bez tego mamy trzecią kopię tego samego parsera odpowiedzi.

- [ ] **Step 2: Dwie metody na harnessie, jedna konwencja argumentów**

```rust
    #[allow(dead_code)]
    pub async fn request_without_token(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        http_request_raw(method, &format!("{}{}", self.base_url, path), body, None).await
    }

    #[allow(dead_code)]
    pub async fn request_with_token(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        http_request_raw(
            method,
            &format!("{}{}", self.base_url, path),
            body,
            self.session_token.as_deref(),
        )
        .await
    }
```

Wszystkie testy w tej fazie używają wyłącznie tych dwóch metod plus istniejących `post_json` / `get_json`. Nie dopisujemy `post_json_without_token` ani `get_raw_authed` — jedna konwencja (`&Value` przez `Option`) zamiast czterech wariantów o różnym przekazywaniu ciała.

- [ ] **Step 3: Sprawdź, że nic nie zgasło**

Run: `cargo test --workspace --features test-helpers`
Expected: PASS (zmiana jest czysto addytywna plus przepięcie dwóch funkcji na wspólną implementację).

- [ ] **Step 4: Commit**

```bash
git add angeld/tests/common/mod.rs
git commit -m "test(common): jedno wejscie http_request_raw dla dowolnej metody i tokenu"
```

### Task 2: Bramka roli jako ekstraktor (fundament WP0.3 i WP0.4)

**Files:**
- Create: `angeld/src/api/gate.rs`
- Modify: `angeld/src/api/mod.rs` (rejestracja modułu)

**Interfaces:**
- Consumes: `acl::{require_role, require_session, AuthorizedCaller, Role}` (bez zmian w `acl.rs`)
- Produces: `ViewerCaller`, `MemberCaller`, `AdminCaller`, `SessionCaller` — ekstraktory `FromRequestParts<ApiState>` zwracające `ApiError` jako rejekcję

Moduł mieszka **wewnątrz** `api/`, a nie w `acl.rs`, bo `ApiState` nie jest publiczne poza drzewem `api` — implementacja `FromRequestParts<ApiState>` w `crate::acl` nie miałaby jak nazwać typu stanu.

- [ ] **Step 1: Napisz ekstraktory**

```rust
// angeld/src/api/gate.rs
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::acl::{self, AuthorizedCaller, Role};
use crate::db::UserSession;

use super::ApiState;
use super::error::ApiError;

pub(super) struct ViewerCaller(pub AuthorizedCaller);
pub(super) struct MemberCaller(pub AuthorizedCaller);
pub(super) struct AdminCaller(pub AuthorizedCaller);
pub(super) struct SessionCaller(pub UserSession);

macro_rules! role_gate {
    ($ty:ident, $role:expr) => {
        impl FromRequestParts<ApiState> for $ty {
            type Rejection = ApiError;

            async fn from_request_parts(
                parts: &mut Parts,
                state: &ApiState,
            ) -> Result<Self, Self::Rejection> {
                acl::require_role(&state.pool, &parts.headers, $role)
                    .await
                    .map($ty)
            }
        }
    };
}

role_gate!(ViewerCaller, Role::Viewer);
role_gate!(MemberCaller, Role::Member);
role_gate!(AdminCaller, Role::Admin);

impl FromRequestParts<ApiState> for SessionCaller {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ApiState,
    ) -> Result<Self, Self::Rejection> {
        acl::require_session(&state.pool, &parts.headers)
            .await
            .map(Self)
    }
}
```

W `angeld/src/api/mod.rs` dopisz `mod gate;`.

Uwagi wykonawcze: axum 0.8 używa natywnego `async fn` w traicie, więc **nie** dodawaj `#[async_trait]`. Jeśli kompilator zgłosi „field is never read" dla któregoś z typów, znaczy to, że żaden handler nie potrzebuje `AuthorizedCaller` przy tej roli — wtedy zamień ten typ na strukturę bez pola, nie tłum ostrzeżenia atrybutem.

- [ ] **Step 2: Test jednostkowy kolejności — bramka przed ciałem**

```rust
// angeld/src/api/gate.rs — mod tests
#[tokio::test]
async fn missing_authorization_header_is_rejected_before_the_body_is_parsed() {
    let pool = crate::db::init_db("sqlite::memory:").await.expect("db");
    let headers = axum::http::HeaderMap::new();
    let err = acl::require_role(&pool, &headers, Role::Viewer)
        .await
        .expect_err("brak naglowka musi byc odrzucony");
    assert!(
        matches!(err, ApiError::Unauthorized { .. }),
        "brak tokenu to 401, nie 403 ani 400"
    );
}
```

- [ ] **Step 3: Uruchom**

Run: `cargo test -p angeld --features test-helpers missing_authorization_header`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add angeld/src/api/gate.rs angeld/src/api/mod.rs
git commit -m "feat(api): bramka roli jako ekstraktor FromRequestParts"
```

### Task 3: Test macierzy uwierzytelnienia (WP0.1, zamyka Z10-14)

**Files:**
- Create: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Consumes: `common::DaemonHarness` (`spawn`, `unlock`, `request_without_token` z Zadania 1)
- Produces: `AUTH_MATRIX: &[(&str, &str, &str, Expect)]` — metoda, trasa **jak zadeklarowana w kodzie**, ścieżka **do wywołania**, oczekiwana bramka

Cztery elementy zamiast trzech, bo poprzednia wersja miała trzy wady naraz: porównywała same ścieżki (więc nowy `POST` na istniejącej ścieżce wchodził bez decyzji), gubiła wielolinijkowe `.route(` (czyli m.in. obie trasy dostawców, które ta faza ma zamknąć) i wysyłała `/api/files/{inode_id}/pin` dosłownie, z klamrami, co daje 400 z `Path<i64>` zamiast 401.

- [ ] **Step 1: Skaner tras, który widzi wywołania wielolinijkowe i metodę HTTP**

```rust
// angeld/tests/e2e_auth_matrix.rs
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Expect {
    /// Celowo publiczne — lista i uzasadnienie w §9.5 ARCHITECTURE.md.
    Public,
    /// Publiczne, ale wymaga nagłówka X-OmniDrive-Local (anty-CSRF, Zadanie 11).
    LocalIntent,
    /// Wymaga dowolnej ważnej sesji.
    Session,
    /// Wymaga sesji z rolą w vaulcie.
    Role,
}

const AUTH_MATRIX: &[(&str, &str, &str, Expect)] = &[
    ("GET", "/api/vault/status", "/api/vault/status", Expect::Public),
    ("POST", "/api/unlock", "/api/unlock", Expect::Public),
    ("GET", "/api/unlock/hello-available", "/api/unlock/hello-available", Expect::Public),
    ("POST", "/api/vault/join", "/api/vault/join", Expect::Public),
    ("POST", "/api/recovery/restore", "/api/recovery/restore", Expect::Public),
    ("GET", "/api/recovery/status", "/api/recovery/status", Expect::Public),
    ("GET", "/api/onboarding/status", "/api/onboarding/status", Expect::Public),
    ("GET", "/api/health", "/api/health", Expect::Public),
    ("GET", "/api/diagnostics/health", "/api/diagnostics/health", Expect::Public),
    ("POST", "/api/unlock/windows-hello", "/api/unlock/windows-hello", Expect::LocalIntent),
    ("GET", "/api/transfers", "/api/transfers", Expect::Role),
    ("GET", "/api/diagnostics", "/api/diagnostics", Expect::Role),
    ("GET", "/api/diagnostics/shell", "/api/diagnostics/shell", Expect::Role),
    ("GET", "/api/diagnostics/sync-root", "/api/diagnostics/sync-root", Expect::Role),
    ("GET", "/api/diagnostics/restore", "/api/diagnostics/restore", Expect::Role),
    ("GET", "/api/storage/cost", "/api/storage/cost", Expect::Role),
    ("GET", "/api/multidevice/status", "/api/multidevice/status", Expect::Role),
    ("GET", "/api/stats/overview", "/api/stats/overview", Expect::Role),
    ("GET", "/api/ingest", "/api/ingest", Expect::Role),
    ("POST", "/api/onboarding/setup-provider", "/api/onboarding/setup-provider", Expect::Role),
    ("POST", "/api/onboarding/complete", "/api/onboarding/complete", Expect::Role),
    ("POST", "/api/onboarding/reset", "/api/onboarding/reset", Expect::Role),
    (
        "DELETE",
        "/api/onboarding/provider/{provider_name}",
        "/api/onboarding/provider/backblaze-b2",
        Expect::Role,
    ),
    (
        "POST",
        "/api/providers/{provider_name}/test",
        "/api/providers/backblaze-b2/test",
        Expect::Role,
    ),
    ("POST", "/api/vault/add-device", "/api/vault/add-device", Expect::Role),
    ("POST", "/api/vault/rotate-key", "/api/vault/rotate-key", Expect::Role),
    ("POST", "/api/maintenance/repair-shell", "/api/maintenance/repair-shell", Expect::Role),
    ("POST", "/api/files/{inode_id}/pin", "/api/files/1/pin", Expect::Role),
];

/// Treść każdego wywołania `.route(...)`, także rozbitego na wiele linii.
fn route_calls(src: &str) -> Vec<String> {
    let bytes = src.as_bytes();
    let mut calls = Vec::new();
    let mut cursor = 0usize;

    while let Some(hit) = src[cursor..].find(".route(") {
        let start = cursor + hit + ".route(".len();
        let mut depth = 1usize;
        let mut in_string = false;
        let mut i = start;

        while i < bytes.len() && depth > 0 {
            let ch = bytes[i];
            let escaped = i > 0 && bytes[i - 1] == b'\\';
            match ch {
                b'"' if !escaped => in_string = !in_string,
                b'(' if !in_string => depth += 1,
                b')' if !in_string => depth -= 1,
                _ => {}
            }
            i += 1;
        }

        calls.push(src[start..i.saturating_sub(1)].to_string());
        cursor = i;
    }
    calls
}

fn first_string_literal(call: &str) -> Option<String> {
    let open = call.find('"')?;
    let rest = &call[open + 1..];
    let close = rest.find('"')?;
    Some(rest[..close].to_string())
}

fn routes_declared_in_source() -> BTreeSet<(String, String)> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/api");
    let mut found = BTreeSet::new();

    for entry in std::fs::read_dir(dir).expect("api dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read source");
        for call in route_calls(&src) {
            let Some(route) = first_string_literal(&call) else {
                continue;
            };
            for verb in ["get", "post", "put", "patch", "delete"] {
                if call.contains(&format!("{verb}(")) {
                    found.insert((verb.to_ascii_uppercase(), route.clone()));
                }
            }
        }
    }
    found
}

#[test]
fn every_declared_route_has_an_entry_in_the_matrix() {
    let declared = routes_declared_in_source();
    let covered: BTreeSet<(String, String)> = AUTH_MATRIX
        .iter()
        .map(|(method, declared, _, _)| ((*method).to_string(), (*declared).to_string()))
        .collect();

    let missing: Vec<_> = declared.difference(&covered).collect();
    assert!(
        missing.is_empty(),
        "trasy bez wpisu w AUTH_MATRIX (dopisz je razem z decyzja o bramce): {missing:?}"
    );

    let stale: Vec<_> = covered.difference(&declared).collect();
    assert!(
        stale.is_empty(),
        "wpisy w AUTH_MATRIX bez odpowiadajacej trasy w kodzie: {stale:?}"
    );
}
```

Skaner celowo szuka `.route(` w całym tekście pliku, a nie na początku linii — inaczej gubi `onboarding.rs:39,48,54` i `mod.rs:290`. Asercja `stale` pilnuje drugiego kierunku: wpis w macierzy po usuniętej trasie ma boleć, bo inaczej macierz cicho gnije.

- [ ] **Step 2: Uruchom i uzupełnij macierz**

Run: `cargo test --features test-helpers --test e2e_auth_matrix every_declared_route -- --nocapture`
Expected: FAIL — lista brakujących par (metoda, trasa). Uzupełniaj `AUTH_MATRIX` aż PASS. Każda nowa pozycja to **decyzja**, nie uzupełnienie tabelki: `Public` wolno nadać tylko trasie, która trafia na listę z §9.5 ARCHITECTURE.md. Trasy z parametrem dostają ścieżkę wywołania z realną wartością (`1`, `backblaze-b2`), nie z klamrą.

- [ ] **Step 3: Test na żywym daemonie**

```rust
mod common;
use common::DaemonHarness;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn routes_behind_a_gate_reject_requests_without_a_token()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;

    let mut offenders = Vec::new();
    for (method, _declared, sample, expect) in AUTH_MATRIX {
        let wanted = match expect {
            Expect::Public => continue,
            // Brak naglowka X-OmniDrive-Local => 403 (Zadanie 11).
            Expect::LocalIntent => 403,
            // acl::extract_session_or_401: brak naglowka Authorization => zawsze 401.
            Expect::Session | Expect::Role => 401,
        };

        let resp = h.request_without_token(method, sample, None).await?;
        if resp.status != wanted {
            offenders.push(format!(
                "{method} {sample} -> {} (oczekiwano {wanted}) {}",
                resp.status, resp.body
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "endpointy bez poprawnej bramki:\n{}",
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

Asercja jest na **dokładny** kod, nie na „401 albo 403". Po Zadaniu 2 bramka biegnie przed ciałem, a `extract_session_or_401` przy braku nagłówka zawsze daje 401 — więc 403 w tym teście oznacza, że handler przepuścił brak tokenu i wywrócił się dopiero na roli, a 400/415/422 oznacza, że bramka znów jest za ekstraktorem ciała. Rozróżnienie ma wartość diagnostyczną i dlatego nie zaokrąglamy go do „dowolny 4xx".

- [ ] **Step 4: Uruchom i zapisz listę czerwonych**

Run: `cargo test --features test-helpers --test e2e_auth_matrix -- --nocapture`
Expected: FAIL z listą naruszeń **oraz** FAIL na `vault_status_never_returns_a_session_token`. Ta lista jest zakresem Zadań 4-14 — zapisz ją, jest kryterium ukończenia fazy.

- [ ] **Step 5: Commit**

```bash
git add angeld/tests/e2e_auth_matrix.rs
git commit -m "test(api): macierz uwierzytelnienia endpointow (czerwona)"
```

### Task 4: `/api/vault/status` przestaje wystawiać token (WP0.2, zamyka Z9-01, Z10-05)

**Files:**
- Modify: `angeld/src/api/vault.rs:141-178`
- Modify: `angeld/static/index.html:4015-4033`
- Test: `angeld/tests/e2e_auth_matrix.rs` (test z Zadania 3, Step 3)

**Interfaces:**
- Consumes: `Expect::Public` dla `/api/vault/status` z Zadania 3
- Produces: `GET /api/vault/status` zwraca `{unlocked, initialized, members_count, multi_user}` — **bez** `session_token`

Konsekwencja dla UI: dashboard otwarty przy odblokowanym Skarbcu nie dostanie już sesji za darmo. Pokazuje ekran odblokowania i prosi o hasło. To jest zamierzone — konsola Skarbca ma wymagać hasła, a tray potrzebuje wyłącznie pola `unlocked` (`omnidrive-tray/src/main.rs:118`).

**Świadomie zostawiamy publiczne:** `members_count` i `multi_user`. To jest informacja o kształcie Skarbca, nie o jego zawartości, a ekran odblokowania używa jej do wyboru wariantu formularza. Jeśli D5 zdecyduje inaczej, wraca to jako osobne zadanie — nie chowamy tego pod „przy okazji".

- [ ] **Step 1: Uruchom istniejący test, żeby zobaczyć czerwony**

Run: `cargo test --features test-helpers --test e2e_auth_matrix vault_status_never_returns -- --nocapture`
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

Run: `cargo test --features test-helpers --test e2e_auth_matrix vault_status_never_returns -- --nocapture`
Expected: PASS

- [ ] **Step 4: Popraw dashboard, żeby prosił o hasło zamiast liczyć na token**

```javascript
// angeld/static/index.html — w bloku startowym, zamiast gałęzi `if (data && data.unlocked)`
          return fetch('/api/vault/status', { headers: { 'Accept': 'application/json' } })
            .then(r => r.ok ? r.json() : Promise.reject())
            .then(data => {
              VAULT_STATE.unlocked = Boolean(data && data.unlocked);
              showLockScreen();
              if (lsInp) setTimeout(() => lsInp.focus(), 100);
            });
```

Bez gałęzi `if (VAULT_STATE.sessionToken) startDashboard()`: token z `/api/unlock` żyje wyłącznie w pamięci karty (Z11-09), więc przy starcie strony jest zawsze pusty i ta gałąź byłaby martwym kodem udającym ścieżkę.

- [ ] **Step 5: Sprawdź ręcznie, że tray dalej działa**

Run: `cargo build --release --workspace`, potem `target/release/angeld.exe` i `target/release/omnidrive-tray.exe`.
Expected: ikona zasobnika przechodzi z `Locked` na `Synced` po odblokowaniu przez dashboard; sonda `SELECT COUNT(*) FROM user_sessions` na **kopii** bazy nie rośnie w czasie bezczynności (dziś rośnie o 20 wierszy na minutę — Z10-05).

- [ ] **Step 6: Commit**

```bash
git add angeld/src/api/vault.rs angeld/static/index.html
git commit -m "fix(api): /api/vault/status nie wystawia juz tokenu sesji (Z9-01)"
```

### Task 5: Sprzątanie wygasłych sesji (WP0.2, zamyka Z2-04)

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
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("sessions.db");
    let pool = crate::db::init_db(&format!("sqlite://{}", db_path.display())).await?;

    crate::db::create_user(&pool, "u-1", "U", None, "local", None).await?;
    create_user_session(&pool, "live", "u-1", "dev-a", SESSION_TTL_SECONDS).await?;
    create_user_session(&pool, "dead", "u-1", "dev-a", -60).await?;

    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_sessions")
        .fetch_one(&pool)
        .await?;
    assert_eq!(before, 2, "obie sesje musza istniec przed sprzataniem");

    let removed = cleanup_expired_sessions(&pool).await?;
    assert_eq!(removed, 1);

    let rows: Vec<String> = sqlx::query_scalar("SELECT token FROM user_sessions")
        .fetch_all(&pool)
        .await?;
    assert_eq!(rows, vec!["live".to_string()], "wygasly wiersz ma zniknac z tabeli");
    Ok(())
}
```

Dwie rzeczy, na których poprzednia wersja tego testu się wykładała:

**Nie `sqlite::memory:`.** `init_db` (`db/schema.rs:13`) tworzy pulę z `min_connections(1)` i **bez** `max_connections`, czyli z domyślnym limitem 10. Przy bazie w pamięci każde połączenie z puli dostaje własną, pustą bazę — `create_user` mógłby trafić w inne połączenie niż `cleanup_expired_sessions` i test wywala się bez żadnej wskazówki, co jest nie tak. Plik w `tempdir` znosi problem u źródła. Jeśli `tempfile` nie ma w `[dev-dependencies]`, dopisz je tam (jest już używane pośrednio przez harness testów e2e — sprawdź przed dodaniem).

**Asercja na tabeli, nie na `validate_user_session`.** Ta funkcja i tak filtruje po `expires_at`, więc zwróciłaby `None` dla wygasłej sesji także wtedy, gdy sprzątanie nic nie skasowało — czyli asercja przechodziłaby przy niedziałającej naprawie. Liczymy wiersze.

TTL `-60` zamiast `0` jest tu ostrożnością, nie koniecznością: `cleanup_expired_sessions` używa `WHERE expires_at <= ?` (`db/sessions.rs:124`), więc `0` też by zadziałało — ale wtedy test opierałby się na tym, że nikt nigdy nie zmieni `<=` na `<`.

- [ ] **Step 2: Uruchom test**

Run: `cargo test -p angeld --features test-helpers cleanup_removes_only_expired_sessions`
Expected: PASS.

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

Run: `cargo test --workspace --features test-helpers`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add angeld/src/main.rs angeld/src/db/sessions.rs
git commit -m "fix(db): sprzataj wygasle sesje co 5 minut (Z2-04)"
```

### Task 6: Bramki na diagnostyce, statystykach i ingescie (WP0.3, zamyka Z9-06, Z9-07, Z9-26)

**Files:**
- Modify: `angeld/src/api/diagnostics.rs` (7 z 9 handlerów)
- Modify: `angeld/src/api/stats.rs` (3 handlery)
- Modify: `angeld/src/api/maintenance.rs:764` (`get_ingest_jobs`)
- Modify: `angeld/src/api/diagnostics.rs` (`get_health` — nowe pole)
- Modify: `omnidrive-tray/src/main.rs:153`
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Consumes: `ViewerCaller` z Zadania 2
- Produces: handlery przyjmują `_: ViewerCaller` zamiast dodatkowego `headers: HeaderMap`; `/api/health` zyskuje pole `ingest_failed: bool`

`diagnostics.rs` ma **9** tras (`:145-153`), nie 12 jak mówi Z9-06. Dwie zostają publiczne: `/api/health` i `/api/diagnostics/health` — używa ich harness do wykrycia gotowości API i tray do stwierdzenia, czy daemon żyje.

**Pułapka, przez którą to zadanie psuje tray.** Tray odpytuje trzy endpointy: `/api/vault/status`, `/api/health` i **`/api/ingest`** (`omnidrive-tray/src/main.rs:118,139,153`). Ostatni służy wyłącznie do wykrycia zadań w stanie `FAILED`. Zamknięcie go rolą (a trzeba, bo oddaje pełne ścieżki plików użytkownika — Z9-26) odbiera trayowi tę informację, a tray nie ma żadnej tożsamości aż do F1/WP1.5. Rozwiązanie tutaj: przenieść **sam sygnał** (`bool`), bez ścieżek, do publicznego `/api/health`, które tray i tak woła.

- [ ] **Step 1: Uruchom test macierzy**

Run: `cargo test --features test-helpers --test e2e_auth_matrix routes_behind_a_gate -- --nocapture`
Expected: FAIL z listą zawierającą `/api/transfers`, `/api/diagnostics`, `/api/diagnostics/shell`, `/api/diagnostics/sync-root`, `/api/diagnostics/restore`, `/api/storage/cost`, `/api/multidevice/status`, `/api/stats/*`, `/api/ingest`.

- [ ] **Step 2: Dodaj bramkę do każdego z nich**

Wzorzec, powtórzony dla każdego handlera z listy (przykład na `get_transfers`):

```rust
async fn get_transfers(
    State(state): State<ApiState>,
    _: ViewerCaller,
) -> Result<Json<Vec<TransferResponse>>, ApiError> {
    let jobs = db::list_recent_upload_jobs(&state.pool, 50).await?;
```

Do zmiany, wszystkie na `ViewerCaller`: `get_transfers`, `get_diagnostics_overview`, `get_shell_state`, `get_sync_root_state`, `get_restore_state`, `get_storage_cost`, `get_multidevice_status` (`diagnostics.rs`); `get_stats_overview`, `get_stats_traffic`, `get_stats_system` (`stats.rs`); `get_ingest_jobs` (`maintenance.rs`).

`get_shell_state` i `get_sync_root_state` nie mają dziś `State<ApiState>` — nie trzeba go dodawać, ekstraktor sam sięga po stan; dopisz wyłącznie `_: ViewerCaller`.

W każdym z trzech plików dopisz import `use super::gate::ViewerCaller;` (i usuń `use crate::acl::{self, Role};` tam, gdzie po zmianie nie zostaje żaden inny użytkownik tych nazw).

Przy okazji **nie** ruszamy `/api/maintenance/retry-storms` ani `/api/maintenance/scrub-errors` (też bez kontroli wg §9b.7) — one wchodzą w Zadaniu 7 razem z resztą `maintenance.rs`, żeby jeden commit odpowiadał jednemu plikowi.

- [ ] **Step 3: Przenieś sygnał o nieudanym ingescie do `/api/health`**

```rust
// angeld/src/api/diagnostics.rs — w strukturze odpowiedzi get_health
    ingest_failed: bool,
```

Wartość: `db::count_ingest_jobs_in_state(&state.pool, "FAILED").await.unwrap_or(0) > 0`. Jeśli takiej funkcji nie ma, dopisz ją w `db/ingest.rs` wzorem sąsiednich zapytań zliczających — jedno `SELECT COUNT(*)`, bez zwracania ścieżek.

W `omnidrive-tray/src/main.rs` usuń wywołanie `/api/ingest` (linia 153 i struktura, którą deserializuje) i czytaj `ingest_failed` z odpowiedzi `/api/health`, którą tray już pobiera w linii 139.

- [ ] **Step 4: Uruchom test i sprawdź tray**

Run: `cargo test --features test-helpers --test e2e_auth_matrix routes_behind_a_gate -- --nocapture`
Expected: wymienione trasy znikają z listy naruszeń.

Run: `cargo build --release --workspace`, uruchom daemona i tray.
Expected: tray nadal pokazuje ikonę błędu, gdy w kolejce ingestu jest zadanie `FAILED`; `/api/ingest` bez tokenu zwraca 401.

- [ ] **Step 5: Popraw dashboard, żeby wysyłał token do tych paneli**

W `angeld/static/index.html` wszystkie wywołania powyższych endpointów muszą używać istniejącego helpera nagłówków (`vaultAuthHeaders()`), a nie gołego `{ Accept: 'application/json' }`. Znajdź je: `grep -n "api/transfers\|api/storage/cost\|api/stats/\|api/ingest\|api/multidevice\|api/diagnostics" angeld/static/index.html`.

- [ ] **Step 6: Uruchom pełne testy**

Run: `cargo test --workspace --features test-helpers`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add angeld/src/api/diagnostics.rs angeld/src/api/stats.rs angeld/src/api/maintenance.rs angeld/src/db/ingest.rs omnidrive-tray/src/main.rs angeld/static/index.html
git commit -m "fix(api): rola Viewer na diagnostyce, statystykach i ingescie; tray czyta sygnal z /api/health (Z9-06, Z9-07, Z9-26)"
```

### Task 7: Bramki na operacjach zmieniających stan (WP0.3, zamyka Z9-08, Z9-19, Z9-20, Z11-02, Z11-12)

**Files:**
- Modify: `angeld/src/api/onboarding.rs` (`post_setup_provider`, `post_complete_onboarding`, `post_reset_onboarding`, `delete_provider`, `post_test_provider`)
- Modify: `angeld/src/api/maintenance.rs:532` (`post_repair_shell`)
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Produces: `AdminAfterOnboarding` — ekstraktor w `api/gate.rs`; `post_repair_shell` i pozostałe operacje `maintenance.rs` bez kontroli dostają `AdminCaller`

**Pułapka do rozwiązania w tym zadaniu:** kreator onboardingu woła `setup-provider` i `complete` **zanim** istnieje jakakolwiek sesja. Rozwiązanie: wymagaj roli **tylko wtedy, gdy onboarding jest już zakończony**. W trakcie kreatora (`onboarding_state != COMPLETED`) endpointy zostają otwarte — Skarbiec nie ma jeszcze czego chronić.

To musi być **ekstraktor**, nie helper wołany w ciele handlera, z tego samego powodu co w Zadaniu 2: `post_setup_provider` przyjmuje `Json<SetupProviderRequest>`, więc żądanie z niepełnym ciałem dostałoby 422 przed jakąkolwiek kontrolą i test poniżej nigdy nie zobaczyłby 401.

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

    let during = h
        .request_without_token("POST", "/api/onboarding/setup-provider", Some(&body))
        .await?;
    assert_ne!(during.status, 401, "w trakcie kreatora endpoint musi byc otwarty");

    h.unlock().await?;
    h.post("/api/onboarding/complete").await?;

    let after = h
        .request_without_token("POST", "/api/onboarding/setup-provider", Some(&body))
        .await?;
    assert_eq!(
        after.status, 401,
        "po zakonczeniu onboardingu endpoint musi wymagac sesji; got {} body={}",
        after.status, after.body
    );
    Ok(())
}
```

Asercja „w trakcie" jest celowo `assert_ne!(401)`, a nie `assert_eq!(200)`: handler natychmiast wykonuje test połączenia z podanym endpointem (§9b.3), więc przy `127.0.0.1:1` odpowie błędem dostawcy. Sprawdzamy tu wyłącznie, że **bramka** go nie zatrzymała.

- [ ] **Step 2: Uruchom test**

Run: `cargo test --features test-helpers --test e2e_auth_matrix setup_provider_is_open -- --nocapture`
Expected: FAIL — po zakończeniu onboardingu status inny niż 401.

- [ ] **Step 3: Dodaj ekstraktor bramki zależnej od stanu onboardingu**

```rust
// angeld/src/api/gate.rs
use crate::onboarding::{OnboardingState, SYSTEM_CONFIG_ONBOARDING_STATE};

pub(super) struct AdminAfterOnboarding;

impl FromRequestParts<ApiState> for AdminAfterOnboarding {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ApiState,
    ) -> Result<Self, Self::Rejection> {
        let completed =
            crate::db::get_system_config_value(&state.pool, SYSTEM_CONFIG_ONBOARDING_STATE)
                .await?
                .is_some_and(|value| {
                    value.eq_ignore_ascii_case(OnboardingState::Completed.as_str())
                });

        if completed {
            acl::require_role(&state.pool, &parts.headers, Role::Admin).await?;
        }
        Ok(Self)
    }
}
```

Dopisz `_: AdminAfterOnboarding` do sygnatur `post_setup_provider`, `post_complete_onboarding`, `post_reset_onboarding`, `delete_provider` i `post_test_provider` — **przed** ewentualnym `Json<…>`.

`post_repair_shell` w `maintenance.rs` dostaje `_: AdminCaller` — nie ma związku z kreatorem, więc bez wariantu warunkowego. Tym samym commitem domknij `/api/maintenance/retry-storms` i `/api/maintenance/scrub-errors` (oddają `pack_id` i nazwy dostawców, §9b.7) — one dostają `_: ViewerCaller`.

Uwaga o zamknięciu drogi ucieczki: `post_reset_onboarding` cofa `onboarding_state`, czyli teoretycznie mógłby otwierać pozostałe endpointy. Nie może — sam jest za `AdminAfterOnboarding`, więc po zakończonym onboardingu wymaga Admina jak reszta. Zweryfikuj to jawnie asercją w teście, zamiast zakładać.

- [ ] **Step 4: Uruchom testy**

Run: `cargo test --features test-helpers --test e2e_auth_matrix -- --nocapture`
Expected: PASS na obu testach z tego zadania.

- [ ] **Step 5: Commit**

```bash
git add angeld/src/api/onboarding.rs angeld/src/api/maintenance.rs angeld/tests/e2e_auth_matrix.rs
git commit -m "fix(api): rola Admin na operacjach dostawcow i repair-shell po onboardingu (Z9-08, Z9-19, Z9-20, Z11-02, Z11-12)"
```

### Task 8: Rotacja hasła wymaga starego hasła (WP0.4, zamyka Z9-21)

**Files:**
- Modify: `angeld/src/api/vault.rs:1040-1087`
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Produces: `RotateKeyRequest { old_passphrase: SecretString, new_passphrase: SecretString }`

> **Ostrzeżenie do zapamiętania przy wykonaniu.** To zadanie sprawia, że `rotate-key` staje się poprawny i zachęcający do użycia, a `rotate_vault_key` pozostaje **nietransakcyjne** (Z3-01): przerwanie w połowie trwale traci DEK-i tych inode'ów, które nie zdążyły się przepakować. Do czasu F3/WP3.1 nie wolno tej ścieżki używać na Skarbcu z realnymi danymi ani polecać jej w UI. Jeżeli po Fazie 0 dashboard miałby wystawić przycisk zmiany hasła, wystawia go dopiero po WP3.1.

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

Run: `cargo test --features test-helpers --test e2e_auth_matrix rotate_key_rejects_wrong_old -- --nocapture`
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
    _: AdminCaller,
    Json(req): Json<RotateKeyRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
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

Reszta handlera (audyt + `spawn_post_rotation_backup`) bez zmian. Jeśli wpis audytu potrzebuje tożsamości wywołującego, zmień `_: AdminCaller` na `AdminCaller(caller): AdminCaller` i używaj `caller.user_id` / `caller.device_id` — nie zostawiaj nieużywanego wiązania, bo to ostrzeżenie kompilatora w kodzie, który ma być czysty.

- [ ] **Step 4: Uruchom test**

Run: `cargo test --features test-helpers --test e2e_auth_matrix rotate_key_rejects_wrong_old -- --nocapture`
Expected: PASS

- [ ] **Step 5: Sprawdź, czy dashboard woła ten endpoint z nowym polem**

Run: `grep -n "rotate-key" angeld/static/index.html`
Jeśli wywołanie istnieje, dopisz `old_passphrase` z pola formularza; jeśli nie istnieje, nic nie rób.

- [ ] **Step 6: Commit**

```bash
git add angeld/src/api/vault.rs angeld/tests/e2e_auth_matrix.rs
git commit -m "fix(api): rotate-key wymaga starego hasla (Z9-21)"
```

### Task 9: `add-device` za bramką i z kompletem kontroli (WP0.4, zamyka Z9-03, Z9-28)

**Files:**
- Modify: `angeld/src/api/vault.rs:585-741`
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Produces: `post_add_device` wymaga `AdminCaller`; `try_auto_wrap_vault_key` sprawdza `enrolled_at` i `revoked_at`

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

    let anon = h
        .request_without_token("POST", "/api/vault/add-device", Some(&body))
        .await?;
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

Run: `cargo test --features test-helpers --test e2e_auth_matrix add_device_requires_admin -- --nocapture`
Expected: FAIL na pierwszej asercji.

- [ ] **Step 3: Dodaj bramkę i przenieś kontrole z `post_accept_device`**

```rust
async fn post_add_device(
    State(state): State<ApiState>,
    _: AdminCaller,
    Json(req): Json<AddDeviceRequest>,
) -> Result<Json<AddDeviceResponse>, ApiError> {
    let vault_id = db::get_vault_params(&state.pool)
```

oraz w `try_auto_wrap_vault_key` (sygnatura: `(state, target_device_id: &str, target_public_key: &[u8], vault_id: &str) -> Option<(String, i64, String)>`, `api/vault.rs:685`), przed wywołaniem `wrap_vault_key_for_device`:

```rust
    let target = db::get_device(&state.pool, target_device_id).await.ok()??;
    if target.revoked_at.is_some() || target.enrolled_at.is_none() {
        return None;
    }
```

Podwójne `?` jest poprawne, bo funkcja zwraca `Option`, a `get_device` daje `Result<Option<DeviceRecord>, sqlx::Error>` — pierwsze `?` zdejmuje `Result` przez `.ok()`, drugie `Option`. To sprawdzone w kodzie, nie założone.

**Czego tu świadomie nie ma:** kontroli klucza zerowego. §9b.4 ARCHITECTURE.md ustala („sprawdzony fałszywy alarm"), że `wrap_vault_key_for_device` woła `validate_x25519_pubkey` i osobno odrzuca sekret ECDH równy 32 bajtom zera — czyli wszystkie punkty niskiego rzędu są już odcięte piętro niżej. Dokładanie tu drugiej kontroli byłoby duplikatem obrony w miejscu, w którym przegląd dowiódł, że jej nie brakuje.

- [ ] **Step 4: Uruchom testy**

Run: `cargo test --features test-helpers --test e2e_auth_matrix add_device -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add angeld/src/api/vault.rs angeld/tests/e2e_auth_matrix.rs
git commit -m "fix(api): add-device za rola Admin i z kontrolami z accept-device (Z9-03, Z9-28)"
```

### Task 10: Podniesienie roli dla weryfikacji urządzeń i owiniętych kluczy (WP0.4, zamyka Z9-13, Z9-30)

**Files:**
- Modify: `angeld/src/api/vault.rs:1143` (`post_verify_device`), `:471` (`get_my_wrapped_key`)
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Produces: `post_verify_device` wymaga `AdminCaller`; `get_my_wrapped_key` zwraca wyłącznie klucz **własnego** urządzenia wywołującego

- [ ] **Step 1: Napisz test — dwie różne odmowy**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrapped_key_endpoint_only_serves_the_calling_device()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;

    let anonymous = h
        .request_without_token("GET", "/api/vault/my-wrapped-key?device_id=cudze", None)
        .await?;
    assert_eq!(
        anonymous.status, 401,
        "bez tokenu to 401 (acl::extract_session_or_401); got {} body={}",
        anonymous.status, anonymous.body
    );

    let authorized = h
        .request_with_token("GET", "/api/vault/my-wrapped-key?device_id=cudze", None)
        .await?;
    assert_eq!(
        authorized.status, 403,
        "z waznym tokenem, ale o cudze urzadzenie: 403; got {} body={}",
        authorized.status, authorized.body
    );
    Ok(())
}
```

Poprzednia wersja tego testu używała `h.get_raw(...)`, które **nie wysyła tokenu** (`tests/common/mod.rs:211-213`), i oczekiwała 403. Dostałaby 401 i wywalała się na naprawionym kodzie. Dwie asercje rozdzielają teraz dwie różne rzeczy: „nie wiem, kto pytasz" i „wiem, kto pytasz, i nie wolno ci".

- [ ] **Step 2: Uruchom test**

Run: `cargo test --features test-helpers --test e2e_auth_matrix wrapped_key_endpoint -- --nocapture`
Expected: FAIL na drugiej asercji (dziś endpoint oddaje klucz dowolnego urządzenia albo 404).

- [ ] **Step 3: Zawęź handler do urządzenia wywołującego**

```rust
async fn get_my_wrapped_key(
    State(state): State<ApiState>,
    ViewerCaller(caller): ViewerCaller,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<WrappedKeyResponse>, ApiError> {
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

W `post_verify_device` zmień bramkę na `_: AdminCaller`.

- [ ] **Step 4: Sprawdź, że nie zerwałeś dołączania urządzenia**

To zawężenie dotyka ścieżki, od której zależy **cała Faza 2**. Świeżo dołączone urządzenie pyta o *swój* klucz, więc `caller.device_id` powinno się zgadzać — ale to trzeba zobaczyć, a nie założyć.

Run: `cargo test --features test-helpers --test e2e_recovery` oraz `grep -rn "my-wrapped-key" angeld/static/ angeld/src/ omnidrive-cli/`
Expected: żaden klient nie pyta o cudze `device_id`. Jeśli któryś pyta — zatrzymaj się i zgłoś, bo to zmienia zakres Fazy 2, a nie jest to decyzja do podjęcia w locie.

- [ ] **Step 5: Uruchom testy**

Run: `cargo test --workspace --features test-helpers`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add angeld/src/api/vault.rs angeld/tests/e2e_auth_matrix.rs
git commit -m "fix(api): my-wrapped-key tylko dla wlasnego urzadzenia, verify-device dla Admina (Z9-13, Z9-30)"
```

### Task 11: Anty-CSRF na endpointach bez ciała (WP0.5, zamyka Z9-02/CSRF)

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
    let resp = h
        .request_without_token("POST", "/api/unlock/windows-hello", None)
        .await?;
    assert_eq!(
        resp.status, 403,
        "POST bez naglowka X-OmniDrive-Local musi byc odrzucony; got {} body={}",
        resp.status, resp.body
    );
    Ok(())
}
```

To jest jedyny endpoint w macierzy z oczekiwaniem `Expect::LocalIntent` — stąd osobny wariant w enumie z Zadania 3. Nazwanie go `Public` (jak w poprzedniej wersji planu) byłoby etykietą fałszywą: po tej zmianie nie jest publiczny, tylko wymaga innej klasy dowodu niż token.

- [ ] **Step 2: Uruchom test**

Run: `cargo test --features test-helpers --test e2e_auth_matrix windows_hello_unlock_requires -- --nocapture`
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

Run: `cargo test --features test-helpers --test e2e_auth_matrix windows_hello_unlock_requires -- --nocapture`
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

### Task 12: Zapamiętywanie hasła staje się opcją (WP0.5, zamyka Z9-02/silent-store)

**Files:**
- Modify: `angeld/src/windows_hello.rs` (nazwa poświadczenia z env + `clear_stored_credential`)
- Modify: `angeld/src/api/auth.rs:66-70`, `:331-334`, `:404` (`get_hello_available`)
- Modify: `angeld/src/api/settings.rs` (nowy endpoint **i jego trasa**)
- Modify: `angeld/tests/common/mod.rs` (izolacja poświadczenia)
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Produces: klucz `system_config` = `windows_hello_enabled` (`"0"` domyślnie); `POST /api/settings/windows-hello {enabled: bool}` wymaga `AdminCaller`; `get_hello_available` przyjmuje `State<ApiState>`

**Dwie rzeczy, które trzeba zrobić najpierw, bo inaczej test tego zadania jest niewykonalny:**

1. `get_hello_available` (`auth.rs:404`) **nie przyjmuje `State`** — zwraca gołe `windows_hello::has_stored_credential()`. Nie ma więc jak odczytać flagi z bazy i asercja „domyślnie `available == false`" nigdy nie przejdzie, choćbyśmy obwarowali sam zapis.
2. `windows_hello.rs:24` używa **stałej nazwy poświadczenia** `"OmniDrive/VaultPassphrase"`, wspólnej dla wszystkich procesów na koncie Windows. Test uruchomiony na Lenovo czytałby i kasował prawdziwe poświadczenie Przemka — a przy `enabled: false` wywołuje `CredDeleteW`. To jest naruszenie Świętej Zasady przez test, nie przez kod produkcyjny, i dlatego izolacja jest częścią zadania, a nie „dobrą praktyką na potem".

- [ ] **Step 1: Odetnij testy od prawdziwego Credential Managera**

```rust
// angeld/src/windows_hello.rs — zamiast const CRED_TARGET
    fn cred_target() -> String {
        std::env::var("OMNIDRIVE_CRED_TARGET")
            .unwrap_or_else(|_| "OmniDrive/VaultPassphrase".to_string())
    }
```

Wszystkie trzy funkcje (`store_passphrase`, `retrieve_passphrase`, `has_stored_credential`) i nowa `clear_stored_credential` używają `cred_target()`. W `angeld/tests/common/mod.rs`, w `spawn()`, ustaw procesowi potomnemu `OMNIDRIVE_CRED_TARGET` na wartość unikalną dla instancji (np. `format!("OmniDrive/Test/{}", port)`) — obok istniejących `env_remove` na zmiennych dostawców.

`clear_stored_credential` dopisz wzorem `store_passphrase`, wołając `CredDeleteW`, **z gałęzią dla nie-Windows** (plik ma już fallbacki w `:143-153`).

- [ ] **Step 2: Napisz test, że domyślnie nic się nie zapisuje**

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
        "bez wlaczenia opcji haslo nie moze trafic do Credential Managera; got {resp}"
    );
    Ok(())
}
```

- [ ] **Step 3: Uruchom test**

Run: `cargo test --features test-helpers --test e2e_auth_matrix unlock_does_not_store -- --nocapture`
Expected: FAIL — `available: true`.

- [ ] **Step 4: Obwaruj zapis i odczyt flagą**

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

(`if a && let …` to let-chain — poprawny w Edition 2024, wymaga toolchaina 1.88+.)

Ten sam warunek w `post_change_password`. `get_hello_available` dostaje `State<ApiState>` i zwraca koniunkcję: flaga włączona **i** poświadczenie faktycznie leży w magazynie.

Dodaj endpoint w `settings.rs`:

```rust
#[derive(Deserialize)]
struct WindowsHelloRequest {
    enabled: bool,
}

async fn post_windows_hello_setting(
    State(state): State<ApiState>,
    _: AdminCaller,
    Json(req): Json<WindowsHelloRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
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

- [ ] **Step 5: Zarejestruj trasę i dopisz ją do macierzy**

```rust
// angeld/src/api/settings.rs — w routes()
        .route("/api/settings/windows-hello", post(post_windows_hello_setting))
```

Bez tego handler jest martwym kodem. Test z Zadania 3 wyłapie to od drugiej strony — asercja `stale` zgłosi wpis w macierzy bez trasy w kodzie, jeśli dopiszesz go do `AUTH_MATRIX`, a zapomnisz o `routes()`. Dopisz wpis: `("POST", "/api/settings/windows-hello", "/api/settings/windows-hello", Expect::Role)`.

- [ ] **Step 6: Uruchom testy**

Run: `cargo test --workspace --features test-helpers`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add angeld/src/api/auth.rs angeld/src/api/settings.rs angeld/src/windows_hello.rs angeld/tests/common/mod.rs angeld/tests/e2e_auth_matrix.rs
git commit -m "feat(auth): zapamietanie hasla w Credential Managerze jako opcja, domyslnie wylaczona (Z9-02)"
```

### Task 13: Limiter na `/api/unlock` i `verify-password` (WP0.5, zamyka Z9-04, Z9-10)

**Files:**
- Modify: `angeld/src/api/mod.rs` (`RecoveryRateLimiter` przyjmuje politykę, `ApiState` + dwa limitery)
- Modify: `angeld/src/api/auth.rs:43` (`post_unlock`)
- Modify: `angeld/src/api/sharing.rs:339` (`verify_share_password`)
- Test: `angeld/tests/e2e_auth_matrix.rs`

**Interfaces:**
- Consumes: `RecoveryRateLimiter::{check(ip) -> Result<(), u64>, record_failure, record_success}` (`api/mod.rs:57-86`)
- Produces: `RecoveryRateLimiter::with_policy(free_attempts, cooldown_secs, window_secs)`; `ApiState.{unlock_limiter, share_limiter}`

**Dlaczego nie da się użyć tego limitera bez zmiany.** `RecoveryRateLimiter` blokuje na **30 sekund po pierwszej** nieudanej próbie (`api/mod.rs:63-68`) — polityka słuszna dla odzyskiwania z 24 słów, absurdalna dla ekranu logowania: jedna literówka w haśle i użytkownik czeka pół minuty patrząc na komunikat o przekroczeniu limitu. Dlatego typ dostaje trzy parametry polityki, a `recovery` zachowuje swoje obecne wartości bez zmiany zachowania.

- [ ] **Step 1: Napisz test**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_wrong_passphrase_is_rate_limited()
-> Result<(), Box<dyn std::error::Error>> {
    let h = DaemonHarness::spawn().await?;
    let body = serde_json::json!({ "passphrase": "zle" });

    let mut last = 0u16;
    for _ in 0..6 {
        let resp = h.request_without_token("POST", "/api/unlock", Some(&body)).await?;
        last = resp.status;
    }

    assert_eq!(
        last, 429,
        "po piatej nieudanej probie /api/unlock musi zwrocic 429; got {last}"
    );
    Ok(())
}
```

Sześć prób przy polityce „5 darmowych, potem 60 s" daje wynik deterministyczny niezależnie od tego, ile trwa Argon2id (przy 256 MiB i `t=3` pojedyncza próba to ułamki sekundy do sekund — pętla oparta na czasie byłaby chwiejna).

- [ ] **Step 2: Uruchom test**

Run: `cargo test --features test-helpers --test e2e_auth_matrix repeated_wrong_passphrase -- --nocapture`
Expected: FAIL — 400 za każdym razem.

- [ ] **Step 3: Sparametryzuj limiter i podłącz go**

W `RecoveryRateLimiter` zamień zaszyte `300` i `30` oraz próg `>= 3` na pola struktury ustawiane w `with_policy(free_attempts, cooldown_secs, window_secs)`; `new()` zostaje jako `with_policy(3, 30, 300)`, żeby `recovery_limiter` zachował dzisiejsze zachowanie co do sekundy. W `ApiState` dodaj `unlock_limiter` (`with_policy(5, 60, 300)`) i `share_limiter` (`with_policy(5, 30, 300)`), inicjując je w `ApiServer::run` obok istniejących dwóch (`api/mod.rs:246-247`).

W `post_unlock` sygnatura to dziś `(State<ApiState>, Json<UnlockRequest>)`. `ConnectInfo` wstaw **między** nie — `Json` musi zostać ostatni, bo jako jedyny konsumuje ciało żądania:

```rust
async fn post_unlock(
    State(state): State<ApiState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<UnlockRequest>,
) -> Result<Json<UnlockResponse>, ApiError> {
```

Serwer startuje przez `into_make_service_with_connect_info::<SocketAddr>()` (`api/mod.rs:324`), więc ekstraktor ma skąd wziąć adres — sprawdzone, nie zakładane. Dalej: 

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

Na ścieżce sukcesu `state.unlock_limiter.record_success(ip)`. Ten sam wzorzec zastosuj w `verify_share_password`, używając `share_limiter`.

- [ ] **Step 4: Sprawdź, że nie zepsułeś odzyskiwania**

Run: `cargo test --features test-helpers -p angeld recovery`
Expected: PASS — testy limitera recovery muszą przejść **bez zmiany asercji**. Jeśli któraś wymaga poprawki, znaczy to, że `with_policy(3, 30, 300)` nie odtwarza dotychczasowego zachowania i trzeba wrócić do Step 3, a nie dopasowywać test.

- [ ] **Step 5: Uruchom testy**

Run: `cargo test --workspace --features test-helpers`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add angeld/src/api/mod.rs angeld/src/api/auth.rs angeld/src/api/sharing.rs angeld/tests/e2e_auth_matrix.rs
git commit -m "fix(api): limiter i audyt nieudanych prob na /api/unlock i verify-password (Z9-04, Z9-10)"
```

### Task 14: Tryb testowy znika z binarki produkcyjnej (WP0.6, zamyka Z11-04)

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

Run: `cargo test --features test-helpers --test e2e_basic happy_path -- --nocapture`
Expected: FAIL — otrzymano `idle`.

- [ ] **Step 3: Dodaj wariant statusu i przestań kłamać**

W `angeld/src/diagnostics.rs` dodaj do `WorkerStatus` wariant `NotStarted` z `as_str()` zwracającym `"not_started"`. W `main.rs:382` zamień pętlę ustawiającą `Idle` na ustawianie `NotStarted` dla workerów, które w tym trybie nie startują.

**Nie** zmieniaj wartości początkowej dla wszystkich workerów globalnie: `Idle` zostaje stanem startowym workera, który istnieje i czeka na pracę, a `NotStarted` znaczy „nie został w tym trybie spawnowany". Zamiana domyślnej wartości przestawiłaby też produkcyjny dashboard na `not_started` w oknie między startem procesu a pierwszym tickiem każdego workera — czyli zamienilibyśmy jedno kłamstwo diagnostyki na drugie.

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

**`cargo test` nie włącza tego feature'a — to trzeba zrobić jawnie.** `test-helpers = []` w `angeld/Cargo.toml:78` nie jest w `default`, a `[dev-dependencies]` go nie zaciąga; features nie „włączają się w testach". Po tej zmianie każdy przebieg bez `--features test-helpers` uruchomi harness przeciw daemonowi z **pełnym** zestawem workerów, w innym trybie niż testy zakładają. Repozytorium ma już ten wzorzec: `auto_lock::test_routes()` stoi za tym samym feature'em (`api/mod.rs:279-282`), a `e2e_recovery` bez niego nie przechodzi.

Konsekwencje do wykonania w tym kroku: wszystkie komendy `Run:` w tym planie mają `--features test-helpers`; dopisz to samo do opisu uruchamiania testów w `docs/` tam, gdzie jest wymienione; jeśli istnieje skrypt CI albo alias — popraw go razem z tym commitem.

- [ ] **Step 5: Uruchom testy i zweryfikuj build produkcyjny**

Run: `cargo test --workspace --features test-helpers`
Expected: PASS.

Run: `cargo test --workspace` (bez feature'a)
Expected: testy e2e zależne od trybu testowego **jawnie padają albo są pomijane** — nie wolno, żeby przechodziły „na zielono", bo wtedy nie wiadomo, co sprawdziły.

Run: `cargo build --release --workspace`, potem `target/release/angeld.exe` z `OMNIDRIVE_E2E_TEST_MODE=1`.
Expected: zmienna jest ignorowana. Na dev-boksie bez skonfigurowanych dostawców daemon wstanie w trybie local-only (upload, API, peer, watcher, pipe) — nie w „pełnym" z dziewięcioma workerami. Sprawdzamy tu jedną rzecz: że `repair`, `scrubber` i `gc` **nie** raportują sfabrykowanego `idle`.

- [ ] **Step 6: Commit**

```bash
git add angeld/src/main.rs angeld/src/diagnostics.rs angeld/tests/e2e_basic.rs angeld/Cargo.toml
git commit -m "fix(daemon): tryb e2e tylko za feature test-helpers, status not_started zamiast falszywego idle (Z11-04)"
```

### Task 15: Zamknięcie Fazy 0

- [ ] **Step 1: Uruchom komplet testów**

Run: `cargo test --workspace --features test-helpers`
Expected: PASS, w tym `e2e_auth_matrix` w całości.

- [ ] **Step 2: Sprawdź, że macierz nie ma już naruszeń**

Run: `cargo test --features test-helpers --test e2e_auth_matrix -- --nocapture`
Expected: PASS na wszystkich czterech testach pliku — w tym `every_declared_route_has_an_entry_in_the_matrix` w obie strony (żadnej trasy bez wpisu, żadnego wpisu bez trasy).

- [ ] **Step 3: Odhacz znaleziska w rejestrze**

W `docs/ARCHITECTURE.md` zmień wagę na ✅ i dopisz „**NAPRAWIONE** `<sha>`" dla: **Z2-04, Z9-01, Z9-02, Z9-03, Z9-04, Z9-07, Z9-08, Z9-10, Z9-13, Z9-19, Z9-20, Z9-21, Z9-26, Z9-28, Z9-30, Z10-05, Z10-14, Z11-02, Z11-04, Z11-12** (20 pozycji).

Trzy pozycje wymagają osobnej uwagi, zamiast zbiorczego odhaczenia:

- **Z9-06** — zamknięte co do kontroli dostępu, ale w rejestrze trzeba **najpierw poprawić liczbę** („12 handlerów" → 9, `diagnostics.rs:145-153`), inaczej odhaczamy coś, czego opis się nie zgadza z kodem.
- **Z11-06** — zamknięta jest wyłącznie połowa „bez bramki", i to przez Z9-06 (endpoint mieszka w `diagnostics.rs`). N+1 zostaje otwarte do F4/WP4.3. Zawęź opis pozycji zamiast stawiać ✅.
- **Z10-14** — wolno odhaczyć dopiero wtedy, gdy macierz jest zielona **z pełną listą tras**, a nie z listą z Zadania 3, Step 1. Jeśli w `AUTH_MATRIX` zostały pozycje nadane hurtem, żeby test przeszedł, ta pozycja zostaje otwarta.

Nanieś przy okazji korekty rejestru z §1 (scalenia Z9-15→Z11-03, Z11-13→Z10-08, Z1-07/Z8-14→Z6-02, Z10-05 jako przypis pod Z9-01) i rozstrzygnięcie D5 co do wag.

- [ ] **Step 4: Commit**

```bash
git add docs/ARCHITECTURE.md
git commit -m "docs(architecture): faza 0 zamknieta, 20 znalezisk naprawionych, korekty rejestru"
```

---

## §3 FAZA 1 — Klienci przez mur

**Wejście:** Faza 0 zamknięta. **Wyjście:** każdy klient, który zostaje w repo, potrafi się uwierzytelnić; klienci, którzy zostają usunięci, są usunięci.

| Pakiet | Zakres | Kryterium ukończenia |
| --- | --- | --- |
| **WP1.1** Z10-01, Z10-04 | `omnidrive-cli` dostaje `--api-token` / `OMNIDRIVE_API_TOKEN` oraz komendę `omnidrive login`, która pyta o hasło i woła `/api/unlock`, zapisując token w pliku `%LOCALAPPDATA%\OmniDrive\cli-session` z ACL tylko dla użytkownika. `recovery restore` przestaje wymagać kompletu env — czyta konfigurację dostawcy z bazy przez `MetadataBackupProviderManager::from_onboarding_db_all`. | `omnidrive ls` i `omnidrive pin` działają po `omnidrive login`; test integracyjny wywołujący obie komendy przeciw harnessowi |
| **WP1.2** Z7-01, Z10-06, Z10-09 | **Decyzja D1 (§9)**: albo instalator wgrywa i rejestruje `omnidrive_shell_ext.dll` i usuwamy wariant rejestrowy z `shell_integration.rs`, albo odwrotnie. Przy wariancie DLL litera dysku **nie może** iść przez `OMNIDRIVE_DRIVE_LETTER` — patrz D1. | Jedno menu kontekstowe, działające, z testem `e2e_shell_recovery` rozszerzonym o sprawdzenie pozycji menu |
| **WP1.3** Z11-03, Z9-15 | Usunięcie `static/legacy.html` i trasy `/legacy` z `api/mod.rs`. To 2258 linii martwego, nieuwierzytelnionego kodu — utrzymywanie go kosztuje więcej niż daje. | `grep -r legacy angeld/` pusty poza historią gita |
| **WP1.4** Z9-24 | `extract_session` w `settings.rs` i `require_session` w `auto_lock.rs` zaczynają sprawdzać członkostwo w vaulcie (nowy `SessionCaller` z Zadania 2 rozszerzony o kontrolę `vault_members`). Callback Google przestaje tworzyć użytkownika, jeśli w vaulcie jest już właściciel, a konto Google nie jest jego członkiem. | Test: sesja z konta bez `vault_members` dostaje 403 na `/api/settings/restart-daemon` |
| **WP1.5** (nowe) | **Tray dostaje tożsamość.** Dziś nie ma żadnej: po Fazie 0 czyta wyłącznie `/api/vault/status` i `/api/health`, obie publiczne. To wystarcza do ikony, ale nie wystarczy do WP5.3, gdzie tray ma wołać `POST /api/settings/restart-daemon` (za sesją). Zakres: token urządzenia lokalnego w pliku `%LOCALAPPDATA%\OmniDrive\tray-session` z ACL tylko dla użytkownika, wystawiany przez daemona przy starcie, czytany przez tray. Ten sam mechanizm obsłuży deinstalator. | Test: tray wykonuje operację za bramką sesji bez pytania użytkownika o hasło; plik nie jest czytelny dla innego konta |

---

## §4 FAZA 2 — Cross-device

**Wejście:** Faza 1 zamknięta. **Wyjście:** „Join Existing Vault" działa end-to-end na Dellu — to jest warunek smoke'u β.a.

| Pakiet | Zakres | Kryterium ukończenia |
| --- | --- | --- |
| **WP2.1** Z8-03, Z8-04, Z11-15 | `graft_restored_metadata_snapshot`: wyłączyć FK **przed** `BEGIN` (albo dopisać `DELETE FROM user_sessions` do listy kasowanych — decyzja **D2**, §9); dopisać kopiowanie tabeli `pack_deks` po `packs`; zawęzić fallback w `dek_for_pack` tak, żeby przy wielu DEK-ach na inode **nie zgadywał** i nie zapisywał zgadywanki, tylko zwracał błąd. | Test e2e: plik **12 MiB** (3 chunki) → migawka → graft na czystej bazie → `restore_file` odtwarza bajt w bajt. Ten test musi być czerwony przed naprawą. |
| **WP2.2** Z8-10, Z8-12 | `r_vault_config` przestaje być `unwrap_or(None)` — brak `vault_config` w migawce to twardy błąd grafta z komunikatem. Kreator w kroku „join" dostaje ostrzeżenie, że lokalne metadane zostaną skasowane, z checkboxem potwierdzenia. | Test: graft migawki bez `vault_config` zwraca `Err`; wizard nie pozwala kliknąć dalej bez zaznaczenia |
| **WP2.3** Z9-22 | `post_revoke_device` i `post_remove_member`: nieudana `rotate_for_revocation` przestaje być `warn!` — zwraca 500 z jasnym komunikatem, a `revoked_at` jest wycofywane w transakcji. Odwołanie albo się udaje w całości, albo wcale. | Test: wstrzyknięty błąd rotacji → 500 → `revoked_at` nadal `NULL` |

> **Zakres WP2.3 kończy się na `revoked_at`.** Obietnica „albo w całości, albo wcale" dotyczy **wycofania flagi odwołania**, a nie atomowości samej rotacji Vault Key. `rotate_for_revocation` jest nietransakcyjne (§3.5), tylko odtwarzalne przez `dek_rewrap_queue` — pełną transakcyjność rotacji dowozi F3/WP3.1. Jeśli przy wykonaniu okaże się, że wycofanie `revoked_at` wymaga też wycofania częściowego re-wrapu DEK-ów, to jest sygnał do przesunięcia WP3.1 przed F2, a nie do rozszerzania tego pakietu w locie.

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
| **WP3.8** Z2-06 | FK na `shared_links(inode_id)`, `shared_links(revision_id)` i **`user_sessions(device_id)`**. | Migracja przechodzi na kopii bazy roboczej; sonda potwierdza, że graft z WP2.1 nadal działa przy włączonych FK |

> **Korekta do WP3.8.** Poprzednia wersja mówiła o FK na `user_sessions(user_id)` z uzasadnieniem „po Fazie 0 sesje są sprzątane, więc FK nie zablokuje grafta". Obie części były nieprawdziwe. Ten klucz obcy **już istnieje** — §8.4 ARCHITECTURE.md wskazuje go jako bezpośrednią przyczynę Z8-03 (`DELETE FROM users` wywala się o `user_sessions`). Z2-06 mówi o **`device_id`**, nie `user_id`. A sprzątanie z Zadania 5 usuwa wyłącznie sesje **wygasłe**; sesja ważna w chwili grafta blokuje tak samo, więc to nie jest żadne zabezpieczenie. Warunkiem bezpieczeństwa tego pakietu jest zamknięte WP2.1 (D2), nie Faza 0.

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
| **WP5.3** Z7-05, Z11-08, Z10-03, Z10-12, Z10-13, **Z9-31** | `post_vault_lock` woła `lock_flow::force_lock_and_dismount`; teardown przestaje być detached — API czeka na wynik i raportuje błędy; **`restart-daemon` faktycznie restartuje** (Z9-31, przeniesione tu z WP7.4); tray i deinstalator wołają ten endpoint zamiast `taskkill /F`, uwierzytelniając się tokenem z WP1.5; restart czeka na zwolnienie portu. | Test: po `POST /api/vault/lock` żaden plik w sync roocie nie jest zhydratowany; „Restartuj demona" z traya kończy się **działającym** daemonem |

> **Dwie zależności, które poprzednia wersja planu miała odwrócone.** (1) `POST /api/settings/restart-daemon` dziś tylko sygnalizuje shutdown i **nic go nie podnosi** (Z9-31) — a Z9-31 leżało w WP7.4, czyli dwie fazy dalej. Przepięcie traya na ten endpoint przed naprawą zamieniłoby przycisk „Restartuj demona" w „Zatrzymaj demona". Dlatego Z9-31 wchodzi do tego pakietu. (2) Endpoint jest za bramką sesji, a tray nie ma tożsamości do WP1.5 — stąd nowy pakiet w F1.
| **WP5.4** Z7-06 | Jedna definicja „bezczynny" dla UI i pętli tick; praca w Eksploratorze dotyka licznika. | Test: hydratacja pliku przez cfapi przesuwa `remaining_seconds` |
| **WP5.5** Z7-07 | Hydratacja bez dostawców zwraca błąd do `cldflt.sys`, nie `STATUS_SUCCESS` z zerem bajtów. | Test: odczyt pliku przy braku dostawców daje błąd I/O, nie pusty plik |
| **WP5.6** Z11-07, Z2-02, Z9-12 | `provider_connection_status` zwraca `FAILED` przy trwałej awarii; `/api/stats/overview` liczy pliki niezależnie od wielkości liter; `post_vault_join` przestaje połykać błędy i nie konsumuje zaproszenia przy niepowodzeniu. | Tray pokazuje ikonę błędu przy awarii dostawcy |

---

## §8 FAZA 6 — Bezpieczeństwo lokalne · FAZA 7 — Dług

**Faza 6 jest rozdzielona na dwie części — patrz D4 (§9).** F6a (**WP6.2** i **WP6.5**) idzie **przed** smoke'em β.a, bo dotyczy dokładnie tego, co smoke uruchamia: dwóch maszyn w jednej sieci oraz uprawnień sync roota, których usunięcie i tak wymaga weryfikacji na żywo. F6b (reszta) po smoke'u, bo zmienia zachowanie na maszynie produkcyjnej i nie ma powodu mieszać tego z weryfikacją cross-device.

| Pakiet | Kiedy | Zakres |
| --- | --- | --- |
| **WP6.1** Z8-01 | F6b | Named Pipe: DACL zawężony do SID interaktywnego użytkownika; `GetNamedPipeClientProcessId` + weryfikacja, że klient to `explorer.exe` albo podpisany `omnidrive_shell_ext.dll`; retry przy zajętej nazwie. **Jeśli D1 wybierze wariant DLL, ten pakiet przechodzi do F6a** — inaczej menu kontekstowe chodzi nad pipe'em dla `Everyone` przez cztery fazy. |
| **WP6.2** Z8-02 | **F6a** | Peer: `trusted = 1` wyłącznie po wyzwaniu podpisanym kluczem urządzenia z tabeli `devices`; ogłoszenie UDP przestaje nieść `vault_id` w jawnej postaci (skrót z solą). |
| **WP6.3** Z8-05, Z9-27, Z9-25 | F6b | Sekrety dostawców i token OAuth pieczętowane Vault Key zamiast DPAPI; `snapshot-local` ogranicza ścieżkę do katalogu runtime. |
| **WP6.4** Z7-02, Z7-03 | F6b | Prawdziwe Windows Hello przez `Windows.Security.Credentials.UI` zamiast samego DPAPI; bufor po `CryptUnprotectData` zerowany i zwalniany; hasło w `SecretString`. Wchodzi na przygotowany grunt: Zadanie 12 Fazy 0 zrobiło już z zapamiętywania hasła opcję domyślnie wyłączoną, więc tu zostaje sama biometria i higiena pamięci. |
| **WP6.5** Z7-04, Z7-14 | **F6a** | Usunięcie ACE `Authenticated Users` z sync roota; obserwator WTS reaguje na przełączenie użytkownika i rozłączenie RDP. §7.7 ARCHITECTURE.md wymaga testu na żywo (rejestracja sync roota + hydratacja) — smoke β.a **jest** tym testem, więc pakiet musi go poprzedzić, a nie po nim nastąpić. |
| **WP6.6** Z9-05, Z11-10, Z11-09, Z11-11 | F6b | Tailwind, jdenticon i Inter serwowane lokalnie z binarki; CSP na `/`; token OAuth przenosi się z `localStorage` do pamięci; Service Worker z zasięgiem `/sw-download/`. |
| **WP6.7** Z8-08, Z8-09, Z8-16, Z10-10 | F6b | `secure_delete` na wszystkich ścieżkach sprzątania łącznie z sidecarami WAL; migawka roster-fetch do katalogu runtime zamiast `%TEMP%`; szyfrowanie lokalnych kopii `.bak`; rotacja logu rozszerzenia powłoki. |

**Faza 7** — 44 pozycje długu (WP7.1-WP7.6 z §1). Do zrobienia hurtem, jednym przebiegiem po każdym module, bez osobnego planu: usunięcie martwego kodu i `#![allow(dead_code)]`, deduplikacja, zamiana `contains()` na typowane błędy, drobiazgi wydajnościowe i uzgodnienie dokumentacji z kodem.

---

## §9 Decyzje przed startem

Te cztery rzeczy muszą zapaść, zanim odpowiednie fazy ruszą. Nie zgaduję ich.

- **D1 (przed F1/WP1.2) — który klient powłoki zostaje?** DLL (`omnidrive-shell-ext`) jest lepszy technicznie: sześć pozycji, `catch_unwind` wszędzie, komunikacja przez pipe. Wariant rejestrowy (`shell_integration.rs`) jest zainstalowany, ale nie działa (Z7-01). **Rekomendacja: zostaje DLL**, instalator go wgrywa i rejestruje, `shell_integration.rs` znika. Trzy koszty, których poprzednia wersja tej decyzji nie wymieniała, a które zmieniają jej wykonanie:
  1. **DLL rozmawia wyłącznie przez Named Pipe** (§8.8), a ten ma DACL `Everyone GR/GW` bez weryfikacji wywołującego (Z8-01, naprawa dopiero w F6/WP6.1). Wybór DLL w F1 oznacza cztery fazy z działającym menu kontekstowym Skarbca nad kanałem, do którego może się podłączyć dowolny proces. Albo przyspieszamy WP6.1 do F1, albo świadomie akceptujemy to okno — ale trzeba to zapisać, nie przemilczeć.
  2. **Litera dysku nie może iść przez zmienną środowiskową.** `OMNIDRIVE_DRIVE_LETTER` jest eksportowane przez `export_env_defaults()` do **procesu daemona** (§1.2), a DLL żyje w `explorer.exe`, który tej zmiennej nigdy nie zobaczy. Z10-09 trzeba więc naprawić inaczej: literę podaje daemon w odpowiedzi na komendę pipe'a albo zapisuje ją do `HKCU` przy montowaniu, a `Initialize` ją stamtąd czyta. To jest inna praca niż „podmień stałą na `env::var`".
  3. Rejestracja wymaga uprawnień administratora, a DLL załadowanego do Explorera nie da się podmienić przy aktualizacji bez ubicia powłoki — deinstalator i upgrade z F5/WP5.3 muszą to uwzględnić.
- **D2 (przed F2/WP2.1) — jak wyłączyć FK w graftcie?** Wariant A: przenieść `PRAGMA foreign_keys = OFF` przed `BEGIN`. Wariant B: dopisać brakujące tabele do listy kasowanych i zostawić FK włączone. **Rekomendacja: wariant B**, ale z mocniejszym argumentem i twardszym warunkiem niż poprzednio:
  - **Przeciw wariantowi A jest argument, którego wcześniej nie postawiono:** pragma działa **per połączenie**, a graft bierze połączenie z puli. To jest dokładnie mechanizm **Z2-03** — połączenie z wyłączonymi kluczami obcymi wraca do puli i jest wydawane innym workerom, dla których FK są od tej chwili martwe. Wariant A leczy Z8-03, wprowadzając własne 🔴 z rozdziału 2.
  - **Wariant B wymaga dowodu, nie zgadywanki.** Przegląd wykazał blokadę wyłącznie na `user_sessions`; §8.4 pisze wprost, że dla pozostałych tabel kolejność `DELETE` działa „przypadkiem". Dopisanie z pamięci `user_sessions` i `invite_codes` zamyka jeden znany przypadek i zostawia resztę na los. Warunek wejścia do WP2.1: przejść `PRAGMA foreign_key_list` po wszystkich 18 kasowanych tabelach **albo** odtworzyć pełną sekwencję grafta na kopii bazy z FK włączonymi i zobaczyć, że przechodzi. Dopiero wynik tej sondy ustala listę tabel.
- **D3 (przed F5/WP5.2) — LAN Share: HTTPS czy wycofanie?** HTTPS z self-signed wymaga dodania certyfikatu do magazynu zaufania na każdym urządzeniu odbiorcy — to psuje obietnicę „wyślij link i gotowe". **Rekomendacja: wycofać tryb A z UI i dokumentacji** — ale **najpierw** trzeba odpowiedzieć na pytanie, którego ta decyzja nie zadaje: czy tryb B w ogóle działa end-to-end dla zdalnego odbiorcy? Tryb B wymaga, żeby odbiorca pobrał szyfrogram z daemona, a architektura jest local-first — daemon słucha na loopbacku i LAN, bez tunelu ([[project-architecture-corrections]]). Jeśli tryb B też nie ma drogi do odbiorcy spoza sieci, to wycofanie trybu A zostawia funkcję „udostępnij plik" bez żadnej działającej ścieżki, a nie z jedną. Przegląd tego nie weryfikował. **Warunek decyzji: jeden ręczny test trybu B z drugiego urządzenia.** Niezależnie od wyniku Z11-14 (cały plik w RAM karty) zostaje — dotyczy też trybu B poniżej progu 50 MiB.
- **D4 (przed F6) — kiedy Faza 6?** Poprzednia rekomendacja („cała F6 po smoke'u β.a, żeby nie mieszać dwóch źródeł zmian") jest argumentem o zarządzaniu zmianą i nie dotyka ekspozycji. Dwie rzeczy ją przewracają:
  - **Smoke β.a jest jedynym momentem, gdy dwie maszyny są naraz w Skarbcu w jednej sieci.** To jest dokładny scenariusz Z8-02: `trusted = 1` na podstawie samego rozgłoszenia UDP, a `/peer/chunks/{hex}` oddaje **plaintext chunka** każdemu, kto poda dwa nagłówki, które sami nadajemy co 5 sekund. Robienie tego testu z otwartym Z8-02 to uruchamianie eksperymentu z uzbrojoną podatnością.
  - **Z7-04 chce tego smoke'u, a nie czeka na niego.** §7.7 mówi wprost, że usunięcie ACE `Authenticated Users` wymaga testu na żywo (rejestracja sync roota + hydratacja) na maszynie docelowej. Odłożenie WP6.5 za smoke oznacza, że trzeba będzie zrobić drugi smoke.
  
  **Rekomendacja: F6 rozdzielić.** Przed smoke'em β.a: **WP6.2** (Z8-02, zaufanie w mesh LAN) i **WP6.5** (Z7-04, ACL sync roota — weryfikowany przy okazji tego samego przejścia). Po smoke'u: WP6.1, WP6.3, WP6.4, WP6.6, WP6.7. Jeśli Przemek woli nie ruszać niczego przed smoke'em, alternatywą jest przeprowadzenie go w sieci odizolowanej od reszty LAN — ale wtedy trzeba to zapisać jako warunek, nie założyć.
- **D5 (przed Zadaniem 15) — wagi w rejestrze.** Legenda w `ARCHITECTURE.md:176-181` definiuje 🔴 i ⚠️ przez **pewność** znaleziska („znaleziony konkretny problem" vs „działa, ale ma dług"), a nie przez skutek — stąd wagi rozjeżdżają się między rozdziałami pisanymi w różnych sesjach. Do rozstrzygnięcia: czy przyjąć kryterium skutku (utrata danych / kompromitacja klucza / funkcja nie działa = 🔴, reszta = ⚠️) i przeważyć rejestr. Przy kryterium skutku w dół idą **Z1-01** (log rośnie — brak skutku dla danych, a bliźniacze Z10-10 z nazwami plików Skarbca ma ⚠️), **Z2-02** (licznik plików w UI, poprawka jednoznakowa) i **Z1-02** (wobec ⚠️ przy Z6-15, gdzie ta sama wada zatrzymuje pilnowanie integralności). W górę: **Z11-05** („usuń trwale" zostawia dane w trzech chmurach i zrywa jedyne powiązanie, po którym gc mógłby je znaleźć), **Z6-09** (deterministycznie przekracza dobowy limit egressu i odpala zatrzask z Z6-01, czyli jest wyzwalaczem 🔴), **Z8-06** (nieograniczone godzinne pobieranie poza `cloud_guard` — ten sam kształt, za który Z4-07 dostało 🔴) i **Z9-24** (dowolne konto Google dostaje sesję honorowaną przez `settings` i `auto-lock`). To jest decyzja porządkowa, nie techniczna — ale bez niej „43 × 🔴" nie znaczy nic konkretnego przy planowaniu.

---

## §10 Kolejność, wersje i smoke

```
F0 (2-3 sesje)  → bump 0.3.30 → cargo test --workspace --features test-helpers
F1 (1-2 sesje)  → bump 0.3.31
F2 (1 sesja)    → bump 0.3.32
F6a (1 sesja)   → WP6.2 + WP6.5 → instalator → SMOKE β.a na Dellu   ← kamień milowy
F3 (2 sesje)    → bump 0.4.0-rc1
F4 (2 sesje)    → bump 0.4.0-rc2
F5 (2 sesje)    → bump 0.4.0-rc3 → SMOKE pełny
F6b (2 sesje)   → bump 0.4.0
F7 (1 sesja)    → bump 0.4.1
```

Po każdej fazie: `cargo build --release --workspace`, kopiowanie binarek do `dist/installer/payload/`, podbicie wersji we **wszystkich** `Cargo.toml`, odhaczenie znalezisk w `docs/ARCHITECTURE.md`.

**Smoke β.a jest po F2, nie wcześniej** — przed naprawą Z8-03 i Z8-04 dołączenie Della do Skarbca albo się nie uda, albo uda się i pliki będą nieodszyfrowywalne. Uruchamianie go teraz spaliłoby sesję na diagnozowaniu czegoś, co już jest zdiagnozowane.

**F6a przed smoke'em to skutek D5/D4** — dwie maszyny w jednej sieci to scenariusz Z8-02, a Z7-04 i tak wymaga weryfikacji na żywo dokładnie w tym przebiegu.

**Czego smoke β.a nie obejmuje i dlaczego to trzeba wiedzieć zawczasu.** Odbywa się przed F3, czyli przy otwartych Z3-01 (zmiana hasła nietransakcyjna — przerwanie trwale traci DEK-i), Z5-01, Z6-07 i Z10-02. Wynikają z tego trzy twarde zakazy na czas tego smoke'u, do zapisania w jego scenariuszu: **nie zmieniamy hasła Skarbca** (ani przez `/api/change-password`, ani przez `rotate-key` z Zadania 8), **nie uruchamiamy `omnidrive recovery restore`** i **nie wskazujemy `OMNIDRIVE_CACHE_DIR` poza NTFS**. Jeśli scenariusz smoke'u miałby którąkolwiek z tych rzeczy zawierać, odpowiedni pakiet z F3 przesuwa się przed kamień milowy.

Szacunki sesji dla F0 i F1 podniesione względem poprzedniej wersji: F0 ma teraz 15 zadań zamiast 13 (doszły harness i ekstraktory, bez których reszta nie działa), a F1 dostało WP1.5.

---

## §11 Self-review

**Pokrycie:** wszystkie 150 identyfikatorów z `docs/ARCHITECTURE.md` mają przypisanie — 4 jako już naprawione, 1 jako WON'T FIX w całości (Z4-13 co do zachowania), 3 przeniesione z WON'T FIX do faz (Z4-03 → WP7.5, Z4-10 częściowo → WP4.2, Z4-12 → WP4.2), reszta rozdzielona na 39 pakietów roboczych. Pozycje informacyjne `ℹ️` spoza tabeli rejestru (Z1-07, Z2-08, Z3-07) są w WP7.1 i WP7.2.

**Placeholdery:** Faza 0 nie zawiera „TBD" ani „podobnie jak zadanie N" — każdy krok ma kod albo dokładną komendę. Fazy 1-7 są świadomie na poziomie pakietów roboczych; §„Jak czytać ten plan" mówi to wprost.

**Weryfikacja w kodzie, nie deklaracja spójności.** Poprzednia wersja tej sekcji twierdziła, że „`Expect` (Task 1) używane w Task 5, 7, 8, 9, 11" — to była nieprawda (te zadania dopisują samodzielne testy i nie dotykają `Expect`), i była to deklaracja kontroli, której nikt nie przeprowadził. Ta wersja planu powstała po otwarciu kodu: lista faktów potwierdzonych i obalonych jest na początku dokumentu, z numerami linii. Osiem założeń poprzedniej wersji okazało się fałszywych, w tym trzy blokujące (bramka za ekstraktorem ciała, `cargo test` bez feature'a, `get_raw` bez tokenu przy asercji 403).

**Znane ryzyka planu:**

1. **Zadanie 4 zmienia zachowanie UI** — dashboard zacznie pytać o hasło przy otwartym Skarbcu. Jeśli to okaże się nie do przyjęcia, alternatywą jest token bootstrapowy w pliku runtime z ACL tylko dla użytkownika (ten sam mechanizm co WP1.5 dla traya), ale to nie należy do Fazy 0.
2. **Zadanie 2 dotyka sygnatur wszystkich bramkowanych handlerów.** To jest większa zmiana mechaniczna niż „dopisz linijkę", za to jednorazowa i wymuszona przez axuma — bez niej test macierzy nie może być zielony. Jeśli w trakcie okaże się, że któryś handler nie da się przepiąć (np. czyta `HeaderMap` do czegoś jeszcze), zostaw mu `require_role` w ciele i **wpisz go do macierzy jako wyjątek z uzasadnieniem** — nie zaokrąglaj asercji testu.
3. **Zadanie 6 zmienia kontrakt `/api/health`** (nowe pole `ingest_failed`) i usuwa wywołanie z traya. Gdyby okazało się, że tray potrzebuje z `/api/ingest` czegoś więcej niż flagi, właściwą odpowiedzią jest przyspieszenie WP1.5, a nie zostawienie endpointu otwartym.
4. **Zadanie 12 wymaga zmiany w kodzie produkcyjnym po to, żeby test był bezpieczny** (nazwa poświadczenia z env). To jest świadomy koszt: bez tego test na maszynie produkcyjnej Przemka kasuje jego prawdziwe poświadczenie DPAPI.
5. **Rozbicie F6 (D4) nie jest darmowe** — WP6.2 i WP6.5 wykonane przed smoke'em wchodzą do tego samego instalatora, co F2. Jeśli smoke wykryje regresję, będzie trzeba rozstrzygnąć, czy pochodzi z grafta, czy z mesh/ACL. Alternatywa (smoke w izolowanej sieci, cała F6 później) jest wymieniona w D4 i wybór należy do Przemka.
