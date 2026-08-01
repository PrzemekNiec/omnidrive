# OmniDrive — mapa architektury (ściąga per-moduł)

> **Cel dokumentu.** Odpowiada na pytanie „co robi ten moduł i **dlaczego** tak, a nie inaczej".
> Nie jest onboardingiem (od tego jest `PROJECT_OVERVIEW.md`) ani roadmapą (od tego jest `STATUS.md`).
> Powstał z pełnego przeczytania kodu, nie z dokumentacji — gdzie kod rozjeżdża się z dokumentacją,
> **kod wygrywa i rozjazd jest odnotowany**.
>
> Stan: **v0.3.28**, branch `main`, HEAD `7ba32f2`, data przeglądu **2026-08-01**.
> Skala: 121 plików `.rs`, ~48 000 linii, 6 crate'ów.

---

## ⏸️ STAN PRZEGLĄDU — czytaj to najpierw przy wznowieniu

**Ostatnia sesja: 2026-08-01.** Przegląd przerwany na warstwie 7 z powodu wyczerpania kontekstu,
nie z powodu problemu w kodzie.

### Metoda (trzymać się jej — sprawdziła się)

1. **Czytać kod, nie dokumentację.** Każde znalezisko w tym pliku zostało potwierdzone
   w kodzie albo empirycznie (sonda SQLite, sonda NTFS, test na czerwono). Zero dedukcji
   podawanej jako fakt.
2. **Pisać rozdział na dysk natychmiast po przeczytaniu warstwy**, nie na końcu — inaczej
   kompresja kontekstu zjada detale, po które się szło.
3. **Znaleziska notować, nie naprawiać** (ustalenie użytkownika), z wyjątkiem sytuacji, gdy
   dotyczą integralności danych — wtedy zapytać.
4. Przy każdym „X jest zepsute" **najpierw sprawdzić**, czy nie ma fallbacku, który to ratuje.
   Dwa razy uratowało to przed fałszywym alarmem (`probe_latency` przechodzi przez `cloud_guard`;
   pierwsza wersja testu kopii konfliktu przechodziła mimo wyłączonej naprawy).

### Co zrobione

| Warstwa | Stan |
| --- | --- |
| 1. Bootstrap | ✅ pełne czytanie |
| 2. Baza danych (24 pliki) | ✅ pełne czytanie |
| 3. Krypto i vault | ✅ pełne czytanie |
| 4. Pipeline zapisu | ⚠️ `packer`, `watcher`, `ingest` przeczytane; **`uploader.rs` (1020 linii) i `aws_http.rs` NIE** |
| 5. Pipeline odczytu | ✅ `downloader/*` + `cache.rs` |
| 6. Integralność | ⚠️ `cloud_guard`, `gc` pełne; **`scrubber.rs` i `repair.rs` tylko strukturalnie** |
| 7. Windows / Ghost Shell | ⛔ ledwo zaczęta — przeczytany wyłącznie `lock_flow.rs` |
| 8. Cross-device | ⛔ nie zaczęta |
| 9. API i Web UI | ⛔ nie zaczęta |
| 10. Satelity i testy | ⛔ nie zaczęta |

### Co zostało do przeczytania

```
warstwa 4 (dokończyć): uploader.rs (1020), aws_http.rs (50)
warstwa 6 (dokończyć): scrubber.rs (504), repair.rs (881)
warstwa 7: smart_sync/* (2292), virtual_drive (348), shell_state (435),
           shell_integration (238), auto_lock (479), win_session (213),
           win_acl (266), acl (300), secure_fs (162), windows_hello (142),
           autostart (175)
warstwa 8: onboarding (1213), db/graft (1460), disaster_recovery (2689),
           peer (535), pipe_server (309), sharing (107 — juz czytane przy Z4-01)
warstwa 9: api/* (14 plikow, ~5500), api_error (160), static/*
warstwa 10: omnidrive-tray (353), omnidrive-shell-ext (567), omnidrive-cli (607),
            angelctl (3), angeld/tests/* (~2600), bin/cfapi_repro (137)
```

### Zadania otwarte poza przeglądem

- **Z4-06** — `ingest.rs:391` używa reguł `EC_2_1` dla każdego packa; pliki z polityką
  `STANDARD`/`LOCAL` zawsze lądują w `FAILED` przy Inbox upload. Naprawa jednolinijkowa.
- **Odświeżyć `docs/PROJECT_OVERVIEW.md`** — stan na 2026-06-04 / v0.3.27, nieaktualny.
- **Rewizja `STATUS.md`** (100 KB) przeciw ustaleniom z tego pliku.

### Zamknięte w tej sesji

`Z4-01` — DEK per pack zamiast per inode (commit `8d24755`), wraz z dwupoziemową kopertą dla
linków share, poprawką migratora V1→V2 i testem e2e. Wersja podbita do **0.3.29**,
`dist/installer/output/OmniDrive-Setup-0.3.29.exe` zbudowany. 210 testów lib + 18 integracyjnych
zielonych.

---

## Legenda stanu modułu

| Znak | Znaczenie |
| --- | --- |
| ✅ | Solidny — czytelny, przetestowany, bez znanych pułapek. |
| ⚠️ | Działa, ale ma dług: duplikacja, kruche założenie, brak testu w newralgicznym miejscu. |
| 🔴 | Znaleziony konkretny problem — opisany w „Znaleziska" danego rozdziału. |

## Spis rozdziałów

