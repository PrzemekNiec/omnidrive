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

## ✅ PRZEGLĄD ZAMKNIĘTY — 2026-08-02

Wszystkie dziesięć warstw przeczytane, plus rozdział 11 domykający fragmenty, które
przy pierwszym przejściu zostały pominięte. **147 znalezisk**: **43 × 🔴**, **100 × ⚠️**,
**4 × ✅** (naprawione w trakcie: Z4-01, Z6-04, Z6-05, Z6-06).
Sześć sesji, 121 plików `.rs`, ~48 000 linii kodu plus ~7600 linii statyków.

**Uczciwie o pokryciu.** Po warstwie 10 tabela statusu mówiła „pełne czytanie" tam, gdzie
czytanie było wybiórcze — największą luką było `api/diagnostics.rs` (moduł bez żadnej
kontroli dostępu, przeczytane ~40 z 738 linii). Rozdział 11 domyka te luki i przynosi
**15 znalezisk, w tym 4 × 🔴 — dwie funkcje, które w ogóle nie działają** (Z11-01, Z11-03).
Jedyne, czego nie czytano linia po linii, to warstwa prezentacji `index.html`
(~2700 linii renderowania DOM poza obiegiem tokenu, escapowaniem i ładowaniem skryptów)
oraz `legacy.html` poza ustaleniem, że nie uwierzytelnia żadnego żądania.

Ten dokument przestaje być „stanem przeglądu", a staje się mapą architektury z rejestrem
długu. Sekcja poniżej zostaje jako zapis metody i przebiegu — przydaje się przy kolejnym
takim przedsięwzięciu.

**Trzy rzeczy, które przegląd ustalił ponad pojedynczymi znaleziskami:**

