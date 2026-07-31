# OmniDrive — Known Issues Tracker

> **Single source of truth dla bugów.** Ten plik (nie GitHub Issues, nie STATUS.md) trzyma listę otwartych problemów z priorytetyzacją.
>
> **Ostatnia aktualizacja:** 2026-07-31
> **Aktualna wersja:** v0.3.28

---

## Priorytetyzacja

| Tier | Definicja | Gate |
|------|-----------|------|
| **P0** | Crash, data loss, niemożliwy unlock — system unusable | Blokuje każdy release, fix natychmiast |
| **P1** | Krytyczna funkcja działa nieprawidłowo, ale nie traci danych — np. ACL fail, niedziałający flow | Blokuje v0.4 release; nie blokuje v0.3.x patcha |
| **P2** | Performance / UX dług który łamie SLA z roadmapy ale system funkcjonalnie OK | Blokuje v0.4 release; tolerowane w v0.3.x |
| **P3** | Drobne UX / kosmetyka / nice-to-have | Nie blokuje v0.4; może iść do v0.4.x patch lub v5.0 |

**Workflow:**
- Claude dodaje wpis po wykryciu (review code lub testy)
- Przemek zatwierdza priorytet ("OK P1") lub koryguje
- Po fixie: status `→ FIXED in vX.Y.Z`, wpis przenoszony do `## Closed` na dole

---

## P0 — Crash / Data Loss

*Brak otwartych. Sukces.*

---

## P1 — Krytyczne błędy logiczne

*Brak otwartych — wszystkie P1 z Dell smoke v0.3.23 zamknięte: P1-001/005 (α.C.b graft), P1-002 (β.b fetch worker), P1-003/004 (β.c cloud redundancy).*

---

## P2 — Performance / SLA dług

### P2-001 — Watcher mieli CPU