1. [Bootstrap i konfiguracja](#1-bootstrap-i-konfiguracja)
2. [Baza danych (`db/*`)](#2-baza-danych)
3. [Krypto i vault](#3-krypto-i-vault)
4. [Pipeline zapisu](#4-pipeline-zapisu) — *bez `uploader.rs`*
5. [Pipeline odczytu](#5-pipeline-odczytu)
6. [Integralność danych](#6-integralnosc-danych)
7. Windows / Ghost Shell — *do zrobienia*
8. Cross-device — *do zrobienia*
9. API i Web UI — *do zrobienia*
10. Satelity i testy — *do zrobienia*

## Rejestr znalezisk

| ID | Waga | Rzecz | Potwierdzone jak |
| --- | --- | --- | --- |
| Z1-01 | 🔴 | `angeld.log` rośnie bez końca; prune po `mtime` nigdy nie tknie aktywnego pliku | grep: zero `RollingFileAppender` |
| Z1-02 | 🔴 | 3 workery poza `tokio::select!` (m.in. `pipe_server`) — śmierć niezauważona | czytanie `main.rs` |
| Z1-03 | ⚠️ | ~450 linii zduplikowanego shutdownu w 4 gałęziach trybów | czytanie |
| Z1-04 | ⚠️ | `panic!` przy niespójności vaulta w procesie GUI-subsystem — niewidoczny | czytanie |
| Z1-05 | ⚠️ | Komentarze z numerami zadań łamią CLAUDE.md §3 | czytanie |
| Z1-06 | ⚠️ | Kod diagnostyczny w binarce produkcyjnej, poza `cloud_guard` | czytanie |
| Z2-01 | 🔴 | Soft-delete blokuje odtworzenie pliku w **podkatalogu** | sonda SQLite |
| Z2-02 | 🔴 | `/api/stats/overview` zawsze 0 plików (`kind = 'file'` małymi) | sonda SQLite |
| Z2-03 | 🔴 | `backfill_uuid_user_ids` może oddać do puli połączenie z `FK = OFF` | czytanie |
| Z2-04 | ⚠️ | `cleanup_expired_sessions` i `delete_expired_oauth_states` nigdy nie wołane | grep |
| Z2-05 | ⚠️ | Projekcja po pojedynczym inode ignoruje soft-delete | czytanie |
| Z2-06 | ⚠️ | Brak FK na `shared_links` i `user_sessions` | czytanie schematu |
| Z2-07 | ⚠️ | `PERMANENTLY_FAILED` niepoliczony w `summarize_pack_shards` | czytanie |
| Z3-01 | 🔴 | Zmiana hasła nietransakcyjna — awaria = trwała utrata DEK-ów | czytanie |
| Z3-02 | 🔴 | Uszkodzony `encrypted_vault_key` cicho nadpisywany zamiast błędu | czytanie |
| Z3-03 | ⚠️ | `Box::leak` na ścieżce błędu | czytanie |
| Z3-04 | ⚠️ | `payloads.rs` w całości nieużywany, `layout.rs` w 4/5 (relikt VFS) | grep |
| Z3-05 | ⚠️ | Trzy klucze root wyprowadzane i nigdy nieużywane | grep |
| Z3-06 | ⚠️ | ML-KEM opisany w roadmapie jako odroczony — **jest zbudowany i podpięty** | grep + czytanie |
| Z4-01 | ✅ | DEK per inode → pliki nieodszyfrowywalne | **NAPRAWIONE** `8d24755` |
| Z4-02 | ⚠️ | `split_ciphertext_into_shards` kopiuje bajt po bajcie (4 mln iteracji/chunk) | czytanie |
| Z4-03 | ⚠️ | Providerzy zaszyci pozycyjnie; `EC_2_1` wymaga dokładnie tych trzech | czytanie |
| Z4-04 | ⚠️ | Każdy restart przepakowuje cały watch root (stąd 1429 rewizji) | czytanie + baza |
| Z4-05 | ⚠️ | Watcher bierny do restartu po zakończeniu onboardingu | czytanie |
| Z4-06 | 🔴 | Ingest ocenia packi regułami `EC_2_1` — `STANDARD`/`LOCAL` zawsze `FAILED` | grep + tabela progów |
| Z5-01 | 🔴 | Cache pisze do alternatywnych strumieni NTFS (`:` w nazwie pliku) | sonda NTFS |
| Z6-01 | 🔴 | Wyłącznik awaryjny chmury zatrzaskuje się do restartu daemona | grep: 1 wołający |
| Z6-02 | ⚠️ | `AppConfig::from_env()` przy każdej operacji chmurowej | czytanie |

---

# 1. Bootstrap i konfiguracja

Warstwa, która zamienia „uruchomiony proces" w „działający daemon". Ustala **gdzie** cokolwiek
leży na dysku, **czy** wolno gadać z chmurą i **które** workery w ogóle wystartują.

## 1.1 Mapa warstwy

| Plik | Linie | Rola w jednym zdaniu |
| --- | --- | --- |
| `angeld/src/lib.rs` | 41 | Lista modułów crate'a — jedyny plik, który widzi całość. |
| `angeld/src/main.rs` | 1113 | Bootstrap daemona: restore bazy → migracje → wybór trybu → spawn workerów → graceful shutdown. |
| `angeld/src/runtime_paths.rs` | 262 | Jedyne źródło prawdy o ścieżkach; rozróżnia tryb **Workspace** (dev) i **Installed**. |
| `angeld/src/config.rs` | 233 | `AppConfig::from_env()` — limity chmury, porty peer, OAuth. Czysty odczyt env. |
| `angeld/src/logging.rs` | 130 | `tracing` → stdout + plik, prune logów starszych niż 7 dni. |

## 1.2 `runtime_paths.rs` — dlaczego istnieje

**Problem, który rozwiązuje:** ta sama binarka działa w dwóch światach. Na Lenovo uruchamiana
wprost z `target/release` (dane w `./.omnidrive/`), na Dellu z instalatora (dane w
`%LOCALAPPDATA%\OmniDrive\`). Bez tego modułu każdy worker sam zgadywałby ścieżki.

**Jak rozpoznaje tryb** (`detect_runtime_mode`, kolejność ma znaczenie):
1. `OMNIDRIVE_RUNTIME_MODE` = `installed` / `workspace` — jawne nadpisanie.
2. `current_exe()` leży pod `ProgramFiles`, `ProgramFiles(x86)` lub `%LOCALAPPDATA%\Programs` → **Installed**.
3. Domyślnie → **Workspace**.

**Kluczowy mechanizm — `export_env_defaults()`.** Ustala ścieżki raz i **eksportuje je do
zmiennych środowiskowych procesu**, ale wyłącznie gdy zmienna jeszcze nie istnieje
(`set_env_default`). Dzięki temu moduły, które czytają env bezpośrednio (`packer`, `downloader`,
`ingest`) dostają spójne wartości bez przekazywania `RuntimePaths` przez pół kodu. To świadomy
kompromis: globalny stan w zamian za brak przewlekania struktury.

**Pułapka:** `set_env_default` używa `unsafe { env::set_var }` (w Edition 2024 `set_var` jest
`unsafe`, bo nie jest thread-safe). Jest wołane w `main()` **przed** startem runtime'u Tokio i
przed spawnem czegokolwiek — i tylko dlatego jest bezpieczne. Każde przyszłe wywołanie
`export_env_defaults()` z wnętrza workera byłoby UB.

**`normalize_sqlite_path`** obcina wiodący `/` z `sqlite:///C:/...` → `C:/...`. Bez tego
Windows dostaje ścieżkę `/C:/Users/...`, której nie otworzy.

**`secure_runtime_directories()`** woła `win_acl::secure_directory` — ale **tylko w trybie
Installed**. W trybie Workspace katalogi runtime nie mają zaostrzonych ACL (świadome:
dev box, katalog w repo).

## 1.3 `main.rs` — sekwencja startu

Kolejność jest krytyczna i nieoczywista. Pełna ścieżka `run_daemon()`:

```
1.  dotenv + RuntimePaths::detect + export_env_defaults + init_logging + panic hook
2.  bootstrap_directories(false)      ← sync root JESZCZE nie, nie wiadomo czy będzie potrzebny
3.  secure_runtime_directories()
4.  zapamiętaj database_missing_on_start
5.  maybe_auto_restore_database()     ← disaster recovery z chmury (tylko debug/test-helpers)
6.  db::init_db()                     ← migracje schematu
7.  cloud_guard::sync_runtime_flags() ← utrwalenie DRY-RUN w bazie
8.  initialize_onboarding_persistence + cleanup_stale_restore_staging + cleanup_stale_uploads
9.  db::sync_upload_targets_from_shards()   ← naprawa duchów po poprzedniej sesji
10. get_active_provider_configs()     ← DECYDUJE o trybie pracy
11. bootstrap_directories(smart_sync_enabled)  ← teraz dopiero sync root
12. bootstrap_default_local_vault
13. AppConfig::from_env + walidacja OAuth loopback
14. ensure_local_device_identity
15. migrate_single_to_multi_user → backfill_uuid_user_ids → ensure_local_device_in_vault
16. verify_vault_device_binding      ← panic! przy niespójności
17. wybór gałęzi trybu → spawn workerów → tokio::select!
```

**Dlaczego `bootstrap_directories` jest wołane dwa razy (kroki 2 i 11):** sync root
(`%LOCALAPPDATA%\OmniDrive\OmniSync`) to katalog rejestrowany w Cloud Files API. Utworzenie go
w trybie local-only zostawiłoby na dysku pusty katalog, który Explorer mógłby pokazać jako
uszkodzony sync root. Więc: najpierw reszta, potem — jeśli faktycznie będzie Smart Sync — sync root.

### Cztery tryby pracy

Tryb wynika z trzech niezależnych sygnałów: flagi `--no-sync`, zmiennej
`OMNIDRIVE_E2E_TEST_MODE` i tego, czy w bazie są aktywni providerzy chmury.

| Tryb | Warunek | Co startuje |
| --- | --- | --- |
| **E2E + no-sync** | `--no-sync` ∧ `E2E_TEST_MODE` | tylko `UploadWorker` + API |
| **E2E** | `E2E_TEST_MODE` | API + rejestracja sync root + projekcja placeholderów |
| **Local-only / setup** | brak aktywnych providerów | upload, API, peer, watcher, pipe; **zwykły dysk** przez `subst`, nie CF |
| **Pełny** | providerzy skonfigurowani | 9 workerów + 3 zadania fire-and-forget |

**Lazy Mount — najważniejsza decyzja architektoniczna tej warstwy.** W trybie pełnym przy starcie
instalowany jest **wyłącznie** runtime hydracji (`smart_sync::install_hydration_runtime`).
`CfRegisterSyncRoot`, `CfConnectSyncRoot` i podpięcie dysku `O:` dzieją się **dopiero w handlerze
`/api/unlock`**, po zweryfikowaniu hasła. Powód jest wprost zero-knowledge: dopóki vault jest
zamknięty, dysk `O:` nie może istnieć, bo nie ma czym odszyfrować niczego, co Explorer by pokazał.
Konsekwencja, o której łatwo zapomnieć przy debugowaniu: **`startup_recover_shell()` jest w tym
trybie celowo pomijane** (remontowałoby `O:` przed unlockiem).

### Workery w trybie pełnym

W `tokio::select!` (śmierć któregokolwiek = shutdown całości):
`upload`, `repair`, `scrubber`, `gc`, `ingest`, `metadata_backup`, `metadata_fetch`, `watcher`, `api`, `peer`.

Fire-and-forget, **poza** `select!` (prefiks `_`, nikt nie patrzy czy żyją):
- `pipe_server::run_pipe_server` — IPC do tray i shell-ext.
- czyszczenie wygasłych tokenów share (co 300 s).
- sweeper soft-delete (co 3600 s, hard-delete po `SOFT_DELETE_GRACE_MS`).

## 1.4 `config.rs`

Płaski `AppConfig::from_env()`, zero I/O poza env. Domyślne limity warte zapamiętania:

| Stała | Wartość | Znaczenie |
| --- | --- | --- |
| `DEFAULT_MAX_PHYSICAL_BYTES_PER_PROVIDER` | 75 GiB | Sufit na providera. |
| `DEFAULT_CACHE_MAX_BYTES` | 50 GiB | Sufit cache lokalnego. |
| `DEFAULT_CLOUD_DAILY_WRITE_OPS_LIMIT` | 1 000 | Bezpiecznik po „B2 bleeding". |
| `DEFAULT_CLOUD_DAILY_READ_OPS_LIMIT` | 5 000 | j.w. |
| `DEFAULT_CLOUD_DAILY_EGRESS_BYTES_LIMIT` | 500 MiB | j.w. — najostrzejszy bezpiecznik. |
| `DEFAULT_MAX_UPLOAD_BYTES_PER_SEC` | `0` | **0 = bez limitu**, nie „zero przepustowości". |
| `DEFAULT_PEER_PORT` / discovery | 8788 / 8789 | LAN mesh. |

`validate_oauth_redirect_loopback_only()` — jedyna funkcja w tym pliku z realną logiką
bezpieczeństwa. Wymusza RFC 8252: redirect OAuth musi być `127.0.0.1` / `localhost` / `[::1]`.
W buildzie release **niespełnienie tego przerywa start daemona**; w debug jest tylko `warn!`.
To bezpośrednia konsekwencja architektury Local-First — publiczny redirect oznaczałby, że kod
autoryzacyjny trafia na host, którego użytkownik nie kontroluje. Ma 5 testów jednostkowych.

## 1.5 `logging.rs`

`tracing-subscriber` z filtrem domyślnym `info,sqlx=warn,hyper=warn,h2=warn,aws_config=warn`
(nadpisywalnym przez `RUST_LOG`). Zapis równolegle na stdout i do pliku, `with_ansi(false)`
(kolory w pliku byłyby śmieciem).

Dwie decyzje pod Windows:
- `open_log_file` otwiera plik z `FILE_SHARE_READ | WRITE | DELETE`. Bez tego użytkownik nie
  otworzy `angeld.log` w Notatniku podczas pracy daemona, a Defender potrafi zablokować plik.
- W **release** nieudana inicjalizacja loggera plikowego **przerywa start** (`return Err`),
  w debug jest fallback na sam stdout. Rozumowanie: daemon w GUI-subsystem bez konsoli i bez
  pliku logu jest nieobserwowalny, więc lepiej żeby nie wstał.

`flush_logs_best_effort()` woła panic hook — `sleep(150 ms)` daje wątkowi `tracing_appender`
szansę dopisać ostatnie linie przed śmiercią procesu.

---

## Znaleziska — rozdział 1

> Zgodnie z ustaleniem: **notuję, nie naprawiam**. Numeracja `Z1-*` do przeniesienia do STATUS/KNOWN_ISSUES.

### 🔴 Z1-01 — `angeld.log` rośnie bez ograniczeń, prune go nigdy nie usunie
`logging.rs:19` czyści pliki starsze niż 7 dni po `mtime`. Ale zapis idzie zawsze do jednego
`angeld.log`, więc jego `mtime` jest odświeżany przy każdej linii logu — **warunek `age > max_age`
nigdy nie będzie prawdziwy dla aktywnego pliku**. Rotacji rozmiaru nie ma nigdzie
(`tracing_appender` jest w zależnościach, ale użyty wyłącznie do `non_blocking`, nie do
`RollingFileAppender`). Efekt: plik rośnie liniowo w nieskończoność, a przy `info` na 10 workerach
to realne setki MB. Weryfikacja: `grep -r "rolling\|RollingFileAppender"` → zero trafień.

### 🔴 Z1-02 — trzy workery są niewidoczne dla nadzoru
`pipe_server`, cleanup tokenów share i sweeper soft-delete są spawnowane z prefiksem `_`
(`main.rs:662, 770, 787`) i nie trafiają do `tokio::select!`. Gdy taki task spanikuje, daemon
działa dalej i **nic tego nie zgłasza** — brak wpisu w `diagnostics`, brak zmiany statusu.
Najgroźniejszy jest `pipe_server`: jego śmierć = tray i rozszerzenie powłoki tracą IPC, a UI
pokazuje ostatni znany stan jakby wszystko grało.

### ⚠️ Z1-03 — ~450 linii zduplikowanego shutdownu
Cztery gałęzie trybów mają własne `tokio::select!`, a w każdym ramieniu ręcznie wypisane
`.abort()` na wszystkich pozostałych taskach. W trybie pełnym to 10 ramion × 9 abortów.
Dodanie jedenastego workera wymaga 40+ edycji w czterech miejscach — i pominięcie jednej
daje zombie-task po shutdownie, którego kompilator nie wyłapie. Naturalne rozwiązanie:
`tokio::task::JoinSet` (abort całości jednym wywołaniem).

### ⚠️ Z1-04 — `panic!` jako obsługa błędu spójności vaulta
`main.rs:315` — przy niepowodzeniu `verify_vault_device_binding` leci `panic!`. W buildzie
release proces jest GUI-subsystem (`windows_subsystem = "windows"`), więc użytkownik **nie
zobaczy niczego** — ani okna, ani konsoli. Zostaje wpis w logu, do którego nikt nie zajrzy.
Intencja (twardy stop przy rozjeździe tożsamości) jest słuszna, forma nie.

### ⚠️ Z1-05 — komentarze łamiące CLAUDE.md §3
W `main.rs` żyją komentarze przywiązane do zadań i wersji: `// v0.3.19: reconcile…` (217),
`// Epic 34.0b` (293), `// Faza J` (299), `// A.9:` (311), `// B.7:` (1147). CLAUDE.md zabrania
ich wprost („historię trzyma git, nie pliki źródłowe"). Uwaga: **treść tych komentarzy jest
wartościowa** (tłumaczą WHY), problem jest tylko z etykietą zadania — przy sprzątaniu nie
kasować całości, tylko prefiks.

### ⚠️ Z1-06 — kod diagnostyczny w binarce produkcyjnej
`smoke_test_r2_upload()` (~46 linii) i `run_upload_diagnostics()` (~42 linie) siedzą w `main.rs`
i odpalają się z `OMNIDRIVE_SMOKE_TEST_R2` / `OMNIDRIVE_DIAG_UPLOADS`. Ten pierwszy tworzy vault
z hasłem `"r2-smoke-test-passphrase"` zapisanym w kodzie i wysyła plik do prawdziwego bucketa.
Nie jest to podatność (wymaga zmiennej środowiskowej i własnych kluczy), ale to ścieżka zapisu do
chmury żyjąca poza całą logiką `cloud_guard`.

### ℹ️ Z1-07 — `RuntimePaths::detect()` wołane wielokrotnie
`main()` → `run_daemon()` → `AppConfig::from_env()` → `logging::default_log_dir()`: każde z nich
robi własny `detect()` z odczytem env i `current_exe()`. Jest idempotentne i tanie, więc to nie
bug — ale gdyby ktoś kiedyś dodał tu efekt uboczny, rozjazd byłby trudny do namierzenia.

---

# 2. Baza danych

SQLite przez `sqlx`, jeden `SqlitePool` współdzielony przez wszystkie ~10 workerów.
Po dekompozycji z 2026-07-31 monolityczny `db.rs` (367 symboli) rozpadł się na 24 moduły
w `angeld/src/db/`. `db/mod.rs` robi `pub use <moduł>::*` dla każdego z nich, więc **cały
kod nadal woła `db::cokolwiek()` bez ścieżki modułu** — dekompozycja była czysto plikowa,
bez zmiany API. To świadome: pozwoliło przenieść 5000 linii bez dotykania wołających.

## 2.1 Mapa modułów

| Moduł | Linie | Odpowiedzialność |
| --- | --- | --- |
| `schema.rs` | 737 | `init_db()` — tworzenie tabel i migracje. Jedyne miejsce z DDL. |
| `graft.rs` | 1460 | Przeszczep metadanych ze snapshotu (→ rozdział 8). |
| `uploads.rs` | 811 | Kolejka `upload_jobs` + `upload_job_targets` (per-provider). |
| `users.rs` | 583 | Użytkownicy, członkostwo w vaultcie, migracja single→multi-user. |
| `packs.rs` | 562 | Packi, statusy zdrowia, wybór trybu składowania. |
| `vault_state.rs` | 514 | Stan vaulta, DEK-i, rotacja VK, kolejka re-wrapu. |
| `shards.rs` | 408 | Shardy packów (EC), stan weryfikacji scrubbera. |
| `shares.rs` | 377 | Linki współdzielone + tokeny hasła. |
| `inodes.rs` | 373 | Drzewo plików, soft-delete. |
| `projection.rs` | 358 | Rekurencyjne CTE budujące ścieżki dla projekcji CF. |
| `chunks.rs` | 352 | `chunk_refs` ↔ `pack_locations`, mapa offsetów. |
| `revisions.rs` | 351 | Rewizje plików i klasyfikacja pokrewieństwa (konflikty). |
| `device_identity.rs` | 334 | Lokalna tożsamość urządzenia + trusted peers. |
| `devices.rs` | 320 | Urządzenia w vaultcie (multi-user), wrapowanie VK. |
| `conflicts.rs` | 293 | Zdarzenia konfliktu i materializacja kopii. |
| `system_config.rs` | 251 | Klucz-wartość + dzienne liczniki użycia chmury. |
| `sessions.rs` | 217 | Tokeny sesji użytkownika. |
| `providers.rs` | 209 | Konfiguracje i zaszyfrowane sekrety providerów. |
| `cache.rs` | 198 | Wpisy cache'u chunków (LRU). |
| `ingest.rs` | 195 | Kolejka `ingest_jobs`. |
| `oauth.rs` | 174 | Stan OAuth (PKCE/CSRF) + zapieczętowany refresh token. |
| `invites.rs` | 151 | Kody zaproszeń. |
| `recovery_keys.rs` | 125 | Klucze odzyskiwania (BIP-39). |
| `metadata_backup.rs` | 119 | Historia backupów metadanych. |
| `migration_v2.rs` | 111 | Zapytania migracji szyfrowania V1→V2. |
| `audit.rs` | 106 | Dziennik audytu. |
| `stats.rs` | 84 | Statystyki dla UI + ruch w kubełkach 2-godzinnych. |

## 2.2 Model danych — jak plik staje się packami

To najważniejszy łańcuch w całym projekcie. Sześć tabel, każda z jasną rolą:

```
inodes            drzewo nazw (parent_id, name) + soft-delete
  └─ file_revisions   każdy zapis = nowa rewizja; dokładnie jedna ma is_current=1
       └─ chunk_refs    (revision_id, chunk_id, file_offset, size) — z czego składa się rewizja
            └─ pack_locations  (chunk_id → pack_id, pack_offset) — gdzie chunk siedzi w packu
                 └─ packs        (pack_id, nonce, gcm_tag, storage_mode, status)
                      └─ pack_shards  (pack_id, shard_index, provider, object_key, checksum)
```

Cztery rzeczy warte zapamiętania:

1. **`chunk_id` to hash treści, nie identyfikator.** `pack_locations` ma go jako PRIMARY KEY,
   więc identyczny chunk w dwóch plikach wskazuje na ten sam pack — **deduplikacja jest wbudowana
   w schemat**, nie w kod. `find_pack_by_plaintext_hash` to samo robi na poziomie całych packów.
2. **Rewizje tworzą DAG przez `parent_revision_id`.** `classify_revision_lineage` odpala
   rekurencyjne CTE i zwraca `Same` / `CandidateDescendsFromCurrent` (fast-forward) /
   `CurrentDescendsFromCandidate` (nieaktualna baza) / `Parallel` (prawdziwy konflikt).
   To jest cały silnik rozstrzygania konfliktów — reszta to konsekwencje.
3. **`pack_shards` ma DWA unikalne indeksy:** `UNIQUE(pack_id, shard_index)` oraz
   `UNIQUE(pack_id, provider)`. Drugi wymusza, że **każdy shard tego samego packa idzie do
   innego providera**. Konsekwencja: `EC_2_1` (3 shardy) wymaga **trzech skonfigurowanych
   providerów**. Przy dwóch pack nigdy nie osiągnie `COMPLETED_HEALTHY`.
4. **Status packa jest wyliczany, nie ustawiany.** `resolve_pack_status_for_mode` mapuje liczbę
   ukończonych shardów na status zależnie od trybu:

   | Tryb | HEALTHY | DEGRADED | UPLOADING | UNREADABLE |
   | --- | --- | --- | --- | --- |
   | `EC_2_1` | ≥3 shardy | ≥2 shardy | jest pending/in-progress | reszta |
   | `SINGLE_REPLICA` | ≥1 shard | — | jest pending/in-progress | reszta |
   | `LOCAL_ONLY` | zawsze | — | — | — |

## 2.3 Migracje — model „addytywny, bez wersji"

**Nie ma tabeli `schema_version` ani ponumerowanych migracji.** `init_db()` przy każdym starcie:
1. wykonuje `CREATE TABLE IF NOT EXISTS` dla wszystkich ~30 tabel,
2. dla każdej dodanej później kolumny woła `ensure_column_exists()` — czyta `PRAGMA table_info`
   i robi `ALTER TABLE ADD COLUMN`, jeśli kolumny brak,
3. w kilku miejscach robi „gołe" `let _ = ALTER TABLE …` ignorując błąd (np. kolumny Kyber
   w `local_device_identity`) — ten sam efekt, mniej elegancko.

**Dlaczego tak:** SQLite nie potrafi zmienić typu ani usunąć constraintu przez `ALTER TABLE`,
a przy zero-knowledge nie ma centralnego serwera, który przeprowadziłby migrację. Model
addytywny jest jedynym, który działa idempotentnie na dowolnie starej bazie użytkownika.

**Czym za to płacimy** (to nie jest usterka, to koszt decyzji — ale trzeba go znać):
- nie da się stwierdzić, „w jakiej wersji" jest baza — tylko empirycznie po obecności kolumn,
- nie da się zmienić typu kolumny ani constraintu (patrz **Z2-01** — dokładnie na to trafiliśmy),
- nie ma migracji „w dół",
- pierwsza linia `init_db` to `DROP TABLE IF EXISTS files` — relikt schematu v1, wykonywany
  przy każdym starcie do końca świata.

## 2.4 Soft-delete (γ.c)

Najświeższa funkcja w tej warstwie. Kasowanie pliku **nie usuwa wiersza** — ustawia
`inodes.deleted_at`:

- `soft_delete_inode` — tylko `kind = 'FILE'`, tylko gdy `deleted_at IS NULL` (idempotentne).
  **Katalogów nie da się soft-delete'ować** — dla nich nadal działa twarde `delete_inode_record`.
- `chunk_refs` **nie są ruszane** — dzięki temu przywrócenie jest darmowe i nie wymaga chmury.
- `restore_soft_deleted_inode` — przy kolizji nazwy dokleja `(restored)`, `(restored 2)`…
- Sweeper w `main.rs` (co godzinę) kasuje twardo wszystko starsze niż
  `SOFT_DELETE_GRACE_MS` = **7 dni**.
- Wszystkie zapytania „na żywo" filtrują `deleted_at IS NULL`; `get_inode_by_id` celowo **nie
  filtruje**, bo przywracanie musi widzieć skasowane.

Do tego dorzucono częściowy indeks unikalny
`idx_inodes_parent_name_root ON inodes(COALESCE(parent_id,-1), name) WHERE deleted_at IS NULL`
— żeby soft-delete zwalniał nazwę. **Ten indeks nie wystarcza** — patrz **Z2-01**.

## 2.5 Jednostki czasu — mieszane, uwaga

W schemacie żyją obok siebie **trzy** konwencje czasu. To najczęstsze źródło subtelnych błędów
w tej warstwie, więc tabela ma być pierwszym miejscem do sprawdzenia przy każdym porównaniu dat:

| Konwencja | Wyrażenie w kodzie | Tabele |
| --- | --- | --- |
| **Milisekundy** | `CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)` | `inodes.deleted_at`, `file_revisions.created_at`, `upload_jobs`, `upload_job_targets`, `pack_shards.last_verified_at`, `conflict_events`, `system_config`, `provider_configs`, `provider_secrets`, `cloud_usage_daily`, `local_device_identity`, `trusted_peers`, `shared_links`, `share_password_tokens` |
| **Sekundy** | `db::epoch_secs()` | `users`, `devices`, `vault_members`, `audit_logs.timestamp`, `invite_codes`, `user_sessions`, `oauth_states`, `ingest_jobs`, `vault_recovery_keys`, `dek_rewrap_queue` |
| **Sekundy (SQL)** | `CAST(strftime('%s','now') AS INTEGER)` | `cache_entries` |

Uwaga na pułapkę: `store_device_keypair` i `store_kyber_keypair` piszą `epoch_secs()` (sekundy)
do `local_device_identity.updated_at`, którego pozostałe zapisy wypełniają milisekundami.
Kolumna ma więc mieszane jednostki — nie jest dziś do niczego porównywana, ale każdy przyszły
`WHERE updated_at > …` będzie zwracał śmieci.

## 2.6 Współbieżność

- Wszystko idzie przez jeden `SqlitePool` (`min_connections(1)`, `max` zostawione na domyślne).
- **Tryb dziennika i `busy_timeout` nie są ustawiane jawnie** — kod polega na domyślnych
  wartościach `sqlx` (WAL + 5 s). Działa, ale to niewidoczna zależność od wersji `sqlx`;
  jedyne miejsca z jawnym `PRAGMA busy_timeout = 10000` to `graft.rs`.
- `apply_cloud_usage_delta_with_limits` to jedyna funkcja z ręcznym
  `BEGIN IMMEDIATE TRANSACTION` — potrzebnym, bo sprawdzenie limitu i inkrementacja licznika
  muszą być atomowe, inaczej dwa workery przepuszczą operację ponad limit („B2 bleeding").
- `get_next_upload_job` i `get_next_pending_ingest_job` to wzorzec „SELECT + UPDATE w transakcji"
  zamiast `SELECT … FOR UPDATE` (SQLite go nie ma). Bezpieczne tylko dlatego, że **worker jest
  jeden na typ zadania**.
- `requeue_upload_job_after` odracza zadanie **w bazie** (`next_attempt_at`), a nie przez `sleep`
  w workerze — bo worker jest jeden i sen zablokowałby całą kolejkę. Ma 4 testy pilnujące
  dokładnie tego. To najlepiej udokumentowana decyzja w całej warstwie.

---

## Znaleziska — rozdział 2

### 🔴 Z2-01 — soft-delete blokuje odtworzenie pliku w podkatalogu (POTWIERDZONE)

`inodes` ma **dwa** ograniczenia unikalności na tej samej parze kolumn:
- z `CREATE TABLE`: `UNIQUE(parent_id, name)` — pełne, **bez** `WHERE`,
- dodany później: `idx_inodes_parent_name_root … WHERE deleted_at IS NULL` — częściowy.

Częściowego indeksu nie da się „nałożyć" na constraint z `CREATE TABLE`, a model migracji
addytywnych (§2.3) nie pozwala go usunąć. Efekt:

| Lokalizacja | soft-delete → ponowne utworzenie tej samej nazwy |
| --- | --- |
| katalog główny (`parent_id IS NULL`) | **działa** — SQLite traktuje NULL-e jako różne, więc constraint nie łapie |
| dowolny podkatalog (`parent_id = N`) | **`UNIQUE constraint failed: inodes.parent_id, inodes.name`** |

**Weryfikacja:** odtworzone na czystym SQLite z dokładnym DDL z `schema.rs` — root przechodzi,
podkatalog wywala się na constraincie.

**Scenariusz awarii:** użytkownik kasuje `O:\docs\raport.docx` (→ `watcher.rs:489` robi
soft-delete), po czym zapisuje w `docs\` nowy plik o tej samej nazwie. `ingest.rs:513` woła
`upsert_inode`, ta przez `get_inode_by_path` (filtrującą `deleted_at IS NULL`) nie widzi
skasowanego wiersza, więc idzie w `INSERT` — i dostaje unique violation. **Nowy plik nigdy nie
trafia do skarbca**, a użytkownik widzi go lokalnie i zakłada, że jest zabezpieczony.
Odblokowuje się dopiero po 7 dniach, gdy sweeper twardo skasuje stary wiersz.

**Dlaczego testy tego nie złapały:** wszystkie 7 testów soft-delete w `inodes.rs` używa
`parent_id = None`, czyli jedynego przypadku, w którym constraint nie działa. Wzorzec identyczny
z tym, co już zapisaliśmy po smoke'u — suita zielona, ścieżka produkcyjna zepsuta.

### 🔴 Z2-02 — `/api/stats/overview` zawsze pokazuje 0 plików (POTWIERDZONE)

`db/stats.rs:20` filtruje `WHERE kind = 'file'` — małymi literami. Kolumna `inodes.kind` nie ma
`COLLATE NOCASE`, a `validate_inode_kind` dopuszcza wyłącznie `'FILE'` i `'DIR'`. Porównanie
nigdy nie jest prawdziwe.

**Weryfikacja:** dwa pliki `FILE` o rozmiarach 100 i 250 bajtów → `kind='file'` zwraca `(0, 0)`,
`kind='FILE'` zwraca `(2, 350)`.

Konsument: `api/stats.rs:39` → `GET /api/stats/overview`. Czyli licznik plików i rozmiar
skarbca w UI są **stale zerowe**, niezależnie od zawartości. Jednoznakowa poprawka.

### 🔴 Z2-03 — `backfill_uuid_user_ids` może zostawić wyłączone klucze obce

`users.rs:375` robi `PRAGMA foreign_keys = OFF` na połączeniu wyjętym z puli, przetwarza pętlę,
i dopiero na końcu (`:415`) włącza je z powrotem. Każdy `?` w środku pętli — a jest ich 9 —
powoduje wcześniejszy return, po którym **połączenie z wyłączonymi kluczami obcymi wraca do
puli** i jest wydawane kolejnym workerom. Od tej chwili wszystkie ograniczenia FK są dla nich
martwe, cicho, do restartu daemona.

`graft.rs` ma ten sam wzorzec, ale tam pod `:832` jest awaryjne `let _ = PRAGMA foreign_keys = ON`
— czyli świadomość problemu istnieje, tylko nie została przeniesiona do `users.rs`.

### ⚠️ Z2-04 — `cleanup_expired_sessions` nigdy nie jest wołane

Funkcja istnieje (`sessions.rs:122`) i ma test, ale w kodzie produkcyjnym nie ma ani jednego
wywołania — `main.rs` spawnuje wyłącznie cykliczny cleanup tokenów share. To samo dotyczy
`delete_expired_oauth_states` (`oauth.rs:47`). Wygasłe sesje i stany OAuth zostają w bazie
na zawsze. Bezpieczeństwo jest w porządku (`validate_user_session` sprawdza `expires_at`),
ale tabele rosną bez ograniczeń, a dwie funkcje utrzymaniowe są martwym kodem.

### ⚠️ Z2-05 — projekcja po pojedynczym inode ignoruje soft-delete

`get_active_files_for_projection` i `list_active_files` filtrują `i.deleted_at IS NULL`,
natomiast `get_active_file_for_projection_by_inode` (`projection.rs:177`) **nie**. Odświeżenie
projekcji dla jednego inode'a może więc wystawić z powrotem placeholder skasowanego pliku.

### ⚠️ Z2-06 — brak kluczy obcych na `shared_links` i `user_sessions`

`shared_links.inode_id` / `revision_id` oraz `user_sessions.device_id` są zwykłymi kolumnami
bez `REFERENCES`. Skasowanie pliku zostawia link współdzielony wskazujący w próżnię — sprawdzi
się to dopiero przy próbie pobrania. Wszystkie sąsiednie tabele mają FK, więc to wygląda na
przeoczenie, nie decyzję.

### ⚠️ Z2-07 — `PERMANENTLY_FAILED` niepoliczony w podsumowaniu shardów

`summarize_pack_shards` (`shards.rs:404`) mapuje `COMPLETED`/`PENDING`/`IN_PROGRESS`/`FAILED`,
a `PERMANENTLY_FAILED` wpada w `_ => {}` — powiększa tylko `total`. Do `resolve_pack_status_for_mode`
trafia więc summary, w którym trwale martwy shard jest nieodróżnialny od nieistniejącego.
Dziś skutek jest łagodny (pack ląduje w `UNREADABLE` przez brak pending), ale liczby w
diagnostyce nie sumują się i każda przyszła zmiana progów będzie się o to potykać.

### ℹ️ Z2-08 — potrójnie powtórzona logika ścieżek bazowych w `projection.rs`

Trzy funkcje (`get_active_files_for_projection`, `get_active_file_for_projection_by_inode`,
`list_unpinned_hydrated_files_for_eviction`) mają ten sam wklejony blok: pobierz polityki sync,
dołóż `OMNIDRIVE_WATCH_DIR` z env, przemapuj ścieżki. Każda z nich czyta env osobno przy każdym
wywołaniu. Trzy razy to samo — i trzy miejsca do poprawienia, gdyby reguła się zmieniła.

---

# 3. Krypto i vault

Warstwa, w której naruszenie zasady zero-knowledge kosztuje prywatne pliki użytkownika.
Podzielona na dwa światy: **`omnidrive-core`** to czysta matematyka bez I/O (crate przeznaczony
też pod UniFFI dla mobile), **`angeld/src/vault.rs` + `identity.rs`** to stan, baza i cykl życia.

## 3.1 Mapa warstwy

| Plik | Linie | Rola |
| --- | --- | --- |
| `omnidrive-core/crypto.rs` | 616 | Argon2id, HKDF, AES-GCM (V1 i V2), AES-KW. Zero I/O. |
| `omnidrive-core/hybrid.rs` | 208 | Kombinator X-Wing: X25519 + ML-KEM-768 → KEK. |
| `omnidrive-core/pqkem.rs` | 106 | ML-KEM-768 (FIPS 203) przez crate `ml-kem`. |
| `omnidrive-core/layout.rs` | 125 | Struktury binarne formatu on-disk (`#[repr(C, packed)]`). |
| `omnidrive-core/payloads.rs` | 134 | Typy serializowane manifestów. **Nieużywane** — patrz Z3-04. |
| `omnidrive-core/ffi.rs` | 63 | Fasada UniFFI (za feature `ffi`). |
| `angeld/vault.rs` | 1571 | `VaultKeyStore`: unlock/lock, DEK-i, rotacja VK, migracja KDF. |
| `angeld/identity.rs` | 981 | Klucze urządzenia (X25519 + Kyber), wrap VK dla urządzeń. |
| `angeld/recovery.rs` | 241 | Klucze odzyskiwania BIP-39. |
| `angeld/device_identity.rs` | 77 | Wyznaczenie `device_id` / nazwy urządzenia. |

## 3.2 Hierarchia kluczy

```
hasło użytkownika
   │ Argon2id (salt z vault_config, params z vault_config)
   ▼
master_key (32 B)  ──────────────────────────────────────────────┐
   │ HKDF-Expand z etykietami                                    │
   ├─ "vault-key-v1"          → vault_key      (V1: szyfrowanie chunków)
   ├─ "kek-v2"                → kek            (V2: wrapuje envelope VK)
   ├─ "manifest-mac-key-v1"   → manifest_mac_key    (nieużywane)
   ├─ "lease-mac-key-v1"      → lease_mac_key       (nieużywane)
   ├─ "local-anchor-key-v1"   → local_anchor_key    (nieużywane)
   └─ "omnidrive-identity-kek-v1" → identity KEK ───┘ (w identity.rs, z master_key)
                                     │ AES-256-GCM
                                     └─ chroni w spoczynku: X25519 priv (60 B),
                                        ML-KEM decaps key (2400 B → 2428 B po zapieczętowaniu)

kek ──AES-KW──▶ encrypted_vault_key (40 B, w vault_state)
                     │ unwrap przy unlock
                     ▼
              envelope_vault_key (32 B, LOSOWY — nie pochodzi z hasła)
                     │ AES-KW
                     ├─ wrapped_dek per inode (data_encryption_keys)
                     ├─ HKDF "omnidrive-oauth-refresh-tokens-v1" → pieczętowanie tokenów OAuth
                     └─ HKDF "legacy-read-key-v1" → pieczętowanie starego vault_key przy migracji KDF
```

**Najważniejsza konsekwencja:** `envelope_vault_key` jest **losowy**, nie wyprowadzany z hasła.
Dlatego zmiana hasła to tylko przepakowanie 40 bajtów, a nie ponowne zaszyfrowanie skarbca —
i dlatego dwa różne urządzenia mogą dostać ten sam VK opakowany różnymi kluczami (ECDH/hybryda).

## 3.3 V1 vs V2 — dwa schematy szyfrowania chunków, obok siebie

| | **V1** | **V2 (Envelope)** |
| --- | --- | --- |
| Klucz | `vault_key` (jeden na skarbiec, z hasła) | `DEK` (losowy, jeden na inode) |
| `chunk_id` | `HMAC(vault_key, plaintext)` | `HMAC(dek, plaintext)` |
| Nonce | **deterministyczny**: `HMAC(vault_key,"nonce"‖chunk_id)[..12]` | **losowy**, 12 B z OsRng |
| Weryfikacja przy odczycie | `decrypt_chunk` zawsze przelicza `chunk_id` | `decrypt_chunk_v2` **nie**; `_v2_verified` tak |
| Kolumna | `packs.encryption_version = 1` | `= 2` |

**Dlaczego V1 ma deterministyczny nonce:** bo `chunk_id` jest funkcją treści, więc ten sam
plaintext daje zawsze ten sam `(chunk_id, nonce, ciphertext)`. To *szyfrowanie konwergentne* —
warunek działania deduplikacji na poziomie `pack_locations` (gdzie `chunk_id` jest kluczem
głównym). Cena jest wpisana w schemat: **operator chmury widzi, że dwa Twoje bloby są identyczne**
(nie wie czym są, ale wie że są takie same).

**Co zmienia V2:** `chunk_id = HMAC(dek, plaintext)`, a DEK jest per-inode
(`data_encryption_keys UNIQUE(inode_id, key_version)`). Ten sam plaintext w dwóch plikach daje
więc **różne** `chunk_id` → dedup na poziomie chunków **przestaje działać między plikami**.
Dedup przeżywa wyłącznie przez `packs.plaintext_hash` + `find_pack_by_plaintext_hash`, czyli
na poziomie całych packów (weryfikacja czym dokładnie jest `plaintext_hash` — rozdział 4).
To realna zmiana charakterystyki systemu, nie detal implementacyjny.

Dwa warianty deszyfrowania V2 istnieją celowo:
`decrypt_chunk_v2_verified` na ścieżce daemona (jest manifest, więc znamy autorytatywny
`chunk_id`), a `decrypt_chunk_v2` dla FFI i dekryptora linków share, gdzie manifestu nie ma.

## 3.4 Hybryda post-kwantowa (α.B.b) — **zbudowana i podpięta**

Wbrew zapisom w roadmapie ML-KEM **nie jest odroczone — jest w kodzie i na ścieżce produkcyjnej**:
`omnidrive-core/hybrid.rs` → `identity::hybrid_wrap_vault_key_for_device` → wołane z
`api/vault.rs:427`. Szczegóły implementacji:

- **Kombinator X-Wing, nie XOR.** `KEK = HKDF-SHA256(salt="omnidrive-hybrid-wrap-v1",
  ikm = x25519_ss ‖ mlkem_ss, info = transkrypt)`.
- **Transkrypt jest prefiksowany długościami** (`append_field`) i wiąże: wersję schematu,
  `vault_id`, `device_id`, kyber ciphertext oraz klucz enkapsulacji. Chroni przed downgrade'em,
  splice'em i rebindingiem. Każdy z tych wektorów ma osobny test (7 testów w `hybrid.rs`).
- Format blobu: `kyber_ct (1088) ‖ aes_kw_wrapped_vk (40)` = **1128 bajtów**.
- `x25519-dalek` celowo **nie jest** zależnością `omnidrive-core` — ECDH liczy `angeld`
  i wstrzykuje gotowy shared secret. Dzięki temu core zostaje czysty pod UniFFI/mobile.
- `select_and_unwrap_vault_key` wybiera hybrydę, gdy komplet (blob + decaps + encaps) istnieje,
  inaczej spada na X25519. Migracja jest więc leniwa i bezstanowa.
- Zgodnie z FIPS 203 `ml_kem_decapsulate` **nie zgłasza błędu** przy zmanipulowanym szyfrogramie
  (implicit rejection) — zwraca pseudolosowy sekret. Wykrycie manipulacji spada na integralność
  AES-KW poziom wyżej. Jest to opisane w komentarzu i pokryte testem.

## 3.5 Cykl życia `VaultKeyStore`

`Arc<RwLock<Option<UnlockedVaultKeys>>>` — `None` znaczy „zamknięty". Klucze w `SecretBox`,
`KeyBytes` ma `ZeroizeOnDrop` i `Debug` wypisujący `KeyBytes([REDACTED])`.

W pamięci po odblokowaniu żyją **cztery** klucze: `master_key`, `vault_key` (V1),
`envelope_vault_key` (V2) i opcjonalnie `previous_envelope_vault_key` — ten ostatni tylko
podczas leniwego przepakowywania DEK-ów po rotacji, żeby stare DEK-i dało się jeszcze odczytać.

**Trzy różne rotacje, trzy różne poziomy bezpieczeństwa:**

| Operacja | Wyzwalacz | Transakcyjna? | Odzysk po awarii |
| --- | --- | --- | --- |
| `migrate_kdf_params_if_needed` | unlock, gdy `parameter_set_version < 2` | **tak** — `migrate_kdf_params_tx` | pełny rollback, jest failpoint w testach |
| `rotate_for_revocation` | odebranie dostępu urządzeniu | nie, ale **kolejkuje** DEK-i w `dek_rewrap_queue` | tak — kolejka jest w bazie |
| `rotate_vault_key` | **zmiana hasła** (`/api/auth`, `/api/vault`) | **nie i nie kolejkuje** | **brak** — patrz Z3-01 |

Docelowe parametry Argon2id: `parameter_set_version = 2`, **262 144 KiB (256 MiB)**,
`time_cost = 3`, `lanes = 1`. Migracja jest odmawiana, gdy w skarbcu jest więcej niż jedno
aktywne urządzenie (`MigrationOutcome::Declined`) — per-device KDF przesunięte do α.C.

## 3.6 Safety numbers

`fingerprint = SHA-256(envelope_vault_key ‖ user_id)` — jedno źródło dla trzech reprezentacji:
60 cyfr (12 bloków po 5, z pierwszych 24 bajtów), 12-wyrazowy BIP-39 (z pierwszych 16 bajtów)
oraz identicon. Docstring wprost zaznacza, że wszystkie trzy muszą pozostać spójne — to jedyny
komentarz w tym pliku, który tłumaczy nieoczywisty invariant, i jest zasadny.

---

## Znaleziska — rozdział 3

### 🔴 Z3-01 — zmiana hasła nie jest transakcyjna: awaria w trakcie = trwała utrata DEK-ów

`rotate_vault_key` (`vault.rs:483`, wołane z `api/auth.rs:325` i `api/vault.rs:1061` — czyli
zwykła zmiana hasła) wykonuje po kolei, **każde jako osobny zapis**:

1. `db::rotate_vault_state(...)` — nowy `encrypted_vault_key` + bump generacji,
2. `db::set_vault_config(...)` — nowy salt,
3. pętla `db::update_wrapped_dek(...)` po **wszystkich** DEK-ach.

Między krokiem 1 a końcem pętli baza jest w stanie niespójnym: `vault_state` ma już nowy VK,
a część DEK-ów jest wciąż zawinięta starym. Nowy `UnlockedVaultKeys` ustawiany na końcu ma
`previous_envelope_vault_key = None`, więc **stary VK nie jest nigdzie zachowany** — ani
w pamięci, ani w bazie.

Skutek zerwania w trakcie (crash, kill, `?` na błędzie zapisu w pętli): DEK-i, które nie zdążyły
się przepakować, są zawinięte kluczem, którego **nikt już nie potrafi odtworzyć** — stare hasło
prowadzi do starego KEK, ale `vault_state` ma już nowy `encrypted_vault_key`. Pliki tych inode'ów
są nieodzyskiwalne.

Co szczególnie zwraca uwagę: **bezpieczny wzorzec jest w tym samym pliku 100 linii niżej**.
`rotate_for_revocation` kolejkuje DEK-i w trwałej tabeli `dek_rewrap_queue` i trzyma stary VK
jako `previous_envelope_vault_key`, więc przerwanie jest odtwarzalne. `rotate_vault_key`
tego nie robi.

### 🔴 Z3-02 — uszkodzony `encrypted_vault_key` jest cicho nadpisywany

W `unlock` (`vault.rs:234`) dopasowanie to
`Some(wrapped_bytes) if wrapped_bytes.len() == WRAPPED_KEY_LEN`. Każda inna wartość — blob
uszkodzony, obcięty, o innej długości (np. 1128 B hybrydy zapisane pomyłkowo w to pole) —
wpada w gałąź `_`, która **generuje nowy losowy Vault Key i nadpisuje nim kolumnę**
(`db::store_encrypted_vault_key`).

To zamienia odwracalny problem („nie umiem odczytać klucza, zgłoś błąd, użytkownik przywraca
z backupu metadanych") w nieodwracalny („poprawny klucz właśnie przestał istnieć, wszystkie
DEK-i są martwe"). Ścieżka złego hasła jest obsłużona poprawnie (`unwrap_key` zwraca `Err`
i `?` propaguje) — problem dotyczy wyłącznie złej **długości** blobu. Brakuje wariantu błędu
w rodzaju `VaultError::CorruptedVaultKey`.

### ⚠️ Z3-03 — `Box::leak` na ścieżce błędu

`vault.rs:625`: `VaultError::InvalidConfig(Box::leak(format!("identity: {e}").into_boxed_str()))`.
`InvalidConfig` trzyma `&'static str`, więc żeby wcisnąć w to dynamiczny tekst, kod przecieka
pamięć — trwale, przy każdym wystąpieniu błędu. W długo żyjącym daemonie z powtarzającym się
błędem tożsamości to stały wyciek. Właściwa poprawka to `InvalidConfig(String)` albo osobny
wariant `Identity(IdentityError)`.

### ⚠️ Z3-04 — `payloads.rs` w całości nieużywany, `layout.rs` w 4/5 nieużywany

`omnidrive-core/payloads.rs` (134 linie: `SuperblockPayload`, `RootManifestPayload`,
`DirManifestPayload`, `FileManifestPayload`, `PackCatalogManifestPayload`,
`GcTombstoneManifestPayload`, `TailIndexPayload`) nie ma **ani jednego** odwołania poza samym
sobą. W `layout.rs` używany jest wyłącznie `ChunkRecordPrefix` plus kilka stałych;
`SuperblockFixed`, `ManifestEnvelopeFixed`, `PackHeader` i `PackFooter` są martwe.

To pozostałość po projekcie VFS z `omnidrive_vfs_technical_spec_v1.md` — formatu opartego na
superbloku i manifestach, którego produkcja nigdy nie przyjęła (stan trzyma SQLite, patrz §2.2).
Nie jest to błąd, ale ~250 linii kodu wyglądającego na obowiązujący format on-disk, którym nie
jest. Każdy czytający ten crate od zera założy, że to opis rzeczywistości.

### ⚠️ Z3-05 — trzy wyprowadzane klucze root nigdy nie użyte

`derive_root_keys` liczy pięć podkluczy, z czego `manifest_mac_key`, `lease_mac_key`
i `local_anchor_key` nie mają żadnego konsumenta (podobnie `LOCAL_CACHE_KEY_INFO`, oznaczony
`#[allow(dead_code)]`). To ta sama rodzina co Z3-04 — szkielet pod projekt VFS. Koszt to trzy
zbędne HKDF przy każdym odblokowaniu (pomijalny) i mylące wrażenie, że MAC manifestów istnieje.

### ⚠️ Z3-06 — roadmapa twierdzi, że ML-KEM jest odroczone; kod mówi inaczej

Notatki projektu opisują α.B.b jako zaplanowane „gdy wróci", tymczasem `hybrid.rs` + `pqkem.rs`
są zaimplementowane, mają 12 testów łącznie i są wołane z `api/vault.rs:427` przez
`identity.rs`. Schemat bazy ma komplet kolumn (`kyber_public_key`, `wrapped_vault_key_kyber`,
`encrypted_kyber_private_key`). To rozjazd do rozstrzygnięcia przy rewizji STATUS.md —
**nie należy planować α.B.b jako pracy do zrobienia**. Otwarte pozostaje pytanie o pokrycie
smoke'em, nie o istnienie kodu.

### ℹ️ Z3-07 — martwa gałąź w `unlock`

W `vault.rs:254` obie odnogi (`if initialized` / `else`) wykonują **identyczne** operacje —
generują klucz, wrapują, zapisują z generacją 1 i budują ten sam `UnlockedVaultKeys`. Różni je
wyłącznie treść `info!`. Ok. 25 linii duplikatu; warunek `initialized` nie wpływa na zachowanie.

---

# 4. Pipeline zapisu

> **Status rozdziału:** `packer.rs`, `watcher.rs`, `ingest.rs` przeczytane i opisane.
> `uploader.rs`, `aws_http.rs` — do dokończenia.

## 4.3 `ingest.rs` — świadome przyjęcie pliku (Inbox)

Druga, niezależna droga wejścia obok watchera. Watcher reaguje na zdarzenia systemu plików;
ingest obsługuje jawne „weź ten plik" i **kończy zamianą pliku w placeholder**.

Maszyna stanów z twardo zadeklarowanymi przejściami (`valid_transitions`):

```
PENDING ──▶ CHUNKING ──▶ UPLOADING ──▶ GHOSTED ──▶ (wiersz kasowany)
   │            │             │
   └────────────┴─────────────┴──▶ FAILED ──▶ PENDING (tylko ręcznie, z API)
```

Dwie rzeczy zrobione tu dobrze i warte zapamiętania:

1. **Przejście w bazie idzie PRZED pracą.** `transition()` robi warunkowy `UPDATE … WHERE state = ?`
   i dopiero po jego powodzeniu rusza robota. Jeśli proces zginie w połowie,
   `recover_interrupted_jobs` przy starcie zresetuje `CHUNKING`/`UPLOADING` → `PENDING`.
   Stan nigdy nie kłamie o tym, co zostało zrobione.
2. **Ghost swap czeka na potwierdzenie z chmury.** `do_uploading` odpytuje w pętli
   `summarize_pack_shards` aż wszystkie packi osiągną `Healthy` albo `Degraded`, z limitem
   `UPLOAD_TIMEOUT = 600 s` i pollingiem co 2 s. Dopiero potem plik zostaje odchudzony do
   placeholdera. To właściwa kolejność — bez tego zamiana pliku na placeholder przed trwałym
   zapisem w chmurze byłaby utratą danych.

   Uwaga: `Degraded` jest **akceptowane** jako „gotowe". Przy `EC_2_1` (Reed-Solomon 2+1) oznacza
   to 2 z 3 shardów, z których plik da się odtworzyć — więc jest to bezpieczne, ale świadomie
   rezygnuje z pełnej nadmiarowości w momencie odchudzenia pliku.

`FAILED` nie wraca samo do `PENDING` — `get_next_pending_ingest_job` bierze wyłącznie `PENDING`,
więc nieudane zadanie czeka na ręczne `retry_ingest_job` z API. To chroni przed pętlą ponowień,
ale znaczy też, że nikt nie ponowi zadania bez interwencji użytkownika.

## 4.0 `watcher.rs` — kiedy pipeline w ogóle rusza

**Bramka bezpieczeństwa na wejściu.** `run()` sprawdza dwa warunki i przy dowolnym z nich
przechodzi w tryb bierny — zostaje żywy w `tokio::select!`, ale **nie dotyka plików**:
- `OMNIDRIVE_DRY_RUN` aktywny,
- onboarding nie ma stanu `Completed` w `system_config`.

To bezpośrednia realizacja Świętej Zasady z CLAUDE.md: na świeżej maszynie watcher milczy,
dopóki onboarding trwa.

**Trzystopniowa obrona przed niepotrzebnym przepakowaniem**, w tej kolejności:
1. `metadata_unchanged` — rozmiar, `mtime` **i** `base_revision_id` zgodne z zapamiętanym stanem.
2. `content_hash` — SHA-256 treści; łapie zdarzenia od Defendera i dotknięcia `mtime`,
   przy których bajty się nie zmieniły.
3. dopiero potem `pack_file_with_expected_parent`.

Stan trzymany jest w `HashMap<PathBuf, TrackedFileState>` przekazywanej **przez `&mut`** do obu
ścieżek — zdarzeniowej i skanu okresowego. Docstring nad `scan_existing_files` tłumaczy dlaczego:
skan z własną, pustą mapą zerował `previous_state`, przez co obie bramki były martwe i każdy tick
co 30 s tworzył nową rewizję niezmienionego pliku (P2-010). To jeden z lepszych komentarzy
w repozytorium — wyjaśnia WHY, którego z kodu nie widać.

**Debounce** ma twardą podłogę 2 s (`.max(Duration::from_millis(2_000))`), niezależnie od
`OMNIDRIVE_WATCH_DEBOUNCE_MS`. Skan okresowy: 30 s, `MissedTickBehavior::Skip`.

Kasowanie pliku → `handle_deleted_path` → `soft_delete_inode` (§2.4).
`should_ignore_path` wyklucza wyłącznie `spool_dir` — nic więcej.

## 4.1 `packer.rs` — od pliku do shardów

Chunkowanie **stałym rozmiarem** `DEFAULT_CHUNK_SIZE = 4 MiB` (nie CDC, mimo że
`layout.rs` przewiduje `CHUNKING_ALGO_CDC`). Dla każdego chunka:

```
plaintext (≤4 MiB)
  │ SHA-256 (bez klucza) → plaintext_hash ─────► próba deduplikacji
  │ encrypt_chunk_v2(DEK_inode, plaintext, aad=[])
  ▼
manifest = ChunkRecordPrefix(80 B) ‖ ciphertext ‖ gcm_tag(16 B)
  │ pack_id = SHA-256(storage_mode ‖ 0x00 ‖ manifest)
  ▼
Reed-Solomon 2+1 na SAMYM ciphertext (nie na manifeście)
  ├─ shard 0 (DATA)   → cloudflare-r2
  ├─ shard 1 (DATA)   → backblaze-b2
  └─ shard 2 (PARITY) → scaleway
```

Rzeczy nieoczywiste, warte zapamiętania:

- **`SHARD_PROVIDERS` to twarda, pozycyjna stała** — `["cloudflare-r2","backblaze-b2","scaleway"]`.
  Indeks sharda **wybiera providera**. To wyjaśnia `UNIQUE(pack_id, provider)` z §2.2 i oznacza,
  że tryb `EC_2_1` jest bezużyteczny bez dokładnie tych trzech providerów. `SINGLE_REPLICA`
  jest przypięty na sztywno do `backblaze-b2`.
- **Erasure coding działa na ciphertext, nie na manifeście.** Rekonstrukcja musi więc odtworzyć
  ciphertext i dopiero potem opakować go w prefiks — dlatego `repair.rs` też importuje
  `ChunkRecordPrefix::SIZE`.
- **`split_ciphertext_into_shards` kopiuje bajt po bajcie w pętli** (`for (offset, byte) in
  ciphertext.iter().copied().enumerate()`). Dla chunka 4 MiB to 4 miliona iteracji zamiast
  dwóch `copy_from_slice`. Działa, ale to gorący punkt na ścieżce zapisu.
- **Wykrywanie konfliktów siedzi w packerze**, nie w watcherze. `pack_file_with_expected_parent`
  porównuje `expected_parent_revision_id` z bieżącą rewizją przez `classify_revision_lineage`
  i przy `Parallel` / `CurrentDescendsFromCandidate` materializuje kopię konfliktu **zanim**
  utworzy nową rewizję.
- `LocalOnly` nie tworzy shardów i od razu dostaje `PackStatus::Healthy` — nie ma czego wysyłać.

## 4.2 Deduplikacja — jak jest zbudowana

`plaintext_hash` to **zwykły SHA-256 plaintextu** (`hex_sha256`), trzymany w kolumnie
`packs.plaintext_hash`. Przy każdym chunku packer robi
`find_pack_by_plaintext_hash(hash, storage_mode)` i jeśli trafi, **pomija szyfrowanie i wysyłkę**,
podpinając nową rewizję pod istniejący pack (`is_deduplicated = true`).

Zapytanie jest **globalne** — nie ogranicza się do tego samego inode'a. I tu jest problem: patrz
**Z4-01**.

(Uwaga poboczna, nie błąd: `plaintext_hash` jest niekluczowanym hashem treści w lokalnej bazie.
Baza jest szyfrowana przed wysłaniem do chmury, więc operator go nie widzi — ale ktoś, kto zdobędzie
odszyfrowaną bazę, może sprawdzić „czy użytkownik ma plik X" bez dostępu do kluczy danych.
Wersja kluczowana — `HMAC(envelope_vault_key, plaintext)` — usunęłaby ten oracle bez utraty funkcji.)

---

## Znaleziska — rozdział 4

### 🔴 Z4-01 — deduplikacja między plikami tworzy pliki NIEODSZYFROWYWALNE (POTWIERDZONE)

**To jest najpoważniejsze znalezisko tego przeglądu.**

W V2 klucz DEK jest **per-inode** (`data_encryption_keys UNIQUE(inode_id, key_version)`,
`get_or_create_dek(pool, inode_id)`). Deduplikacja natomiast jest **globalna** — szuka packa
po `plaintext_hash` w całym skarbcu, bez ograniczenia do inode'a.

Sekwencja:

1. Plik **A** (inode 1) zawiera chunk o treści C. Packer szyfruje go `DEK₁`, tworzy pack P
   z `chunk_id₁ = HMAC(DEK₁, C)` i zapisuje `plaintext_hash = SHA-256(C)`.
2. Plik **B** (inode 2) zawiera ten sam chunk C. `find_pack_by_plaintext_hash` trafia w pack P.
3. Packer wchodzi w gałąź dedup (`packer.rs:237-264`) i przepisuje **`chunk_id` z packa A**
   do rewizji pliku B: `register_chunk(revision_B, chunk_id₁, …)`.
4. Nie powstaje nowy pack, nie ma uploadu. Zapis kończy się sukcesem — **użytkownik widzi plik
   jako zabezpieczony**.
5. **Odczyt pliku B**: `downloader/read.rs` pobiera DEK przez `get_or_create_dek(pool, inode_2)`
   → dostaje `DEK₂`. `decrypt_chunk_record` widzi `record_version = 2` i woła
   `decrypt_chunk_v2_verified(DEK₂, …)` na danych zaszyfrowanych `DEK₁`.
6. **Weryfikacja tagu GCM zawodzi. Nie ma żadnego fallbacku** (`chunk.rs:175-187` — gałąź `2`
   ma jeden klucz i koniec). Plik B jest nie do odzyskania.

**Dlaczego nie ratuje tego `dek_id_hint`:** `ChunkRecordPrefix` ma pole `dek_id_hint`, które
packer sumiennie wypełnia (`packer.rs:612`). Ale w całym repozytorium **nie ma ani jednego
miejsca, które by je czytało** — weryfikacja: `grep dek_id_hint` zwraca wyłącznie definicję
w `layout.rs` i dwa zapisy w `packer.rs`. Mechanizm ratunkowy istnieje w formacie i nie jest
podłączony.

**Kiedy to wystrzeli:** przy dowolnym powtórzonym chunku 4 MiB między dwoma plikami.
Najzwyklejszy przypadek to **kopia pliku** — użytkownik robi `raport.docx` → `raport-kopia.docx`,
i kopia jest od razu martwa. Dalej: te same załączniki w dwóch folderach, wspólny nagłówek
dwóch dokumentów z szablonu, plik przeniesiony i odtworzony.

**Skąd to się wzięło:** w V1 `chunk_id = HMAC(vault_key, plaintext)` — jeden klucz na cały
skarbiec, więc dedup między plikami był **poprawny**. Przejście na kopertowe DEK-i per-plik
unieważniło założenie, na którym stała ta gałąź, ale gałąź została.

**Dlaczego testy tego nie widzą:** testy packera (`packer.rs:722+`) tworzą pojedyncze inode'y.
Dedup w obrębie jednego inode'a używa tego samego DEK, więc działa poprawnie i test przechodzi.
Żaden test nie pakuje dwóch różnych inode'ów o wspólnej treści, a potem ich nie odczytuje.

#### Z4-01 ma DWA wektory — i wystrzelił ten drugi

Ta sama przyczyna źródłowa („odwołanie do cudzego `chunk_id` przy DEK-u per-inode") ma dwie
niezależne drogi wejścia:

| Wektor | Kod | Czy wystrzelił na tej maszynie |
| --- | --- | --- |
| **A — deduplikacja między plikami** | `packer.rs:237-264` | **nie** (jeszcze) |
| **B — materializacja kopii konfliktu** | `db/conflicts.rs:121` → `copy_chunk_refs` | **tak, raz** |

Wektor B jest wręcz prostszy do wywołania: `materialize_conflict_copy_from_revision` tworzy
**nowy inode** (`create_inode`, linia 90) i kopiuje do niego `chunk_refs` źródłowej rewizji
(`copy_chunk_refs`, linia 121). Nowy inode = nowy DEK = te same `chunk_id` pod innym kluczem.
**Każda kopia konfliktu jest z definicji nieodszyfrowywalna.** Nie potrzeba do tego dedupu.

#### Triage na produkcyjnej bazie (2026-08-01)

Wykonany na **kopii** `omnidrive.db` w scratchpadzie, tryb `mode=ro`, zero zapisu do oryginału.

```
inodes FILE: 1 żywy + 3 soft-deleted     packs: 10 (2× V1, 8× V2)
file_revisions: 1480                     chunk_refs: 2909 (8 unikalnych chunk_id)
data_encryption_keys: 6                  conflict_events: 1

chunk_id współdzielony przez >1 inode: 1  (w packu V2 → zagrożony)
  inode 15  placeholder-probe.bin                             [soft-deleted]
  inode 16  placeholder-probe (conflict - PN-THINKPAD - …).bin [soft-deleted]

packi z tym samym plaintext_hash >1 raz: 0  → wektor A nigdy nie wystrzelił
```

**Wnioski z triage'u:**

1. **Realnych danych użytkownika nie ma i nic nie zostało utracone.** Skarbiec zawiera wyłącznie
   artefakty testowe (`smoke-5mb.bin`, `*-probe.bin`), z czego trzy są soft-deleted.
2. **Jedyna faktyczna kolizja to kopia konfliktu** — `chunk_id 05EBEF25…` w packu
   `0efbd6fd…` (V2) wisi pod rewizjami inode'a 15 (1468, 1469) **oraz** 16 (1472).
   Inode 15 ma `dek_id=5`, inode 16 ma `dek_id=6` — różne klucze, ten sam szyfrogram.
   Inode 16 jest nieodczytywalny.
3. **γ.b „conflict-copy" jest zbudowane i wadliwe.** Roadmapa opisywała je jako
   „zbudowane/nietestowane" — ten triage jest tym brakującym testem i wypada negatywnie.
4. Efekt uboczny wart odnotowania: inode 11 ma **1429 rewizji** dla jednego pliku — osad po
   naprawionym w 12.7c churnie rewizji watchera. Dane zostały, choć bug jest załatany.

**Konsekwencja dla priorytetów:** ryzyko jest przyszłe, nie zrealizowane. Ale wektor B
(kopie konfliktu) jest aktywny przy każdym konflikcie cross-device — czyli dokładnie w scenariuszu
smoke'u β.a na Dellu, który jest następny w kolejce.

#### NAPRAWIONE 2026-08-01 — klucz przeniesiony z inode'a na pack

Zasada: **klucz podąża za danymi, nie za nazwą pliku.** Pack jest jednostką szyfrowania,
transportu i naprawy; inode to tylko jedno z możliwych odwołań do niego.

| Element | Gdzie |
| --- | --- |
| `pack_deks(pack_id → dek_id)` | `db/schema.rs` |
| `set_pack_dek`, `get_pack_dek_id`, `get_wrapped_dek_by_id`, `next_dek_key_version`, `creating_inode_for_pack` | `db/vault_state.rs` |
| `create_pack_dek`, `dek_for_pack` | `vault.rs` |
| Świeży DEK na każdy nowy pack | `packer.rs` |
| Klucz rozwiązywany przez pack (4 miejsca) | `downloader/read.rs`, `downloader/prefetch.rs` |

Osobna tabela zamiast kolumny w `data_encryption_keys`, bo tamta ma `UNIQUE(inode_id, key_version)`,
którego w modelu addytywnym (§2.3) nie da się zdjąć — ta sama pułapka co w Z2-01.

**Stare packi naprawiają się same.** `dek_for_pack` przy braku mapowania wyznacza inode twórcy
przez najwcześniejszą rewizję i dopisuje wpis. Dzięki temu `inode 16` z triage'u staje się
odczytywalny bez ręcznej migracji. Uwaga: to ścieżka odczytu, która **pisze do bazy** —
idempotentnie i pod `ON CONFLICT DO NOTHING`, ale świadomie.

Testy (obie ścieżki oglądane na czerwono przed naprawą):
`packer::tests::deduplicated_chunk_stays_readable_from_the_second_inode`,
`packer::tests::conflict_copy_stays_readable`.

#### Następstwo: linki share stały się dwupoziomową kopertą

DEK per pack oznacza, że plik wielochunkowy ma wiele kluczy, a link niósł dotąd jeden.
Rozwiązanie: fragment URL niesie losowy `share_key`, a daemon trzyma
`AES-GCM(HKDF(share_key,"omnidrive-share-dek-v1"), DEK_pack, aad = pack_id)` w tabeli
`share_pack_keys`. Serwer przechowuje więc wyłącznie szyfrogramy, których sam nie umie otworzyć —
zero-knowledge zostaje nienaruszone, a link zostaje krótki.

**Pułapka interoperacyjności, warta zapamiętania:** Rust `derive_subkey` używa
`Hkdf::from_prk` — czyli **samego HKDF-Expand, bez kroku extract**. WebCrypto ma tylko pełne
HKDF (extract + expand), więc nie da się go tu użyć. Przeglądarka liczy ten jeden blok ręcznie:
`HMAC-SHA256(share_key, info ‖ 0x01)`. Kontrakt jest przypięty testem
`sharing::tests::share_wrapping_key_is_one_hmac_block` — jeśli padnie, linki przestaną działać
w przeglądarce, mimo że reszta suity będzie zielona.

**Zmiana łamiąca:** linki utworzone przed tą zmianą zwracają `410 Gone`
(`"share predates per-pack keys and must be recreated"`) zamiast po cichu deszyfrować śmieciem.
Trzeba je wygenerować ponownie.

### ⚠️ Z4-02 — `split_ciphertext_into_shards` przepisuje bajt po bajcie

`packer.rs:554` — pętla `for (offset, byte) in ciphertext.iter().copied().enumerate()`
z dzieleniem i modulo na każdy bajt. Dla domyślnego chunka 4 MiB to ~4,2 mln iteracji na chunk,
zamiast dwóch `copy_from_slice` na wyliczonych zakresach. Semantyka jest poprawna, koszt
niepotrzebny — i leży dokładnie na ścieżce, po której idzie każdy zapisywany bajt.

### 🔴 Z4-06 — ingest ocenia każdy pack regułami `EC_2_1`, więc tryby inne niż EC zawsze „padają"

`ingest.rs:391` woła `db::resolve_pack_status(summary)` — wariant **bez** trybu składowania,
który w `db/packs.rs:466` na sztywno podstawia `StorageMode::Ec2_1`. Wszystkie pozostałe miejsca
w kodzie (`scrubber.rs:259`, `uploader.rs:546`, `uploader.rs:809`) używają
`resolve_pack_status_for_mode` z faktycznym trybem packa. Ingest jest jedynym wyjątkiem.

Skutek, wprost z tabeli progów w §2.2:

| Tryb packa | Stan po udanym uploadzie | Werdykt reguł `EC_2_1` |
| --- | --- | --- |
| `EC_2_1` | 3 shardy `COMPLETED` | `Healthy` — poprawnie |
| `SINGLE_REPLICA` | 1 shard `COMPLETED` | `completed < 2` i brak pending → **`Unreadable`** |
| `LOCAL_ONLY` | 0 shardów (z założenia) | same zera → **`Unreadable`** |

`Unreadable` ustawia `any_failed = true`, więc `do_uploading` zwraca błąd
„one or more packs failed upload", a zadanie ląduje w `FAILED`. Plik z polityką `STANDARD`
lub `LOCAL` **nigdy nie przejdzie przez Inbox**, mimo że jego packi są w komplecie.

Łagodzące: ponieważ `do_uploading` zawodzi **przed** ghost swapem, oryginalny plik zostaje
nietknięty — to fałszywa porażka, nie utrata danych. Poprawka to pobranie trybu packa i użycie
`resolve_pack_status_for_mode`, dokładnie jak robi to `uploader.rs`.

### ⚠️ Z4-04 — każdy restart daemona przepakowuje wszystkie pliki

`TrackedFileState` — w tym `content_hash` — żyje **wyłącznie w pamięci** (`processed_files`).
Po restarcie mapa startuje pusta, więc dla każdego pliku `previous_state` to `None`:
`metadata_unchanged` jest fałszem, a bramka `content_hash` jest pomijana, bo nie ma się do czego
porównać. W efekcie pierwszy skan po starcie **przepakowuje cały watch root** i tworzy nową
rewizję na każdy plik.

To ta sama klasa błędu co naprawiony P2-010 — tam skan okresowy dostawał pustą mapę, tu daje ją
restart. Skutek jest łagodniejszy (deduplikacja po `plaintext_hash` blokuje ponowny upload, więc
chmura nie krwawi), ale `file_revisions` i `chunk_refs` rosną liniowo z liczbą restartów.
Ślad tego widać wprost w produkcyjnej bazie: **inode 11 ma 1429 rewizji jednego pliku**.

Naprawa wymaga utrwalenia `content_hash` — np. kolumna przy rewizji albo tabela obok
`smart_sync_state` — żeby stan przeżył restart.

### ⚠️ Z4-05 — watcher zostaje bierny do restartu po zakończeniu onboardingu

Bramka trybu biernego jest sprawdzana **raz, przy starcie `run()`**. Gdy użytkownik dokończy
onboarding w działającym daemonie, watcher śpi dalej w pętli `sleep(3600 s)` — pliki nie są
zabezpieczane, dopóki ktoś nie zrestartuje procesu. Komunikat w logu mówi o tym wprost
(`restart daemon after onboarding completes`), więc jest to znane, ale z perspektywy użytkownika
wygląda jak cicha awaria synchronizacji tuż po udanej konfiguracji.

### ⚠️ Z4-03 — providerzy zaszyci pozycyjnie w kodzie

---

# 5. Pipeline odczytu

## 5.1 Mapa warstwy

| Plik | Linie | Rola |
| --- | --- | --- |
| `downloader/read.rs` | 657 | `restore_file`, `read_range`, hydracja pojedynczego chunka. |
| `downloader/pack.rs` | 285 | Pobranie packa: wybór providera, EC, fallback na peera. |
| `downloader/chunk.rs` | 232 | Parsowanie `ChunkRecordPrefix` i deszyfrowanie rekordu. |
| `downloader/provider.rs` | 190 | Konstrukcja providerów, `probe_latency`. |
| `downloader/prefetch.rs` | 103 | Wyprzedzające pobieranie sąsiednich chunków. |
| `cache.rs` | 278 | Cache chunków na dysku — **szyfrowany**, LRU. |

## 5.2 Ścieżka odczytu chunka — pięć poziomów, w tej kolejności

```
1. cache lokalny        cache.get_chunk(revision_id:chunk_index)
2. peer w LAN           try_fetch_chunk_from_peer  (tylko w load_plaintext_chunk)
3. spool pobrań         plik już leży w download_spool_dir
4. chmura               download_shard × N, wybór po zmierzonym opóźnieniu
5. rekonstrukcja EC     reconstruct_ciphertext, gdy brakuje sharda
```

**Wybór providera jest mierzony, nie zgadywany.** `download_pack` odpytuje `probe_latency`
dla każdego sharda, sortuje kandydatów po `(opóźnienie, czy status = COMPLETED)` i pobiera
**tylko tyle shardów, ile trzeba** (`required_shards`: 2 dla `EC_2_1`, 1 dla `SINGLE_REPLICA`),
po czym przerywa pętlę. Dzięki temu zdrowy pack EC nigdy nie kosztuje trzeciego pobrania.

Warto znać koszt operacyjny tej strategii: sonda też przechodzi przez `cloud_guard`
z `count: 1`, więc jeden pack `EC_2_1` to **3 sondy + 2 pobrania = 5 operacji odczytu**.
Przy `DEFAULT_CLOUD_DAILY_READ_OPS_LIMIT = 5000` daje to ok. 1000 chunków, czyli ~4 GiB
odczytu na dobę, zanim bezpiecznik zacznie odmawiać.

Deduplikacja równoległych pobrań: `pack_download_locks` (mapa `Mutex` per `pack_id`) sprawia,
że gdy kilka callbacków cfapi zażąda tego samego packa, do chmury idzie tylko pierwszy —
reszta czeka na mutex i trafia w gotowy plik w spoolu.

## 5.3 `cache.rs` — cache też jest zero-knowledge

Cache **nie trzyma plaintextu**. Każdy chunk jest szyfrowany AES-256-GCM kluczem wyprowadzonym
z `master_key` (`derive_cache_key`), a jako AAD idzie `cache_key` (`revision_id:chunk_index`) —
więc podmiana pliku cache'u między chunkami zostanie wykryta.

Cache sam się leczy: nieudane deszyfrowanie albo niezgodność rozmiaru kasuje wpis i zgłasza
chybienie, zamiast zwrócić śmieci. Eksmisja LRU po `last_accessed_at`, z ochroną właśnie
zapisanego klucza (`protected_key`), żeby świeży wpis nie wypadł natychmiast przy pełnym cache'u.
Metryki trafień/chybień w `AtomicU64` w `OnceLock` — globalne, wspólne dla wszystkich instancji.

---

## Znaleziska — rozdział 5

### 🔴 Z5-01 — cache zapisuje dane do alternatywnych strumieni NTFS (POTWIERDZONE)

`cache_path_for_key` buduje ścieżkę `root/aa/bb/{cache_key}.bin`, gdzie
`cache_key = format!("{revision_id}:{chunk_index}")`. Nazwa pliku zawiera więc **dwukropek**,
a Windows interpretuje `plik:strumień` jako Alternate Data Stream, nie jako nazwę.

**Weryfikacja** (zapis pliku `1468:0.bin` w pustym katalogu):

```
zapis:  OK        odczyt: OK (dane wracają)
os.listdir  →  ['1468']            ← widać plik "1468", 0 bajtów
dir /r      →  23  1468:0.bin:$DATA ← dane siedzą w strumieniu
```

Czyli: **działa przez przypadek.** Wszystkie chunki jednej rewizji lądują jako strumienie
doczepione do jednego zerobajtowego pliku o nazwie równej `revision_id`. Konsekwencje:

- **Zajętość dysku jest niewidoczna.** Eksplorator, `dir`, większość narzędzi pokazują 0 bajtów.
  Użytkownik szukający, co zjadło 50 GiB (`DEFAULT_CACHE_MAX_BYTES`), nie znajdzie nic.
  Rozliczenie w `evict_if_needed` opiera się na sumie z bazy, więc sama eksmisja działa.
- **Działa wyłącznie na NTFS.** Na exFAT (pendrive), FAT32 czy udziale sieciowym zapis się
  wywali — a `OMNIDRIVE_CACHE_DIR` jest konfigurowalny.
- Strumienie są **po cichu gubione** przy kopiowaniu na inne systemy plików i bywają usuwane
  przez narzędzia backupowe oraz antywirusy.
- `delete_entry` kasuje strumień, ale **plik-nosiciel zostaje** jako zerobajtowa sierota;
  nic go nigdy nie sprząta.

Naprawa jest jednoliniowa — separator inny niż `:` w `cache_key` (np. `-`) albo kodowanie
nazwy pliku z samego skrótu, który i tak już jest liczony w `cache_path_for_key`.

---

# 6. Integralność danych

Cztery workery pilnujące, żeby to, co poszło do chmury, dało się z niej odzyskać —
plus bezpiecznik, który powstał po incydencie „B2 bleeding".

| Plik | Linie | Rola |
| --- | --- | --- |
| `repair.rs` | 881 | Odtworzenie brakujących shardów z parzystości; rekoncyliacja trybu składowania. |
| `scrubber.rs` | 504 | Okresowa weryfikacja shardów w chmurze (LIGHT / DEEP). |
| `cloud_guard.rs` | 305 | Bezpiecznik: DRY-RUN, dzienne limity, wyłącznik awaryjny. |
| `gc.rs` | 275 | Kasowanie osieroconych packów. |
| `migrator.rs` | 485 | Przepakowanie V1 → V2 (opisane w §3.3). |
| `diagnostics.rs` | 153 | Statusy workerów dla UI. |

## 6.1 `cloud_guard.rs` — bezpiecznik po „B2 bleeding"

Każda operacja chmurowa przechodzi przez `current_decision`, które zwraca jeden z czterech
werdyktów: `Allowed`, `DryRun`, `Suspended`, `QuotaExceeded`. Kolejność sprawdzeń ma znaczenie:

```
1. DRY-RUN?      (env ALBO flaga w system_config)   → DryRun, nic nie leci do chmury
2. zawieszone?   (system_config cloud_suspended)     → Suspended
3. limit dzienny (apply_cloud_usage_delta_with_limits, BEGIN IMMEDIATE)
      przekroczony → set_cloud_suspension + QuotaExceeded
4. dopisz do licznika sesji (AtomicU64 w OnceLock)   → Allowed
```

**Kluczowa właściwość:** sprawdzenie limitu i inkrementacja licznika są w jednej transakcji
`BEGIN IMMEDIATE` (§2.6). Bez tego dwa workery przepuściłyby operację ponad limit — a to był
dokładnie mechanizm „B2 bleeding". Liczniki dzienne trzymane w `cloud_usage_daily`, sesyjne
w pamięci.

Domyślne progi z `config.rs`: 1 000 zapisów, 5 000 odczytów, **500 MiB egressu na dobę** —
ten ostatni jest najciaśniejszy i to on odpali się pierwszy.

## 6.2 `gc.rs`, `scrubber.rs`, `repair.rs` — skrót

- **`gc`** (co 10 s) kasuje packi bez żadnego `chunk_refs` wskazującego na nie
  (`gc_orphan_packs`, §2.1). Usuwa komplet: `upload_job_targets` → `upload_jobs` →
  `pack_locations` → `packs`, a `pack_shards` schodzi kaskadą FK.
- **`scrubber`** wybiera shardy do weryfikacji zapytaniem, które priorytetyzuje nigdy
  nieweryfikowane (`last_verified_at IS NULL` idzie pierwsze), potem najstarsze, potem te
  z największą liczbą wcześniejszych porażek. Rozróżnia weryfikację `LIGHT` (rozmiar) i `DEEP`
  (suma kontrolna, kosztuje egress). Używa `resolve_pack_status_for_mode` z faktycznym trybem.
- **`repair`** odtwarza brakujące shardy z parzystości Reed-Solomon i dokonuje rekoncyliacji,
  gdy faktyczny tryb składowania packa rozjechał się z polityką (`get_next_pack_requiring_reconciliation`).

> **Głębokość przeglądu:** `cloud_guard.rs` i `gc.rs` przeczytane w całości.
> `scrubber.rs` i `repair.rs` — struktura i punkty styku z bazą; pełne czytanie ich logiki
> rekonstrukcji EC pozostaje do zrobienia.

---

## Znaleziska — rozdział 6

### 🔴 Z6-01 — wyłącznik awaryjny zatrzaskuje się do restartu daemona (POTWIERDZONE)

Po przekroczeniu dziennego limitu `current_decision` woła `set_cloud_suspension`, które zapisuje
`cloud_suspended = 1` w `system_config`. Sprawdzenie tej flagi (linia 138) jest **przed** logiką
limitów dziennych, więc od tej chwili każda operacja zwraca `Suspended`.

`clear_cloud_suspension` ma w całym kodzie **dokładnie jednego wołającego**:
`sync_runtime_flags` (`cloud_guard.rs:99`), uruchamiane wyłącznie z `main.rs` przy starcie.
Nie ma endpointu API, nie ma zadania czyszczącego, nie ma resetu o północy.

**Skutek:** licznik dzienny zeruje się o północy (nowy `day_epoch`), ale flaga zawieszenia — nie.
Użytkownik, który w środę w południe wyczerpie 500 MiB egressu, ma OmniDrive **martwy aż do
restartu daemona** — nie do końca doby, tylko na zawsze. Objaw wygląda jak zerwana łączność
z chmurą, a przyczyna jest jednym wierszem w `system_config`.

Naprawa: czyścić zawieszenie przy zmianie `day_epoch` albo wystawić je jako operację w API.

### ⚠️ Z6-02 — `AppConfig::from_env()` przy każdej operacji chmurowej

`current_decision` na wejściu robi pełne `AppConfig::from_env()` — ok. 20 odczytów zmiennych
środowiskowych i cztery `RuntimePaths::detect()` w środku (Z1-07). Do tego 2–3 zapytania do bazy
na flagi i osobna transakcja `BEGIN IMMEDIATE` na licznik.

Rachunek dla jednego packa `EC_2_1` przy odczycie (§5.2): 5 wywołań strażnika × (1 × from_env
+ ~4 zapytania) ≈ **20 zapytań do SQLite i 100 odczytów env na każde 4 MiB**. Nie jest to błąd
poprawnościowy, ale to najgorętsza ścieżka w programie i najtańsze możliwe przyspieszenie
całego I/O.

`SHARD_PROVIDERS: [&str; 3] = ["cloudflare-r2","backblaze-b2","scaleway"]` oraz
`SINGLE_REPLICA_PROVIDER = "backblaze-b2"` to stałe kompilacji, choć providerzy są
konfigurowalni w bazie (`provider_configs`). Skonfigurowanie innego zestawu (albo tylko dwóch)
nie zmienia tego przypisania — shardy dalej pójdą pod te nazwy, a `EC_2_1` po cichu nigdy nie
osiągnie `COMPLETED_HEALTHY`. To wyjaśnia, dlaczego zaległość „Scaleway IAM" blokuje więcej,
niż mogłoby się wydawać.