1. **Uwierzytelnienie API jest pozorne.** `Z9-01` (`GET /api/vault/status` bez auth wystawia
   token sesji) znosi działanie każdego `require_role` w projekcie, a `Z9-21` (`rotate-key`
   bez znajomości starego hasła) zamienia ten token w przejęcie Skarbca. Do tego trzy
   niezależne kanały omijające API w całości: `Z8-01` (Named Pipe dla `Everyone`),
   `Z8-02` (`trusted = 1` z broadcastu UDP), `Z9-02` („Windows Hello" bez auth, przez CSRF).
2. **Cross-device nie działa end-to-end.** `Z8-03` blokuje przeszczep na urządzeniu, które
   kiedykolwiek się odblokowało, a `Z8-04` psuje odczyt plików po przeszczepie, który się uda.
   To jest ta sama klasa co naprawione `Z4-01`, tylko na styku dwóch maszyn.
3. **Klienci rozjechali się z API.** `Z7-01` (menu rejestrowe → 401), `Z10-01`
   (CLI → 403 w 6 z 12 komend) i `Z11-03` (`legacy.html` → 403 w 9 z 21) to jeden błąd
   popełniony raz, przy wprowadzaniu ról, i nieprzeprowadzony przez żadnego klienta poza
   `index.html` i `wizard.js`. `Z10-14` mówi dlaczego nikt tego nie zauważył — nie ma testu,
   który przechodzi listę endpointów i sprawdza uwierzytelnienie.
4. **Testy potwierdzają wykonanie kroków, nie właściwości systemu.** `Z11-04` jest tego
   skrajnym przykładem: `e2e_basic` asertuje zdrowie pięciu workerów, których w tym trybie
   nie ma, bo gałąź testowa w `main.rs` wpisuje im status `Idle`. `Z11-15` pokazuje to samo
   od drugiej strony — test regresyjny `Z4-01` używa ładunku, przy którym badana ścieżka
   nie może wystąpić. Wyjątkiem jest `e2e_scrubber_repair`, który sprawdza właściwość
   („odczyt nie zawodzi w trakcie naprawy") zamiast kroków.

---

## ⏸️ STAN PRZEGLĄDU — zapis przebiegu

**Ostatnia sesja: 2026-08-02 (piąta).** Warstwy 9 i 10 domknięte. Warstwa 10:
15 znalezisk (Z10-01..Z10-15, 3 × 🔴) na 5281 linii — CLI, tray, rozszerzenie powłoki,
dwa artefakty uboczne i 19 funkcji testowych rozłożonych na 3372 linie.

**Wcześniej w tej samej sesji:** warstwa 9 **domknięta** — rozdziały 9 i 9b,
**31 znalezisk (Z9-01..Z9-31, 9 × 🔴)**. Przeczytane: całe `api/*` (7713 linii),
`share.html` + `share-sw.js`, `wizard.js`, przegląd `index.html` pod kątem obiegu tokenu
i escapowania.

**Najcięższe: `Z9-01`** — `GET /api/vault/status` bez uwierzytelnienia wystawia token sesji,
gdy Skarbiec jest odblokowany. To znosi działanie wszystkich `require_role` w całym API.
Sonda potwierdza skutek uboczny w bazie: 131 sesji, 22 w jednej minucie, 100 % wygasłych
i nieusuniętych — i to właśnie te wiersze wywalają graft z `Z8-03`. Z2-04, Z8-03 i Z9-01
spotykają się w jednej tabeli.

Do tego trzy rzeczy, które razem z Z9-01 tworzą gotowy łańcuch przejęcia: **Z9-21**
(`rotate-key` zmienia hasło bez znajomości starego), **Z9-20** (`setup-provider` bez auth
przestawia bucket i klucze działającego Skarbca) i **Z9-22** (odwołanie urządzenia melduje
sukces mimo nieudanej rotacji klucza). Osobno **Z9-23**: tryb LAN Share nie może zadziałać,
bo `crypto.subtle` wymaga bezpiecznego kontekstu, a link LAN to `http://` po adresie IP —
i strona obwinia za to przeglądarkę odbiorcy.

**Druga korekta poprzednich warstw** (po Z7-01 w sesji czwartej): Z2-04 mówiło o dwóch
niewołanych funkcjach sprzątających — `delete_expired_oauth_states` jest wołane
(`oauth.rs:39`). Bez wywołań pozostaje wyłącznie `cleanup_expired_sessions`.

---

**Sesja 2026-08-02 (czwarta).** Warstwa 8 domknięta — `onboarding.rs` (1341),
`db/graft.rs` (1621), `disaster_recovery.rs` (3044), `peer.rs` (592), `pipe_server.rs` (360)
przeczytane w całości, wynik w rozdziale 8, **18 nowych znalezisk (5 × 🔴)**. Pliki okazały się
o ~1000 linii dłuższe niż mówił poprzedni spis (tamten liczył bez testów).

Sondy znów zrobiły robotę, tym razem trzy: `PRAGMA foreign_keys = OFF` w transakcji jest no-opem
(fk=1 po wyłączeniu), odtworzenie sekwencji `DELETE` grafta na kopii bazy roboczej wywala się na
`DELETE FROM users` (Z8-03), a odwzorowanie obu zapytań fallbacku DEK pokazuje zły klucz dla
2 z 3 packów (Z8-04). Reguła „najpierw sprawdź fallback" uratowała przed dwoma fałszywymi
alarmami (rozjazd soli KDF w graftcie — sól idzie z `vault_config`; zakleszczenie backoffu peera —
czyści je udany handshake) i wykryła **błąd w poprzednim rozdziale**: Z7-01 dotyczy rejestrowego
menu kontekstowego, a nie DLL-a rozszerzenia, który w ogóle nie ma zależności HTTP (§8.8).

**Najpilniejsze z tej warstwy:** Z8-04 (klucz packa nie przechodzi przez granicę urządzenia)
i Z8-03 (join-existing pada na FK) razem oznaczają, że **„Join Existing Vault" nie działa
end-to-end** — pierwsze blokuje przeszczep na używanym urządzeniu, drugie psuje odczyt plików
po udanym przeszczepie. To jest ta sama klasa co Z4-01, tylko na styku dwóch maszyn.

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
| 4. Pipeline zapisu | ✅ pełne czytanie (`uploader.rs` + `aws_http.rs` domknięte — rozdział 4b) |
| 5. Pipeline odczytu | ✅ `downloader/*` + `cache.rs` |
| 6. Integralność | ✅ pełne czytanie (`scrubber.rs` + `repair.rs` domknięte — rozdział 6b) |
| 7. Windows / Ghost Shell | ✅ pełne czytanie (18 znalezisk, 7 × 🔴) |
| 8. Cross-device | ✅ pełne czytanie (18 znalezisk, 5 × 🔴) |
| 9. API i Web UI | ✅ pełne czytanie (31 znalezisk, 9 × 🔴 — rozdziały 9 i 9b) |
| 10. Satelity i testy | ✅ pełne czytanie (15 znalezisk, 3 × 🔴) |

### Co zostało do przeczytania

**Nic — przegląd całego kodu jest zamknięty.** Jedyne, czego nie czytano linia po linii, to
`static/legacy.html` (2258, ekran zastępczy pod `/legacy`) i część `index.html` poza obiegiem
tokenu, escapowaniem i ładowaniem skryptów.

Uwaga do planowania: liczby w tym spisie pochodzily z outline'u bez testow. Warstwa 8 miala
w nim 6200 linii, a realnie 7191. Warstwa 10 (same testy) urosnie najbardziej.

### Decyzje z sesji 2026-08-01 (druga)

- **Z7-02 zostaje w rejestrze, nie jest naprawiane teraz** (decyzja Przemka) — mimo że jako
  jedyne z 47 znalezisk kompromituje hasło główne, a nie pojedyncze urządzenie.
- **Z7-04**: pochodzenie ustalone przez `git log -S` (commit `04a58e7`, „test: stabilize Smart
  Sync e2e bootstrap diagnostics", 2026-03-24) — ACE wszedł ubocznie przy naprawianiu
  bootstrapu w testach, nie ma commita, który by go uzasadniał. Przed usunięciem trzeba to
  sprawdzić na żywo, patrz §7.7.

### Zadania otwarte poza przeglądem

- **Z8-04** — *dotyka integralności danych, do decyzji Przemka.* Graft nie kopiuje `pack_deks`,
  a fallback nie tylko zgaduje źle, ale **zapisuje błędne powiązanie na stałe** (`vault.rs:501`)
  i wynosi je do chmury kolejną kopią metadanych. Naprawa ma dwie części: dopisać `pack_deks`
  do grafta (kilkanaście linii, wzorem pozostałych tabel) i zawęzić fallback tak, żeby przy
  wielu DEK-ach na inode nie zapisywał zgadywanki. Do rozstrzygnięcia: czy ruszać to teraz
  (przed β.a smoke na Dellu), skoro to jedyna droga, którą Dell dostaje klucze.
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
4b. [Pipeline zapisu — dokończenie](#4b-pipeline-zapisu--dokonczenie-uploaderrs-aws_httprs)
6. [Integralność danych](#6-integralnosc-danych)
6b. [Integralność — dokończenie](#6b-integralnosc--dokonczenie-scrubberrs-repairrs)
7. [Windows / Ghost Shell](#7-windows--ghost-shell)
8. [Cross-device](#8-cross-device--dolaczanie-urzadzen-kopia-metadanych-mesh-lan)
9. [API i Web UI](#9-api-i-web-ui--czesciowo-punkt-wznowienia-w-910)
9b. [API i Web UI — dokończenie](#9b-api-i-web-ui--dokonczenie-maintenance-vault-oauth-files-statyki)
10. [Satelity i testy](#10-satelity-i-testy)
11. [Domknięcie luk](#11-domkniecie-luk--co-znalazlo-sie-w-nieprzeczytanych-fragmentach)

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
| Z2-04 | ⚠️ | `cleanup_expired_sessions` nigdy nie wołane (**korekta §9b.9:** `delete_expired_oauth_states` **jest** wołane w `oauth.rs:39`) | grep |
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
| Z4-07 | 🔴 | Pack zablokowany kwotą wraca co 2 s bez końca — `mark_*_failed` nie rusza `attempts` | czytanie + `db/shards.rs:163` |
| Z4-08 | 🔴 | `is_retryable()` uznaje 403/404 za przejściowe — odwołane klucze ponawiane ~37 dni | czytanie |
| Z4-09 | 🔴 | `UploadWorker::run` kończy pętlę na dowolnym błędzie SQLite (w parze z Z1-02) | czytanie |
| Z4-10 | ⚠️ | `all_from_env()` wymaga kompletu 3 providerów; `ALLOW_EMPTY_UPLOADERS` daje zero | czytanie |
| Z4-11 | ⚠️ | Trzy warstwy ponawiania mnożą się (SDK `adaptive` × attempt timeout × pętla) | czytanie |
| Z4-12 | ⚠️ | `with_webpki_roots()` — magazyn zaufania systemu ignorowany | czytanie |
| Z4-13 | ⚠️ | `allow_http` z prefiksu endpointu — literówka cicho degraduje transport do HTTP | czytanie |
| Z4-14 | ⚠️ | `#![allow(dead_code)]` na całym pliku 1116 linii | czytanie |
| Z5-01 | 🔴 | Cache pisze do alternatywnych strumieni NTFS (`:` w nazwie pliku) | sonda NTFS |
| Z6-01 | 🔴 | Wyłącznik awaryjny chmury zatrzaskuje się do restartu daemona | grep: 1 wołający |
| Z6-02 | ⚠️ | `AppConfig::from_env()` przy każdej operacji chmurowej | czytanie |
| Z6-03 | 🔴 | Scrubber weryfikuje shardy jeszcze niewysłane — `PENDING` idzie pierwszy w kolejce, 404 → `FAILED` → pack degradowany → repair pobiera 4 MiB bez powodu | czytanie + sonda SQLite |
| Z6-04 | ✅ | `run_batch_now` bez `sleep`/kursora — `POST /repair/now` i `/reconcile/now` mogły nigdy nie wrócić | **NAPRAWIONE** `9768a5e` (ubocznie przy Z6-05) |
| Z6-05 | ✅ | Repair bez licznika prób: nienaprawialny pack wracał co 10 s (~34 GiB egressu/dobę) i blokował wszystkie pozostałe | **NAPRAWIONE** `9768a5e` |
| Z6-06 | ✅ | Repair nie sprawdzał `pack_shards.checksum` — odtwarzał z niezweryfikowanych shardów, a gc kasował oryginał | **NAPRAWIONE** `f667d4f` |
| Z6-07 | 🔴 | Wyścig reconcile ↔ gc w gałęzi `LocalOnly` (brak osłony `!= 'UPLOADING'`, brak FK na `pack_locations`) | czytanie + schemat |
| Z6-08 | ⚠️ | Dwie definicje sieroty; endpoint `/api/maintenance/gc` kasuje metadane bez obiektów w chmurze | czytanie + sonda SQLite |
| Z6-09 | ⚠️ | DEEP verify zawsze na pierwszym shardzie partii (`batch_index == 0`) — ≥576 MiB/dobę przy limicie 500 MiB | czytanie + sonda SQLite |
| Z6-10 | ⚠️ | Klasyfikacja błędów przez `contains("404"/"500"/"tls")` na sklejce `display + debug + source` | czytanie |
| Z6-11 | ⚠️ | Poprawka `request_checksum_calculation` tylko w `uploader.rs`; repair/scrubber/gc bez niej | grep |
| Z6-12 | ⚠️ | `repair_pack` przy statusie Healthy nie zapisuje wyniku → gorąca pętla w workerze | czytanie |
| Z6-13 | ⚠️ | Globalny `reset_in_progress_pack_shards()` na starcie repaira kradnie shardy uploaderowi | czytanie |
| Z6-14 | ⚠️ | Spool rośnie ~4 MiB na każdą naprawę — nikt nie kasuje pobranych shardów | czytanie + spool |
| Z6-15 | ⚠️ | Pętle scrubbera i repaira giną na pierwszym błędzie SQLite, poza `tokio::select!` | czytanie |
| Z6-16 | ⚠️ | `#![allow(dead_code)]` na obu produkcyjnych modułach integralności | grep |
| Z7-01 | 🔴 | Rejestrowe menu kontekstowe (`shell_integration.rs`) nie wysyła `Authorization` — 5/5 pozycji zwraca 401. **Uściślenie w §8.8:** DLL rozszerzenia to osobna implementacja, chodzi przez Named Pipe (Z8-01), nie przez HTTP | czytanie + grep endpointów |
| Z7-02 | 🔴 | „Windows Hello" to samo DPAPI — brak biometrii, hasło odzyskiwalne przez dowolny proces użytkownika | grep: 0 trafień API Hello |
| Z7-03 | 🔴 | Bufor po `CryptUnprotectData` niezwolniony i niewyzerowany; hasło jako zwykły `String` | czytanie |
| Z7-04 | 🔴 | DACL sync roota daje `Authenticated Users` GR/GW/GX, dziedziczenie włączone; wszedł ubocznie w `04a58e7` (commit o testach e2e), brak uzasadnienia | czytanie SDDL + `git log -S` |
| Z7-05 | 🔴 | Teardown po blokadzie detached; błędy dehydratacji i wyrejestrowania połykane | czytanie |
| Z7-06 | 🔴 | UI i tick loop inaczej czytają `last_activity == 0`; praca w Eksploratorze nie dotyka licznika | czytanie + grep `touch` |
| Z7-07 | 🔴 | Hydratacja bez providerów zwraca `STATUS_SUCCESS` z zerem bajtów zamiast błędu | czytanie |
| Z7-08 | ⚠️ | `HYDRATION_CONTEXT` to `OnceLock` z `let _ = set()` — drugi wywołujący przegrywa | grep: 2 wywołujących |
| Z7-09 | ⚠️ | `CANCEL_FETCH_DATA` tylko loguje; pobieranie leci dalej i płaci za egress | czytanie |
| Z7-10 | ⚠️ | `powershell.exe` + `icacls` na każde odblokowanie tylko dla `trace!` | czytanie |
| Z7-11 | ⚠️ | Błędy `subst` rozpoznawane po tekście PL/EN — zależne od języka systemu | czytanie |
| Z7-12 | ⚠️ | Ikona/etykieta dysku w rejestrze wbrew CLAUDE.md, a `is_healthy()` tego wymaga | czytanie + CLAUDE.md |
| Z7-13 | ⚠️ | `require_session_no_touch` identyczne z `require_session` | czytanie + test |
| Z7-14 | ⚠️ | Obserwator WTS ignoruje przełączenie użytkownika i rozłączenie RDP | czytanie |
| Z7-15 | ⚠️ | `CLAUDE.md` wskazuje nieistniejący katalog `angeld/src/cfapi/` | glob: brak plików |
| Z7-16 | ⚠️ | `.omnidrive_acl_probe` zostaje w sync roocie przy ubiciu procesu | czytanie |
| Z7-17 | ⚠️ | Hartowanie ACL wyłączone w buildach debug | czytanie |
| Z7-18 | ⚠️ | `evict_unpinned_hydrated_files` bez wywołujących — brak eksmisji cache'u | grep: 0 wywołujących |
| Z8-01 | 🔴 | Named Pipe z DACL `Everyone GR/GW`, zero weryfikacji wywołującego — 6 komend omija `acl.rs` | czytanie SDDL + `[Files]` w `.iss` |
| Z8-02 | 🔴 | `trusted = 1` na podstawie samego broadcastu UDP → plaintext chunków dla dowolnego hosta w LAN | czytanie + `db/device_identity.rs:237` |
| Z8-03 | 🔴 | `PRAGMA foreign_keys = OFF` w transakcji to no-op → `DELETE FROM users` wywala graft o `user_sessions` | sonda SQLite (kopia + ROLLBACK) |
| Z8-04 | 🔴 | Graft nie kopiuje `pack_deks`; fallback bierze zły DEK dla każdego packa poza ostatnim i utrwala błąd | grep + sonda obu zapytań |
| Z8-05 | 🔴 | `CryptProtectData` bez entropii dla kluczy S3 — poświadczenia do bucketów bez hasła głównego | czytanie |
| Z8-06 | ⚠️ | Fetch metadanych z `pool = None` — poza `cloud_guard` i poza licznikiem egressu, w pętli przy złym `vault_id` | czytanie |
| Z8-07 | ⚠️ | `cleanup_stale_uploads` za flagą, której nikt nie ustawia — porzucone multiparty nigdy nie sprzątane | grep: 1 trafienie |
| Z8-08 | ⚠️ | `cleanup_stale_restore_staging` bez zerowania, filtr `.db` pomija sidecary WAL | czytanie |
| Z8-09 | ⚠️ | Plaintextowa migawka w `%TEMP%`, dwie z czterech ścieżek sprzątania bez zerowania | czytanie |
| Z8-10 | ⚠️ | `r_vault_config` przez `unwrap_or(None)` — migawka bez `vault_config` przechodzi cicho | czytanie |
| Z8-11 | ⚠️ | `probe_endpoint_reachability` próbuje tylko `addrs[0]` | czytanie |
| Z8-12 | ⚠️ | Graft kasuje 18 tabel, w tym lokalne `inodes`; kreator nie ostrzega | czytanie + `wizard.js` |
| Z8-13 | ⚠️ | Klasyfikacja błędów przez `contains()` decyduje o „złe hasło" vs „brak sieci" | czytanie |
| Z8-14 | ⚠️ | `RuntimePaths::detect()` per komenda pipe'a, `AppConfig::from_env()` per `fetch_chunk` | czytanie |
| Z8-15 | ⚠️ | Obiekt `.omnidrive_probe/…` zostaje przy błędzie delete; `let _ = secrets;` | czytanie |
| Z8-16 | ⚠️ | Trzy nieszyfrowane `omnidrive.db.bak.<stamp>` obok bazy, nigdzie nie policzone | czytanie |
| Z8-17 | ⚠️ | `#![allow(dead_code)]` na `onboarding.rs` + komentarz „Epic 30" (CLAUDE.md §3) | grep |
| Z8-18 | ⚠️ | `run_pipe_server` kończy się bez retry przy zajętej nazwie pipe'a | czytanie |
| Z9-01 | 🔴 | `GET /api/vault/status` bez auth **wystawia token sesji** przy odblokowanym Skarbcu — `require_role` przestaje cokolwiek znaczyć | czytanie + sonda `user_sessions` |
| Z9-02 | 🔴 | `POST /api/unlock/windows-hello` bez auth i bez ciała odblokowuje Skarbiec (CSRF); hasło ląduje w DPAPI przy każdym unlocku, bez zgody | czytanie `auth.rs:68`, `:411` |
| Z9-03 | 🔴 | `POST /api/vault/add-device` bez auth owija Vault Key na klucz publiczny z żądania i zwraca go | czytanie `vault.rs:585` |
| Z9-04 | 🔴 | `POST /api/unlock` bez limitera i bez audytu nieudanych prób | czytanie |
| Z9-05 | 🔴 | Web UI ładuje Tailwind i jdenticon z publicznych CDN; identikon liczb bezpieczeństwa rysuje kod z sieci; CSP tylko na `/wizard` | grep `<script src>` |
| Z9-06 | ⚠️ | `api/diagnostics.rs` — 12 handlerów, zero kontroli dostępu; `/api/multidevice/status` oddaje `vault_id` + `device_id` | audyt pokrycia |
| Z9-07 | ⚠️ | `api/stats.rs` — 3 handlery, zero kontroli dostępu | audyt pokrycia |
| Z9-08 | ⚠️ | `/api/onboarding/reset` i `/complete` bez auth i bez ciała → CSRF | czytanie sygnatur |
| Z9-09 | ⚠️ | `max_downloads` zlicza tylko pobrania ostatniego chunka — retry zjada limit | czytanie `sharing.rs:502` |
| Z9-10 | ⚠️ | `verify-password` linku share bez limitera przy lekkim Argon2id (8 MiB, t=2) | czytanie |
| Z9-11 | ⚠️ | Token share w query stringu, choć CORS dopuszcza nagłówek `x-share-token` | czytanie |
| Z9-12 | ⚠️ | `post_vault_join`: `user_id` sterowane przez klienta, błędy tylko `warn!`, zaproszenie skonsumowane | czytanie |
| Z9-13 | ⚠️ | Oznaczenie liczb bezpieczeństwa jako zweryfikowanych wymaga tylko roli `Viewer` | czytanie |
| Z9-14 | ⚠️ | `ApiError::Internal` odsyła surowy komunikat błędu do klienta | czytanie |
| Z9-15 | ⚠️ | `/legacy` bez nagłówków bezpieczeństwa, które ma `/` | czytanie |
| Z9-16 | ⚠️ | Limitery nie czyszczą wpisów per IP; `JoinRateLimiter` karze maks. 30 s | czytanie |
| Z9-17 | ⚠️ | Recovery restore nie unieważnia sesji ani nie aktualizuje poświadczenia DPAPI | czytanie |
| Z9-18 | ⚠️ | `share_base_url` buduje link z nagłówka `Host` | czytanie |
| Z9-19 | ⚠️ | `POST /api/maintenance/repair-shell` — jedyna zmiana stanu w `maintenance.rs` bez kontroli roli | audyt pokrycia |
| Z9-20 | 🔴 | `POST /api/onboarding/setup-provider` bez auth nadpisuje endpoint/bucket/klucze dostawcy także po onboardingu — packi lecą do cudzego bucketa | czytanie |
| Z9-21 | 🔴 | `POST /api/vault/rotate-key` zmienia hasło Skarbca **bez weryfikacji starego** (inaczej niż `/api/change-password`) | czytanie obu |
| Z9-22 | 🔴 | Odwołanie urządzenia melduje `"revoked"` mimo nieudanej rotacji VK — odwołane urządzenie zachowuje działający klucz | czytanie |
| Z9-23 | 🔴 | Tryb A (LAN Share) nie może działać — `crypto.subtle` i Service Worker wymagają bezpiecznego kontekstu, link LAN to `http://` po IP | czytanie `share.html` + `sharing.rs` |
| Z9-24 | ⚠️ | Callback Google mintuje sesję dowolnemu kontu; endpointy na `extract_session` (autostart, restart-daemon, auto-lock) ją honorują | czytanie + `acl.rs:78` |
| Z9-25 | ⚠️ | `snapshot-local` przyjmuje dowolną ścieżkę wyjściową; plaintextowy `*.tmp.db` powstaje w katalogu wskazanym przez wywołującego | czytanie |
| Z9-26 | ⚠️ | `GET /api/ingest` bez auth zwraca pełne ścieżki plików użytkownika | czytanie |
| Z9-27 | ⚠️ | `google_refresh_token` w plaintekście w `users`, dopóki ktoś nie odblokuje Skarbca | czytanie |
| Z9-28 | ⚠️ | `try_auto_wrap_vault_key` pomija kontrole `enrolled_at`/`revoked_at` z `post_accept_device` | czytanie obu |
| Z9-29 | ⚠️ | `normalize_filesystem_api_path` zduplikowane w `pipe_server::normalize_path` | czytanie + komentarz |
| Z9-30 | ⚠️ | `get_my_wrapped_key` (Viewer) oddaje owinięty VK dowolnego urządzenia | czytanie |
| Z9-31 | ⚠️ | `restart-daemon` tylko sygnalizuje shutdown; nic w daemonie go nie podnosi | czytanie |
| Z10-01 | 🔴 | CLI nie wysyła `Authorization` — 6 z 12 komend kończy się 403 (`ls`, `history`, `restore`, `pin`, `unpin`, `backup-now`) | grep + audyt ACL |
| Z10-02 | 🔴 | `omnidrive recovery restore` nadpisuje żywą `omnidrive.db` migawką z chmury — bez grafta, kopii i potwierdzenia | czytanie |
| Z10-03 | 🔴 | Tray i deinstalator zabijają daemona `taskkill /F` zamiast graceful shutdown → teardown z Z7-05 przepada, plaintext zostaje | czytanie + `.iss` |
| Z10-04 | ⚠️ | `recovery restore` wymaga kompletu 3 dostawców w env — na maszynie z instalatora nie ruszy | czytanie |
| Z10-05 | ⚠️ | Tray odpytuje `/api/vault/status` co 3 s, każde wywołanie mintuje sesję (Z9-01) | czytanie + sonda |
| Z10-06 | ⚠️ | `omnidrive_shell_ext.dll` budowany i kopiowany do payloadu, ale instalator go nie instaluje ani nie rejestruje | grep po `.iss` |
| Z10-07 | ⚠️ | `angelctl` to `println!("Hello, world!")`, a buduje się, ląduje w payloadzie i wymaga bumpu wersji | czytanie |
| Z10-08 | ⚠️ | `cfapi_repro.exe` budowany domyślnie obok binarek produkcyjnych (klasa Z1-06) | `ls target/release` |
| Z10-09 | ⚠️ | Rozszerzenie powłoki hardkoduje `O:\`, a daemon montuje pod pierwszą wolną literą `D..Z` | czytanie obu |
| Z10-10 | ⚠️ | Log rozszerzenia w `%TEMP%` bez rotacji, ze ścieżkami plików Skarbca | czytanie |
| Z10-11 | ⚠️ | `load_icon` panikuje przy braku PNG, a release nie ma konsoli → tray znika bez śladu | czytanie |
| Z10-12 | ⚠️ | `restart_daemon` = kill + `sleep(500 ms)` + spawn, bez weryfikacji | czytanie |
| Z10-13 | ⚠️ | `taskkill /F /IM angeld.exe` ubija też instancję dev-ową z `target/release` | czytanie |
| Z10-14 | ⚠️ | 19 funkcji testowych na 3372 linie; testy negatywne uwierzytelnienia tylko dla auto-locka | inwentaryzacja |
| Z10-15 | ⚠️ | `e2e_recovery` i `e2e_sync` hardkodują `Y:` i nie robią `subst /D` w `Drop` — stąd porzucone mapowania | czytanie |
| Z11-01 | 🔴 | Linki share z hasłem są nie do otwarcia — klient czeka na pole `requires_password`, którego API nie wysyła | czytanie obu stron + grep |
| Z11-02 | 🔴 | `DELETE /api/onboarding/provider/{name}` bez auth kasuje konfigurację, a `ON DELETE CASCADE` zabiera poświadczenia DPAPI | czytanie + schemat |
| Z11-03 | 🔴 | `legacy.html` (2258, pod `/legacy`) nie wysyła `Authorization` — 9 z 21 endpointów zwraca 403. Czwarty taki klient | grep + audyt ról |
| Z11-04 | 🔴 | `OMNIDRIVE_E2E_TEST_MODE` w binarce produkcyjnej wyłącza workery integralności i **ustawia im status `Idle`**; `e2e_basic` asertuje te sfabrykowane statusy | czytanie `main.rs` + testu |
| Z11-05 | ⚠️ | `purge_trash` kasuje metadane, nie obiekty w chmurze — „usuń trwale" nie usuwa danych z bucketów | czytanie |
| Z11-06 | ⚠️ | `/api/storage/cost` bez bramki robi N+1 zapytań przy każdym odświeżeniu dashboardu | czytanie |
| Z11-07 | ⚠️ | `provider_connection_status` nigdy nie zwróci `FAILED` przy błędzie → ikona błędu w trayu jest martwa | czytanie obu stron |
| Z11-08 | ⚠️ | `post_vault_lock` duplikuje teardown zamiast `lock_flow::force_lock_and_dismount`; lock nie czyści DPAPI, więc Z9-02 go odwraca | czytanie + CLAUDE.md |
| Z11-09 | ⚠️ | Token OAuth trwale w `localStorage` (czytelny dla skryptów z CDN), token z `/api/unlock` tylko w pamięci | czytanie |
| Z11-10 | ⚠️ | Trzeci zewnętrzny origin: `fonts.googleapis.com` | czytanie |
| Z11-11 | ⚠️ | Service Worker rejestrowany z zasięgiem całego origin zamiast `/sw-download/` | czytanie |
| Z11-12 | ⚠️ | `POST /api/providers/{name}/test` bez auth wykonuje `put_object` i `delete_object` w buckecie | czytanie |
| Z11-13 | ⚠️ | `cfapi_repro` z zaszytą ścieżką `C:\Users\Przemek\...`, rejestruje prawdziwy sync root | czytanie |
| Z11-14 | ⚠️ | Bez Service Workera `share.html` buforuje cały plik w RAM — na LAN to jedyna ścieżka | czytanie |
| Z11-15 | ⚠️ | Test regresyjny Z4-01 (8 KiB przy chunku 4 MiB) z konstrukcji nie może wykryć Z8-04 | czytanie + `packer.rs:24` |

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

> **Głębokość przeglądu:** cała warstwa przeczytana w całości. Skrót powyżej opisuje intencję —
> **pełne czytanie `scrubber.rs` i `repair.rs` jest w rozdziale 6b** i w kilku miejscach
> prostuje ten skrót (m.in. opis `gc` miesza worker z endpointem — patrz §6b.10, oraz
> „co dwudziesty shard" w scrubberze nie działa tak, jak wygląda — §6b.3).

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

---

# 7. Windows / Ghost Shell

Warstwa, która zamienia „daemon z bazą" w **dysk `O:\`, który widzi Eksplorator**. Cztery
niezależne mechanizmy Windows, spięte razem: Cloud Files API (`cldflt.sys`), `subst`, rejestr
HKCU i sesja WTS. Żaden z nich nie wie o pozostałych — spójność trzyma się na kolejności wywołań
w `api/auth.rs` i `lock_flow.rs`.

## 7.1 Mapa warstwy

| Plik | Rola |
| --- | --- |
| `smart_sync/mod.rs` (265) | Fasada `#[cfg(windows)]` — 12 funkcji publicznych, każda z gałęzią `UnsupportedPlatform`. |
| `smart_sync/imp/registration.rs` (577) | `CfRegisterSyncRoot` / `CfConnectSyncRoot` / audyt / naprawa. |
| `smart_sync/imp/callbacks.rs` (436) | Trzy callbacki cfapi: FETCH_DATA, FETCH_PLACEHOLDERS, CANCEL_FETCH_DATA. |
| `smart_sync/imp/placeholder.rs` (383) | Pin/unpin, hydratacja, dehydratacja, `convert_to_ghost`. |
| `smart_sync/imp/projection.rs` (353) | Rzutowanie bazy na placeholdery + łańcuch katalogów. |
| `smart_sync/imp/lifecycle.rs` (77) | `dismount_after_lock` / `mount_after_unlock`. |
| `smart_sync/imp/paths.rs` (130) | Normalizacja ścieżek, konwersja czasu na FILETIME. |
| `smart_sync/imp/state.rs` (71) | Globalne `CONNECTION_KEY`, `HYDRATION_CONTEXT`, apartament COM. |
| `virtual_drive.rs` (348) | `subst O: <ścieżka>` + ikona/etykieta w rejestrze + ukrycie sync roota. |
| `shell_state.rs` (435) | Audyt i naprawa stanu powłoki (dysk + rejestr + autostart). |
| `shell_integration.rs` (238) | Menu kontekstowe Eksploratora (`HKCU\Software\Classes`). |
| `auto_lock.rs` (479) | Monitor bezczynności (`AtomicU64`, tick 10 s). |
| `win_session.rs` (213) | Obserwator WTS — Win+L → lock. |
| `lock_flow.rs` (127) | Jedno źródło prawdy dla „zablokuj i rozmontuj". |
| `win_acl.rs` (266) | SDDL dla sync roota (+ fallback `icacls`). |
| `acl.rs` (300) | RBAC dla API (nie ma związku z `win_acl.rs` mimo nazwy). |
| `secure_fs.rs` (162) | Retry na blokadach plików + „bezpieczne" kasowanie. |
| `windows_hello.rs` (142) | Zapamiętane hasło w Credential Managerze. |
| `autostart.rs` (175) | Wpis w `HKCU\...\Run`. |

**Rozjazd z dokumentacją:** `CLAUDE.md` wskazuje `angeld/src/cfapi/` jako miejsce integracji
z Cloud Files. Taki katalog nie istnieje (glob: zero plików) — kod żyje w `smart_sync/`.

## 7.2 Cykl życia sync roota

```
unlock  -> register_sync_root_public -> CfRegisterSyncRoot -> CfConnectSyncRoot
                                     -> project_vault_to_sync_root (N x CfCreatePlaceholders)
                                     -> hide_sync_root + subst O:
lock    -> dehydrate_directory_recursive (rekursja po FS)
        -> CfDisconnectSyncRoot -> CfUnregisterSyncRoot -> subst O: /D
```

Trzy rzeczy w tym cyklu są zrobione **świadomie i dobrze**:

- `CONNECTION_KEY` został celowo zmieniony z `OnceLock` na `Mutex<Option<_>>`, żeby cykl
  lock ↔ unlock w ogóle był możliwy (komentarz w `state.rs:32`). To jest właściwa poprawka.
- Wszystkie trzy callbacki cfapi są opakowane w `catch_unwind`, a FETCH_DATA na ścieżce paniki
  domyka transfer przez `complete_transfer_failure`. Panika w callbacku `extern "system"` to UB;
  tutaj tego nie ma.
- `FETCH_PLACEHOLDERS` zwraca zero wpisów z flagą `DISABLE_ON_DEMAND_POPULATION` (0x2). Komentarz
  przy tym miejscu tłumaczy WHY (bez tego `cldflt.sys` blokuje tworzenie plików błędem
  `0x80070781`) — to jest dokładnie ten rodzaj komentarza, którego CLAUDE.md §3 wymaga.

Ale **`HYDRATION_CONTEXT` pozostał `OnceLock`** i jest ustawiany przez `let _ = set(...)`, czyli
z zignorowanym wynikiem. Wywołujących jest dwóch: `main.rs:536` i `api/onboarding.rs:1039`. Drugi
zawsze przegrywa po cichu. Ratuje to jedynie fakt, że onboarding w gałęzi `Some` używa **tego
samego** `Arc<Downloader>` i odświeża go przez `reload_active_providers_from_db()`, a
`has_remote_providers()` czyta `RwLock` na żywo. W gałęzi `None` powstaje nowy `Downloader`,
który jest natychmiast wyrzucany do kosza (Z7-08).

## 7.3 Hydratacja — od kliknięcia w Eksploratorze do bajtów

`fetch_data_callback` dekoduje 16-bajtowy `PlaceholderIdentity { inode_id, revision_id }`
z `FileIdentity`, dotyka licznika bezczynności, po czym wypycha pracę na runtime Tokio.
`read_range_streamed` woła zwrotkę per chunk, a każdy chunk trafia do Windows osobnym
`CfExecute` — szczyt RAM to jeden chunk, nie cały plik. To jest dobra architektura i widać,
że była projektowana pod duże pliki.

Dwa miejsca psują ten obraz:

**Brak providerów = sukces z zerem bajtów.** Gdy `has_remote_providers()` zwraca `false`,
callback woła `complete_transfer_success(&request, &[])` — czyli `CF_OPERATION_TYPE_TRANSFER_DATA`
ze `STATUS_SUCCESS`, `Buffer: null`, `Length: 0`. Aplikacja czytająca plik dostaje udany odczyt
zakresu, w którym nie ma danych, zamiast błędu. Wynik `CfExecute` jest dodatkowo połknięty przez
`let _ =` (Z7-07).

**Anulowanie nic nie anuluje.** `cancel_fetch_data_callback` tylko loguje `warn!`. Zadanie
pobierające leci dalej, dalej płaci za egress i dalej woła `CfExecute` na unieważnionym
`TransferKey`. Przy dużym pliku i użytkowniku, który wciśnie Esc, pobranie i tak zostanie
opłacone w całości — kontekst do [[project-b2-bleeding-root-cause]] (Z7-09).

## 7.4 Blokada skarbca — czym naprawdę jest „P0 lock sequence"

Docstring w `smart_sync/mod.rs:162` obiecuje: *„Recursive dehydrate of every file in OmniSync
(removes decrypted cache)"*. Co robi kod:

1. `lock_flow::force_lock_and_dismount` kasuje klucze, wpisuje audyt i **odpala teardown przez
   `tokio::spawn`**, po czym natychmiast zwraca `true`. API odpowiada „zablokowane", zanim
   cokolwiek zostało usunięte z dysku.
2. `dehydrate_directory_recursive` idzie rekurencyjnie po katalogach i na każdym błędzie robi
   `trace!` + `continue`. Plik trzymany otwarty przez inny proces (Word, Defender, indekser)
   **zostaje zhydratowany, czyli w plaintekście**, i nikt się o tym nie dowie.
3. `unregister_sync_root` łapie każdy błąd `CfUnregisterSyncRoot` i mimo to zwraca `Ok(())`.

Sumarycznie: sekwencja blokady nie ma ani synchronicznej gwarancji, ani weryfikacji, ani kanału
raportowania. Jeśli daemon zostanie ubity zaraz po `logout` (a przy wylogowaniu to scenariusz
domyślny), spawnowane zadanie ginie razem z procesem (Z7-05).

Poboczna obserwacja o tej samej wadze co całość: `win_session.rs` reaguje **wyłącznie** na
`WTS_SESSION_LOCK`. Przełączenie użytkownika i rozłączenie sesji RDP
(`WTS_CONSOLE_DISCONNECT`, `WTS_REMOTE_DISCONNECT`) zostawiają skarbiec otwarty (Z7-14).

## 7.5 Licznik bezczynności — dwie różne definicje „bezczynny"

`AutoLockMonitor` trzyma `last_activity: AtomicU64` (sekundy od startu daemona, `Relaxed`) —
wzorzec zgodny z [[feedback-atomic-for-hot-path]] i słusznie. Problem jest w tym, że dwie
funkcje czytają tę samą liczbę w **sprzeczny sposób**:

```rust
// remaining_secs() — to, co widzi UI
if last == 0 { return timeout; }          // „jeszcze nie wystartował"

// run_tick_loop() — to, co blokuje skarbiec
let elapsed = now.saturating_sub(last);   // last == 0 -> elapsed == uptime
if elapsed < timeout { return; }
```

Dopóki cokolwiek dotknie licznika, obie zgadzają się co do wyniku. Ale dotykają go tylko dwa
źródła: `TouchSource::AuthApi` (dwa miejsca w `api/auth.rs`) i `TouchSource::CfApi` (dwa
callbacki). **Przeglądanie dysku w Eksploratorze, otwieranie już zhydratowanych plików i zapis
do nich nie generują żadnego dotknięcia** — cfapi woła FETCH_DATA tylko dla plików
odhydratowanych. Użytkownik pracujący na przypiętych plikach jest dla monitora bezczynny (Z7-06).

Świadoma decyzja, którą warto odnotować, żeby jej przypadkiem nie „naprawić": `require_session`
i `require_session_no_touch` **celowo** nie dotykają licznika, co utrwala test
`require_session_variants_do_not_touch`. Skutek uboczny jest taki, że obie funkcje mają
identyczne ciała, a docstring drugiej opisuje nieistniejącą różnicę (Z7-13).

## 7.6 Menu kontekstowe Eksploratora — pięć pozycji, zero działania

`register_explorer_context_menu` zakłada pod `HKCU\Software\Classes\{*,Directory}\shell\OmniDrive`
pięć podpoleceń (trzy polityki ochrony, „Free up space", „Always keep on this device"). Każde
uruchamia `powershell.exe -WindowStyle Hidden -ExecutionPolicy Bypass` z `Invoke-RestMethod`
do lokalnego API. Wygenerowane żądanie ma dokładnie jeden nagłówek: `Content-Type`.

Wszystkie trzy endpointy docelowe wymagają sesji:

```
api/files.rs:370   acl::require_role(&state.pool, &headers, Role::Member).await?;   // set-policy
api/files.rs:405   acl::require_role(...)                                           // pin
api/files.rs:432   acl::require_role(...)                                           // unpin
```

Bez nagłówka `Authorization: Bearer …` `extract_session_or_401` zwraca 401, zanim cokolwiek się
wydarzy. Menu kontekstowe **nie działa w żadnej pozycji**, a ponieważ okno jest ukryte, a wynik
idzie w `| Out-Null`, użytkownik nie widzi nawet błędu — kliknięcie po prostu nic nie robi
(Z7-01). To nie jest regresja jednego endpointu, tylko cała funkcja Ghost Shell, o której
`STATUS.md` mówi „35.2 DONE".

## 7.7 Uprawnienia i tożsamość

`win_acl::build_sync_root_sddl` nadaje sync rootowi:

```
D:AI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;<SID użytkownika>)(A;OICI;GRGWGX;;;AU)
```

Ostatni ACE to **Authenticated Users z prawem odczytu, zapisu i wykonania**, a `D:AI` (zamiast
`D:PAI`) zostawia dziedziczenie włączone. Fallback przez `icacls` robi to samo
(`*S-1-5-11:(OI)(CI)RX`). Dopóki pliki są odhydratowane, wyciekają tylko nazwy i rozmiary; w
momencie hydratacji **każde inne konto na maszynie czyta plaintext** (Z7-04).

Pochodzenie tego ACE ustalone przez `git log -S`: wszedł **2026-03-24 w commicie `04a58e7`
„test: stabilize Smart Sync e2e bootstrap diagnostics"**, razem ze 151 liniami zmian w
`win_acl.rs`, 183 w `smart_sync.rs` i nowym plikiem `e2e_sync.rs`. Czyli powstał podczas
walki z bootstrapem `CfRegisterSyncRoot` w testach, a nie jako decyzja o modelu uprawnień —
nie ma osobnego commita, który by go uzasadniał. `cldflt.sys` działa jako `SYSTEM`, które ma
własny ACE `(A;OICI;FA;;;SY)`, więc hipoteza „to było potrzebne dla filtra" nie broni się na
pierwszy rzut oka. Usunięcie wymaga jednak testu na żywo (rejestracja sync roota + hydratacja),
bo dokładny powód dodania nie jest udokumentowany nigdzie poza tym commitem.

Osobno: całe hartowanie ACL jest wyłączone w buildach debug (`#[cfg(not(debug_assertions))]`
w obu funkcjach). Testy nigdy nie przechodzą przez tę ścieżkę (Z7-17).

`windows_hello.rs` nie ma nic wspólnego z Windows Hello. Grep po całym repozytorium:
`KeyCredentialManager`, `UserConsentVerifier`, `RequestVerificationAsync` — **zero trafień**.
Moduł robi `CryptProtectData` (DPAPI, zakres użytkownika, `CRYPTPROTECT_UI_FORBIDDEN` = zakaz
jakiegokolwiek promptu) i zapisuje wynik do Credential Managera. Znaczy to, że:

- odblokowanie „przez Hello" nie prosi o odcisk palca ani PIN — biometria nie występuje w tym
  przepływie w ogóle;
- dowolny proces działający na tym koncie Windows odzyskuje **hasło do skarbca w plaintekście**
  jednym wywołaniem `CryptUnprotectData` (Z7-02);
- przechowywane jest samo hasło, nie klucz pochodny dla tego urządzenia — kompromitacja daje
  dostęp do skarbca na każdym urządzeniu, nie tylko na tym.

Do tego bufor zwrócony przez `CryptUnprotectData` nie jest ani zerowany, ani zwalniany
(`LocalFree`), a hasło wraca jako zwykły `String` bez `Zeroizing` — plaintext hasła zostaje
w pamięci procesu do końca jego życia (Z7-03).

## 7.8 Dysk wirtualny i rejestr

`virtual_drive.rs` opiera się na `subst` — proces potomny na każdą operację montowania,
odmontowania i **każde odpytanie o stan** (`list_virtual_drives`). Rozpoznanie „nie ma takiego
dysku" przy odmontowaniu robione jest przez porównanie tekstu stderr z dwoma zaszytymi
łańcuchami: angielskim i polskim (`virtual_drive.rs:240-248`). Na systemie z innym językiem
interfejsu zwyczajne „dysku nie ma" zamieni się w twardy błąd `CommandFailed` (Z7-11).

`configure_virtual_drive_appearance` zapisuje ikonę i etykietę do
`HKCU\...\Explorer\DriveIcons\{litera}`. `CLAUDE.md` deklaruje wprost: *„Nie używamy hacków
w rejestrze do podmiany ikon wirtualnego dysku (rezygnacja z mystyfikacji)"*. Kod nie tylko to
robi, ale `ShellStateSnapshot::is_healthy()` **wymaga** obecności obu kluczy, żeby uznać stan
powłoki za zdrowy (Z7-12). Kod wygrywa — decyzja z CLAUDE.md nie została wykonana.

Sondę bezpieczeństwa `debug_log_sync_root_security`, odpalaną przy **każdej** rejestracji sync
roota (czyli przy każdym odblokowaniu), warto zobaczyć w całości: uruchamia `powershell.exe`
z `Get-Acl` i osobno `icacls`, czeka na oba, a wynik wrzuca do `trace!` — czyli przy domyślnym
poziomie logowania wyrzuca do kosza. Dwa procesy potomne na odblokowanie za nic (Z7-10). To
ta sama rodzina co Z1-06.

## 7.9 Co jest napisane dobrze

Żeby rejestr znalezisk nie zniekształcił obrazu — kilka miejsc w tej warstwie jest zrobionych
lepiej niż średnia repozytorium:

- `normalize_relative_placeholder_path` odrzuca segmenty puste, `.`, `..` **oraz zawierające
  `:`**. To jest dokładnie ta walidacja, której zabrakło w `cache.rs` i która wyprodukowała
  Z5-01 (zapis do alternatywnych strumieni NTFS). Wzorzec do przeniesienia.
- `project_vault_to_sync_root` liczy błędy per plik i mimo nich montuje resztę skarbca; komentarz
  przy tej pętli opisuje regresję, której zapobiega. Jeden nieprojektowalny plik nie zabiera
  dostępu do całości.
- `convert_to_ghost` przed konwersją porównuje rozmiar pliku z tym, co zostało zaingestowane,
  i przerywa przy niezgodności zamiast zamienić w ducha plik, który zmienił się w trakcie.
- `secure_fs::retry_io` rozróżnia `ERROR_SHARING_VIOLATION`/`ERROR_LOCK_VIOLATION` od reszty
  i nie ponawia niczego innego — realizacja zalecenia z CLAUDE.md, nie ślepa pętla.
- `read_registry_string` czyta rejestr przez API zamiast przez `reg.exe`, z komentarzem
  wyjaśniającym dlaczego (bezpieczne w pętli odpytującej).

Dwie rzeczy pośrednie, bez kategorii „błąd": `secure_fs::secure_delete` nadpisuje plik zerami
przed skasowaniem, co na SSD z wear-levellingiem, przy kopiach w cieniu (VSS) i przy
`$LogFile` NTFS nie daje gwarancji, którą sugeruje nazwa. Oraz `evict_unpinned_hydrated_files`
nie ma **żadnego wywołującego** (stąd `#[allow(dead_code)]`) — polityka eksmisji cache'u nie
istnieje, zhydratowane pliki rosną na dysku bez ograniczenia aż do blokady skarbca (Z7-18).

## 7.10 Znaleziska

| ID | Waga | Rzecz | Potwierdzone jak |
| --- | --- | --- | --- |
| Z7-01 | 🔴 | Menu kontekstowe Eksploratora nie wysyła `Authorization` — wszystkie 5 pozycji zwraca 401, po cichu | czytanie + grep endpointów |
| Z7-02 | 🔴 | „Windows Hello" to samo DPAPI — brak biometrii, hasło odzyskiwalne przez dowolny proces użytkownika | grep: 0 trafień API Hello |
| Z7-03 | 🔴 | Bufor po `CryptUnprotectData` niezwolniony i niewyzerowany; hasło jako zwykły `String` | czytanie |
| Z7-04 | 🔴 | DACL sync roota daje `Authenticated Users` GR/GW/GX, dziedziczenie włączone | czytanie SDDL + fallbacku icacls |
| Z7-05 | 🔴 | Teardown po blokadzie jest detached, błędy dehydratacji i wyrejestrowania połykane | czytanie |
| Z7-06 | 🔴 | `remaining_secs()` i `run_tick_loop()` inaczej czytają `last_activity == 0`; praca w Eksploratorze nie dotyka licznika | czytanie + grep `touch` |
| Z7-07 | 🔴 | Hydratacja bez providerów zwraca `STATUS_SUCCESS` z zerem bajtów zamiast błędu | czytanie |
| Z7-08 | ⚠️ | `HYDRATION_CONTEXT` to `OnceLock` z `let _ = set()` — drugi wywołujący przegrywa po cichu | grep: 2 wywołujących |
| Z7-09 | ⚠️ | `CANCEL_FETCH_DATA` tylko loguje; pobieranie leci dalej i płaci za egress | czytanie |
| Z7-10 | ⚠️ | `powershell.exe` + `icacls` na każde odblokowanie tylko po to, by zapisać `trace!` | czytanie |
| Z7-11 | ⚠️ | Obsługa błędów `subst` porównuje stringi po angielsku i polsku — zależna od języka systemu | czytanie |
| Z7-12 | ⚠️ | Ikona/etykieta dysku w rejestrze wbrew CLAUDE.md, a `is_healthy()` tego **wymaga** | czytanie + CLAUDE.md |
| Z7-13 | ⚠️ | `require_session_no_touch` identyczne z `require_session`; docstring opisuje nieistniejącą różnicę | czytanie + test |
| Z7-14 | ⚠️ | Obserwator WTS ignoruje przełączenie użytkownika i rozłączenie RDP; `Drop` na `OnceLock` nigdy nie biegnie | czytanie |
| Z7-15 | ⚠️ | `CLAUDE.md` wskazuje nieistniejący katalog `angeld/src/cfapi/` | glob: brak plików |
| Z7-16 | ⚠️ | `assert_sync_root_writable` zostawia `.omnidrive_acl_probe` w sync roocie przy ubiciu procesu | czytanie |
| Z7-17 | ⚠️ | Hartowanie ACL wyłączone w buildach debug — testy nigdy nie wchodzą na tę ścieżkę | czytanie |
| Z7-18 | ⚠️ | `evict_unpinned_hydrated_files` bez wywołujących — brak polityki eksmisji cache'u | grep: 0 wywołujących |

---

# 4b. Pipeline zapisu — dokończenie (`uploader.rs`, `aws_http.rs`)

Uzupełnienie rozdziału 4, dopisane 2026-08-01 po przeczytaniu 1116 linii `uploader.rs`
i 55 linii `aws_http.rs`. To jest warstwa, która płaci rachunki za chmurę — każdy błąd
w logice ponawiania widać na fakturze, nie w logu.

## 4b.1 Kształt modułu

`Uploader` to cienka koperta na klienta S3 (jeden na providera). `UploadWorker` to pętla:
`get_next_upload_job` → `process_job` → oznacz wynik → `sleep`. Praca dzieje się per **shard**,
nie per pack: `get_incomplete_pack_shards` zwraca to, co jeszcze nie jest `COMPLETED` ani
`PERMANENTLY_FAILED`, a status packa wyliczany jest na końcu z `summarize_pack_shards`.

Trzy rzeczy zrobione dobrze i warte zachowania przy każdej przyszłej zmianie:

- **Poprawka `request_checksum_calculation(WhenRequired)`** ma komentarz wyjaśniający WHY
  (domyślne `WhenSupported` dokleja CRC32 w kodowaniu `aws-chunked` z trailerem przy ciele
  strumieniowym: R2 zrywa połączenie, Scaleway czeka do timeoutu, B2 toleruje — stąd mylny
  trop, że winna jest sieć) **oraz test regresyjny** `s3_config_does_not_add_automatic_checksums`.
  To jest dokładnie ta poprawka z live smoke'u opisana w [[project-next-session-plan]], zapisana
  tak, że nie da się jej cofnąć niezauważenie.
- `throttled_byte_stream` opakowuje plik w `SdkBody::retryable` z buforem 64 KiB i token-bucketem,
  więc ponowienie po stronie SDK czyta plik od nowa zamiast trzymać go w RAM. `buffered_uploads`
  (całe ciało w pamięci) jest opt-in przez env i domyślnie wyłączone.
- `cleanup_remote_backed_pack_spool` kasuje spool przez `secure_delete`, i tylko wtedy, gdy pack
  jest realnie w chmurze (`storage_mode != LocalOnly` i status `Healthy`/`Degraded`).

## 4b.2 Dwa liczniki prób, które się nie spotykają

W `process_job` każdy nieudany shard idzie jedną z dwóch dróg:

```
requeue_pack_shard / requeue_upload_target   -> attempts += 1, max_attempts aktualizowane
mark_pack_shard_failed / mark_upload_target_failed -> status = FAILED, attempts BEZ ZMIAN
```

Drogą „failed bez inkrementu" idą dokładnie dwa przypadki: przekroczenie limitu rozmiaru
pojedynczego uploadu (`enforce_single_upload_size_limit`) i **przekroczenie kwoty providera**
(`projected_usage > max_physical_bytes_per_provider`). Oraz brak pliku shardu w spoolu.

Pierwszy odruch przy czytaniu tego jest taki, że `FAILED` to stan końcowy i pack umiera na
zawsze. Sprawdzenie w bazie mówi co innego — `db/shards.rs:163` wyklucza tylko `COMPLETED`
i `PERMANENTLY_FAILED`, więc shard `FAILED` **wraca** do kolejki przy następnym przebiegu.
Fałszywy alarm.

Problem jest odwrotny i gorszy. Skoro `attempts` nie rośnie, to:

- `max_attempts` zostaje `0`, więc na końcu `process_job` leci `retry_delay(max_attempts.max(1))`
  = `retry_delay(1)` = `retry_base_delay` = **2 sekundy**;
- plateau po 100 próbach (1 próba/h) nigdy nie zadziała, bo licznik stoi;
- `UPLOAD_PERMANENT_FAILURE_AT` (1000) nigdy nie zostanie osiągnięte, bo
  `escalate_target_if_permanent` dostaje `target_attempts` wyłącznie z `requeue_*`.

Pack zablokowany kwotą wraca więc do przetwarzania **co 2 sekundy, bez końca**, a każdy przebieg
to komplet zapytań do SQLite (`get_pack`, `get_pack_shards`, `ensure_upload_targets`,
`get_incomplete_pack_shards`, `get_physical_usage_for_provider` per shard) plus wpis do
`diagnostics`. Dokładnie ten kształt — gorąca pętla ponawiania, która nie eskaluje — opisuje
[[project-b2-bleeding-root-cause]] (Z4-07).

## 4b.3 Czym naprawdę jest „retryable"

```rust
fn is_retryable(&self) -> bool {
    matches!(self, Self::Upload { .. } | Self::Timeout { .. })
}
```

`UploaderError::Upload` powstaje w `sdk_error()` z **każdego** błędu SDK — 500 od providera,
zerwane połączenie, ale też `403 InvalidAccessKeyId`, `403 SignatureDoesNotMatch` i
`404 NoSuchBucket`. Odwołany klucz dostępu jest więc klasyfikowany jako błąd przejściowy.

Ścieżka takiego błędu: 100 prób z narastającym backoffem (do 60 s), potem plateau 1 próba/h,
aż do 1000 prób. To jest **około 900 godzin, czyli 37 dni**, zanim cokolwiek zostanie oznaczone
`PERMANENTLY_FAILED` i przestanie pukać do providera. Komentarz nad `UPLOAD_RETRY_PLATEAU_AT`
mówi wprost: *„Prevents retry storms against persistently broken providers (e.g., revoked
credentials)"* — nie zapobiega, tylko rozrzedza do jednej próby na godzinę i robi to przez
ponad miesiąc (Z4-08).

Do tego ponawiania są trzy niezależne warstwy, które się mnożą: `RetryConfig::adaptive()`
w `aws_http.rs`, `operation_attempt_timeout` (90 s) wewnątrz jednej operacji SDK, i pętla
workera. Jedna „próba" workera to w rzeczywistości kilka żądań HTTP (Z4-11).

## 4b.4 Pętla, która umiera po cichu

```rust
pub async fn run(mut self) -> Result<(), UploaderError> {
    ...
    match self.process_job(&job).await? { ... }
}
```

Każdy błąd SQLite — w `get_next_upload_job`, w dowolnym `mark_*`, w `summarize_pack_shards` —
propaguje się przez `?` i **kończy pętlę workera**. `process_job` woła bazę kilkanaście razy na
przebieg, a CLAUDE.md ostrzega osobno, że operacje na `omnidrive.db` bywają blokowane przez
Defendera i Eksploratora. Jedna taka blokada zatrzymuje uploady do restartu daemona, a ponieważ
worker stoi poza `tokio::select!` (Z1-02), nikt się o tym nie dowie (Z4-09).

Kontrast wewnątrz tego samego pliku jest wymowny: pojedynczy shard ma pełną obsługę błędów
z trzema wariantami wyniku i eskalacją, a pętla, która to wszystko trzyma, nie ma żadnej.

## 4b.5 Konfiguracja providerów i transport

`Uploader::all_from_env()` woła `from_r2_env()?`, `from_scaleway_env()?` i `from_b2_env()?`
z operatorem `?` na każdym. Skonfigurowanie **dwóch** providerów zamiast trzech wywala całość
na `MissingEnv`. Jedyne wyjście to `OMNIDRIVE_ALLOW_EMPTY_UPLOADERS`, które daje **zero**
uploaderów zamiast dwóch działających — z `warn!` jako całą informacją dla użytkownika. Ścieżka
z bazy (`reload_uploaders_from_db`) jest wolna od tego problemu, ale ścieżka `from_env` dalej
istnieje i jest tą, której używa `--no-onboarding` (Z4-10). To ten sam rdzeń co Z4-03.

W `aws_http.rs` dwie decyzje warte odnotowania:

- `with_webpki_roots()` — zaufanie jest przypięte do korzeni **wkompilowanych w binarkę**,
  magazyn certyfikatów systemu jest ignorowany. Rotacja CA u providera albo firmowy proxy TLS
  kończy się nieprzechodzącym uploadem, którego nie da się naprawić konfiguracją (Z4-12).
- `allow_http` jest wyprowadzane z `config.endpoint.starts_with("http://")`. Literówka albo
  wklejony niepoprawnie endpoint cicho przełącza transport na czysty HTTP — bez ostrzeżenia,
  bez wpisu w logu. Zawartość jest zaszyfrowana klientem, więc nie jest to wyciek treści, ale
  metadane (nazwy obiektów, rozmiary, wzorzec ruchu) idą wtedy otwartym tekstem (Z4-13).

Na koniec drobiazg o dużym zasięgu: `#![allow(dead_code)]` stoi na górze całego pliku
z komentarzem *„reserved for Epic 32.5 / Epic 33"*. W module o 1116 liniach, który obsługuje
pieniądze i integralność danych, wyłącza to na stałe jedyny automatyczny sygnał o kodzie,
który przestał być używany (Z4-14).

## 4b.6 Znaleziska

| ID | Waga | Rzecz | Potwierdzone jak |
| --- | --- | --- | --- |
| Z4-07 | 🔴 | Pack zablokowany kwotą wraca co 2 s bez końca: `mark_*_failed` nie rusza `attempts`, więc plateau i `PERMANENTLY_FAILED` są nieosiągalne | czytanie + `db/shards.rs:163` |
| Z4-08 | 🔴 | `is_retryable()` uznaje 403/404 za przejściowe — odwołane klucze ponawiane ~37 dni zanim ustaną | czytanie |
| Z4-09 | 🔴 | `UploadWorker::run` kończy pętlę na dowolnym błędzie SQLite; w parze z Z1-02 śmierć niezauważona | czytanie |
| Z4-10 | ⚠️ | `all_from_env()` wymaga kompletu trzech providerów; `ALLOW_EMPTY_UPLOADERS` daje zero zamiast dwóch | czytanie |
| Z4-11 | ⚠️ | Trzy warstwy ponawiania mnożą się (SDK `adaptive` × attempt timeout × pętla workera) | czytanie |
| Z4-12 | ⚠️ | `with_webpki_roots()` — magazyn zaufania systemu ignorowany | czytanie |
| Z4-13 | ⚠️ | `allow_http` z prefiksu endpointu — literówka cicho degraduje transport do HTTP | czytanie |
| Z4-14 | ⚠️ | `#![allow(dead_code)]` na całym pliku 1116 linii | czytanie |

---

# 6b. Integralność — dokończenie (`scrubber.rs`, `repair.rs`)

Uzupełnienie rozdziału 6, dopisane 2026-08-01 po przeczytaniu 545 linii `scrubber.rs`
i 959 linii `repair.rs` (rozdział 6 podawał 504/881 — liczby były z wcześniejszego stanu plików).
To jest warstwa, która **pisze do plików użytkownika i kasuje obiekty w chmurze** na podstawie
własnej oceny stanu. Każdy błąd tej oceny jest drogi w obie strony: albo płacimy za egress,
albo tracimy jedyną dobrą kopię.

Punkt wyjścia dla rachunków niżej: `DEFAULT_CHUNK_SIZE` = 4 MiB, `DATA_SHARDS` = 2,
`PARITY_SHARDS` = 1, więc jeden shard pełnego chunka to **2 MiB**, a odtworzenie jednego
brakującego shardu wymaga pobrania **dwóch** (4 MiB egressu na pack).

## 6b.1 Pętla integralności — kto komu przekazuje pałeczkę

Cztery workery tworzą łańcuch, który w kodzie nigdzie nie jest opisany jako całość:

```
scrubber:  HEAD/GET obiektu -> update_shard_verification_status
              status HEALTHY   -> pack_shards.status = 'COMPLETED'
              cokolwiek innego -> pack_shards.status = 'FAILED'   <-- przekazanie palki
           -> summarize_pack_shards -> resolve_pack_status_for_mode -> packs.status
repair:    get_next_degraded_pack (status = 'COMPLETED_DEGRADED')
           -> pobierz 2 shardy -> Reed-Solomon reconstruct -> PUT brakujacego -> Healthy
gc:        pack bez wiersza w pack_locations -> DELETE obiektow + wierszy + plikow spoola
```

Spoiwem jest **jedna linijka** w `db/shards.rs:354`: `update_shard_verification_status` mapuje
wynik weryfikacji na operacyjny `status` shardu (`HEALTHY` → `COMPLETED`, wszystko inne →
`FAILED`). Bez niej scrubber byłby tylko rejestratorem. Konsekwencja uboczna: shard oznaczony
`FAILED` wraca również do kolejki **uploadera** (`db/shards.rs:163` wyklucza tylko `COMPLETED`
i `PERMANENTLY_FAILED`), a jego plik w spoolu dawno skasował `cleanup_remote_backed_pack_spool`.
Uploader trafia więc na „brak pliku shardu" → `mark_*_failed` → i mamy gorącą pętlę co 2 sekundy
z Z4-07. **Jedna nieudana weryfikacja starego packa uruchamia sztorm ponawiania w innym module.**

Co jest zrobione dobrze i warto zachować:

- **Każde** wyjście do sieci w obu modułach przechodzi przez `cloud_guard` — HEAD autoryzowany
  na 0 bajtów, GET na `shard.size`, a po pobraniu `reconcile_read_bytes` koryguje licznik
  o różnicę między szacunkiem a rzeczywistością. To jest ten sam fallback, który uratował
  `probe_latency` w rozdziale 6; tutaj jest zastosowany konsekwentnie.
- Wszystkie wywołania SDK są opakowane w `tokio::time::timeout`, niezależnie od timeoutów
  z `TimeoutConfig`.
- Istnieje test e2e pełnej pętli: `e2e_scrubber_repair.rs` (886 linii) —
  `scrubber_detects_missing_shard_and_repair_restores_health_without_read_failures` sabotuje
  wybrany shard w mocku S3 na dysku, czeka na `COMPLETED_DEGRADED` i sprawdza, że repair
  przywraca `COMPLETED_HEALTHY` **i że odczyt pliku po drodze się nie psuje**. Ścieżka szczęśliwa
  jest więc pilnowana. Wszystko poniżej dotyczy przypadków, których ten test nie tworzy.

## 6b.2 Kolejka scrubbera nie odróżnia „wysłane" od „jeszcze nie wysłane"

`get_next_shards_for_scrub` (`db/shards.rs:309`) nie ma **żadnego** `WHERE`. Bierze wszystkie
wiersze `pack_shards`, a sortowanie stawia na początku te, które nigdy nie były weryfikowane:

```sql
ORDER BY CASE WHEN last_verified_at IS NULL THEN 0 ELSE 1 END ASC, ...
```

Shard świeżo zarejestrowany przez packera ma `last_verified_at = NULL` i status `PENDING` —
czyli ląduje **na pierwszym miejscu kolejki weryfikacji, zanim ktokolwiek go wyśle**.
Sonda na kopii bazy roboczej (`ROLLBACK`): po wstawieniu jednego świeżego shardu `PENDING`
zapytanie scrubbera zwraca go jako pozycję **1 z 16**, przed wszystkimi realnie wysłanymi.

`verify_shard` nie patrzy na `shard.status` (pole jest w `ScrubShardRecord`, tylko nieużywane).
Robi HEAD, dostaje 404, `is_missing_error` mówi „MISSING", więc:

- `pack_shards.status` = `'FAILED'`, `verification_failures += 1`, `last_error` = treść 404,
- `summarize_pack_shards` + `resolve_pack_status_for_mode` przeliczają status packa.

Dla packa, którego dwa shardy już poszły, a trzeci jeszcze czeka w kolejce, daje to
`completed = 2` → **`COMPLETED_DEGRADED`**. Repair budzi się na packu, któremu nic nie jest,
pobiera 2 shardy (**4 MiB egressu**), odtwarza z parzystości trzeci i wysyła go — równolegle
z uploaderem, który właśnie miał wysłać oryginał. Pierwszy przebieg scrubbera startuje
natychmiast po starcie daemona (pętla robi partię *przed* pierwszym `sleep`), czyli dokładnie
wtedy, gdy backlog uploadu jest największy.

Docelowo shard i tak zostanie wysłany i oznaczony `COMPLETED`, więc stan sam się goi — ale po
drodze płacimy egress, zapisujemy fałszywy `MISSING` do historii weryfikacji i pokazujemy
użytkownikowi zdegradowany skarbiec (Z6-03).

## 6b.3 Deep verify: „co dwudziesty shard" to w rzeczywistości „pierwszy z każdej partii"

```rust
batch_index.is_multiple_of(modulus)
    || usize::try_from(shard.id).ok().is_some_and(|id| id % modulus == 0)
```

`batch_index` zaczyna się od zera, a **zero jest wielokrotnością każdej liczby**. Pierwszy shard
każdej partii idzie więc w tryb `DEEP` (pełny GET + SHA-256) niezależnie od modulusa. Cały
mechanizm „spokojnego harmonogramu dla małego vaulta" (`SMALL_VAULT_DEEP_MODULUS = 100`)
nie potrafi zejść poniżej jednego pełnego pobrania na partię.

Potwierdzenie w bazie roboczej: na 30 shardów jest **29 weryfikacji `LIGHT` i dokładnie jedna
`DEEP`** — shard `id = 36`. `36 % 100 ≠ 0`, więc reguła identyfikatora nie zadziałała; jedyna
głęboka weryfikacja w historii tej bazy wzięła się z `batch_index == 0`. Przy `id` od 1 do 36
reguła modulusa nie ma prawa nigdy trafić — cała realna próbka DEEP to „pierwszy shard partii".

Rachunek dla vaulta powyżej progu 100 packów (poll co 300 s): 288 partii na dobę × co najmniej
1 pełny GET × 2 MiB = **≥ 576 MiB egressu na dobę z samego scrubbera**, przy dziennym limicie
500 MiB z `config.rs`. Scrubber sam z siebie przekracza limit, `cloud_guard` zawiesza chmurę,
a zawieszenie — patrz **Z6-01** — nie odwiesza się do restartu daemona (Z6-09).

## 6b.4 Repair: jeden pack blokuje wszystkie pozostałe

```sql
SELECT ... FROM packs WHERE status = 'COMPLETED_DEGRADED' ORDER BY pack_id ASC LIMIT 1
```

`get_next_degraded_pack` zwraca zawsze **ten sam, leksykograficznie pierwszy** wiersz, a
`RepairWorker` nie ma żadnego licznika prób, backoffu narastającego ani listy pomijanych packów
(`pack_shards.attempts` rośnie przy `requeue_pack_shard`, ale repair go nie czyta). Pack, którego
nie da się naprawić — brak skonfigurowanego providera dla brakującego shardu, trwały błąd PUT,
niezgodna długość shardu — wraca do przetwarzania co `retry_delay` = **10 sekund, bez końca**,
i **żaden inny zdegradowany pack nigdy nie zostanie naprawiony**, bo kolejka nie idzie dalej.

Cena jest w egressie, nie w CPU: każda próba najpierw pobiera 2 shardy (4 MiB), zanim wywróci
się na uploadzie. 4 MiB co 10 s to **~34 GiB na dobę**. Realnym hamulcem jest tu wyłącznie
`cloud_guard` — po ~125 próbach (500 MiB) chmura idzie w `Suspended` i zostaje tam do restartu
(Z6-01). To samo dotyczy gałęzi rekoncyliacji, która przed każdą nieudaną próbą pobiera pełny
ciphertext packa.

**Naprawione w `9768a5e`.** Zastosowany wzorzec **już istniał w tym repozytorium** — `upload_jobs`
mają kolumnę `next_attempt_at`, funkcję `requeue_upload_job_after` i test
`deferred_job_does_not_starve_later_jobs`, czyli dokładnie to samo lekarstwo na dokładnie tę samą
chorobę, tylko warstwę wyżej. Packi dostały analogiczne, addytywne kolumny `repair_attempts`
i `repair_next_attempt_at` (§2.3), odroczenie z backoffem wykładniczym od `retry_delay` (10 s)
do 1 godziny, czyszczone po udanej naprawie. `get_next_degraded_pack` pomija odroczone w SQL,
`get_next_pack_requiring_reconciliation` — przez `pack_repair_is_deferred`.

Odraczany jest też pack, który **nie** jest objęty wymogiem `healthy`: nie jest błędem, ale bez
odroczenia dalej stałby na czele kolejki i blokował resztę. Do tego `run_batch_now` dostał kursor
odwiedzonych packów, więc pętla kończy się zamiast kręcić w miejscu — to przy okazji zamyka obie
pętle bez wyjścia z **Z6-04**.

Cztery testy w `db/packs.rs` (napisane przed poprawką, każdy najpierw czerwony) pilnują, że
odroczony pack nie głodzi następnych, że wraca po upływie zwłoki, że sukces zeruje licznik prób
i że rekoncyliacja też respektuje odroczenie.

## 6b.5 Trzy wyjścia z `repair_pack`, które nic nie zapisują

`repair_pack` ma trzy wczesne `return Ok(())`. Sprawdzenie, czy któreś potrafi zapętlić workera
(metoda: najpierw szukaj fallbacku):

| Wyjście | Czy osiągalne | Skutek |
| --- | --- | --- |
| `storage_mode != Ec2_1` | **nie** — `COMPLETED_DEGRADED` powstaje wyłącznie w gałęzi `Ec2_1` funkcji `resolve_pack_status_for_mode`, a grep po całym `angeld/src` nie znajduje innego zapisu tego statusu | — |
| `PackStatus::Unreadable` | tak | zapisuje `UNREADABLE`, wiersz wypada z kolejki — poprawnie |
| `PackStatus::Healthy` | tak | **nic nie zapisuje**; wiersz zostaje `COMPLETED_DEGRADED` |

Trzeci przypadek to klasyczne „wykrył i zapomniał zapisać". Powstaje w oknie między oznaczeniem
shardu jako `COMPLETED` przez uploadera a przeliczeniem przez niego statusu packa. W workerze
gałąź sukcesu `repair_pack` **nie ma `sleep`** — pętla wraca natychmiast po ten sam wiersz
i kręci się na pełnych obrotach, logując przy każdym obiegu `repair worker restored pack X to
healthy` (w parze z Z1-01: log rośnie bez ograniczeń). Jedyne, co to gasi, to uploader zapisujący
status kilka milisekund później. Fallback istnieje, ale jest cudzy i przypadkowy (Z6-12).

## 6b.6 `run_batch_now` — pętle, z których nie ma wyjścia

`run_batch_now` obsługuje trzy endpointy administracyjne: `POST /scrub/now`, `POST /repair/now`,
`POST /reconcile/now` (`api/maintenance.rs:393/434/486`) oraz jedną ścieżkę onboardingu
(`api/onboarding.rs:988`). W przeciwieństwie do `run()` **nie ma tu ani jednego `sleep`, ani
kursora** — pętla kończy się dopiero wtedy, gdy zapytanie zwróci `None`:

```rust
if !db::pack_requires_healthy(&self.pool, &pack.pack_id).await? {
    continue;                      // stan bazy sie nie zmienil
}                                  // -> to samo zapytanie -> ten sam pack -> ...
```

`pack_requires_healthy` zwraca `false`, gdy pack nie ma **żadnego** referencjonującego inode'a
(`db/packs.rs:578`). Sonda na bazie roboczej: takie packi **istnieją** — dwa z dziesięciu
(`c56049a3…`, `809f521b…`) mają zero referencji przez `pack_locations ⋈ chunk_refs ⋈
file_revisions`. Wystarczy, że jeden z nich znajdzie się w stanie `COMPLETED_DEGRADED`
(np. po weryfikacji z 6b.2), a `POST /api/maintenance/repair/now` **nigdy nie wraca** i wysyca
rdzeń CPU. Ta sama konstrukcja w gałęzi `ReconcileOnly` powtarza pracę w kółko, jeśli
`reconcile_pack_mode` skończy się wczesnym `Ok(())`.

Warto zestawić: `run()` w tym samym przypadku śpi 5 s i idzie dalej. Ta sama logika w dwóch
opakowaniach — w jednym z zabezpieczeniem, w drugim bez.

**Naprawione w `9768a5e`** ubocznie przy Z6-05: `run_batch_now` prowadzi zbiór odwiedzonych
`pack_id` i przerywa pętlę, gdy zapytanie odda pack już przetworzony. Partia przestaje być
nieskończona niezależnie od tego, czy pack zmienił stan.

## 6b.7 Reconcile — jedyne miejsce, w którym chunk zmienia pack pod użytkownikiem

`reconcile_pack_mode` jest najcięższą operacją w całym daemonie i jedyną, która **podmienia
docelowy pack dla istniejącego chunka**:

```
load_ciphertext_for_pack (GET z chmury albo odczyt spoola)
  -> build_manifest_bytes -> compute_pack_id(desired_mode) -> nowy pack_id
  -> create_pack(status = Uploading | Healthy dla LocalOnly)
  -> register_pack_shard x N (PENDING)
  -> upload_shard x N
  -> update_pack_status
  -> link_chunk_to_pack   <-- SWAP: pack_locations to UPSERT po chunk_id
```

`pack_locations` ma `chunk_id` jako **PRIMARY KEY**, więc `link_chunk_to_pack` nie dodaje
powiązania, tylko **przenosi** je na nowy pack. Stary pack w tej samej chwili traci wiersz
w `pack_locations` → staje się sierotą dla workera gc → `collect_pack` kasuje jego **obiekty
w chmurze**, wiersze w bazie i pliki spoola. Podmiana jest więc nieodwracalna w ciągu ~10 sekund.

**Wyścig z gc.** Między `create_pack` a `link_chunk_to_pack` nowy pack nie ma wiersza
w `pack_locations`, czyli spełnia definicję sieroty. Sprawdzenie fallbacku:
`get_orphaned_pack_ids` ma warunek `AND p.status != 'UPLOADING'` — i to ratuje ścieżkę chmurową,
bo nowy pack powstaje jako `UPLOADING` i zmienia status dopiero po wszystkich uploadach.
**Fałszywy alarm dla EC/STANDARD.** Ale:

- gałąź `LocalOnly` tworzy pack od razu jako `PackStatus::Healthy`, więc **nie jest osłonięta**.
  W oknie między `create_pack` a `link_chunk_to_pack` gc może skasować wiersz packa **oraz plik
  `.odpk` ze spoola** (`gc.cleanup_local_files`), a `pack_locations` **nie ma klucza obcego do
  `packs`** (`schema.rs:345` — zwykły `TEXT NOT NULL`), więc `link_chunk_to_pack` spokojnie
  wskaże nieistniejący pack. Efekt: plik objęty polityką `LOCAL` przestaje być odczytywalny,
  a jego jedyna kopia zniknęła ze spoola. Okno to dwa zapytania (~ms), gc chodzi co 10 s —
  prawdopodobieństwo niskie, skutek nieodwracalny (Z6-07).
- endpoint `POST /api/maintenance/gc` woła **inną** funkcję (`db::gc_orphan_packs`), która
  warunku `!= 'UPLOADING'` nie ma w ogóle — ręczne uruchomienie gc w trakcie rekoncyliacji
  skasuje metadane nowego packa również w trybie chmurowym.

**Co się dzieje, gdy nowy pack zniknie w trakcie uploadu** (prześledzone krok po kroku):
kolejne `mark_pack_shard_completed` trafiają w zero wierszy, `summarize_pack_shards` zwraca same
zera, `resolve_pack_status_for_mode` daje `Unreadable`, warunek `status == Healthy` nie
przechodzi i **SWAP się nie wykonuje**. Dane użytkownika są bezpieczne — ale wysłane shardy
zostają w bucketach bez żadnego wiersza w `pack_shards`, więc gc nigdy ich nie znajdzie.
Płacimy za nie bez końca, a rekoncyliacja rusza od zera przy następnym obiegu.

## 6b.8 Repair ufa bajtom, których nie sprawdził

`pack_shards.checksum` (`TEXT NOT NULL`, SHA-256 shardu) jest w `PackShardRecord`, jest liczony
przy pakowaniu i jest weryfikowany przez scrubber w trybie DEEP. **`repair.rs` nie odwołuje się
do niego ani razu.** `download_shard` sprawdza wyłącznie długość:

```rust
if bytes.len() != shard_len { return Err(RepairError::InvalidShardLayout(...)); }
```

Shard uszkodzony w środku, ale o poprawnej długości — a to jest typowa postać bit rotu i
dokładnie ten przypadek, dla którego istnieje tryb DEEP — przechodzi. Reed-Solomon nie ma jak
tego wykryć przy 2+1 (brak nadmiaru na detekcję), więc `reconstruct` zwraca poprawnie wyglądające
śmieci. Dalej: PUT, `mark_pack_shard_completed`, `COMPLETED_HEALTHY`.

Skutki idą w dwie strony:

- **Naprawa:** przebudowany shard ma teraz inną treść niż suma kontrolna w bazie. Najbliższa
  weryfikacja DEEP zgłosi `CORRUPTED` → `FAILED` → `COMPLETED_DEGRADED` → repair znowu odtworzy
  go z tych samych złych danych. Pętla wykrywania i „naprawiania" tego samego uszkodzenia,
  z pełnym egressem przy każdym obiegu.
- **Rekoncyliacja:** ciphertext złożony z niezweryfikowanych shardów staje się podstawą nowego
  packa, `link_chunk_to_pack` przepina na niego chunk, a gc kasuje stary pack **wraz z obiektami
  w chmurze**. Jedyną linią obrony jest tu AES-GCM przy odczycie — wykryje, że dane są złe, ale
  wykrycie następuje po skasowaniu dobrej kopii. To jest **detekcja bez ratunku** (Z6-06).

**Naprawione w `f667d4f`** (decyzja Przemka, 2026-08-01): `download_shard` porównuje
`hex_sha256(bytes)` z `pack_shards.checksum` i odrzuca bajty **przed** zapisem do spoola,
na wszystkich trzech ścieżkach pobrania. Sprawdzone przed zmianą, żeby bramka nie zablokowała
istniejących packów: sumę zapisują tylko `packer`, `migrator` i sam `repair` (wszystkie przez
`hex_sha256` po dokładnie tych bajtach, które idą do chmury), a `db/graft.rs` przepisuje
wartość ze snapshotu — format jest jednolity. Testy: `rejects_bytes_with_flipped_bit`
i `accepts_bytes_matching_registered_checksum` budują prawdziwy shard przez
`packer::build_shards` i porównują z jego zarejestrowaną sumą.

**Skutek uboczny:** uszkodzony shard zamiast po cichu zepsuć packa zatrzymuje teraz naprawę tego
packa. Dopóki kolejka nie miała kursora, zatrzymywał też naprawę wszystkich pozostałych — dlatego
**Z6-05 zostało naprawione w tej samej sesji** (`9768a5e`, §6b.4). Po obu zmianach pack z
uszkodzonym shardem jest odraczany z narastającym backoffem i reszta kolejki idzie dalej.

## 6b.9 Drobiazgi o dużym zasięgu

- **Klasyfikacja błędów przez `contains()` na sklejce tekstu.** `format_error_details` skleja
  `display`, `debug` i cały łańcuch `source`, a `is_missing_error` / `is_transient_error` szukają
  w tym podciągów `"404"`, `"500"`, `"tls"`, `"dns"`. Identyfikator żądania, nagłówek albo treść
  XML od providera wystarczą, by błąd przejściowy został zaklasyfikowany jako `MISSING`
  (a więc: shard → `FAILED`, pack → degradacja, repair → egress). `is_missing_error` jest
  sprawdzane **przed** `is_transient_error`, więc kolizja rozstrzyga się na niekorzyść.
  Ten sam wzorzec siedzi w `gc.rs` (`is_not_found_details`), gdzie decyduje, czy uznać
  skasowanie obiektu za udane (Z6-10).
- **Poprawka `request_checksum_calculation(WhenRequired)` nie obowiązuje w tej warstwie.**
  Grep po repozytorium: występuje wyłącznie w `uploader.rs:219` (plus test regresyjny).
  `repair.rs`, `scrubber.rs` i `gc.rs` budują własne `aws_sdk_s3::config::Builder` bez niej.
  Dziś repair wysyła ciało w pamięci (`ByteStream::from(Vec<u8>)`), więc SDK policzy sumę jako
  zwykły nagłówek zamiast kodowania `aws-chunked` z trailerem i objaw z live smoke'u (R2 zrywa
  połączenie, Scaleway czeka do timeoutu) najpewniej się nie powtarza — **to jest rozumowanie
  o mechanizmie, nie obserwacja z sieci**. Ryzyko jest inne: test regresyjny pilnuje tylko
  uploadera, więc przejście repair na ciało strumieniowe przywróci błąd po cichu (Z6-11).
- **`reset_in_progress_pack_shards()` na starcie repaira** to globalny `UPDATE pack_shards SET
  status='PENDING' WHERE status='IN_PROGRESS'` — bez ograniczenia do packów, którymi repair się
  zajmuje. Uploader startuje w tej samej serii `tokio::spawn` w `main.rs`; shard, który zdążył
  oznaczyć jako `IN_PROGRESS`, wraca do kolejki i zostanie wysłany drugi raz (podwójny PUT,
  podwójny licznik kwoty) (Z6-13).
- **Spool rośnie po każdej naprawie.** `download_shard` zapisuje każdy pobrany shard do spoola
  i nikt tego nie kasuje, dopóki pack nie zostanie osierocony. Naprawa jednego packa zostawia
  ~4 MiB. W bieżącym `.omnidrive/spool` leży 6 plików `.shard*` (7,3 MB) po testach (Z6-14).
- **Obie pętle umierają po cichu.** `ScrubberWorker::run` i `RepairWorker::run` propagują każdy
  błąd SQLite przez `?`, a oba zadania stoją poza `tokio::select!` w `main.rs` (Z1-02, Z4-09).
  Jedna blokada bazy przez Defendera i integralność przestaje być pilnowana aż do restartu —
  diagnostyka zostaje na ostatnio ustawionym statusie, więc UI dalej pokazuje `Idle` (Z6-15).
- **`#![allow(dead_code)]` na obu plikach**, z komentarzami „reserved for future integrity-scrubbing
  epic" i „reserved for future repair epic" — przy modułach, które są spawnowane w `main.rs`,
  wystawione na trzech endpointach i objęte testem e2e. Realnie martwe są: `provider_clients_from_env`
  (w obu plikach) i `ScrubberWorker::should_deep_verify` — zero wywołujących (Z6-16).

## 6b.10 Sprostowanie do §6.2

Opis gc w rozdziale 6 miesza dwie różne funkcje. Obie istnieją i mają **różne definicje sieroty
oraz różne skutki**:

| | worker `GcWorker::run` | endpoint `POST /api/maintenance/gc` |
| --- | --- | --- |
| funkcja | `db::get_orphaned_pack_ids` | `db::gc_orphan_packs` |
| kryterium | brak wiersza w `pack_locations` **i** `status != 'UPLOADING'` | brak `pack_locations ⋈ chunk_refs` |
| kasuje obiekty w chmurze | **tak** (`delete_object` per shard) | **nie** |
| kasuje pliki spoola | tak | nie |

Zdanie z §6.2 („`gc` kasuje packi bez żadnego `chunk_refs` wskazującego na nie… usuwa komplet:
`upload_job_targets` → `upload_jobs` → `pack_locations` → `packs`") opisuje **endpoint**,
a przypisuje to workerowi. Praktyczna konsekwencja rozjazdu: pack, który stracił `chunk_refs`,
ale zachował wiersz w `pack_locations`, jest niewidzialny dla workera, a endpoint skasuje jego
metadane **bez kasowania obiektów** — po czym `pack_shards` zniknie kaskadą i nikt już nie będzie
wiedział, jakie klucze zostały w bucketach. Sonda: w bieżącej bazie roboczej są **2 takie packi
z 10** (Z6-08).

## 6b.11 Znaleziska

| ID | Waga | Rzecz | Potwierdzone jak |
| --- | --- | --- | --- |
| Z6-03 | 🔴 | Scrubber weryfikuje shardy jeszcze niewysłane: `get_next_shards_for_scrub` bez `WHERE`, a `last_verified_at IS NULL` stawia je pierwsze → 404 → `FAILED` + `MISSING` → pack `COMPLETED_DEGRADED` → repair pobiera 4 MiB na packu, któremu nic nie było | czytanie + sonda SQLite (świeży `PENDING` = pozycja 1 z 16) |
| Z6-04 | ✅ | `run_batch_now` bez `sleep` i bez kursora: `continue` przy `!pack_requires_healthy` i przy wczesnym `Ok(())` z `repair_pack` dawał pętlę bez wyjścia w handlerze HTTP | **NAPRAWIONE** `9768a5e` — kursor odwiedzonych packów |
| Z6-05 | ✅ | `get_next_degraded_pack` = `ORDER BY pack_id LIMIT 1`, zero prób/backoffu/skip-listy → nienaprawialny pack wracał co 10 s i blokował naprawę wszystkich pozostałych | **NAPRAWIONE** `9768a5e` — odroczenie jak w `upload_jobs` |
| Z6-06 | ✅ | Repair nie sprawdzał `pack_shards.checksum` — tylko długość; uszkodzony shard → RS odtwarzał śmieci → `COMPLETED_HEALTHY`, a przy rekoncyliacji gc kasował oryginał **wraz z obiektami w chmurze** | **NAPRAWIONE** `f667d4f` |
| Z6-07 | 🔴 | Wyścig reconcile ↔ gc w gałęzi `LocalOnly`: pack powstaje jako `Healthy`, więc osłona `status != 'UPLOADING'` go nie obejmuje; gc kasuje wiersz **i manifest `.odpk` ze spoola**, a `pack_locations` nie ma FK do `packs` → chunk wskazuje nieistniejący pack | czytanie + `schema.rs:345` (brak FK) + sprawdzony fallback dla ścieżki chmurowej |
| Z6-08 | ⚠️ | Dwie definicje sieroty; endpoint `/api/maintenance/gc` kasuje metadane **bez** kasowania obiektów w chmurze — klucze przepadają razem z `pack_shards` | czytanie obu funkcji + sonda (2 z 10 packów spełniają tylko kryterium endpointu) |
| Z6-09 | ⚠️ | `batch_index.is_multiple_of(modulus)` — indeks 0 zawsze trafia, więc pierwszy shard każdej partii idzie DEEP; „spokojny tryb" małego vaulta nic nie zmienia. Dla vaulta >100 packów daje ≥576 MiB/dobę przy limicie 500 MiB → zatrzask z Z6-01 | czytanie + sonda (jedyna weryfikacja DEEP: `id=36`, `36 % 100 ≠ 0`) |
| Z6-10 | ⚠️ | Klasyfikacja błędów przez `contains("404"/"500"/"tls"/"dns")` na sklejce `display + debug + source`; `MISSING` sprawdzane przed `transient`. Ten sam wzorzec decyduje w `gc.rs` o uznaniu skasowania za udane | czytanie |
| Z6-11 | ⚠️ | Poprawka `request_checksum_calculation(WhenRequired)` istnieje wyłącznie w `uploader.rs`; repair/scrubber/gc budują klienta S3 bez niej, a test regresyjny pilnuje tylko uploadera | grep: 1 trafienie w kodzie produkcyjnym |
| Z6-12 | ⚠️ | `repair_pack` przy `PackStatus::Healthy` nie zapisuje statusu; wiersz zostaje `COMPLETED_DEGRADED`, a gałąź sukcesu w `run()` nie ma `sleep` → gorąca pętla z logiem „restored pack X to healthy" do czasu, aż status zapisze uploader | czytanie + tabela osiągalności wyjść |
| Z6-13 | ⚠️ | `reset_in_progress_pack_shards()` na starcie repaira jest globalny — kasuje `IN_PROGRESS` również uploaderowi startującemu w tej samej serii `spawn` → podwójny PUT | czytanie + `main.rs:757-760` |
| Z6-14 | ⚠️ | `download_shard` zapisuje każdy pobrany shard do spoola i nikt tego nie kasuje, dopóki pack nie osieroci → ~4 MiB na każdą naprawę | czytanie + zawartość `.omnidrive/spool` |
| Z6-15 | ⚠️ | `ScrubberWorker::run` i `RepairWorker::run` kończą się na `?` przy pierwszym błędzie SQLite, oba poza `tokio::select!` — integralność cicho przestaje być pilnowana (para z Z1-02, Z4-09) | czytanie |
| Z6-16 | ⚠️ | `#![allow(dead_code)]` na obu plikach („reserved for future … epic") przy modułach produkcyjnych; realnie martwe: `provider_clients_from_env` ×2, `should_deep_verify` | grep: 0 wywołujących |

---

# 8. Cross-device — dołączanie urządzeń, kopia metadanych, mesh LAN

Warstwa, która odpowiada na pytanie „skąd drugie urządzenie wie cokolwiek o Skarbcu".
Trzy niezależne kanały, żaden nie wie o pozostałych:

1. **Kopia metadanych w chmurze** (`disaster_recovery.rs`) — cała baza SQLite zaszyfrowana
   kluczem z hasła głównego, wrzucona jako jeden obiekt. Kanał wolny (raz na dobę), ale jedyny,
   który przenosi klucze.
2. **Mesh LAN** (`peer.rs`) — broadcast UDP + HTTP na porcie 8788, oddaje **plaintextowe chunki**
   sąsiadowi w tej samej sieci, żeby nie płacić za egress.
3. **Named Pipe** (`pipe_server.rs`) — kanał sterowania z rozszerzenia powłoki do daemona.

`onboarding.rs` i `db/graft.rs` to sekwencja „Join Existing Vault": pobierz kopię (1), rozszyfruj
hasłem, przeszczep do lokalnej bazy.

## 8.1 Mapa warstwy

| Plik | Linie | Rola |
| --- | --- | --- |
| `onboarding.rs` | 1341 | Stan kreatora, drafty providerów z `.env`, DPAPI dla sekretów S3, walidacja połączenia, orkiestracja restore. |
| `db/graft.rs` | 1621 | `graft_restored_metadata_snapshot` (destrukcyjny, przy dołączaniu) i `graft_roster_additive` (przyrostowy, worker). |
| `disaster_recovery.rs` | 3044 (1694 kodu) | Migawka `VACUUM INTO` → AES-GCM → S3; pobieranie, deszyfrowanie, dwa workery godzinowe. |
| `peer.rs` | 592 | Odkrywanie po UDP + serwer HTTP oddający chunki sąsiadom. |
| `pipe_server.rs` | 360 | Named Pipe `\\.\pipe\omnidrive_shellcmd`, 6 komend z menu kontekstowego. |
| `sharing.rs` | 233 | Czytane przy Z4-01 (dwupoziomowa koperta dla linków share). |

## 8.2 „Join Existing Vault" — pełna ścieżka

```
wizard krok 5 (tryb „join")
  POST /api/onboarding/join-existing {passphrase, provider_id}
    → dla każdego providera po kolei:
        perform_vault_restore(pool, runtime_paths, passphrase, provider_id)
          1. MetadataBackupProviderManager::from_onboarding_db  (klient S3 z DPAPI-owych sekretów)
          2. restore_metadata_from_cloud → latest.db.enc, potem do 32 migawek (od najnowszej)
             → decrypt_metadata_backup (Argon2 z parametrów z NAGŁÓWKA pliku, nie z lokalnej bazy)
             → write_plaintext_snapshot_if_valid → init_db(snapshot) → czy jest vault_state?
          3. db::graft_restored_metadata_snapshot(pool, staging)   ← przeszczep
          4. secure_fs::secure_delete(staging)
    → ensure_local_device_in_vault + generate_local_device_keypair + create_session_for_local_device
```

Dwie rzeczy w tym projekcie są zrobione **wyraźnie dobrze**:

- **Parametry Argon2 jadą w nagłówku pliku kopii**, nie są brane z lokalnej bazy. Świeże urządzenie
  nie ma jeszcze żadnego `vault_config`, więc każde inne rozwiązanie wymagałoby drugiego kanału.
  `MetadataBackupKeyCache` dokłada do tego cache po `RootKdfParams`, żeby przy 32 kandydatach
  nie liczyć Argon2 32 razy.
- **Fallback na starsze migawki.** Jeśli `latest.db.enc` jest uszkodzony, restore idzie po liście
  `snapshots/` posortowanej **malejąco** (`newest_snapshot_keys`) i bierze pierwszą, która się
  rozszyfruje i ma wiersz `vault_state`. Sprawdzone: sortowanie jest odwrotne (`b.cmp(a)`), więc
  nie ma pułapki „przy awarii wraca najstarsza kopia". `MAX_DECRYPT_FAILURES = 3` odróżnia przy tym
  „złe hasło" od „nie ma czego pobrać" — bez tego użytkownik dostawałby komunikat o sieci
  przy literówce w haśle.

Nieoczywisty, ale nośny mechanizm: `snapshot_has_vault_state_row` otwiera pobraną migawkę przez
**`db::init_db`**, czyli przepuszcza ją przez pełny zestaw migracji. To dlatego graft może robić
`SELECT ... FROM users` na migawce z czasów sprzed multi-user — tabela zostaje dorobiona w locie.
Skutek uboczny: gałęzie `unwrap_or_default()` w `graft.rs` (l. 320-374), opisane w komentarzu jako
zabezpieczenie przed starym snapshotem, są w praktyce nieosiągalne — do grafta trafia zawsze baza
w bieżącym schemacie.

## 8.3 Przeszczep — co dokładnie ginie, co zostaje

`graft_restored_metadata_snapshot` to operacja **destrukcyjna**: kasuje 18 tabel lokalnych
(od `upload_job_targets` po `users`) i wstawia zawartość migawki. Lokalne pliki, których nie ma
w migawce, znikają z metadanych bezpowrotnie. To jest zamierzone — „dołączam do cudzego Skarbca"
nie jest scalaniem — ale nigdzie nie jest to napisane w UI kreatora.

Zachowane lokalnie są dokładnie trzy rzeczy: `master_key_salt` + `argon2_params` w `vault_state`
(gałąź `Some(local)`, l. 405-427), `provider_secrets` (DPAPI jest per-maszyna, nie da się przenieść)
i `local_device_identity`.

**Sprawdzony fałszywy alarm.** Gałąź `Some(local)` bierze lokalny `master_key_salt`, ale zdalny
`encrypted_vault_key` — wygląda to na gwarantowany rozjazd KDF. Nie jest: `vault_state.master_key_salt`
nie bierze udziału w wyprowadzaniu klucza. `unlock()` czyta `vault_config` (`ensure_vault_config`),
a `ensure_local_vault_params` używa `master_key_salt` wyłącznie do zbudowania nazwy `local-vault-…`.
`vault_config` jest grafowany osobno (l. 456-475), więc EVK i sól pochodzą z tej samej migawki.

Co **nie** jest grafowane, a powinno: **`pack_deks`**. Szczegóły w §8.5.

## 8.4 `PRAGMA foreign_keys = OFF` w transakcji — no-op

Graft otwiera `BEGIN IMMEDIATE TRANSACTION`, a **potem** wyłącza klucze obce (l. 389-396).
SQLite ignoruje tę pragmę wewnątrz transakcji. Sonda:

```
fk before tx: 1
fk inside tx after OFF: 1     ← pragma nic nie zrobiła
```

Pula ma `foreign_keys(true)` (`schema.rs:12`), więc egzekwowanie FK trwa przez cały przeszczep.
Kolejność `DELETE`/`INSERT` w graftie **przypadkiem** to znosi dla większości tabel (rodzic ma
zawsze niższe `id` niż dziecko, bo powstał wcześniej), z jednym wyjątkiem: `user_sessions.user_id
REFERENCES users(user_id)`, a `user_sessions` nie jest na liście kasowanych. Odtworzenie sekwencji
na kopii bazy roboczej (`ROLLBACK`, oryginał nietknięty):

```
OK   DELETE FROM inodes
OK   DELETE FROM vault_members
OK   DELETE FROM devices
FAIL DELETE FROM users -> FOREIGN KEY constraint failed
```

131 wierszy w `user_sessions` w bieżącej bazie. Świeża instalacja przechodzi (kreator w trybie
„join" nie woła `/api/vault/unlock`, więc sesji jeszcze nie ma), ale każde urządzenie, które
kiedykolwiek odblokowało Skarbiec — czyli także scenariusz „reset onboardingu i dołącz do innego
Skarbca" — dostanie `apply_failed` bez wskazówki, co jest nie tak. `cleanup_expired_sessions`
nigdy nie jest wołane (Z2-04), więc sesje same nie znikną.

## 8.5 Klucz packa nie przechodzi przez granicę urządzenia

Po naprawie Z4-01 wiążącą jest tabela `pack_deks (pack_id → dek_id)`. Graft jej nie kopiuje —
grep po `graft.rs` daje zero trafień. Migawka ją ma (`VACUUM INTO` kopiuje całą bazę), gubi ją
dopiero przepisywanie tabela-po-tabeli.

Ratunkiem miał być fallback w `dek_for_pack` (`vault.rs:494-503`): brak wiersza → znajdź inode,
którego najwcześniejsza rewizja odwołuje się do packa → weź jego DEK. Tyle że `packer.rs:262`
mintuje **DEK na każdy chunk** w pętli, wszystkie z tym samym `inode_id` i rosnącym `key_version`,
a `get_wrapped_dek` zwraca `ORDER BY key_version DESC LIMIT 1`. Sonda na wiernym odwzorowaniu obu
zapytań (plik z trzech chunków):

```
packA: correct dek=1  fallback dek=3  *** WRONG ***
packB: correct dek=2  fallback dek=3  *** WRONG ***
packC: correct dek=3  fallback dek=3  OK
```

Czyli: po dołączeniu urządzenia każdy plik większy niż jeden chunk odszyfrowuje się poprawnie
wyłącznie w ostatnim chunku, reszta kończy się błędem tagu GCM. Gorzej — linia `vault.rs:501`
robi `db::set_pack_dek(pool, pack_id, record.dek_id)`, czyli **zapisuje błędne powiązanie na stałe**,
a kolejna kopia metadanych z tego urządzenia wynosi je do chmury.

## 8.6 Dwa workery godzinowe

| Worker | Tick | Warunek | Co robi |
| --- | --- | --- | --- |
| `start_metadata_backup_worker` | 60 min | ≥24 h od ostatniego sukcesu | `VACUUM INTO` → AES-GCM → PUT `snapshots/<ts>.db.enc` (+ `latest.db.enc`); po drodze kopia lokalna `omnidrive.db.bak.<stamp>`, retencja 3. |
| `start_metadata_fetch_worker` | 60 min | ≥60 min od ostatniego zastosowania | LIST wszystkich providerów, najnowsza migawka → GET → `graft_roster_additive` (tylko `devices` i `vault_members`, `INSERT OR IGNORE`). |

**Bezpiecznik wart odnotowania.** `latest_pointer_may_advance` nie przesuwa wskaźnika `latest`,
jeśli migawka nie ma `vault_state`/`vault_config` albo jeśli liczba inode'ów lub DEK-ów spadła
do zera przy niezerowej poprzedniej wartości. Migawka i tak leci do `snapshots/`, więc nic nie ginie,
ale zdegradowana baza nie nadpisze punktu odniesienia dla restore'u. To jest właściwie
zaprojektowana ochrona przed „kopia zapasowa pustki".

Oba workery mają ten sam problem, którego bezpiecznik nie łapie: **`run_metadata_fetch_now`
przekazuje `pool = None`** do `list_snapshot_keys` i `download_bytes` (l. 1153, 1177).
Sygnatura tych funkcji ma `Option<&SqlitePool>` właśnie po to, żeby wejść w `cloud_guard`, ale
w tej ścieżce nikt go nie podaje. Skutek: godzinne LIST-y na trzech providerach i pobranie całej
migawki nie podlegają ani wyłącznikowi awaryjnemu (Z6-01), ani liczeniu egressu. Dodatkowo marker
`last_applied_roster_snapshot_at` przesuwa się **dopiero po udanym graftcie** — a `graft_roster_additive`
odrzuca migawkę o innym `vault_id`. Urządzenie z niepasującym `vault_id` będzie więc pobierać tę
samą migawkę co godzinę, bez końca i bez rachunku.

## 8.7 Mesh LAN — zaufanie z rozgłoszenia UDP

`PeerService` startuje bezwarunkowo w obu gałęziach trybu (`main.rs:661` i `:765`) i nasłuchuje
na **`0.0.0.0:8788`** (HTTP) oraz `0.0.0.0:8789` (UDP). Co 5 sekund rozgłasza na broadcast:

```json
{"device_id":"…","device_name":"…","vault_id":"…","peer_port":8788}
```

Odbiór cudzego ogłoszenia → `db::note_peer_seen(...)`. Ta funkcja wstawia wiersz z **`trusted = 1`**
i `ON CONFLICT … SET trusted = 1`. Żadnego wyzwania kryptograficznego, żadnego podpisu, żadnego
sprawdzenia klucza publicznego z tabeli `devices`. `handshake_peer` woła potem `/peer/hello`
u nadawcy, ale jego niepowodzenie zapisuje tylko `last_error` — **`trusted` zostaje 1**.

`GET /peer/chunks/{chunk_hex}` autoryzuje wyłącznie po dwóch nagłówkach ustawianych przez klienta:
`x-omnidrive-caller-device` musi być w `trusted_peers` z `trusted != 0`, a `x-omnidrive-vault-id`
musi się zgadzać z lokalnym. Oba są w rozgłoszeniu, które sami wysyłamy co 5 sekund w otwartej
sieci. Odpowiedź to **plaintext chunka**.

Zakres ograniczają dwie rzeczy, obie sprawdzone: `read_plaintext_chunk_by_id` trafia najpierw
do cache'u, ale `CacheManager::get_chunk` deszyfruje wpis kluczem sesji, więc przy zablokowanym
Skarbcu endpoint zwróci 500, nie dane. I ruch nie wychodzi poza segment broadcastu.

**Sprawdzony fałszywy alarm.** `evaluate_peer_policy` liczy backoff jako
`last_error.is_some() && now - last_seen_at < error_backoff_ms`, a `note_peer_seen` odświeża
`last_seen_at` co 5 s przy backoffie 15 s — wygląda na zakleszczenie „jeden błąd = peer martwy
na zawsze". Nie jest: udany `handshake_peer` woła `upsert_trusted_peer(..., None)`, a ta gałąź
ma `last_error = excluded.last_error`, czyli czyści błąd. Backoff gryzie tylko wtedy, gdy
`/peer/hello` też nie odpowiada — czyli zgodnie z intencją.

## 8.8 Named Pipe — kanał sterowania bez uwierzytelnienia

```rust
const SDDL_EVERYONE_RW: &str = "D:(A;;GRGW;;;WD)\0";
```

`WD` to `Everyone`. Pipe `\\.\pipe\omnidrive_shellcmd` przyjmuje połączenia od dowolnego procesu
w systemie i nie sprawdza nadawcy w żaden sposób — brak `GetNamedPipeClientProcessId`,
brak porównania tokenu, brak sekretu w protokole. Sześć komend (`free_space`, `download`,
`set_lokalnie`, `set_combo`, `set_chmura`, `set_forteca`) omija komplet kontroli z `acl.rs`,
które chronią odpowiadające im endpointy HTTP. `download` wymusza hydratację (egress + plaintext
na dysku), `set_lokalnie` degraduje politykę pliku do braku redundancji w chmurze.

Komentarz nad `create_pipe_instance` uzasadnia luźny ACL potrzebą rozmowy nieelevowanego
Eksploratora z elevowanym daemonem. Uzasadnienie jest prawdziwe, ale rozwiązanie za szerokie —
`GRGW` dla `WD` zamiast SID-u interaktywnego użytkownika.

Drugi problem tego samego pliku: `FILE_FLAG_FIRST_PIPE_INSTANCE` na pierwszej instancji sprawia,
że jeśli **inny proces zdąży zająć tę nazwę wcześniej**, `create_pipe_instance(true)` zwraca błąd,
`run_pipe_server` loguje `error!` i **kończy się** (nie ma retry). Rozszerzenie powłoki rozmawia
wtedy z podstawionym pipe'em i oddaje mu ścieżki plików. Ta sama funkcja kończy pętlę na błędzie
odtworzenia instancji (l. 147), co dokłada się do Z1-02.

**Korekta do Z7-01.** Rozszerzenie powłoki w `omnidrive-shell-ext/` **nie używa HTTP w ogóle** —
w `Cargo.toml` nie ma żadnej zależności sieciowej, a wszystkie sześć pozycji menu idzie przez
`send_pipe_command`. Z7-01 dotyczy drugiej, równoległej implementacji menu — rejestrowej
(`shell_integration.rs`, `HKCU\Software\Classes`, 5 pozycji wołających `api_base`), i tam zarzut
o brak `Authorization` jest trafny. Istotny jest sam fakt dwóch niezależnych menu kontekstowych
o pokrywającym się zakresie. Praktycznie w instalatorze (`installer/omnidrive.iss`, sekcja `[Files]`)
jest tylko `angeld.exe`, tray i `omnidrive.exe` — **DLL rozszerzenia nie jest instalowany**,
więc na Dellu działa wariant rejestrowy (ten z 401), a pipe stoi otwarty bez żadnego
legalnego klienta.

## 8.9 Sekrety dostawców — DPAPI bez entropii

`seal_provider_secrets` woła `CryptProtectData` z `pOptionalEntropy = None` (l. 1272). Klucze
dostępowe do R2/B2/Scaleway są więc odzyskiwalne przez **dowolny proces działający na koncie
użytkownika**, bez znajomości hasła głównego — wystarczy odczytać `provider_secrets` z bazy
i wywołać `CryptUnprotectData`. To ta sama klasa co Z7-02/Z7-03, ale dotyczy innego zasobu:
nie hasła do Skarbca, tylko poświadczeń, którymi można skasować całą zawartość bucketów.
Rozwiązaniem współgrającym z resztą architektury byłoby opieczętowanie ich Vault Key
(jak `google_refresh_token_ciphertext`), a nie DPAPI.

## 8.10 Higiena plików tymczasowych

Trzy miejsca zapisują na dysk **plaintextową bazę metadanych**:

| Ścieżka | Sprzątanie |
| --- | --- |
| `runtime_base_dir/restore-staging-<ms>.db` | `secure_delete` po graftcie (zerowanie + retry) ✅ |
| ten sam plik po ubiciu procesu | `cleanup_stale_restore_staging` — zwykły `remove_file`, **bez zerowania**; filtr `.db` nie łapie `-wal`/`-shm` |
| `%TEMP%/omnidrive-roster-fetch-<ms>.db` | `secure_delete` na ścieżce sukcesu, `fs::remove_file` na dwóch ścieżkach błędu (l. 1099, 1106) |

Komentarz w `perform_vault_restore` (l. 769-771) obiecuje, że „`cleanup_stale_restore_staging()`
dokończy robotę na następnym starcie" — dokończy tylko odlinkowanie, nie nadpisanie.
Do tego `snapshot_has_vault_state_row` otwiera migawkę przez `init_db`, czyli w trybie WAL,
co dokłada sidecary o nazwach niepasujących do filtra sprzątającego.

Osobno: `run_local_db_backup_if_due` trzyma **trzy nieszyfrowane kopie** `omnidrive.db.bak.<stamp>`
obok bazy. Same w sobie nie ujawniają więcej niż `omnidrive.db`, ale przeżywają każde „bezpieczne
skasowanie bazy" i nikt ich nie liczy przy rozważaniu, gdzie leżą metadane.

## 8.11 Co jest napisane dobrze

- **Format kopii metadanych** — magic + wersja + parametry KDF + sól + nonce + ciphertext + tag,
  parser sprawdza długości przed każdym wycięciem, `parse_metadata_backup` nie ma ani jednego
  indeksowania bez `get`/`try_into`. Kopia jest samoopisująca się i da się ją odczytać na maszynie,
  która nigdy nie widziała tego Skarbca.
- **`retry_with_backoff` z predykatem `is_retryable`** — 403 przerywa natychmiast zamiast tłuc się
  czterokrotnie. To dokładnie ta poprawka, której brakuje w `uploader.rs` (Z4-08), zrobiona tu
  poprawnie i przetestowana trzema testami.
- **Komunikat przy `AccessDenied` na prefiksie metadanych** (l. 974-981) tłumaczy wprost, że to
  polityka IAM, a nie błąd kodu, i że pozostali providerzy nie są dotknięci. Rzadki przypadek
  logu, który oszczędza godzinę diagnostyki.
- **`graft_roster_additive`** odrzuca migawkę o cudzym `vault_id` i nie dotyka `data_encryption_keys`,
  `vault_state` ani `vault_recovery_keys` — ma na to dedykowany test (`…never_touches_dek`).

## 8.12 Czego nie łapią testy

`graft.rs` ma 9 testów, `disaster_recovery.rs` 19. Żaden nie wstawia wiersza do `user_sessions`
przed graftem (Z8-03 przechodzi), żaden nie tworzy pliku z więcej niż jednym chunkiem (Z8-04
przechodzi), żaden nie sprawdza, czy `pack_deks` przetrwało przeszczep. Wzorzec zgodny z
[[feedback-smoke-over-unit-tests]]: testy sprawdzają, że zaimplementowane kroki robią to,
co miały robić, a nie że po całej operacji urządzenie potrafi odczytać plik.

## 8.13 Znaleziska

| ID | Waga | Rzecz | Potwierdzone jak |
| --- | --- | --- | --- |
| Z8-01 | 🔴 | Named Pipe z DACL `Everyone GR/GW` i zerową weryfikacją wywołującego — dowolny lokalny proces wymusza hydratację i zmienia politykę ochrony pliku, omijając `acl.rs`. Instalator nie wgrywa DLL-a rozszerzenia, więc pipe stoi otwarty bez legalnego klienta | czytanie SDDL + `[Files]` w `installer/omnidrive.iss` |
| Z8-02 | 🔴 | `note_peer_seen` ustawia `trusted = 1` na podstawie samego ogłoszenia UDP; `/peer/chunks/{hex}` autoryzuje po dwóch nagłówkach, które sami rozgłaszamy co 5 s → plaintext plików dla dowolnego hosta w LAN (przy odblokowanym Skarbcu) | czytanie + `db/device_identity.rs:237` (`trusted` = 1 na sztywno) |
| Z8-03 | 🔴 | `PRAGMA foreign_keys = OFF` wewnątrz `BEGIN IMMEDIATE` to no-op → `DELETE FROM users` wywala się o FK z `user_sessions` → cały graft `ROLLBACK`, join-existing niemożliwy na urządzeniu, które kiedykolwiek się odblokowało | sonda SQLite (fk=1 po OFF; DELETE FAIL na kopii bazy roboczej, 131 sesji) |
| Z8-04 | 🔴 | Graft nie kopiuje `pack_deks`; fallback `dek_for_pack` bierze DEK o najwyższym `key_version` dla inode'a, a packer mintuje DEK na chunk → po dołączeniu wszystkie packi poza ostatnim dostają zły klucz, a `set_pack_dek` utrwala błąd | grep (0 trafień w `graft.rs`) + sonda odwzorowująca oba zapytania |
| Z8-05 | 🔴 | `CryptProtectData` bez `pOptionalEntropy` dla kluczy S3 — poświadczenia do wszystkich bucketów odzyskiwalne przez dowolny proces użytkownika, bez hasła głównego | czytanie `onboarding.rs:1265-1288` |
| Z8-06 | ⚠️ | `run_metadata_fetch_now` przekazuje `pool = None` → godzinne LIST-y i pobranie migawki poza `cloud_guard` i poza licznikiem egressu; przy niepasującym `vault_id` marker nie przesuwa się i pobieranie powtarza się w kółko | czytanie (l. 1153, 1177 vs sygnatura `Option<&SqlitePool>`) |
| Z8-07 | ⚠️ | `cleanup_stale_uploads` schowane za `OMNIDRIVE_ENABLE_MULTIPART_CLEANUP`, której nikt nie ustawia → porzucone multiparty nigdy nie sprzątane (kontekst [[project-b2-bleeding-root-cause]]) | grep: 1 trafienie w całym repo = sama definicja |
| Z8-08 | ⚠️ | `cleanup_stale_restore_staging` używa `remove_file` zamiast `secure_delete` wbrew obietnicy w komentarzu; filtr `.db` pomija sidecary WAL zostawione przez `init_db` na migawce | czytanie + `secure_fs.rs:95` |
| Z8-09 | ⚠️ | `run_metadata_fetch_now` pisze plaintextową migawkę do `%TEMP%`; dwie z czterech ścieżek sprzątania nie zerują zawartości | czytanie (l. 1090-1126) |
| Z8-10 | ⚠️ | `r_vault_config` czytane przez `unwrap_or(None)` — migawka bez `vault_config` przechodzi cicho, a urządzenie dostaje EVK, którego nie potrafi rozpakować | czytanie + `vault.rs:228` |
| Z8-11 | ⚠️ | `probe_endpoint_reachability` łączy się wyłącznie z `addrs[0]` — pierwszy rekord AAAA w sieci bez IPv6 daje fałszywe „endpoint nieosiągalny" w kreatorze | czytanie |
| Z8-12 | ⚠️ | `graft_restored_metadata_snapshot` kasuje 18 tabel, w tym lokalne `inodes` — kreator nigdzie nie ostrzega, że dołączenie do Skarbca kasuje lokalne metadane | czytanie + brak ostrzeżenia w `wizard.js` |
| Z8-13 | ⚠️ | Klasyfikacja błędów przez `contains()` na tekście komunikatu (`classify_provider_error`, `map_restore_download_error`) — ten sam wzorzec co Z6-10, tu decyduje o komunikacie „złe hasło" vs „brak sieci" | czytanie |
| Z8-14 | ⚠️ | `RuntimePaths::detect()` przy każdej komendzie pipe'a i `AppConfig::from_env()` przy każdym `fetch_chunk` — ta sama klasa co Z6-02 | czytanie |
| Z8-15 | ⚠️ | Test połączenia zostawia obiekt `.omnidrive_probe/<provider>_<ms>`, jeśli `delete_object` padnie; `validate_provider_connection` przyjmuje `secrets` i wykonuje na nich `let _ = secrets;` | czytanie |
| Z8-16 | ⚠️ | Trzy nieszyfrowane kopie `omnidrive.db.bak.<stamp>` obok bazy, tworzone przez worker kopii metadanych; przeżywają „bezpieczne skasowanie bazy" i nie są nigdzie policzone | czytanie `run_local_db_backup_if_due` |
| Z8-17 | ⚠️ | `#![allow(dead_code)]` na całym `onboarding.rs` z komentarzem „reserved for Epic 30" (CLAUDE.md §3) + 6 × `#[allow(dead_code)]` w `disaster_recovery.rs` na typach, które są używane | grep |
| Z8-18 | ⚠️ | `run_pipe_server` kończy się (bez retry) gdy nazwa pipe'a jest zajęta albo gdy nie uda się odtworzyć instancji — para z Z1-02 | czytanie |

---

# 9. API i Web UI — *część 1; dokończenie w rozdziale 9b*

Jedyna warstwa, w której cały system jest sterowalny z zewnątrz procesu. 42 endpointy POST
i ~30 GET na `127.0.0.1:8787`, statyczny dashboard serwowany z tej samej binarki, plus osobna
publiczna powierzchnia linków share.

Modelu zagrożeń są tu dwa i trzeba je rozróżniać, bo dają różne wnioski:

- **Lokalny proces** — dowolny program na koncie użytkownika może wysłać dowolne żądanie i
  **przeczytać odpowiedź**. Nic go nie ogranicza poza logiką w handlerze.
- **Strona WWW w przeglądarce użytkownika** — może *wysłać* proste żądanie na loopback (POST bez
  nietypowych nagłówków nie wymaga preflightu), ale bez CORS **nie przeczyta odpowiedzi**. Liczy
  się więc wyłącznie efekt uboczny. Daemon nie ma żadnej ochrony CSRF ani sprawdzania `Origin`
  poza trasami share.

## 9.1 Mapa warstwy

| Plik | Linie | Rola | Handlery / kontrole auth |
| --- | --- | --- | --- |
| `api/mod.rs` | 469 | Router, `ApiState`, dwa limitery, CORS dla share, serwowanie statyków | 6 / 0 |
| `api_error.rs` (+`api/error.rs`) | 168 + 1 | `ApiError` → JSON + status | — |
| `api/vault.rs` | 1167 | Skarbiec, urządzenia, zaproszenia, rotacja klucza, liczby bezpieczeństwa | 18 / 12 |
| `api/onboarding.rs` | 1238 | Kreator, providerzy, join-existing (czytane w warstwie 8) | 13 / 0 |
| `api/maintenance.rs` | 921 | gc / scrub / repair / reconcile / kopia metadanych | 21 / 11 |
| `api/files.rs` | 766 | Listing, kosz, rewizje, polityki, pin/unpin | 19 / 15 |
| `api/diagnostics.rs` | 738 | Zdrowie, transfery, stan powłoki, multidevice | 12 / **0** |
| `api/sharing.rs` | 546 | Tworzenie i obsługa linków share | 12 / 5 |
| `api/auth.rs` | 495 | Unlock, sesje, zmiana hasła, „Windows Hello" | 10 / 3 |
| `api/recovery.rs` | 425 | Klucze odzyskiwania (24 słowa) | 4 / 2 |
| `api/oauth.rs` | 309 | Google OAuth2 | 2 / 0 |
| `api/stats.rs` | 140 | Statystyki dashboardu | 3 / **0** |
| `api/settings.rs` | 135 | Autostart, restart daemona | 3 / 4 |
| `api/auto_lock.rs` | 117 | Konfiguracja licznika bezczynności | 4 / 3 |
| `api/audit.rs` | 78 | Dziennik audytu | 1 / 1 |
| `static/*` | ~7600 | `index.html` (4044), `legacy.html` (2258), `wizard.js` (698), `share.html` (510), `share-sw.js` (144) | — |

Kolumna „kontrole auth" to liczba wystąpień `require_role` / `require_session` / `extract_session`
w pliku — przybliżenie, ale wystarczające, żeby zobaczyć, gdzie ich nie ma wcale.

## 9.2 Sesja, którą można dostać bez uwierzytelnienia

```rust
async fn get_vault_status(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let unlocked = state.vault_keys.require_key().await.is_ok();
    ...
    if unlocked {
        let session = super::auth::create_session_for_local_device(&state.pool).await.ok();
        Json(serde_json::json!({ "unlocked": true, "session_token": session.map(|s| s.token), ... }))
```

`GET /api/vault/status` **nie ma żadnej kontroli dostępu** i przy odblokowanym Skarbcu **wystawia
świeży token sesji** urządzenia lokalnego — czyli sesję użytkownika, który jest właścicielem
vaulta. Wszystkie `acl::require_role(..., Role::Admin)` i `Role::Owner` w pozostałych 40 plikach
są od tego momentu dekoracją: wystarczy jedno nieautoryzowane GET, żeby dostać token, którym
przechodzi się każdą z nich.

To nie jest przeoczenie ukryte w kodzie — frontend o tym wie i ma na to komentarz
(`index.html:4021`: „`/api/vault/status` mints a fresh session on every call"). Intencją było
ułatwienie dashboardowi startu bez logowania; skutkiem jest brak jakiejkolwiek granicy
uwierzytelnienia po odblokowaniu Skarbca.

Efekt uboczny widać w bazie. Sonda na kopii bazy roboczej:

```
sessions: 131      distinct users: 2
busiest minute holds 22 sessions
expired already: 131   (100 %)
```

22 sesje w jednej minucie to ślad dashboardu odpytującego status w pętli. Żadna nie została
usunięta, bo `cleanup_expired_sessions` nigdy nie jest wołane (Z2-04) — i to właśnie te wiersze
wywalają graft przy dołączaniu do Skarbca (Z8-03). Trzy znaleziska z trzech różnych warstw
spotykają się w jednej tabeli.

## 9.3 Odblokowanie bez hasła i bez zgody

`POST /api/unlock/windows-hello` przyjmuje **puste żądanie**: żadnego ciała, żadnego nagłówka,
żadnej kontroli dostępu. Handler odczytuje hasło z Credential Managera (DPAPI), odblokowuje
Skarbiec, montuje `O:` i zwraca token sesji.

Dwie rzeczy czynią to gorszym, niż wygląda:

1. **Użytkownik nigdy się na to nie godzi.** `post_unlock` woła
   `windows_hello::store_passphrase(...)` przy **każdym** udanym odblokowaniu hasłem
   (`auth.rs:68`, komentarz: *„Silently store passphrase in Windows Credential Manager"*).
   Nie ma ustawienia, przełącznika ani pytania. Kto raz odblokował Skarbiec hasłem, ma je
   w DPAPI na stałe. To zmienia wagę Z7-02: tam chodziło o to, że „Hello" to w istocie DPAPI —
   tu o to, że jest włączone zawsze i bez wiedzy użytkownika.
2. **Jest to osiągalne przez CSRF.** Handler nie ma ekstraktora `Json`, więc żądanie nie musi
   mieć `Content-Type: application/json` — a POST bez nietypowych nagłówków to „simple request",
   który przeglądarka wysyła **bez preflightu**. Dowolna odwiedzona strona może wykonać
   `fetch('http://127.0.0.1:8787/api/unlock/windows-hello', {method:'POST', mode:'no-cors'})`.
   Odpowiedzi nie przeczyta, ale skutek — odblokowany Skarbiec i zamontowany dysk `O:` —
   już się wydarzy.

`POST /api/unlock` jest od tego wolne, bo `Json<UnlockRequest>` wymusza `Content-Type`,
a to wywołuje preflight, który bez CORS nie przejdzie. Ta sama różnica dotyczy
`/api/onboarding/reset` i `/api/onboarding/complete` — oba bez ciała i bez auth (Z9-08).

`POST /api/unlock` ma z kolei inny brak: **żadnego limitera i żadnego audytu nieudanej próby**.
Endpointy recovery i join-existing mają limitery per-IP, główne wejście do Skarbca nie ma nic.
Nieudane odblokowanie nie zostawia nawet wpisu w `audit_logs` — sukces zostawia.

## 9.4 Vault Key na żądanie

`POST /api/vault/add-device` też nie ma kontroli dostępu. Jedyna bariera to warunek, że
`req.user_id` jest członkiem vaulta. Jeśli jest, a Skarbiec jest odblokowany,
`try_auto_wrap_vault_key` owija **Vault Key na klucz publiczny X25519 podany w żądaniu**
i zwraca go w odpowiedzi (`wrapped_vault_key`, `vault_key_generation`,
`wrapping_device_public_key`).

`user_id` nie jest sekretem: `post_vault_join` tworzy je jako `format!("user-{}", req.device_id)`,
a `GET /api/multidevice/status` — również bez auth — oddaje `device_id` i `vault_id`. Dla vaulta,
do którego cokolwiek kiedyś dołączyło przez zaproszenie, łańcuch jest zamknięty bez znajomości
ani hasła, ani tokenu.

Uczciwie: przy odblokowanym Skarbcu Z9-01 daje to samo mniejszym kosztem, więc praktyczna waga
Z9-03 jest o tyle mniejsza. Ale są to dwie niezależne dziury i naprawa jednej nie zamyka drugiej.

## 9.5 Powierzchnia bez uwierzytelnienia — zestawienie

| Endpoint | Metoda | Co oddaje / robi |
| --- | --- | --- |
| `/api/vault/status` | GET | **token sesji** (gdy odblokowany), liczba członków |
| `/api/unlock/windows-hello` | POST | **odblokowuje Skarbiec**, montuje `O:`, token sesji |
| `/api/unlock/hello-available` | GET | czy hasło leży w DPAPI |
| `/api/vault/add-device` | POST | **Vault Key owinięty na podany klucz publiczny** |
| `/api/vault/join` | POST | dołączenie urządzenia na kod zaproszenia (świadomie) |
| `/api/recovery/restore` | POST | zmiana hasła na podstawie 24 słów (świadomie, z limiterem) |
| `/api/recovery/status` | GET | ile kluczy odzyskiwania i kiedy ostatni |
| `/api/multidevice/status` | GET | `vault_id`, `device_id`, lista peerów z `peer_api_base` |
| `/api/diagnostics/*`, `/api/transfers`, `/api/health` | GET | stan workerów, nazwy bucketów, ścieżki |
| `/api/stats/*` | GET | statystyki vaulta |
| `/api/onboarding/status`, `/reset`, `/complete` | GET/POST | stan i reset kreatora |
| `/api/share/*` | GET/POST | publiczna powierzchnia linków (świadomie) |

Trzy pozycje z listy są zaprojektowane jako publiczne i mają to uzasadnione (`join`, `recovery/restore`,
`share`). Reszta wygląda na pominięcie, nie decyzję.

## 9.6 Linki share — dobra kryptografia, nieszczelna księgowość

Konstrukcja jest przemyślana i warto ją odnotować w całości:

- Klucz linku żyje **we fragmencie URL** (`…/share/<id>#<base64url>`), którego przeglądarka
  nie wysyła na serwer. Daemon nigdy go nie widzi.
- Dla każdego packa osobno zapieczętowany DEK (`seal_dek_for_share`), z `pack_id` jako **AAD** —
  sealed DEK jednego packa nie da się podstawić pod inny.
- `revoke_share` i `delete_share` kasują też `share_pack_keys`, więc odwołany link zostaje martwy
  nawet przy późniejszym wycieku bazy.
- `get_share_chunk` oddaje **zaszyfrowane** bajty; deszyfruje przeglądarka odbiorcy.
- Hasło linku: Argon2id z solą, nie SHA.

Trzy rzeczy nie trzymają tego poziomu:

**Licznik pobrań liczy co innego, niż obiecuje.** `increment_shared_link_download_count`
wykonuje się wyłącznie przy pobraniu **ostatniego** chunka. Ponowne pobranie tego chunka
(retry po zerwaniu połączenia, Service Worker, wznowienie) zużywa limit, a pobranie wszystkich
chunków poza ostatnim nie zużywa go wcale. Przy `max_downloads = 1` jedno mrugnięcie sieci
zabija link w połowie transferu.

**Weryfikacja hasła linku nie ma limitera.** Endpointy recovery i join mają, `verify-password`
nie. Parametry Argon2 są tu celowo lekkie (8 MiB, t=2) — komentarz w `sharing.rs` uzasadnia to
tym, że hasło linku jest mniej krytyczne niż hasło Skarbca. Lekkie parametry i brak limitera to
jednak dwie decyzje, które trzeba podejmować razem, a nie osobno.

**Token dostępu jedzie w query stringu** (`?token=…`), mimo że warstwa CORS dopuszcza nagłówek
`x-share-token`, a handler go nie czyta. Query string ląduje w logach pośredników i w `Referer`.

Osobno: `create_share_link` woła `dek_for_pack` — czyli po przeszczepie z Z8-04 zapieczętowałby
w linku **zły DEK**. Skutek: link, który u odbiorcy nie otwiera pliku, bez żadnego komunikatu
po stronie właściciela.

## 9.7 Web UI — trzy skrypty z internetu w aplikacji local-first

```html
<script src="https://cdn.tailwindcss.com?plugins=forms,container-queries"></script>
<script src="https://cdn.jsdelivr.net/npm/jdenticon@3.2.0/dist/jdenticon.min.js" async></script>
```

`index.html`, `legacy.html` i `wizard.html` ładują Tailwind z CDN Cloudflare, a dashboard dodatkowo
jdenticon z jsDelivr. Konsekwencje:

- **Bez internetu UI jest nieużywalne** — a to jest aplikacja, której cała teza brzmi „local-first"
  (`CLAUDE.md`: *„Vanilla JS/HTML/Tailwind serwowane z pamięci/lokalnie przez daemona"*,
  [[project-architecture-corrections]]).
- **Skrypt z CDN dostaje pełne prawa origin dashboardu** — a w tym origin leży token sesji.
  Kompromitacja albo podmiana na CDN to kompromitacja Skarbca.
- Najgorszy pojedynczy przypadek: **jdenticon rysuje identikon liczb bezpieczeństwa**
  (`index.html:3249`, `#safetyIdenticon`). Wizualne porównanie identikonów między urządzeniami
  to kontrola bezpieczeństwa — i wykonuje ją skrypt pobrany z sieci przy każdym otwarciu panelu.

CSP jest ustawione **tylko na `/wizard`** i i tak dopuszcza `cdn.tailwindcss.com` oraz
`'unsafe-inline'`. Strona główna `/` — ta z tokenem sesji i wszystkimi panelami — nie ma CSP
w ogóle. `/legacy` nie ma nawet `no-store`, `X-Frame-Options` ani `Referrer-Policy`, które `/`
ustawia.

Sprawdzony fałszywy alarm: `qrcode.min.js` ma w `wc -l` zero linii, co wyglądało na pusty plik
wysyłany zamiast biblioteki. Plik ma 19 927 bajtów — jest zminifikowany, bez znaku końca linii.

## 9.8 Co jest napisane dobrze

- **`api/recovery.rs`** to najlepiej zabezpieczony moduł w całym projekcie: limiter per-IP
  (30 s między próbami, 3 próby na 5 minut), niezależna blokada stanu „jedna próba odzyskiwania
  na dobę" z furtką w `system_config`, audyt próby / porażki / zablokowania z IP i skróconym
  User-Agentem, dopasowanie mnemoniku przez integralność AES-KW zamiast porównywania hashy,
  świeża sól przy zachowaniu generacji Vault Key (DEK-i nietknięte).
- **`is_allowed_origin`** parsuje host dokładnie, nie prefiksem — komentarz wprost wymienia
  `localhost.evil.com` jako powód. Odrzuca IPv6 i wszystko poza loopbackiem i RFC 1918.
  Zgodne z [[feedback-daemon-cors-loopback-only]].
- **Nie ma endpointu oddającego zawartość pliku po HTTP.** `api/files.rs` to wyłącznie metadane;
  bajty idą przez cfapi i `O:`. Przy tej liczbie dziur w uwierzytelnianiu to jedyny powód,
  dla którego nie da się wyciągnąć plików samym `curl`-em (poza `/api/share/*`, gdzie i tak
  wychodzi szyfrogram).
- **Dwufazowe dołączanie urządzenia** — `join` tworzy urządzenie bez klucza, dopiero
  `accept-device` (Admin) owija Vault Key. Właściwy podział, szkoda że `add-device` go omija.
- `MaintenanceLevel` / `MaintenanceStatus<T>` daje jeden kształt odpowiedzi dla wszystkich
  operacji utrzymaniowych — dashboard ma jedną ścieżkę renderowania zamiast dziesięciu.

## 9.9 Znaleziska

| ID | Waga | Rzecz | Potwierdzone jak |
| --- | --- | --- | --- |
| Z9-01 | 🔴 | `GET /api/vault/status` bez uwierzytelnienia **wystawia token sesji** przy odblokowanym Skarbcu — każdy lokalny proces dostaje uprawnienia właściciela, wszystkie `require_role` przestają cokolwiek znaczyć | czytanie `vault.rs:159-169` + sonda (131 sesji, 22 w jednej minucie, 100 % wygasłych) + komentarz w `index.html:4021` |
| Z9-02 | 🔴 | `POST /api/unlock/windows-hello` bez auth i bez ciała odblokowuje Skarbiec i montuje `O:`; osiągalne przez CSRF z dowolnej strony (simple request, brak preflightu). Hasło trafia do DPAPI **przy każdym** udanym odblokowaniu, bez zgody i bez ustawienia | czytanie `auth.rs:68` i `:411-419` |
| Z9-03 | 🔴 | `POST /api/vault/add-device` bez auth owija Vault Key na klucz publiczny z żądania i zwraca go; `user_id` jest odgadywalne (`user-<device_id>`), a `device_id` oddaje nieautoryzowane `/api/multidevice/status` | czytanie `vault.rs:585-683`, `:280` |
| Z9-04 | 🔴 | `POST /api/unlock` bez limitera i bez audytu nieudanych prób — jedyne zabezpieczenie to koszt Argon2; recovery i join-existing limitery mają | czytanie + `mod.rs:45-144` |
| Z9-05 | 🔴 | Cały Web UI ładuje Tailwind i jdenticon z publicznych CDN — bez internetu UI nie działa, a skrypt z CDN ma pełne prawa origin z tokenem sesji; identikon liczb bezpieczeństwa rysuje kod pobrany z sieci. CSP tylko na `/wizard`, i tak z `'unsafe-inline'` | grep `<script src>` + `index.html:3249` + `CLAUDE.md` |
| Z9-06 | ⚠️ | `api/diagnostics.rs` — 12 handlerów, zero kontroli dostępu; `/api/multidevice/status` oddaje `vault_id`, `device_id` i listę peerów (komplet materiału do Z8-02), `/api/transfers` nazwy bucketów | audyt pokrycia + czytanie |
| Z9-07 | ⚠️ | `api/stats.rs` — 3 handlery, zero kontroli dostępu | audyt pokrycia |
| Z9-08 | ⚠️ | `POST /api/onboarding/reset` i `/api/onboarding/complete` bez auth i bez ciała → wykonalne przez CSRF; reset cofa kreator i wyłącza tryb chmurowy | czytanie sygnatur |
| Z9-09 | ⚠️ | `max_downloads` zlicza wyłącznie pobrania **ostatniego** chunka: retry sieciowy zjada limit, a pobranie reszty pliku go nie rusza | czytanie `sharing.rs:502-506` |
| Z9-10 | ⚠️ | `verify-password` dla linków share bez limitera, przy celowo lekkim Argon2id (8 MiB, t=2) | czytanie + `sharing.rs:85` |
| Z9-11 | ⚠️ | Token dostępu do share przekazywany w query stringu, choć CORS dopuszcza nagłówek `x-share-token`, którego handler nie czyta | czytanie |
| Z9-12 | ⚠️ | `post_vault_join`: `user_id` = `user-<device_id>` sterowane przez klienta, a błędy `create_user`/`create_device`/`add_vault_member` tylko `warn!` — kod zaproszenia zostaje skonsumowany i zwracany jest sukces | czytanie `vault.rs:274-331` |
| Z9-13 | ⚠️ | `POST /api/devices/{id}/verify` — oznaczenie liczb bezpieczeństwa jako zweryfikowanych wymaga tylko roli `Viewer` | czytanie `vault.rs:1148` |
| Z9-14 | ⚠️ | `ApiError::Internal` odsyła surowy komunikat (pełny tekst błędu SQLite, ścieżki) w ciele odpowiedzi | czytanie `api_error.rs:103-110` |
| Z9-15 | ⚠️ | `/legacy` serwowane bez `no-store`, `X-Frame-Options` i `Referrer-Policy`, które `/` ustawia | czytanie `mod.rs:348-361` |
| Z9-16 | ⚠️ | Oba limitery trzymają wpis per IP w `DashMap` i czyszczą go tylko przy sukcesie; `JoinRateLimiter` karze maksymalnie 30 s | czytanie `mod.rs:39-144` |
| Z9-17 | ⚠️ | `POST /api/recovery/restore` nie unieważnia istniejących sesji ani nie aktualizuje poświadczenia DPAPI — po odzyskaniu „Windows Hello" próbuje odblokować starym hasłem i cicho pada | czytanie `recovery.rs:358-395` |
| Z9-18 | ⚠️ | `share_base_url` buduje link z nagłówka `Host` żądania | czytanie `sharing.rs:537-543` |
| Z9-19 | ⚠️ | `POST /api/maintenance/repair-shell` — jedyna operacja zmieniająca stan w `maintenance.rs` bez kontroli roli; bez ciała, więc też przez CSRF. Robi `subst O:` i zapisy do `HKCU` | audyt pokrycia + czytanie `maintenance.rs:532` |

## 9.10 Zakres części pierwszej (domknięte w rozdziale 9b)

Przeczytane w całości: `api/mod.rs`, `api_error.rs`, `api/error.rs`, `api/auth.rs`,
`api/recovery.rs`, `api/sharing.rs`; z `api/vault.rs` — routing, `get_vault_status`,
`post_vault_join`, `post_add_device`, `try_auto_wrap_vault_key`, mapa ról pozostałych handlerów;
z `api/files.rs` — routing, kosz, `delete_file` i audyt ról; `api/diagnostics.rs` i `api/stats.rs`
na poziomie tras i pokrycia kontrolami dostępu; `api/onboarding.rs` — ścieżka join-existing
(w warstwie 8).

**Przeczytane w części drugiej (rozdział 9b), w tej kolejności:**

```
api/maintenance.rs (921)   — 10 handlerów bez widocznej kontroli roli, w tym gc i repair-shell
api/vault.rs (reszta)      — accept-device, rotate-key, revoke-device, remove-member, safety-numbers
api/oauth.rs (309)         — obieg tokenów Google, PKCE, oauth_states
api/files.rs (reszta)      — rewizje, restore, conflict copy, polityki filesystem
api/onboarding.rs (reszta) — setup-identity, setup-provider, status
settings.rs / auto_lock.rs / audit.rs / stats.rs (470 razem)
static/index.html (4044)   — panele, obieg tokenu, escapowanie w szablonach
static/share.html (510) + share-sw.js (144) — deszyfrowanie po stronie odbiorcy
static/legacy.html (2258)  — czy w ogóle jest jeszcze osiągalne
```

Jedno pytanie z tej listy zostało zamknięte od razu, bo było tanie: w `api/maintenance.rs`
wszystkie dziesięć handlerów bez kontroli roli to odczyty statusu — **z jednym wyjątkiem,
`post_repair_shell`** (Z9-19). `gc-orphans`, `repair-now`, `scrub-now`, `reconcile-now`,
`backup-now`, `fetch-now` i obie operacje na kolejce ingest mają `require_role(Admin)`.
Reszta pliku (treść handlerów, nie ich autoryzacja) zostaje do przeczytania.

---

# 9b. API i Web UI — dokończenie (`maintenance`, `vault`, `oauth`, `files`, statyki)

Domknięcie listy z §9.10. Wnioski z części pierwszej się nie zmieniają — dochodzą cztery
nowe 🔴, z czego jeden funkcjonalny: **tryb LAN Share nie może działać**.

## 9b.1 Rotacja hasła dwiema drogami o różnych progach

Dwa endpointy robią to samo — podmieniają hasło Skarbca — i mają różne wymagania:

| Endpoint | Wymaga sesji | Wymaga **starego hasła** |
| --- | --- | --- |
| `POST /api/change-password` | tak | **tak** (`verify_passphrase`) |
| `POST /api/vault/rotate-key` | tak, rola Admin | **nie** |

`post_rotate_key` sprawdza tylko, że nowe hasło nie jest puste, i woła `rotate_vault_key`.
Kto ma token sesji z rolą Admin, ustawia dowolne nowe hasło, nie znając obecnego. W połączeniu
z Z9-01 (token bez uwierzytelnienia z `GET /api/vault/status`) daje to pełne przejęcie Skarbca
przez lokalny proces: pobierz token → ustaw własne hasło → hasło właściciela przestaje działać.
Słabsza z dwóch bram decyduje o bezpieczeństwie całości (Z9-21).

## 9b.2 Odwołanie urządzenia, które może nic nie odwołać

`post_revoke_device` i `post_remove_member` wykonują dwa kroki: ustawiają `revoked_at`, a potem
rotują Vault Key, żeby klucz, który odwołane urządzenie **już ma u siebie**, przestał otwierać dane.
Drugi krok jest opcjonalny:

```rust
let rotation = match state.vault_keys.rotate_for_revocation(&state.pool).await {
    Ok(r) => Some(r),
    Err(e) => { warn!("VK rotation after revocation failed: {e}"); None }
};
```

Przy nieudanej rotacji odpowiedź nadal ma `"status": "revoked"` — różnicę widać wyłącznie w polu
`vk_rotation: null`, którego UI nie musi czytać, i w `warn!` w logu. Odwołane urządzenie zachowuje
działający `wrapped_vault_key` i czyta wszystkie dotychczasowe dane. Kontrola bezpieczeństwa
melduje sukces, wykonawszy połowę pracy (Z9-22).

## 9b.3 Konfiguracja dostawcy do przestawienia bez logowania

`POST /api/onboarding/setup-provider` nie ma kontroli dostępu, a komentarz w samym handlerze
mówi wprost, że endpoint jest używany **także po zakończeniu onboardingu**:

> *„If onboarding is already COMPLETED, this endpoint is being used to update or re-validate
> provider credentials from the dashboard"*

Wynika z tego, że dowolny lokalny proces może na działającym Skarbcu podmienić `endpoint`,
`bucket` i klucze dostępowe dowolnego z trzech dostawców. Skutki:

- kolejne packi lecą do bucketa atakującego (szyfrogram, więc nie jest to wyciek plaintekstu,
  ale jest to trwałe rozbicie redundancji i kontrola nad tym, co wróci przy naprawie),
- handler od razu wykonuje **test połączenia** z podanym endpointem, czyli daemon łączy się
  tam, gdzie każe mu nieuwierzytelnione żądanie (w parze z Z4-13: prefiks `http://`
  cicho degraduje transport),
- wpis `provider_secrets` zostaje nadpisany, więc oryginalne poświadczenia znikają.

To najcięższy z endpointów kreatora, bo w przeciwieństwie do `reset`/`complete` nie tylko
przestawia flagi, ale przekierowuje strumień danych (Z9-20).

## 9b.4 `add-device` omija własne zabezpieczenia

`post_accept_device` (rola Admin) sprawdza po kolei: czy urządzenie nie ma już owiniętego klucza,
czy nie jest odwołane, czy ma `enrolled_at`, czy klucz publiczny ma 32 bajty i nie jest punktem
niskiego rzędu. To jest dobrze zrobiona bramka.

`try_auto_wrap_vault_key`, wołane z **nieuwierzytelnionego** `post_add_device`, nie sprawdza
żadnego z tych warunków poza długością klucza — i owija Vault Key od razu, zapisując w audycie
`auto_accept_device` z uzasadnieniem `existing_member_auto_accept`. Cały dwufazowy projekt
(dołączenie → akceptacja przez Admina) da się ominąć wywołaniem drugiej fazy bez pierwszej
(Z9-28).

**Sprawdzony fałszywy alarm.** Wyglądało na to, że brak kontroli punktów niskiego rzędu w tej
ścieżce pozwala podstawić klucz publiczny dający przewidywalny (zerowy) sekret ECDH i odzyskać
Vault Key bez klucza prywatnego. Warstwa krypto to jednak łapie: `wrap_vault_key_for_device`
woła `validate_x25519_pubkey`, a potem jawnie odrzuca wynik ECDH równy 32 bajtom zera —
a to jest dokładnie ten warunek, który dają wszystkie punkty niskiego rzędu X25519.
Zabezpieczenie jest w jedynym miejscu, w którym musi być.

## 9b.5 Tryb LAN Share nie może zadziałać

Komentarz przy `share_cors_layer` opisuje dwa tryby udostępniania, z których **Tryb A (LAN Share)**
polega na tym, że deszyfrator jest serwowany przez ten sam daemon, do którego odbiorca sięga
po adresie LAN. `share_base_url` wprost to wspiera: `OMNIDRIVE_SHARE_HOST=http://192.168.1.10:8787`
z komentarzem *„for LAN sharing"*, a w braku nadpisania bierze nagłówek `Host` żądania.

`share.html` deszyfruje przez `crypto.subtle`, a pliki powyżej 50 MiB strumieniuje przez
Service Workera. **Oba API są dostępne wyłącznie w bezpiecznym kontekście** — HTTPS albo
`localhost` / `127.0.0.1`. Link LAN to zwykły `http://` na adresie IP, czyli kontekst
niebezpieczny: `window.crypto.subtle` jest `undefined`, `navigator.serviceWorker` również.

Strona to wykrywa i pokazuje:

```
'Przegladarka nie wspiera WebCrypto. Uzyj nowoczesnej przegladarki.'
```

Diagnoza jest błędna — przeglądarka jest nowoczesna i WebCrypto wspiera; brakuje bezpiecznego
kontekstu. Odbiorca dostaje komunikat, który wskazuje na jego stronę, choć przyczyna jest
po stronie architektury linku. Tryb A działa tylko wtedy, gdy odbiorcą jest ta sama maszyna
(`127.0.0.1`), co przeczy sensowi „LAN Share" (Z9-23).

## 9b.6 Sesja bez członkostwa — druga klasa tokenów

`acl::require_role` odrzuca użytkownika, który nie ma wiersza w `vault_members` (403). Ale część
endpointów nie używa ról, tylko `extract_session` / `require_session`, które sprawdzają wyłącznie
ważność tokenu:

| Endpoint | Bramka |
| --- | --- |
| `/api/settings/paths` | `extract_session` |
| `/api/settings/autostart` | `extract_session` |
| `/api/settings/restart-daemon` | `extract_session` |
| `/api/auto-lock/timeout`, `/status`, `/touch` | `require_session` |

Tokeny tej klasy potrafi wydać `GET /api/auth/google/callback`: dowolne konto Google, które
przejdzie przepływ OAuth, dostaje wiersz w `users`, wiersz w `user_sessions` i token — bez
jakiegokolwiek sprawdzenia, czy to konto ma cokolwiek wspólnego z tym Skarbcem. Do danych
nie sięgnie (`require_role` odmówi), ale **wyłączy autostart i ubije daemona** (Z9-24).

Przy okazji: `post_restart_daemon` mimo nazwy jedynie wysyła sygnał na `daemon_shutdown_tx`,
czyli wywołuje graceful shutdown. Czy po nim nastąpi restart, zależy od czegoś poza tym procesem
(tray, usługa) — w samym daemonie nie ma nic, co by go podniosło (Z9-31).

Trzecia rzecz z tego samego pliku: gdy Skarbiec jest zablokowany, `google_refresh_token`
zostaje w `users` **w plaintekście** do najbliższego odblokowania. Komentarz to opisuje jako
świadomy kompromis, ale reguła Zero-Knowledge z `CLAUDE.md` mówi o tokenach OAuth wprost (Z9-27).

## 9b.7 Drobniejsze rzeczy z `maintenance.rs` i `files.rs`

- `POST /api/metadata-backup/snapshot-local` przyjmuje **dowolną ścieżkę wyjściową** z żądania
  (rola Admin). `normalize_encrypted_output_path` wymusza końcówkę `.enc`, więc nie da się
  nadpisać binarki ani skryptu — ale można zapisać plik w dowolnym katalogu dostępnym dla
  daemona, a `create_encrypted_metadata_snapshot` tworzy po drodze **plaintextowy `*.tmp.db`
  w tym samym katalogu**, kasowany dopiero po zaszyfrowaniu (Z9-25, wątek z §8.10).
- `GET /api/ingest` nie ma kontroli dostępu i zwraca `file_path` każdego zadania — czyli
  **pełne ścieżki plików użytkownika** na dysku lokalnym (Z9-26).
- `GET /api/maintenance/retry-storms` i `/scrub-errors` też bez kontroli: `pack_id`
  i nazwy dostawców.
- `normalize_filesystem_api_path` z `files.rs` ma bliźniaka w `pipe_server::normalize_path`;
  komentarz w `pipe_server` mówi wprost *„Mirror of `normalize_filesystem_api_path`"*. Dwie kopie
  tej samej normalizacji, z których jedna stoi za nieuwierzytelnionym pipe'em (Z9-29).
- `get_my_wrapped_key` wymaga tylko roli `Viewer` i oddaje owinięty Vault Key **dowolnego**
  urządzenia podanego w `device_id`, nie tylko własnego. Bez klucza prywatnego tego urządzenia
  jest to bezużyteczne, ale to niepotrzebnie szeroka odpowiedź (Z9-30).

Ścieżki w `files.rs` są rozwiązywane przez `db::resolve_path` na logicznych ścieżkach z bazy,
więc `..` nie wyprowadza poza Skarbiec — po prostu się nie rozwiązuje. Tu nie ma traversalu.

## 9b.8 Co jest napisane dobrze (ciąg dalszy)

- **`share.html` reimplementuje HKDF-Expand-bez-Extract ręcznie przez HMAC-SHA256**, bo WebCrypto
  robi extract+expand, a Rust `derive_subkey` tylko expand. Nad kodem stoi komentarz tłumaczący
  dokładnie tę różnicę. To jest ta sama pułapka, którą opisuje
  [[project-z4-01-dek-per-pack]] — tutaj rozpoznana i udokumentowana w miejscu, w którym boli.
- **Obsługa punktów niskiego rzędu X25519** — `validate_x25519_pubkey` plus odrzucenie zerowego
  sekretu ECDH, symetrycznie w `wrap_` i `unwrap_vault_key_from_device`.
- **PKCE** — 96 bajtów losowości, `S256`, `state` kasowany przy odbiorze
  (`get_and_delete_oauth_state`), trzy testy jednostkowe pilnujące długości i zgodności
  challenge↔verifier.
- **`escapeHtml` jest w dashboardzie używane konsekwentnie** — na 45 miejsc z `innerHTML`
  tylko jedno wstawia niezescapowany tekst, i to komunikat błędu (`index.html:3312`).
- **`api/audit.rs`** — jedyny moduł, w którym limit z query jest twardo klamrowany
  (`clamp(1, 500)`), a wynik filtrowany po `vault_id` wywołującego.

## 9b.9 Korekta do Z2-04

Z2-04 twierdzi, że `cleanup_expired_sessions` **i** `delete_expired_oauth_states` nie są nigdy
wołane. Połowa tego jest nieprawdziwa: `delete_expired_oauth_states` jest wołane
w `api/oauth.rs:39`, na starcie każdego przepływu logowania Google. Bez wywołań pozostaje
wyłącznie `cleanup_expired_sessions` — i to ono odpowiada za 131 martwych wierszy z §9.2
oraz za `Z8-03`.

## 9b.10 Znaleziska

| ID | Waga | Rzecz | Potwierdzone jak |
| --- | --- | --- | --- |
| Z9-20 | 🔴 | `POST /api/onboarding/setup-provider` bez uwierzytelnienia nadpisuje endpoint, bucket i klucze dostawcy **także po zakończeniu onboardingu** (komentarz w handlerze to potwierdza) → kolejne packi lecą do cudzego bucketa, a daemon łączy się z endpointem podanym w żądaniu | czytanie `onboarding.rs:230-359` |
| Z9-21 | 🔴 | `POST /api/vault/rotate-key` zmienia hasło Skarbca **bez weryfikacji starego**, w przeciwieństwie do `/api/change-password`; z tokenem z Z9-01 to pełne przejęcie Skarbca | czytanie `vault.rs:1045-1065` vs `auth.rs:309-321` |
| Z9-22 | 🔴 | Odwołanie urządzenia i usunięcie członka tolerują nieudaną rotację Vault Key (`warn!`), a odpowiedź nadal mówi `"revoked"` — odwołane urządzenie zachowuje działający `wrapped_vault_key` | czytanie `vault.rs:805-823`, `:897-914` |
| Z9-23 | 🔴 | Tryb A (LAN Share) nie może działać: `crypto.subtle` i Service Worker wymagają bezpiecznego kontekstu, a link LAN to `http://` na adresie IP; komunikat błędu obwinia przeglądarkę zamiast wskazać przyczynę | czytanie `share.html:274-277`, `:181`, `:438` + `sharing.rs:518-543` |
| Z9-24 | ⚠️ | Callback Google tworzy `users` + `user_sessions` dla **dowolnego** konta bez sprawdzenia członkostwa; `require_role` odmówi, ale endpointy na `extract_session`/`require_session` (autostart, restart-daemon, auto-lock, settings/paths) przepuszczą obcą sesję | czytanie `oauth.rs:228-236` + `acl.rs:78-82` + `settings.rs` |
| Z9-25 | ⚠️ | `POST /api/metadata-backup/snapshot-local` przyjmuje dowolną ścieżkę wyjściową; rozszerzenie wymuszone na `.enc`, ale plaintextowy `*.tmp.db` powstaje po drodze w katalogu wskazanym przez wywołującego | czytanie `maintenance.rs:603-630` + `disaster_recovery.rs:537` |
| Z9-26 | ⚠️ | `GET /api/ingest` bez kontroli dostępu zwraca `file_path` każdego zadania — pełne ścieżki plików użytkownika | czytanie `maintenance.rs:764-785` |
| Z9-27 | ⚠️ | Przy zablokowanym Skarbcu `google_refresh_token` zostaje w `users` w plaintekście do najbliższego odblokowania — świadome, ale wbrew regule Zero-Knowledge z `CLAUDE.md` | czytanie `oauth.rs:212-226` |
| Z9-28 | ⚠️ | `try_auto_wrap_vault_key` (z nieuwierzytelnionego `add-device`) pomija komplet kontroli, które robi `post_accept_device`: `enrolled_at`, `revoked_at`, jawne odrzucenie klucza zerowego | czytanie `vault.rs:376-412` vs `:685-704` |
| Z9-29 | ⚠️ | `normalize_filesystem_api_path` zduplikowane jako `pipe_server::normalize_path`; jedna z kopii obsługuje nieuwierzytelniony pipe (Z8-01) | czytanie obu + komentarz `pipe_server.rs:319` |
| Z9-30 | ⚠️ | `get_my_wrapped_key` z rolą `Viewer` oddaje owinięty Vault Key dowolnego urządzenia, nie tylko własnego | czytanie `vault.rs:471-524` |
| Z9-31 | ⚠️ | `POST /api/settings/restart-daemon` mimo nazwy tylko sygnalizuje graceful shutdown; nic w daemonie nie podnosi go z powrotem | czytanie `settings.rs:77-90` |
| Z2-04 | ✅ | **Korekta:** `delete_expired_oauth_states` **jest** wołane (`oauth.rs:39`). Bez wywołań pozostaje wyłącznie `cleanup_expired_sessions` | grep |

---

# 10. Satelity i testy

Wszystko, co nie jest daemonem: trzy klienty (CLI, tray, rozszerzenie powłoki), dwa artefakty
uboczne (`angelctl`, `cfapi_repro`) i pakiet testów e2e. Razem 5281 linii — mniej niż każda
z dwóch poprzednich warstw, ale to tutaj widać, **czy reszta systemu jest w ogóle używalna**.

## 10.1 Mapa warstwy

| Plik / crate | Linie | Rola | Instalowany? |
| --- | --- | --- | --- |
| `omnidrive-cli/src/main.rs` | 690 | `omnidrive` — 12 komend przeciw API daemona | tak (`omnidrive.exe`) |
| `omnidrive-shell-ext/src/lib.rs` | 654 | Menu kontekstowe Eksploratora (COM, Named Pipe) | **nie** |
| `omnidrive-tray/src/main.rs` | 409 | Ikona w zasobniku, sondowanie daemona co 3 s | tak |
| `angeld/src/bin/cfapi_repro.rs` | 153 | Izolowana reprodukcja `CfRegisterSyncRoot` | nie (ale budowany) |
| `angelctl/src/main.rs` | 3 | `println!("Hello, world!")` | nie (ale budowany) |
| `angeld/tests/*.rs` | 3372 | 9 plików, **19 funkcji testowych** | — |

## 10.2 CLI, który nie potrafi się uwierzytelnić

W całym `omnidrive-cli` nie ma ani jednego wystąpienia słów `Authorization`, `Bearer`, `token`
czy `session` — grep daje zero trafień. Każde żądanie leci bez nagłówka. Zestawienie komend
z bramkami ustalonymi w warstwie 9:

| Komenda | Endpoint | Bramka | Wynik |
| --- | --- | --- | --- |
| `status` | `/api/quota`, `/api/health/vault` | brak | ✅ działa |
| `cache status` | `/api/cache/status` | brak | ✅ |
| `maintenance status` | `/api/maintenance/status` | brak | ✅ |
| `maintenance errors` | `/api/maintenance/scrub-errors` | brak | ✅ |
| `recovery status` | `/api/metadata-backup/status` | brak | ✅ |
| `recovery restore` | — (lokalnie, patrz §10.3) | — | ✅ |
| `service register` / `unregister` | — (lokalnie) | — | ✅ |
| `ls` | `/api/files` | `Viewer` | ❌ 403 |
| `history` | `/api/files/{id}/revisions` | `Viewer` | ❌ 403 |
| `restore` | `.../revisions/{id}/restore` | `Member` | ❌ 403 |
| `pin` / `unpin` | `/api/files/{id}/pin` \| `/unpin` | `Member` | ❌ 403 |
| `recovery backup-now` | `/api/metadata-backup/backup-now` | `Admin` | ❌ 403 |

**Sześć z dwunastu komend nie może zadziałać** — i to dokładnie te, które robią cokolwiek
z plikami. Wzorzec jest identyczny z Z7-01: klient napisany przeciw wcześniejszej wersji API,
którego nie przeprowadzono przez wprowadzenie ról (Z10-01). Komunikat, jaki dostaje użytkownik,
brzmi `request to http://127.0.0.1:8787/api/files failed with status 403 Forbidden` — czyli
przynajmniej nie milczy, w odróżnieniu od menu kontekstowego z Z7-01.

Ironiczna konsekwencja Z9-01: żeby to naprawić, wystarczyłoby jedno `GET /api/vault/status`
przed każdą komendą — token przychodzi bez uwierzytelnienia. Nie jest to jednak naprawa,
którą warto robić; naprawą jest zamknięcie Z9-01 i danie CLI prawdziwego logowania.

## 10.3 `omnidrive recovery restore` — nadpisanie żywej bazy bez pytania

Jedyna komenda CLI, która nie rozmawia z daemonem, tylko sama sięga do chmury:

```rust
let output_db_path = /* OMNIDRIVE_DB_PATH | OMNIDRIVE_DB_URL | RuntimePaths::detect().db_file_path */;
eprint!("Master Password: ");
let passphrase = rpassword::read_password()?;
let provider_manager = MetadataBackupProviderManager::from_env().await?;
restore_metadata_from_cloud(&provider_manager, &passphrase, &output_db_path, None).await?;
```

Domyślną ścieżką wyjściową jest **żywa baza metadanych**. `restore_metadata_from_cloud` kończy
się `fs::write(output_db_path, plaintext)`, czyli surowym nadpisaniem — bez grafta, bez kopii
zapasowej, bez potwierdzenia i bez sprawdzenia, czy daemon aktualnie tej bazy nie trzyma
otwartej. Ścieżka przez API (`join-existing`) robi to zupełnie inaczej: pobiera do pliku
przejściowego i przeszczepia transakcyjnie. Tutaj jedno wywołanie zamienia lokalne metadane
na migawkę z chmury (Z10-02).

Dwa łagodzące szczegóły, oba sprawdzone: `write_plaintext_snapshot_if_valid` odrzuca migawkę
bez wiersza `vault_state`, więc nie nadpisze bazy śmieciem; a `MetadataBackupProviderManager::from_env()`
wymaga **kompletu trzech dostawców w zmiennych środowiskowych** (Z4-10) — na maszynie
z instalatora poświadczenia leżą w bazie zapieczętowane DPAPI, nie w `.env`, więc komenda
kończy się `failed to initialize recovery providers` (Z10-04). Innymi słowy: na Dellu nie
zadziała wcale, a na Lenovo z pełnym `.env` zadziała aż za dobrze.

Hasło jest czytane przez `rpassword` (nie trafia do historii powłoki), ale trzymane jako zwykły
`String` i nigdy nie zerowane. Wywołanie idzie z `pool = None`, więc `cloud_guard` nie widzi
tego ruchu (ten sam brak co Z8-06).

## 10.4 Tray zabija daemona zamiast go poprosić

Menu zasobnika ma „Zatrzymaj demona" i „Restartuj demona". Oba prowadzą do:

```rust
Command::new("taskkill").args(["/F", "/IM", "angeld.exe"])
```

`/F` to zabicie bez szans na sprzątanie. Warstwa 7 ustaliła, że sekwencja blokady Skarbca
jest odpalana przez `tokio::spawn` i ginie razem z procesem (Z7-05) — a więc po kliknięciu
„Zatrzymaj demona" **sync root zostaje zarejestrowany, `O:` zostaje podstawiony, a pliki
zhydratowane zostają na dysku w plaintekście**. Daemon ma na to właściwą drogę:
`POST /api/settings/restart-daemon` sygnalizuje graceful shutdown i `main.rs` przechodzi
normalną ścieżką sprzątania. Tray z niej nie korzysta (Z10-03).

To samo robi instalator przy deinstalacji:

```
[UninstallRun]
Filename: "taskkill"; Parameters: "/F /IM {#TrayExeName}"
Filename: "taskkill"; Parameters: "/F /IM {#AppExeName}"
```

Czyli odinstalowanie OmniDrive nigdy nie przeprowadza uporządkowanego demontażu Skarbca.

Dwie rzeczy poboczne z tego samego kodu: `taskkill /F /IM angeld.exe` ubija **wszystkie**
instancje, w tym uruchomioną z `target/release` na dev-boxie (Z10-13), a `restart_daemon`
to `kill` → `sleep(500 ms)` → `spawn`, bez sprawdzenia, czy port 8787 zdążył się zwolnić
i czy nowy proces w ogóle wstał (Z10-12).

## 10.5 Tray jako generator sesji

`poll_daemon_state` odpytuje `/api/vault/status` co `POLL_INTERVAL = 3 s`. Ten endpoint
przy odblokowanym Skarbcu **wystawia nowy token sesji przy każdym wywołaniu** (Z9-01),
a `cleanup_expired_sessions` nikt nie woła (Z2-04). Tray działający przy odblokowanym Skarbcu
produkuje więc 20 wierszy `user_sessions` na minutę, których nikt nie usuwa (Z10-05).

Sonda na kopii bazy roboczej pokazuje, że sesje faktycznie powstają w gęstych seriach —
16 par w tej samej sekundzie, 10 par w odstępie sekundy — czyli że **coś** odpytuje ten
endpoint w pętli. Nie da się z samych znaczników czasu orzec, czy to tray, dashboard, czy
testy; kod tray-a daje tu twardy fakt (3 s), a rozkład odstępów tylko go nie przeczy.
Wystarczy to jednak, żeby domknąć łańcuch z warstwy 8: im dłużej działa zasobnik, tym pewniej
`Z8-03` zablokuje dołączenie do Skarbca.

## 10.6 Rozszerzenie powłoki, którego nikt nie instaluje

`omnidrive_shell_ext.dll` jest budowany przez `cargo build --release --workspace` i **leży
w `dist/installer/payload/`** — ale sekcja `[Files]` w `installer/omnidrive.iss` wymienia
tylko `angeld.exe`, tray, `omnidrive.exe`, `static\*`, `icons\*` i launcher autostartu.
Grep po `shell_ext`, `regsvr` i `DllRegister` w całym `.iss` daje zero trafień. DLL nie jest
ani kopiowany, ani rejestrowany (Z10-06). Pipeline budowania sugeruje, że komponent jest
dostarczany — instalator go pomija. To wyjaśnia, dlaczego na Dellu działa wyłącznie rejestrowy
wariant menu z Z7-01 (ten zwracający 401).

Gdyby ktoś zarejestrował DLL ręcznie (`regsvr32`, wymaga administratora — wpisy idą do `HKCR`
i `HKLM\...\Shell Extensions\Approved`), dostałby dwie rzeczy warte odnotowania:

**Twardo zapisane `O:\`.** `Initialize` przerywa z `E_FAIL`, jeśli ścieżka nie zaczyna się
od `O:\` lub `o:\`. Tymczasem daemon montuje przez `select_mount_drive_letter`, które przy
zajętej literze preferowanej **bierze pierwszą wolną z zakresu `D..Z`**. Skarbiec pod `P:`
oznacza menu kontekstowe, które nigdy się nie pokazuje (Z10-09).

**Log bez rotacji ze ścieżkami plików Skarbca.** `log_to_file` dopisuje do
`%TEMP%\omnidrive_shell_ext.log`, nigdy nie przycina, a `Initialize` zapisuje tam
`Initialize — target: {path}` dla każdego kliknięcia prawym przyciskiem. W Skarbcu
zero-knowledge same nazwy plików są danymi wrażliwymi, a `%TEMP%` nie jest chroniony
(Z10-10, klasa Z1-01).

**Sprawdzony fałszywy alarm.** `register_server` rejestruje handler pod
`HKCR\*\shellex\ContextMenuHandlers` i `HKCR\Directory\...`, czyli formalnie **dla każdego
pliku i katalogu w systemie**. Wyglądało to na menu OmniDrive doklejone do wszystkiego.
Nie jest — filtr `O:\` w `Initialize` odcina resztę systemu, zanim menu zostanie zbudowane.
Rejestracja jest szeroka, ale zachowanie wąskie.

## 10.7 Dwa artefakty, które nie powinny się budować

- **`angelctl`** to trzy linie: `fn main() { println!("Hello, world!"); }`. Mimo to jest
  członkiem workspace (`Cargo.toml:2`), buduje się przy każdym `--release --workspace`,
  `angelctl.exe` leży w payloadzie instalatora, a `CLAUDE.md` §3 każe podbijać w nim wersję
  razem z resztą crate'ów. Utrzymywany narzut na pustym pliku (Z10-07).
- **`cfapi_repro`** to narzędzie diagnostyczne (izolowana reprodukcja `CfRegisterSyncRoot`).
  Nie ma `required-features`, więc `cargo build --release --workspace` produkuje
  `target/release/cfapi_repro.exe` obok binarek produkcyjnych (Z10-08, klasa Z1-06).

Potwierdzenie: `ls target/release/*.exe` → `angelctl.exe`, `angeld.exe`, `cfapi_repro.exe`,
`omnidrive.exe`, `omnidrive-tray.exe`.

## 10.8 Testy — 3372 linie, 19 asercji

Rozkład jest nietypowy: 9 plików, ale tylko **19 funkcji testowych**. Reszta objętości to
rusztowanie — mock S3 (`e2e_reconciliation.rs`: `mock_put_object`, `mock_head_object`,
`mock_get_object`, `mock_delete_object`), ręczny klient HTTP na gniazdach, obsługa sync roota,
`spawn_heartbeat`, `wait_for_*`. To jest inwestycja we właściwym miejscu: testy startują
**prawdziwego daemona** (`CARGO_BIN_EXE_angeld`) i gadają z nim po HTTP, zamiast wołać funkcje
wewnętrzne.

Czego mimo to nie łapią — w zestawieniu z rejestrem znalezisk:

`e2e_auto_lock.rs` ma trzy testy negatywne uwierzytelnienia
(`e2e_status_endpoint_rejects_unauthenticated`, `e2e_unauthenticated_health_does_not_touch`,
`auto_lock_timeout_endpoint_rejects_unauthenticated`). Autorzy **umieją i chcą** pisać takie
testy — ale napisali je tylko dla modułu, nad którym akurat pracowali. Nie ma testu, który
przechodzi listę endpointów zmieniających stan i sprawdza, że każdy odrzuca żądanie bez tokenu.
Dokładnie tą szczeliną przeszły Z9-01, Z9-02, Z9-03, Z9-20 i Z9-21 (Z10-14).

**Sprawdzony fałszywy alarm.** Podejrzewałem, że pakiet testów pobiera token z
`/api/vault/status` — co uczyniłoby Z9-01 zachowaniem utrwalonym w testach. Nie: wszystkie
cztery harnessy logują się przez `POST /api/unlock` i czytają `session_token` z odpowiedzi,
czyli legalną drogą. Z9-01 nie ma w testach żadnego oparcia — ani pozytywnego, ani negatywnego.

## 10.9 Skąd się biorą porzucone mapowania `subst`

[[feedback-e2e-subst-cleanup]] odnotowuje, że po `cargo test` zostają mapowania na literach
E, F, H, I, J, K, L. Kod tłumaczy mechanizm w całości:

1. `e2e_recovery.rs:122` i `e2e_sync.rs:71` ustawiają **na sztywno** `OMNIDRIVE_DRIVE_LETTER=Y:`.
2. Ich `Drop` (odpowiednio `:201` i `:127`) ubija proces i kasuje katalog tymczasowy,
   ale **nie woła `subst /D`** — grep po `subst` w obu plikach daje zero trafień.
   Jedyny harness, który sprząta, to `e2e_shell_recovery` (w `shutdown()`, więc i tak nie
   przy panice).
3. Przy kolejnym uruchomieniu `Y:` jest zajęte, więc daemon wchodzi w
   `select_mount_drive_letter`, które skanuje `('D'..='Z')` **rosnąco** i bierze pierwszą wolną.

Stąd litery od D w górę, narastające z każdym przebiegiem (Z10-15). Naprawa jest po stronie
testów: `subst /D` w `Drop`, nie w `shutdown()`.

## 10.10 Co jest napisane dobrze

- **Testy e2e uruchamiają prawdziwą binarkę.** `CARGO_BIN_EXE_angeld` + własny port +
  własny `LOCALAPPDATA` + `env_remove` na wszystkich zmiennych dostawców, żeby test nie
  dotknął prawdziwej chmury. To jest właściwy poziom izolacji dla maszyny produkcyjnej
  i zgodne ze „Świętą Zasadą" z `CLAUDE.md`.
- **`e2e_shell_recovery` jest `#[ignore]` z uzasadnieniem** („requires an unrestricted desktop
  session for subst-backed virtual drive mapping") — zamiast być testem, który czasem miga
  na czerwono.
- **Wszystkie trzy eksporty COM w rozszerzeniu powłoki są opakowane w `catch_unwind`**
  (`DllGetClassObject`, `ClassFactory::CreateInstance`, `Initialize`, `QueryContextMenu`,
  `InvokeCommand`), a `InvokeCommand` świadomie **nie propaguje błędu pipe'a jako błędu COM**
  — komentarz mówi wprost: *„Explorer must not crash"*. Panika w kodzie ładowanym do
  `explorer.exe` to zabicie powłoki użytkownika; tutaj tego nie ma.
- **`extract_first_path`** używa nowoczesnego `SHCreateShellItemArrayFromDataObject`
  zamiast ręcznego `STGMEDIUM`/`HDROP`, z komentarzem tłumaczącym dlaczego.
- **Tray zmienia ikonę tylko przy zmianie stanu** (`if state != last_state`), zamiast
  przemalowywać ją co 3 sekundy.

## 10.11 Znaleziska

| ID | Waga | Rzecz | Potwierdzone jak |
| --- | --- | --- | --- |
| Z10-01 | 🔴 | `omnidrive-cli` nie wysyła `Authorization` — grep: 0 trafień. 6 z 12 komend (`ls`, `history`, `restore`, `pin`, `unpin`, `recovery backup-now`) kończy się 403; działają tylko te trafiające w endpointy bez bramki | grep + zestawienie z audytem ACL warstwy 9 |
| Z10-02 | 🔴 | `omnidrive recovery restore` nadpisuje **żywą `omnidrive.db`** migawką z chmury: surowe `fs::write`, bez grafta, bez kopii, bez potwierdzenia i bez sprawdzenia, czy daemon trzyma plik | czytanie `main.rs:592-620` + `disaster_recovery.rs:755-764` |
| Z10-03 | 🔴 | Tray zabija daemona `taskkill /F` zamiast wołać `POST /api/settings/restart-daemon`; to samo robi `[UninstallRun]` instalatora → sekwencja z Z7-05 nie ma szans się wykonać, plaintext zostaje na dysku | czytanie `main.rs:204-248` + `omnidrive.iss` |
| Z10-04 | ⚠️ | `recovery restore` używa `MetadataBackupProviderManager::from_env()`, które wymaga kompletu trzech dostawców w env (Z4-10) — na maszynie z instalatora sekrety są w bazie, więc komenda nie ruszy | czytanie + `onboarding.rs` |
| Z10-05 | ⚠️ | Tray odpytuje `/api/vault/status` co 3 s, a ten mintuje sesję przy każdym wywołaniu (Z9-01) — 20 nieusuwalnych wierszy `user_sessions` na minutę | czytanie `POLL_INTERVAL` + sonda rozkładu odstępów |
| Z10-06 | ⚠️ | `omnidrive_shell_ext.dll` jest budowany i kopiowany do payloadu, ale `[Files]` go nie instaluje, a nic go nie rejestruje — pipeline sugeruje dostarczenie komponentu, którego nie ma | `ls payload` + grep po `.iss` (0 trafień) |
| Z10-07 | ⚠️ | `angelctl` to `println!("Hello, world!")`, a mimo to jest w workspace, buduje `angelctl.exe`, leży w payloadzie i podlega bumpowi wersji wg `CLAUDE.md` §3 | czytanie + `ls target/release` |
| Z10-08 | ⚠️ | `cfapi_repro` nie ma `required-features` — `cargo build --release --workspace` produkuje diagnostyczne `cfapi_repro.exe` obok binarek produkcyjnych (klasa Z1-06) | `ls target/release/*.exe` |
| Z10-09 | ⚠️ | Rozszerzenie powłoki twardo sprawdza prefiks `O:\`, a daemon przy zajętej literze montuje pod pierwszą wolną z `D..Z` → menu nie pojawia się w ogóle | czytanie `lib.rs:397` + `virtual_drive.rs:221` |
| Z10-10 | ⚠️ | `log_to_file` dopisuje bez rotacji do `%TEMP%\omnidrive_shell_ext.log` ścieżki plików Skarbca przy każdym kliknięciu prawym przyciskiem | czytanie `lib.rs:42-57`, `:401` |
| Z10-11 | ⚠️ | `load_icon` panikuje przy braku PNG, a release ma `windows_subsystem = "windows"` → tray znika bez komunikatu; ostatni fallback `resolve_icons_dir` zwraca ścieżkę bez sprawdzenia istnienia (same ikony **są** w payloadzie — sprawdzone) | czytanie `main.rs:52-59`, `:371-409` + `ls payload/icons` |
| Z10-12 | ⚠️ | `restart_daemon` = `kill` + `sleep(500 ms)` + `spawn`, bez sprawdzenia, czy port się zwolnił i czy proces wstał | czytanie `main.rs:242-248` |
| Z10-13 | ⚠️ | `taskkill /F /IM angeld.exe` ubija wszystkie instancje, w tym uruchomioną z `target/release` na dev-boxie ([[feedback-lenovo-no-install]]) | czytanie |
| Z10-14 | ⚠️ | 19 funkcji testowych na 3372 linie; są testy negatywne uwierzytelnienia, ale wyłącznie dla auto-locka — brak testu przechodzącego listę endpointów zmieniających stan. Tą szczeliną przeszły Z9-01/02/03/20/21 | inwentaryzacja testów + grep |
| Z10-15 | ⚠️ | `e2e_recovery` i `e2e_sync` hardkodują `OMNIDRIVE_DRIVE_LETTER=Y:` i nie wołają `subst /D` w `Drop`; daemon przy zajętym `Y:` bierze pierwszą wolną literę od `D` w górę — stąd porzucone mapowania z [[feedback-e2e-subst-cleanup]] | czytanie testów + `select_mount_drive_letter` |

---

# 11. Domknięcie luk — co znalazło się w nieprzeczytanych fragmentach

Po zamknięciu przeglądu okazało się, że tabela statusu w kilku miejscach mówiła więcej,
niż faktycznie przeczytano. Ten rozdział domyka listę: `api/diagnostics.rs` w całości,
resztę `api/vault.rs`, `api/files.rs` i `api/onboarding.rs`, statyki (`share.html`,
`share-sw.js`, `wizard.js`, `legacy.html`, obieg tokenu w `index.html`), ogon CLI
i rozszerzenia powłoki, `cfapi_repro` oraz pakiet testów wraz z `tests/common/mod.rs`
(424 linie, których wcześniejsza inwentaryzacja w ogóle nie widziała — `wc` nie wchodził
do podkatalogu).

**15 nowych znalezisk, 4 × 🔴.** Dwa z nich to funkcje, które w ogóle nie działają,
a nikt tego nie zgłosił, bo żaden test ich nie dotyka.

## 11.1 Hasło do linku share nigdy nie zostanie zapytane

`share.html` obsługuje odpowiedź 401 tak:

```js
if (resp.status === 401) {
  const data = await resp.json();
  if (data.requires_password) { /* pokaż formularz hasła */ }
}
```

Serwer w tej sytuacji zwraca `ApiError::Unauthorized { message: "password required" }`,
co `IntoResponse` zamienia na `{"error":"unauthorized","message":"password required"}`.
**Pola `requires_password` nie ma** — grep po całym `angeld/src` daje zero trafień.
Warunek jest więc zawsze fałszywy, kod leci dalej do `if (!resp.ok)` i pokazuje
„Blad podczas pobierania informacji o pliku.".

Skutek: **link chroniony hasłem jest nie do otwarcia**. Odbiorca nie dostaje formularza,
tylko komunikat o błędzie. Cała funkcja — hasło, Argon2id, `share_password_tokens`,
`verify-password`, TTL 10 minut — jest po stronie serwera zbudowana i nieosiągalna
z jedynego klienta, który ją obsługuje (Z11-01).

Ta sama klasa błędu piętro wyżej: przy 410 strona czyta `data.reason` i mapuje go na trzy
komunikaty (`revoked` / `expired` / `download_limit_reached`). Serwer wysyła
`{"error":"gone","message":"share is invalid: revoked"}` — pola `reason` też nie ma,
więc użytkownik zawsze widzi generyczne „Ten link udostepniania jest juz nieaktywny.".
Trzy precyzyjne komunikaty to martwy kod.

## 11.2 Nieuwierzytelnione skasowanie poświadczeń dostawcy

`DELETE /api/onboarding/provider/{provider_name}` nie ma żadnej kontroli dostępu.
Wywołuje `delete_provider_config`, czyli `DELETE FROM provider_configs WHERE provider_name = ?`.
A schemat mówi:

```sql
CREATE TABLE IF NOT EXISTS provider_secrets (
    provider_name TEXT PRIMARY KEY REFERENCES provider_configs(provider_name) ON DELETE CASCADE,
```

Klucze obce są w puli włączone (`schema.rs:12`), więc kasowanie konfiguracji **kasuje kaskadą
zapieczętowane DPAPI poświadczenia**. Jedno nieuwierzytelnione żądanie DELETE niszczy dostęp
do bucketa — a poświadczeń nie da się odtworzyć z niczego, co zostało w systemie.
Shardy leżące u tego dostawcy stają się nieosiągalne do czasu ręcznego wpisania kluczy
od nowa (Z11-02).

Obok tego `POST /api/providers/{provider_name}/test` — również bez auth — uruchamia pełny
test połączenia: `head_bucket`, `list_objects_v2`, `put_object` i `delete_object`.
Przechodzi przez `cloud_guard`, więc jest ograniczony kwotą, ale to nadal nieuwierzytelniony
sposób na wykonywanie operacji zapisu w cudzym buckecie (Z11-12).

## 11.3 Czwarty klient bez uwierzytelnienia

`static/legacy.html` (2258 linii) to kompletny, starszy dashboard serwowany pod `/legacy`.
Ma jeden helper sieciowy:

```js
async function fetchJson(url, options = {}) {
  const response = await fetch(url, options);
```

W całym pliku **zero wystąpień `Authorization` i `Bearer`**. Wywołuje 21 endpointów,
z czego dziewięć jest za bramką ról:

| Endpoint | Rola | Wynik |
| --- | --- | --- |
| `/api/files`, `/api/shares` | Viewer | 403 |
| `/api/maintenance/scrub-now`, `/repair-now`, `/reconcile-now`, `/repair-sync-root` | Admin | 403 |
| `/api/metadata-backup/backup-now` | Admin | 403 |
| `/api/recovery/generate`, `/api/recovery/revoke` | Owner | 403 |

To domyka obraz: **każdy klient w repozytorium poza `index.html` i `wizard.js` nie potrafi
się uwierzytelnić** — menu rejestrowe (Z7-01), CLI (Z10-01) i teraz stary dashboard (Z11-03).
Nie jest to trzy razy ten sam błąd; to jeden błąd popełniony raz, w momencie wprowadzania ról,
i nieprzeprowadzony przez klientów.

Ciekawostka: `/api/maintenance/repair-shell` z listy legacy **działa** — bo jako jedyna
operacja zmieniająca stan w `maintenance.rs` nie ma kontroli roli (Z9-19).

## 11.4 Jedna zmienna środowiskowa wyłącza integralność i zakłamuje diagnostykę

`tests/common/mod.rs` startuje daemona z `OMNIDRIVE_E2E_TEST_MODE=1`. To nie jest flaga
kompilacji ani `#[cfg(test)]` — `main.rs:110` czyta ją przez zwykłe `env_flag`, w binarce
produkcyjnej. Robi dwie rzeczy:

1. `main.rs:320` — w parze z `--no-sync` startuje **wyłącznie uploader i API**. Repair,
   scrubber, gc, watcher i kopia metadanych nie powstają.
2. `main.rs:382` — **ustawia tym nieistniejącym workerom status `Idle`**.

Czyli `/api/diagnostics/health` melduje „repair: idle, scrubber: idle, gc: idle,
watcher: idle, metadata_backup: idle" dla workerów, których w procesie nie ma.
Diagnostyka nie odróżnia „bezczynny" od „nie istnieje" (Z11-04).

Konsekwencja dla testów jest gorsza niż dla produkcji. `e2e_basic.rs:81-85`:

```rust
assert_eq!(health.worker_statuses.repair, "idle");
assert_eq!(health.worker_statuses.scrubber, "idle");
assert_eq!(health.worker_statuses.gc, "idle");
assert_eq!(health.worker_statuses.watcher, "idle");
assert_eq!(health.worker_statuses.metadata_backup, "idle");
```

Test „happy path" asertuje pięć statusów, które w tym trybie są wpisane na sztywno przez
gałąź testową. Asercje przejdą niezależnie od tego, w jakim stanie są te workery naprawdę —
bo w tym uruchomieniu ich nie ma. To nie jest test zdrowia workerów, tylko test tego,
że `main.rs:382` się wykonało.

## 11.5 Kosz, który nie opróżnia chmury

`purge_trash` (`files.rs:257`) na trwałe kasuje plik z kosza:

```rust
db::delete_file_chunks(&state.pool, inode_id).await?;
db::delete_inode_record(&state.pool, inode_id).await?;
```

Obie operacje dotykają wyłącznie bazy. **Obiekty w bucketach zostają** — a razem
z `chunk_refs` znika jedyne powiązanie, po którym worker gc potrafiłby je znaleźć
(druga definicja sieroty z Z6-08 zostaje, ale endpoint `/api/maintenance/gc` też kasuje
metadane bez obiektów). Użytkownik klika „usuń trwale", dostaje `{"purged": true}`,
a zaszyfrowane dane leżą w trzech chmurach i są rozliczane bez końca (Z11-05).

## 11.6 Drobniejsze rzeczy z domkniętych plików

**`/api/storage/cost` to nieuwierzytelniony wzmacniacz obciążenia.** `build_storage_cost_response`
robi `db::list_active_packs(&pool, 100_000)`, a potem `count_reconcile_backlog` odpytuje bazę
**osobno dla każdego packa** (`get_desired_storage_mode_for_pack`). Klasyczne N+1, bez bramki,
wołane przez dashboard przy każdym odświeżeniu (Z11-06).

**Tray nigdy nie pokaże błędu dostawcy.** `provider_connection_status` mapuje
`"PENDING" | "FAILED" if has_error` na `"DEGRADED"`. Cel z `FAILED` praktycznie zawsze ma
`last_error` (ustawia go `mark_*_failed`), więc `"FAILED"` z gałęzi `other` nie wystąpi.
Tray sprawdza dokładnie `connection_status == "FAILED"` — czyli warunek, który nie zachodzi.
Ikona błędu dla awarii dostawcy jest martwa (Z11-07).

**`post_vault_lock` duplikuje teardown.** Handler inline'uje `dismount_after_lock` +
`unmount_virtual_drive` w `tokio::spawn` zamiast wołać `lock_flow::force_lock_and_dismount`,
który `CLAUDE.md` wskazuje jako *„jedno źródło prawdy dla zablokuj i rozmontuj"*. Dwie ścieżki
blokowania, dwa zestawy zachowań. Żadna z nich nie czyści poświadczenia w Credential Managerze,
więc po zablokowaniu Skarbca nieuwierzytelniony `POST /api/unlock/windows-hello` (Z9-02)
otwiera go z powrotem — co czyni auto-lock, Win+L i ręczny lock ozdobą (Z11-08).

**Dwa tokeny o różnym czasie życia.** Token z `/api/unlock` żyje w `VAULT_STATE.sessionToken`,
czyli w pamięci karty. Token z OAuth ląduje w `localStorage`
(`index.html:3624`) i przeżywa restart przeglądarki. Komentarz obok pokazuje świadomość
wycieku przez historię i `Referer` (`history.replaceState`), ale nie tego, że `localStorage`
jest czytelny dla **każdego skryptu w origin** — a w tym origin działają skrypty z dwóch
CDN-ów (Z11-09).

**Trzeci zewnętrzny origin.** Z9-05 wymieniał `cdn.tailwindcss.com` i `cdn.jsdelivr.net`.
Jest jeszcze `fonts.googleapis.com` (`index.html:8`) — przy czym `material-symbols-outlined.ttf`
jest już serwowany lokalnie. Font Inter zostaje jedyną rzeczą, dla której dashboard odpytuje
Google przy każdym otwarciu (Z11-10).

**Service Worker o zasięgu całego origin.** `share.html` rejestruje `/share-sw.js` bez opcji
`scope`, więc SW przejmuje kontrolę nad `/` — całym dashboardem. Przechwytuje wyłącznie
`/sw-download/*` i dla reszty robi wczesny `return`, więc realnie nic nie proxy'uje, ale
zasięg jest znacznie szerszy niż potrzebny (Z11-11).

**`cfapi_repro` ma zaszytą cudzą ścieżkę.** Linia 61:
`PathBuf::from(r"C:\Users\Przemek\AppData\Local\OmniDrive_StandAlone\SyncRoot")`.
Binarka tworzy ten katalog i **rejestruje prawdziwy sync root** (`CfRegisterSyncRoot`,
provider `OmniDrive_SA`, stały GUID). Buduje się przy każdym `--release --workspace` (Z10-08),
a uruchomiona rejestruje w systemie drugiego, konkurencyjnego dostawcę Cloud Files (Z11-13).

**Ścieżka bez Service Workera buforuje cały plik w RAM.** `startDownload` zbiera
`decryptedChunks` w tablicy i dopiero na końcu składa `Blob`. SW włącza się tylko powyżej
50 MiB i tylko gdy jest aktywny — a przez Z9-23 na LAN nie jest aktywny nigdy. Czyli
na linku LAN każdy plik, niezależnie od rozmiaru, przechodzi przez pamięć karty (Z11-14).

**Test regresyjny Z4-01 z konstrukcji nie może złapać Z8-04.** `e2e_pack_key_readback.rs`
używa ładunku 8 KiB przy `DEFAULT_CHUNK_SIZE = 4 MiB`, więc powstaje jeden chunk, jeden pack
i jeden DEK na inode. Ścieżka, którą psuje Z8-04 — inode z wieloma DEK-ami i fallback biorący
`MAX(key_version)` — nie jest w tym teście osiągalna. Wystarczyłby ładunek 12 MiB (Z11-15).

## 11.7 Co się obroniło przy dokładnym czytaniu

Trzy hipotezy upadły po sprawdzeniu i warto to zapisać, żeby nie wracały:

- **XSS przez komunikat błędu dostawcy.** Łańcuch wyglądał realnie: nieuwierzytelniony
  `setup-provider` z `endpoint` zawierającym `<script>` → błąd walidacji zapisany jako
  `last_test_error` → render w kreatorze. `providerStatusBanner` przepuszcza jednak
  **wszystko** przez `escape()` — nagłówek i treść. Nie ma wstrzyknięcia.
- **`wizard.js` nie utrwala sekretów.** `saveSession` wypisuje pola jawnie i pomija zarówno
  `st.secrets` (klucze S3), jak i `st.security` (hasło główne). W `sessionStorage` ląduje
  wyłącznie konfiguracja bez materiału kluczowego.
- **`escapeHtml` w dashboardzie jest stosowane konsekwentnie** — na 45 miejsc z `innerHTML`
  jedno wstawia niezescapowany `${err.message}` (`index.html:3312`), i tylko tam server
  może wpłynąć na treść (przez Z9-14).

Dodatkowo `e2e_scrubber_repair` okazał się najlepszym testem w projekcie: startuje prawdziwego
daemona przeciw mockowi S3 na dysku, kasuje losowy shard, czeka aż scrubber wykryje brak,
a repair odtworzy pack — i przez cały czas trzyma **równoległy heartbeat czytający plik**,
asertując zero błędów odczytu. To jest test, który rzeczywiście sprawdza właściwość systemu,
a nie wykonanie kroków.

## 11.8 Znaleziska

| ID | Waga | Rzecz | Potwierdzone jak |
| --- | --- | --- | --- |
| Z11-01 | 🔴 | Linki share chronione hasłem są nie do otwarcia — klient czeka na pole `requires_password`, którego API nigdy nie wysyła; formularz hasła nie pokazuje się nigdy. Analogicznie martwe są trzy komunikaty dla 410 (`data.reason`) | czytanie `share.html:309-317` + `api_error.rs:72-80` + grep (0 trafień `requires_password`) |
| Z11-02 | 🔴 | `DELETE /api/onboarding/provider/{name}` bez uwierzytelnienia kasuje konfigurację dostawcy, a `ON DELETE CASCADE` usuwa razem z nią zapieczętowane DPAPI poświadczenia | czytanie `onboarding.rs:896` + `schema.rs:90` |
| Z11-03 | 🔴 | `static/legacy.html` (2258 linii, serwowane pod `/legacy`) nie wysyła `Authorization` — 9 z 21 wołanych endpointów zwraca 403. Czwarty klient z tym samym defektem co Z7-01 i Z10-01 | grep (0 trafień `Bearer`) + zestawienie z audytem ról |
| Z11-04 | 🔴 | `OMNIDRIVE_E2E_TEST_MODE` czytane przez binarkę produkcyjną: startuje daemona bez workerów integralności i **ustawia im status `Idle`**. `e2e_basic` asertuje właśnie te sfabrykowane statusy, więc test zdrowia workerów niczego nie sprawdza | czytanie `main.rs:110,320,382` + `e2e_basic.rs:81-85` |
| Z11-05 | ⚠️ | `purge_trash` kasuje `chunk_refs` i wiersz inode'a, ale **nie obiekty w chmurze** — „usuń trwale" zostawia zaszyfrowane dane w trzech bucketach i zrywa ostatnie powiązanie, po którym gc mógłby je znaleźć | czytanie `files.rs:257-280` |
| Z11-06 | ⚠️ | `/api/storage/cost` bez bramki: `list_active_packs(100_000)` + jedno zapytanie na pack w `count_reconcile_backlog` (N+1), wołane przy każdym odświeżeniu dashboardu | czytanie `diagnostics.rs:518-525, 682-694` |
| Z11-07 | ⚠️ | `provider_connection_status` zwraca `DEGRADED` dla `FAILED` z błędem, a tray sprawdza dokładnie `== "FAILED"` — ikona błędu dostawcy nigdy się nie zapali | czytanie `diagnostics.rs:729-738` + `omnidrive-tray/main.rs:145` |
| Z11-08 | ⚠️ | `post_vault_lock` inline'uje teardown zamiast wołać `lock_flow::force_lock_and_dismount` (wskazany w `CLAUDE.md` jako jedyne źródło prawdy); żadna ścieżka blokady nie czyści poświadczenia DPAPI, więc Z9-02 natychmiast ją odwraca | czytanie `vault.rs:1008-1021` + `CLAUDE.md` |
| Z11-09 | ⚠️ | Token OAuth trafia do `localStorage` i przeżywa restart przeglądarki, czytelny dla każdego skryptu w origin (w tym dwóch z CDN); token z `/api/unlock` żyje tylko w pamięci — dwa różne czasy życia dla tego samego typu sekretu | czytanie `index.html:3624, 2206` |
| Z11-10 | ⚠️ | Trzeci zewnętrzny origin w dashboardzie: `fonts.googleapis.com` (Z9-05 wymieniał dwa), mimo że font ikon jest już serwowany lokalnie | czytanie `index.html:8,15` |
| Z11-11 | ⚠️ | `share.html` rejestruje Service Workera bez `scope`, więc obejmuje całe origin zamiast `/sw-download/` | czytanie `share.html:182` |
| Z11-12 | ⚠️ | `POST /api/providers/{name}/test` bez uwierzytelnienia wykonuje `put_object` i `delete_object` w buckecie (ograniczone tylko przez `cloud_guard`) | czytanie `onboarding.rs:859` |
| Z11-13 | ⚠️ | `cfapi_repro` ma zaszytą ścieżkę `C:\Users\Przemek\...` i rejestruje prawdziwy sync root Cloud Files pod własnym GUID-em | czytanie `cfapi_repro.rs:61,111` |
| Z11-14 | ⚠️ | Bez Service Workera `share.html` buforuje cały odszyfrowany plik w pamięci karty; przez Z9-23 na LAN to jedyna dostępna ścieżka | czytanie `share.html:449-483, 438` |
| Z11-15 | ⚠️ | Test regresyjny Z4-01 używa 8 KiB przy chunku 4 MiB — nigdy nie tworzy inode'a z wieloma DEK-ami, więc z konstrukcji nie może wykryć Z8-04 | czytanie `e2e_pack_key_readback.rs:47` + `packer.rs:24` |