- **Wykryto:** Subiektywna obserwacja Przemka, brak benchmarku
- **Symptom:** `angeld.exe` w taskmgr pokazuje wysokie CPU nawet w idle (do potwierdzenia liczbowego)
- **SLA cel:** < 1% CPU idle, < 5% active (per roadmap v0.4)
- **Pomiar (Faza 0, 2026-05-17):** perf baseline M3 watcher CPU idle **0%** + M4 load **avg 0.01% / max 0.14%** — **w pełni w SLA** (`docs/perf-baseline-2026-05-17.md`). Pierwotna subiektywna obserwacja NIE potwierdzona benchmarkiem.
- **Fix scope:** brak — wynik PASS. Pozostawione OPEN do formalnego domknięcia decyzją (czy zamknąć jako „resolved-by-measurement", czy re-mierzyć po Fazie β z aktywnym watcherem na realnym obciążeniu).
- **Status:** OPEN (kandydat do zamknięcia — pomiar PASS). **Faza β.d** = bez akcji.

### P2-002 — VFS laguje przy dużych plikach

- **Wykryto:** Subiektywna obserwacja Przemka, brak benchmarku
- **Symptom:** Otwarcie dużego pliku (>50MB?) z O:\ trwa zauważalnie długo
- **SLA cel:** Cold fetch < 2s/10MB, < 10s/100MB; warm < 100ms (per roadmap v0.4)
- **Fix scope:** (1) Benchmark: cold fetch 1MB/10MB/100MB/1GB; warm fetch tych samych. (2) Audit ścieżki hydracji — sprawdzić: streaming hydration czy fetch-all-then-decrypt? EC reconstruction blokująca? Cache hit path? **Punkt (2) jest teraz tańszy:** `smart_sync.rs` został zdekomponowany (P2-008), ścieżka callbacków cfapi siedzi w `smart_sync/imp/callbacks.rs`, hydracja w `imp/placeholder.rs`.
- **Status:** OPEN — **jedyny otwarty dług funkcjonalny w trackerze**. Wymaga pomiarów; dekompozycja `smart_sync.rs` nie była fixem, tylko usunięciem przeszkody. **Faza ε.a**.

---

## P3 — Drobne UX / kosmetyka

*Brak otwartych.*

---

## Closed

### P1-008 — Placeholder serwował starą wersję pliku po aktualizacji (2026-07-31)

- **Wykryto:** live smoke. Hydracja uporczywie prosiła o `revision=1410`, mimo że bieżąca rewizja pliku była już 1419, potem 1445. Dopiero ręczne skasowanie placeholdera i ponowna projekcja przestawiały go na aktualną.
- **Przyczyna źródłowa:** placeholder cfapi trzyma parę `(inode_id, revision_id)` w blobie `FileIdentity`, nadanym w chwili tworzenia. `create_projection_placeholder` miał wczesne wyjście `if !target_path.exists()`, więc dla **istniejącego** pliku nie robił nic — ani nie przestawiał tożsamości, ani nie aktualizował rozmiaru. Baza (`smart_sync_state.revision_id`) była aktualizowana poprawnie, więc rozjazd był niewidoczny z poziomu danych.
- **Skutek:** po zmianie pliku Eksplorator serwował **poprzednią zawartość** — cicho, bez błędu, z nieaktualnym rozmiarem. Dotyczyło też scenariusza wielourządzeniowego: plik zmieniony na urządzeniu A byłby na urządzeniu B nadal stary.
- **Status:** ✅ FIXED, commit `0228239`. Nowe `update_placeholder_revision` (`CfUpdatePlaceholder` z nową tożsamością, rozmiarem i czasem + `DEHYDRATE` starej treści + `MARK_IN_SYNC`). Wywoływane **tylko** gdy rewizja faktycznie się zmieniła — porównanie z wartością odczytaną **przed** nadpisaniem przez `ensure_smart_sync_state`. Bezwarunkowa aktualizacja kasowałaby lokalne kopie przy każdej projekcji i generowała niepotrzebny egress, który w tym projekcie jest limitowany.
- **Naprawione w trzech miejscach z tą samą wadą:** projekcja całego vaulta, `sync_placeholder_pin_state`, `hydrate_placeholder_now` — to ostatnie sprawiało, że „Pobierz teraz" na zmienionym pliku ściągało starą wersję.
- **Weryfikacja:** cfapi nie da się sensownie pokryć testem jednostkowym, więc dowód jest z żywego vaulta: plik A (1 MB, rew. 1469) → nadpisany B (2 MB, rew. 1473) → re-projekcja **bez** kasowania placeholdera → odczyt z `O:\` zwrócił 2 097 152 B i sha256 wersji B; log `placeholder repointed to revision`, hydracja prosi o rew. 1473. Bramka: fmt + clippy oba tryby + release + core 28 + angeld lib 204.

### P1-003 + P1-004 — PRAWDZIWA przyczyna: automatyczne sumy kontrolne SDK (2026-07-31)

> **To jest korekta wcześniejszej diagnozy.** P1-004 zamknięto 2026-06-06 jako „R2 zrywa idle keep-alive, hyper reużywa martwy socket" i naprawiono `pool_idle_timeout=10s` + retry. P1-003 przypisano IAM-owi Scaleway. Obie diagnozy były **niepełne** — objawy wracały, bo prawdziwa przyczyna leżała gdzie indziej.

- **Objaw:** `cloudflare-r2 put_object failed: ConnectionReset 10054` oraz `scaleway put_object failed: request has timed out after 120s` — na shardach o rozmiarze 1,5–2 MB. Backblaze B2 wysyłał te same shardy w 0,6 s.
- **Dowód rozstrzygający:** ten sam shard, ta sama minuta — `repair.rs` wysłał go na R2 w 0,8 s, a `uploader.rs` poległ. Różnica: `repair.rs` używa `ByteStream::from(bytes.to_vec())` (bufor), `uploader.rs` używa ciała **strumieniowego**.
- **Przyczyna źródłowa:** `aws-sdk-s3` 1.119 ma domyślnie `request_checksum_calculation = WhenSupported`. Dla ciała strumieniowego SDK dokleja CRC32 w kodowaniu **`aws-chunked` z trailerem**. R2 zrywa wtedy połączenie, Scaleway czeka do timeoutu operacji. B2 to toleruje — i to właśnie tolerancja B2 przez tygodnie kierowała diagnozę na sieć i IAM.
- **Status:** ✅ FIXED, commit `338e641`. `.request_checksum_calculation(RequestChecksumCalculation::WhenRequired)` w konfiguracji klienta S3. Diff: 7 linii.
- **Weryfikacja na żywo:** przed — R2 0 udanych / 2 błędy, Scaleway 0 udanych / 2 błędy; po — **R2 i Scaleway 0 błędów, wszystkie 21 shardów `COMPLETED`**, cały zaległy backlog wyczyszczony. Test `s3_config_does_not_add_automatic_checksums` pilnuje ustawienia, bo regresja byłaby niewidoczna w CI (B2 przechodzi mimo błędu).
- **Hipotezy obalone po drodze** (obie wycofane z kodu, żeby nie zostawiać martwych łatek): brak `SizeHint` w strumieniu; własne ciało `http-body` 1.x przechodzące przez adapter hyper 0.14 (`ByteStream::from_path` też nie pomógł).
- **Do rozważenia osobno:** `pool_idle_timeout=10s` z β.c był leczeniem objawu tej samej choroby. Nie usuwam go — nie szkodzi — ale nie jest już potrzebny z tego powodu.

### P1-007 — Jeden zepsuty provider blokował całą kolejkę uploadu (2026-07-31)

- **Wykryto:** live smoke po dekompozycjach P2-007/008/009. Job #7 `IN_PROGRESS` z 9 próbami (R2 „dispatch failure", Scaleway timeout), a jobs #8 i #9 stały `PENDING` z **zerem prób** — mimo że Backblaze B2 działał bez zarzutu.
- **Przyczyna źródłowa (trzy składniki, wszystkie konieczne):**
  1. `db::get_next_upload_job` wybierał ściśle `ORDER BY id ASC`.
  2. Nieudane zadanie wracało do `PENDING` z tym samym, najniższym id → było wybierane ponownie zamiast ustąpić miejsca kolejnym.
  3. Backoff spał w **głównej pętli** workera (`uploader.rs::run`), a worker uploadu jest **jeden** — więc sen zatrzymywał wszystkie pozostałe zadania, nie tylko felerne.
- **Skala:** przy `UPLOAD_RETRY_PLATEAU_AT=100` (odstęp 1 h po 100 próbach) i `UPLOAD_PERMANENT_FAILURE_AT=1000` zepsuty provider mógł wstrzymać wysyłkę **wszystkich** plików na ~37 dni. Użytkownik widzi pliki w skarbcu i zakłada, że są w chmurze — a leżą wyłącznie na dysku lokalnym. Stąd P1: to zagrożenie trwałości danych, nie wydajności.
- **Status:** ✅ FIXED, commit `0d57c82`. Odroczenie zapisywane w bazie zamiast spania w pętli: kolumna `upload_jobs.next_attempt_at` (migracja addytywna przez `ensure_column_exists`), `get_next_upload_job` pomija zadania których czas nie nadszedł, `requeue_upload_job` → `requeue_upload_job_after(delay_ms)` bez `sleep`. `reset_in_progress_upload_jobs` kasuje odroczenia — restart daemona to jawna intencja ponowienia, więc naprawiony provider jest próbowany od razu, a nie po godzinie plateau.
- **Weryfikacja:** 4 testy w `db/uploads.rs`, sprawdzone mutacyjnie (po usunięciu filtru padają 3 z 4). **Potwierdzenie na żywo:** o `16:52:46.548` job #7 pada i zostaje odroczony, o `16:52:46.564` — 16 ms później — rusza job #8. Przed poprawką worker spałby w tym miejscu 60 s i wziął ponownie job #7. Bramka: fmt + clippy oba tryby + release + core 28 + angeld lib **203**.
- **Nie zmieniono świadomie:** progi `UPLOAD_RETRY_PLATEAU_AT`/`UPLOAD_PERMANENT_FAILURE_AT`. Po tej poprawce długie odstępy nie szkodzą innym zadaniom, więc agresywne odstawianie providera przy przejściowej awarii sieci byłoby gorsze niż cierpliwe czekanie.

### P2-003 — Bin `angeld` duplikuje 27 modułów z lib (dual-compile)

- **Wykryto:** 2026-05-17, Task 1 Fazy 0 / fix CI-red (clippy 1.94). Audyt znalazł 7 lintów w lib, ale `cargo clippy --workspace --all-targets` ujawnił 6 dodatkowych w bin których lib-only check nie złapał.
- **Symptom:** `angeld/src/main.rs` deklaruje `mod xxx;` dla **27 modułów** które są jednocześnie `pub mod xxx;` w `angeld/src/lib.rs` (acl, api_error, autostart, ingest, aws_http, cache, cloud_guard, config, db, device_identity, diagnostics, disaster_recovery, downloader, identity, logging, migrator, onboarding, packer, peer, pipe_server, recovery, runtime_paths, secure_fs, smart_sync, uploader, vault, win_acl). Każdy z nich jest kompilowany dwa razy (raz jako część `lib angeld`, raz jako część `bin angeld`).
- **Bin-only moduły (10, prawidłowo poza lib):** api, gc, repair, scrubber, sharing, shell_integration, shell_state, virtual_drive, watcher, windows_hello.
- **Konsekwencje:**
  - **2× compile time** dla 27 modułów (w tym `db.rs` 8.6k linii, `smart_sync.rs` 2.2k, `downloader.rs` 1.7k).
  - **2× clippy reports** z różnymi setami warningów per target — bug pattern wykryty w audycie: lib-only `cargo clippy --workspace -- -D warnings` przepuścił 6 błędów które ujawniły się dopiero przy `--all-targets`.
  - **Risk inkonsystencji**: jeśli kiedyś `lib` i `bin` rozjadą się (np. różne ścieżki w `mod xxx { ... }` body), będą efektywnie dwie wersje tego samego symbolu — debugowanie trudne.
  - **Drift między lib API a bin internals**: niektóre symbole są `pub` w lib ale używane prywatnie w bin → utrudnia świadome projektowanie API biblioteki (np. dla przyszłej integracji testów e2e jako library consumer).
- **Fix scope (opcje, do decyzji w Fazie α/β):**
  - **Opcja A (preferowana):** Usunąć `mod xxx;` z `main.rs` dla 27 zduplikowanych modułów, zamienić na `use angeld::xxx;`. Bin staje się cienkim wrapperem nad library. Wymaga: przeniesienia bin-only modułów albo do lib (jeśli mają sens jako reusable), albo zostawienia w `main.rs` (private to bin).
  - **Opcja B:** Skasować `angeld/src/lib.rs` całkowicie (bin-only crate). Tracimy library API dla testów e2e i przyszłej integracji.
  - **Opcja C (status quo + safeguard):** Zostawić duplikację, ale dodać do CI sztywne `cargo clippy --workspace --all-targets -- -D warnings` żeby zawsze sprawdzać obie konfiguracje.
- **Impact:** Dług techniczny. Nie blokuje funkcjonalności, ale zwiększa risk regresji (jeden review nie wystarczy — trzeba uruchomić oba targety) + 2× czas CI + utrudnia świadome projektowanie API biblioteki.
- **Status:** ✅ CLOSED 2026-07-31 (Opcja A1). `main.rs` nie deklaruje już żadnego `mod` — 9 modułów bin-only przeniesionych do `lib.rs`, referencje `crate::`→`angeld::`, moduły gołe przez `use angeld::{…}`. Podniesiono do `pub` itemy lib używane przez bin (`onboarding::get_active_provider_configs`, `downloader::from_provider_configs`, `uploader::ProviderConfig`+`provider_name`). Każdy moduł kompiluje się raz; `clippy --all-targets` = jeden spójny set. Suita: core 28, angeld lib 199. Commit `ca263de`. Spec `docs/superpowers/specs/2026-07-30-p2-003-dual-compile-design.md`, plan `…/plans/2026-07-30-p2-003-dual-compile.md`.

### P2-009 — `downloader.rs` monolit 1 730 linii (dekompozycja, 2026-07-31)

- **Wykryto:** audyt `docs/superpowers/specs/2026-05-11-code-audit.md §2.4` — „częściowy split: dekrypcja chunków V1/V2, prefetcher, peer client, pack cache. Średni risk."
- **Symptom:** 1 730 linii, z czego **988 to jeden blok `impl Downloader`** (17 metod). Rozdanie bloków top-level — metoda z P2-007 i P2-008 — niczego by nie rozwiązało.
- **Status:** ✅ CLOSED, commit `0701a59`. `angeld/src/downloader/` = `mod.rs` 163 (typy, `DownloaderError`, konwersje `From`) + `read.rs` 729 (z testem roundtrip 158 linii + 7 helperów mock S3), `pack.rs` 309, `chunk.rs` 264, `provider.rs` 209, `prefetch.rs` 114, `util.rs` 38.
- **Metoda:** blok `impl Downloader` rozbity na **4 bloki `impl Downloader`** w modułach `read`/`pack`/`provider`/`prefetch` (Rust dopuszcza inherent `impl` w dowolnym module crate'a definiującego typ). Z zewnątrz typ zachowuje się identycznie.
- **Widoczność:** podział wymusił `pub(super)` na **13 elementach** — 7 metod prywatnych (`load_plaintext_chunk`, `try_fetch_chunk_from_peer`, `maybe_schedule_prefetch`, `download_pack`, `probe_latency`) i 6 wolnych funkcji (`reconstruct_ciphertext`, `build_manifest_bytes`, `decrypt_chunk_record`, `env_path`, `duration_from_env`, `to_usize`, `to_u64`, `format_error_details`).
- **API zachowane:** `EncryptedChunkBytes` był publiczny na poziomie `downloader::`; po przeniesieniu do `downloader::chunk` ścieżka jest przywrócona przez `pub use chunk::*` w `mod.rs`.
- **Weryfikacja zero-drift:** 17 metod `impl` + 26 bloków top-level = **43, w tym 42 bajt-w-bajt**. Jedyny wyjątek: `download_pack` — dopisanie `pub(super) ` wypchnęło sygnaturę poza 100 kolumn i `rustfmt` ją złamał. Kryterium: wynik identyczny z `rustfmt(baseline + ta sama widoczność)`, formatowanym **w kontekście `impl`** (tam rustfmt ma 4 kolumny mniej).
- **Bramka:** fmt + clippy `--all-targets -D warnings` oba tryby + `build --release --workspace` + core **28** + angeld lib **199** + kompilacja wszystkich testów e2e (konsumują `Downloader` jako library consumer). Bez bumpu wersji.
- **Pokrycie testami:** 3 testy w pliku (roundtrip pack→download→`restore_file` z mockiem S3 → `read.rs`, dwa na format `EncryptedChunkBytes` → `chunk.rs`). Ścieżkę sieciową pokrywają dodatkowo `angeld/tests/e2e_*`.
- Spec `docs/superpowers/specs/2026-07-31-downloader-decomposition-design.md`, plan `…/plans/2026-07-31-downloader-decomposition.md`.

### P2-008 — `smart_sync.rs` monolit 2 236 linii (dekompozycja, 2026-07-31)

- **Wykryto:** audyt `docs/superpowers/specs/2026-05-11-code-audit.md §2.2` — „clean split candidate", ocena ryzyka: zero.
- **Symptom:** warstwa publiczna (16 `pub fn`) i 1 940 linii wnętrza `mod imp` (cfapi/Cloud Files) w jednym pliku; ~60 funkcji, 3 callbacki `unsafe extern "system"`, statiki połączenia i kontekstu hydracji.
- **Status:** ✅ CLOSED, commit `d0f7876`. `angeld/src/smart_sync/` = `mod.rs` (298, warstwa publiczna bez zmian) + `imp/` z 7 modułami: `registration.rs` 622, `callbacks.rs` 480, `placeholder.rs` 340, `projection.rs` 321, `paths.rs` 148, `state.rs` 86, `lifecycle.rs` 86, `mod.rs` 13. Wywołania `imp::*` z warstwy publicznej nietknięte (re-eksporty w `imp/mod.rs`); moduły siostrzane importują wprost z `super::<moduł>::*`.
- **⚠️ Korekta oceny ryzyka z audytu:** „risk zero" było niedoszacowane. Prywatność funkcji `imp` nie jest ułatwieniem, tylko źródłem jedynego realnego kosztu: podział wymusił **`pub(super)` na 30 elementach + 13 polach struktur** (`CONNECTION_KEY`, `HYDRATION_CONTEXT`, `HydrationContext`/`HydrationRequest` z polami, `ComApartmentGuard`, stałe providera, `wide_path`, `wide_str`, `apply_pin_state`, `mark_in_sync`, `dehydrate_placeholder`, `create_projection_placeholder`, `projection_path_for_inode`, callbacki i inne). Widoczność wyliczona z realnych referencji + domknięcie przechodnie na typy wyciekające przez podniesione sygnatury — nie z intuicji.
- **Weryfikacja zero-drift:** **100 bloków**, z czego **97 bajt-w-bajt**; 3 (`create_projection_placeholder`, `ensure_path_inside_user_profile`, `powershell_literal_output`) różnią się wyłącznie łamaniem linii — zmniejszenie wcięcia o 4 znaki zmieniło zawijanie na 100 kolumnach. Dla nich kryterium było ostrzejsze niż porównanie tekstu: wynik musi być **dokładnie tym, co `rustfmt` produkuje z bloku baseline**.
- **Bramka:** fmt + clippy `--all-targets -D warnings` oba tryby + `build --release --workspace` + core **28** + angeld lib **199**. Bez bumpu wersji, bez migracji.
- **⚠️ Brak testów jednostkowych modułu.** Ten plik nie miał i nadal nie ma testów; suita 199 go nie pokrywa. Bezpiecznikiem był kompilator (kod `cfg(windows)`, realnie budowany na tej maszynie) i dowód zero-drift. **Poprawność runtime cfapi weryfikuje wyłącznie live smoke — nieprzeprowadzony.** Jeden przebieg suity w trakcie bramki zgłosił 1 fail bez zapisanej nazwy testu; trzy kolejne przebiegi 199/199.
- Spec `docs/superpowers/specs/2026-07-31-smart-sync-decomposition-design.md`, plan `…/plans/2026-07-31-smart-sync-decomposition.md`.

### P2-007 — `db.rs` monolit 10 649 linii (dekompozycja, 2026-07-31)

- **Wykryto:** audyt `docs/superpowers/specs/2026-05-11-code-audit.md §2.1` (2026-05-17) — wskazany jako najpilniejszy kandydat do dekompozycji, zalecany **przed mobile** (UniFFI łatwiej projektować na modułach niż na monolicie).
- **Symptom:** jeden plik, ~14 domen, 238 `pub async fn`, blok testowy 2 100 linii. Każda zmiana w jednej domenie wymagała nawigacji po całości; plik nie mieścił się w kontekście edytora.
- **Status:** ✅ CLOSED. Rozbity na `angeld/src/db/` — 29 modułów + `test_support.rs`. Płaskie re-eksporty w `mod.rs` (`pub use xxx::*`), więc **912 call-site'ów `db::` poza katalogiem pozostało nietkniętych**. 58 testów rozdzielonych do 16 modułów wg asertowanego zachowania. `mod.rs` = 138 linii (enumy, `epoch_secs`, deklaracje, re-eksporty). Największe pliki: `graft.rs` 1621 (986 kodu + testy), `uploads.rs` 789, `schema.rs` 788.
- **Weryfikacja zero-drift:** przeniesienie wykonane deterministycznym ekstraktorem blokow (parser udowodnił round-trip: odtworzenie baseline'u co do bajtu). Kontrola końcowa: **342 bloki produkcyjne identyczne** z `942a442:angeld/src/db.rs`, jedyna zmiana widoczności = `normalize_policy_path` → `pub(super)` (używany przez `projection.rs`); **58 nazw testów zachowanych**; `git diff` poza `db/` i `docs/` pusty.
- **Bramka:** fmt + clippy `--all-targets -D warnings` w obu trybach (default + `test-helpers`) + `build --release --workspace` + **core 28** + **angeld lib 199**. Bez bumpu wersji, bez migracji schematu, bez nowych testów.
- **Odchylenia od planu:** (1) plan pominął 5 funkcji (`get_next_pack_requiring_reconciliation`, `get_chunks_for_pack`, `link_chunk_to_pack`, `get_file_chunks`, `list_active_files`) — wykrył je ekstraktor i przypisał do `packs`/`chunks`/`projection`; (2) jeden `use super::*` w testach `vault_state` jest ocgowany `#[cfg(feature = "test-helpers")]`, bo wszystkie testy tego modułu są za tą flagą — cfg zamiast tłumika `#[allow(unused_imports)]`.
- Spec `docs/superpowers/specs/2026-07-31-db-decomposition-design.md`, plan `docs/superpowers/plans/2026-07-31-db-decomposition.md`. Commity `d68bdc1..da092f7`.

### Faza β — β.3: P3-002 Panic Mitigation (2026-06-06)

- ~~**P3-002** — 2 eskalowane prod-panics (`peer.rs:159` reqwest build `.expect`, `ingest.rs:184` packer init `.expect`)~~ → **FIXED** (`63bbde3`). `PeerClient::new` → `Result<Self, PeerError>` (reqwest build err via `.map_err(PeerError::Http)?`); `IngestWorker::new` → `Result<Self, IngestError>` (`Packer::new` via `?`, `From<PackerError>` istniał). Callerzy (main.rs ×2 w `run_daemon`→`Box<dyn Error>` + tests/e2e_ingest.rs) z `?`. 2 happy-path testy. Pozostałe 21 unwrap/expect z triage to **świadome, udokumentowane decyzje** (11× UI tray fail-fast, 3× mutex-poison idiom, 3× post-invariant guard, 3× hardcoded-Argon2 sanity, 1× `api/mod.rs` post-len-guard) — NIE bugi, zostają jako akceptowane. Bramka --all-targets oba tryby + core 28 + angeld **159** lib green.

### Faza β — β.c: P1-003 & P1-004 Cloud Redundancy (2026-06-06)

Plan: `docs/superpowers/plans/2026-06-06-beta-task2-p1003-p1004-cloud-redundancy.md`. Commity `5cbf3ae`/`e6e20de`/`cdb7443`, TDD subagent-driven. Bramka `--all-targets` oba tryby + core 28 + angeld **157** lib green. Bez bumpu (v0.3.27).

- ~~**P1-004** — R2 ConnectionReset 10054 przy PUT snapshotu (stale keep-alive socket)~~ → **FIXED (kod)** (`5cbf3ae`+`e6e20de`). Root cause = R2 zrywa idle keep-alive, hyper reużywa martwy socket. Fix w współdzielonym `aws_http::load_shared_config`: krótki `pool_idle_timeout` (10s, prune martwych socketów przed RST R2) + adaptive `RetryConfig` (cały workspace, też pack-upload). Plus app-level `retry_with_backoff` (4 próby, exp backoff) wokół uploadu snapshotu — retryuje transienty (ConnReset/timeout), permanentne (403) fail-fast. Pooling NIE wyłączony (perf hot-path zachowany). **Live smoke R2 = osobna akceptacja.**
- ~~**P1-003** — Scaleway 403 AccessDenied na PUT do prefiksu `_omnidrive/system/`~~ → **ROOT CAUSE = IAM/bucket policy (NIE kod)**, kod-side zaadresowany (`cdb7443`). Dowód: `upload_system_file` (snapshoty) i `upload_pack` wołają TEN SAM `upload_file` z identycznym żądaniem i klientem (ten sam `force_path_style`); skoro `packs/` PUT działa na Scaleway a `_omnidrive/system/` nie — to prefix-scoped policy, nie path-style/endpoint. **Kod:** actionable diagnostic (`upload_error_is_access_denied` → warn „IAM/bucket-policy denial, verify s3:PutObject/GetObject/ListBucket on prefix") + potwierdzona graceful degradation (per-provider, 2/3 spełnia QG, Scaleway-403 nie blokuje B2+R2). **⚠️ AKCJA INFRA WYMAGANA (Przemek, konsola Scaleway):** nadać kluczowi dostępowemu uprawnienia `s3:PutObject`+`s3:GetObject`+`s3:ListBucket` na prefiks `_omnidrive/system/*` (lub cały bucket). Po zmianie IAM → live smoke potwierdzi 3/3. Bez tego redundancja działa na 2/3 (B2+R2), co spełnia QG „≥1, docelowo 2/3".

### Faza β — β.b: P1-002 Snapshot Fetch Worker (2026-06-06)

Plan: `docs/superpowers/plans/2026-06-06-beta-task1-p1002-snapshot-fetch-worker.md`. 8 commitów `fe3dcdd..73403fb`, TDD subagent-driven. Bramka `--all-targets` (oba tryby) + core 28 + angeld **151** lib zielone. Bez bumpu (v0.3.27).

- ~~**P1-002** — Lenovo nie widzi Della w MultiDevice po join (jednokierunkowy snapshot: upload worker był, fetch workera nie było)~~ → **FIXED** (β.b). Periodyczny fetch worker (`start_metadata_fetch_worker`, 1h tick, mirror backup workera) + `run_metadata_fetch_now` (newest-wins po `created_at`, marker `last_applied_roster_snapshot_at`, idempotentny, best-effort non-fatal). **Strategia ROSTER-MERGE ONLY** (data-safety): `db::graft_roster_additive` — `INSERT OR IGNORE` **wyłącznie** `devices`+`vault_members` w atomowej tx; **NIGDY** nie dotyka `data_encryption_keys`/`vault_state`/`vault_recovery_keys` (vs JOIN-graft który robi wipe+copy DEK → data-loss). Defense-in-depth: jawna walidacja `vault_id` snapshotu == lokalny PRZED INSERT-em + `decrypt_metadata_backup_with_master` (worker bez passphrase). DoD e2e: aktywne urządzenie uczy się peera, lokalne DEK + revoke-state nietknięte, drugi tick no-op. **Live SMOKE Dell↔Lenovo (Dell join → Lenovo widzi Della po ≤1 tick) = osobna akceptacja operacyjna, NIE bramkuje DONE kodu.**

### Faza β — Task 0: Crypto Debt Elimination (2026-06-06, dyrektywa ZERO DŁUGU TECHNICZNEGO)

Plan: `docs/superpowers/plans/2026-06-06-beta-task0-crypto-debt-elimination.md`. Wszystkie 3 findings QG5 naprawione TDD subagent-driven przed jakąkolwiek logiką sieciową β. Bramka `--all-targets` (oba tryby) + core 28 + angeld 142 lib zielone. Bez bumpu wersji (v0.3.27).

- ~~**P2-006 (F-1)** — `revoke_device` nie NULLuje `wrapped_vault_key_kyber` (niekompletna rewokacja hybrydowa)~~ → **FIXED** (`d0c03ce` + test-strengthen `900a92e`). SQL czyści teraz OBIE kolumny wrapu w jednym atomowym UPDATE; `kyber_public_key` świadomie zostaje (klucz publiczny). Test `revoke_device_nulls_both_wraps` (oba wrapy NULL + generation NULL + public key survives).
- ~~**P3-003 (F-2)** — V2 chunk nie rekomputuje chunk_id po dekrypcji~~ → **FIXED** (`3053216`). Nowy `decrypt_chunk_v2_verified` (rekomputuje `HMAC(DEK, plaintext)`, parytet z V1) wpięty w daemon read-path (downloader, z DB-autorytatywnym chunk_id). **FFI/share-link (`ffi_decrypt_chunk_v2`) i `migrator.rs` świadomie nietknięte** (browser nie ma manifestu). Testy: roundtrip OK + wrong-id → `ChunkIdMismatch`.
- ~~**P3-004 (F-3)** — świeży vault na słabszym parameter_set 1~~ → **FIXED** (`5cd36bd` + cfg-gate `03f276c`). `ensure_vault_config` tworzy świeże vaulty od razu na parameter_set 2 (m=256 MiB) → brak okna słabszego KDF + brak podwójnego Argon2id przy 1. unlocku. Logika re-key migracji v1→v2 nietknięta i nadal testowana (test_pool_v1 jawnie seeduje v1). `DEFAULT_*` consts → `#[cfg(test)]` (legacy v1, test-only). Testy: `fresh_vault_starts_at_target_param_set` + `fresh_vault_needs_no_kdf_migration`.

### Faza α — Crypto Hardening (v0.3.24–v0.3.27, zamknięte 2026-06-06)

- ~~**P1-001 + P1-005** — Graft pomija krytyczne pola krypto (`vault_state.encrypted_vault_key`/`vault_key_generation` + `data_encryption_keys`) → różne EVK/safety-numbers + `aes-gcm operation failed` cross-device~~ → **FIXED w α.C.b** (HEAD `226ee72`, v0.3.27). `graft_restored_metadata_snapshot` rozszerzony o pełen identity bundle (vault_state EVK+gen+legacy_read_key, `data_encryption_keys`, `vault_recovery_keys`) w tx `BEGIN IMMEDIATE`. DoD Rust gate zamknięty in-process: joined EVK == source + safety_numbers identyczne (P1-005) + grafted DEK unwrapuje ten sam plaintext (P1-001). **Live SMOKE Dell↔Lenovo (C3/D7) = osobna akceptacja operacyjna, NIE blokuje zamknięcia kodu.**
- ~~**P1-006** — `/api/auth/logout` nie blokuje vaulta (klucze zostają w RAM)~~ → **FIXED w α.A.a** (commit `ed35ecb`, v0.3.24). `post_auth_logout` woła `vault_keys.lock()` PRZED `delete_user_session` + teardown CF/dysku. SMOKE H1 4/4 PASS na Lenovo.
- ~~**P2-004** — Brak auto-lock po idle~~ → **FIXED w α.A.b** (v0.3.25). Konfigurowalny idle timeout (`vault.auto_lock_idle_minutes`, default 15) + Win+L hook (`WM_WTSSESSION_CHANGE`) + UI chip/settings + `lock_flow::force_lock_and_dismount`. Bug ACL idle-timer reset znaleziony i naprawiony (`8e0d116`). SMOKE H2/H3 PASS live.
- ~~**P2-005** — Brak Zeroize na temp kopiach kluczy~~ → **FIXED w α.A.c** (HEAD `285b913`, v0.3.26). `KeyBytes` newtype z `#[derive(Zeroize, ZeroizeOnDrop)]` + redacted Debug + non-Copy + buildery in-place. SMOKE H4 memdump: after-lock = 0 trafień known-key.
- ~~**P3-001** — AAD pusty (`&[]`) na chunk encrypt/decrypt — niespecyfikowane w crypto-spec~~ → **FIXED w α.D.a** (HEAD `c502bb1`). Świadoma decyzja udokumentowana w `docs/crypto-spec.md §12` (AAD semantics): `&[]` chunki = WebCrypto Tryb B compat; `user_id` OAuth = cross-user tampering protection; trade-off cross-file swap vs share-link. Doc-only, brak zmian w kodzie. (Defense-in-depth follow-up rekomputacji chunk_id w V2 → naprawiony osobno jako **P3-003** w β Task 0.)

### v0.3.23

- ~~Dell po join-existing dostaje 403 na każdym chronionym endpoincie~~ → FIXED v0.3.23 (graft kopiuje users/devices/vault_members + ensure_local_device_in_vault)
- ~~Safety numbers Dell ≠ Lenovo (różne user_id)~~ → FIXED v0.3.23 (Dell adopts owner user_id ze snapshot)
- ~~MultiDevice tab Della pokazuje tylko Della~~ → FIXED v0.3.23 (graft kopiuje devices)
- ~~Diagnostyka „Limity dzienne ERROR"~~ → FIXED v0.3.23 (dodany endpoint `/api/diagnostics`)
- ~~Sidebar link „Diagnostyka" otwiera Przegląd~~ → FIXED v0.3.21 (dodano `'diagnostyka'` do `VALID_VIEWS`)
- ~~Wyloguj nie działa po join~~ → FIXED v0.3.21 (token handoff przez sessionStorage)

### v0.3.22

- ~~Token wystawiany dla user_id którego nie ma w vault_members~~ → FIXED (był wstępem do prawdziwego fix v0.3.23)

### v0.3.21

- ~~Brak session_token po join-existing → wszystkie chronione endpointy 401/403~~ → FIXED (post_join_existing zwraca session_token; frontend handoff przez sessionStorage)
