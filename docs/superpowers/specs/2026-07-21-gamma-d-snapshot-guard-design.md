# γ.d — Snapshot upload guard (design)

**Data:** 2026-07-21
**Baza:** HEAD `8fda8fc` origin/main, v0.3.28
**Moduł:** `angeld/src/disaster_recovery.rs` (+ 1 argument z `angeld/src/main.rs`)
**Migracje schematu:** brak. **Bump wersji:** brak.

---

## 1. Kontekst i weryfikacja premisy

Roadmapa (`STATUS.md` §12.7) definiuje γ.d jako: *„Nie nadpisuj dobrego snapshotu przy all-provider-fail; lokalny backup"*.

Audyt kodu 2026-07-21 pokazuje, że **pierwsza połowa tej intencji jest już spełniona strukturalnie**, i to mocniej niż spec:

| Mechanizm | Lokalizacja | Efekt |
|---|---|---|
| Klucz snapshotu timestampowany `snapshots/{created_at}.db.enc` | `upload_metadata_backup:627` | historia **append-only** — żaden snapshot nigdy nie jest nadpisywany |
| `latest.db.enc` wysyłany dopiero po udanym uploadzie snapshotu, per provider | `upload_metadata_backup:700` | fail transportu → wskaźnik `latest` nietknięty |
| 0 sukcesów → `NoSuccessfulUploads`, wiersze `metadata_backups` = `FAILED` | `upload_metadata_backup:744` | `get_last_successful_metadata_backup_at` (`db.rs:3779`, `MAX(created_at) WHERE status='COMPLETED'`) się nie przesuwa → worker ponawia co 1h, nie czeka 24h |
| Restore próbuje `latest`, potem 32 timestampowane (od najnowszego), waliduje każdy przez `snapshot_has_vault_state_row` | `restore_metadata_from_cloud:553-576` | fallback do ostatniego dobrego snapshotu istnieje |
| `local_store` fallback (`OMNIDRIVE_METADATA_BACKUP_DIR`) | `MetadataBackupProviderManager::from_env:132` | pełna ścieżka backup/restore bez chmury (i bez creds — kluczowe dla testów) |

**Werdykt:** γ.d *jako specowane* jest w praktyce moot — podobnie jak γ.a. Lektura kodu wyciągnęła natomiast trzy realne braki, które ta specyfikacja domyka.

---

## 2. Zakres

### G1 — restore nie może się wywrócić na jednym zepsutym obiekcie *(realny bug)*

`restore_metadata_from_cloud:541` i `:568` wołają `decrypt_metadata_backup(&encoded, passphrase)?` **wewnątrz pętli po kandydatach**. Jeden nieodszyfrowywalny obiekt przerywa cały restore: bez fallbacku do starszych snapshotów i bez próby kolejnego providera. Ponieważ `latest.db.enc` jest kandydatem **pierwszym**, uszkodzony wskaźnik unieruchamia całą historię append-only — dokładnie w scenariuszu, dla którego ta historia istnieje.

**Zmiana:** błąd dekrypcji trafia do `errors` i `continue`. Pętla po kandydatach i po providerach dobiega końca; dopiero potem `DownloadFailed(errors)`.

**Konsekwencja do rozbrojenia:** `decrypt_metadata_backup:984` woła `derive_root_keys` = Argon2id z parametrami z nagłówka pliku. Naiwna poprawka daje przy złym haśle do 33× Argon2id po 256 MiB. Dlatego derywacja idzie do cache kluczowanego `(parameter_set_version, salt, memory_cost_kib, time_cost, lanes)`; wszystkie snapshoty jednego vaulta mają te parametry identyczne, więc realnie wychodzi **jedna derywacja na cały restore**.

**Refaktor przy okazji (uzasadniony zadaniem):** `decrypt_metadata_backup` duplikuje ~55 linii parsowania nagłówka, które ma już `parse_metadata_backup:1017`. Cache potrzebuje parametrów KDF z nagłówka, więc `MetadataBackupParsed` zostaje rozszerzone o `RootKdfParams`, a obie ścieżki dekrypcji (passphrase i `decrypt_metadata_backup_with_master`) korzystają z jednego parsera.

**Zmiana semantyki błędu:** złe hasło daje dziś szybki `BackupDecryptFailed`, po zmianie da `DownloadFailed` z listą wszystkich kandydatów. Akceptowane — komunikat pozostaje czytelny, a odzyskiwalność jest ważniejsza niż zwięzłość błędu.

### G2 — periodyczny lokalny `.bak` *(brak z audytu, potwierdzony)*

`grep omnidrive.db.bak` po `*.rs` = 0 trafień; istniejące pliki `omnidrive.db.bak.preSmoke-*` / `.preCleanup-*` powstały ręcznie.

Wartość nie sprowadza się do „drugiej kopii": worker chmurowy robi `continue` gdy vault jest zalockowany (`start_metadata_backup_worker:288`), a `VACUUM INTO` klucza nie potrzebuje. Vault zamknięty tygodniami = **zero świeżych snapshotów w chmurze**, podczas gdy lokalny `.bak` powstanie.

**Zmiana:** nowy krok na początku pętli `start_metadata_backup_worker`, **przed** `require_master_key`:

- `start_metadata_backup_worker` przyjmuje `Option<PathBuf>` (ścieżka żywej bazy) z `main.rs` — `sqlite_db_file_path(&runtime_paths.db_url)` zwraca `None` dla `:memory:`, więc krok sam się wyłącza w testach i e2e;
- nazwa: `<nazwa_bazy>.bak.YYYYMMDD_HHMMSS`, znacznik czasu w **UTC** (sortowanie leksykograficzne = chronologiczne);
- „kiedy ostatni raz" wynika z nazwy najnowszego pliku — **bez** nowej kolumny/klucza w schemacie. Efekt uboczny pożądany: reset bazy nie kasuje wiedzy o backupach, a brak plików → `.bak` powstaje natychmiast;
- próg: 24h; tworzenie przez `VACUUM INTO` (spójna kopia, bez rozdartego WAL);
- retencja: 3 najnowsze, starsze kasowane;
- **parser nazw akceptuje wyłącznie wzorzec `\d{8}_\d{6}`** → ręczne `.bak.preSmoke-*` / `.bak.preCleanup-*` są niewidoczne dla rotacji i nigdy nie zostaną skasowane;
- każdy błąd tego kroku = `warn!` + `continue`, nigdy fatal.

### G3 — sanity guard treści snapshotu

Dziś strona uploadu nie waliduje **niczego**: cokolwiek jest w lokalnej bazie, zostaje zVACUUMowane, zaszyfrowane, wysłane i awansowane na `latest`. Append-only chroni historię, ale automatyczny restore sięga po najnowszy snapshot.

**Kryterium (drop-do-zera + struktura):** blokuj awans `latest.db.enc`, gdy
- brak wiersza `vault_state` **lub** `vault_config`, **albo**
- `COUNT(*) FROM inodes` == 0 przy poprzedniej wartości > 0, **albo**
- `COUNT(*) FROM data_encryption_keys` == 0 przy poprzedniej wartości > 0.

Żadnych progów procentowych — spadek 1240 → 3 przechodzi (użytkownik kasował pliki), 0 → 0 przechodzi (świeży vault).

**Źródło liczników:** żywy pool w `run_metadata_backup_now`, przed utworzeniem snapshotu. `VACUUM INTO` jest wierną kopią, więc liczniki źródła == liczniki snapshotu; oszczędza to otwierania pliku snapshotu (i uniknięcia `init_db`, które odpaliłoby migracje na artefakcie przed zaszyfrowaniem).

**Baseline:** `last_snapshot_inode_count` / `last_snapshot_dek_count` w `system_config` (istniejące `db::get_system_config_value` / `set_system_config_value`).

**Zachowanie przy blokadzie:**
- timestampowany snapshot leci do **wszystkich** providerów normalnie (historia rośnie);
- pomijany jest wyłącznie PUT `latest.db.enc`;
- `warn!` z konkretnymi liczbami (poprzednie vs bieżące);
- wiersz `metadata_backups` = `COMPLETED` — backup faktycznie się udał; oznaczenie `FAILED` powodowałoby ponawianie co godzinę w nieskończoność (koszt + zaśmiecanie bucketów);
- **liczniki bazowe w `system_config` NIE są aktualizowane** → guard pozostaje wzbudzony dopóki baza jest zdegradowana, a `latest` trzyma się ostatniego dobrego snapshotu. Odblokowanie następuje samoczynnie, gdy liczniki wrócą.

Decyzja jest czystą funkcją `(struktura, prev_counts, curr_counts) -> bool`, więc testowalna tabelką.

---

## 3. Czego świadomie NIE robimy

- **Test all-provider-fail w uploadzie** — wymagałby wstrzyknięcia failującego `Uploader`, czyli refaktoru na trait/mock. Inwariant („`latest` nie rusza się bez udanego uploadu snapshotu") jest strukturalny — widoczny wprost w przepływie sterowania `:700`. Równoważną gwarancję od strony odzyskiwania pokrywają testy G1.
- **Progi procentowe w guardzie** — arbitralna stała, która blokowałaby legalne masowe kasowanie (np. purge Kosza po grace 7d z γ.c).
- **Szyfrowanie lokalnego `.bak`** — plik ma dokładnie ten sam profil ekspozycji co żywa `omnidrive.db` leżąca obok (klucze w środku są zapieczętowane, nazwy plików nie).
- **Surfacing guardu w UI** — na razie tylko logi. Gdyby okazało się potrzebne, to osobny task.

---

## 4. Plan testów

Wszystkie offline, przez `local_store` (`OMNIDRIVE_METADATA_BACKUP_DIR`) — zero creds chmurowych. Snapshoty testowe budowane z `vault_config` o tanich parametrach Argon2, żeby nie palić 256 MiB × N w suite.

1. **G1 fallback:** katalog z uszkodzonym `latest.db.enc` + poprawnym starszym snapshotem → `restore_metadata_from_cloud` kończy się sukcesem, odtwarzając ze starszego.
2. **G1 brak wczesnego abortu:** wszyscy kandydaci uszkodzeni → `DownloadFailed` z listą **wszystkich** prób.
3. **G3 decyzja (unit, tabelka):** brak `vault_state` → blok; prev>0 → 0 → blok; prev 1240 → 3 → przepuść; prev 0 → 0 → przepuść.
4. **G3 integracyjny:** baseline w `system_config` > 0, baza wyzerowana → w `local_store` pojawia się nowy timestampowany snapshot, a `latest.db.enc` ma bajty sprzed operacji.
5. **G2 `.bak`:** tworzy przy braku; no-op przy wywołaniu w ciągu 24h; retencja zostawia 3 najnowsze; plik `*.bak.preSmoke-*` nietknięty.

---

## 5. Smoke

Sensowny wyłącznie dla G2: uruchomić daemona na Lenovo, potwierdzić że `.bak` powstał obok żywej bazy i że normalny backup dalej awansuje `latest` (linia logu `metadata backup worker uploaded a fresh recovery snapshot`). G1/G3 wymagałyby celowego uszkodzenia obiektu w chmurze — pokrywają je testy 1–4.

---

## 6. DoD

- `restore_metadata_from_cloud` przechodzi do kolejnego kandydata i kolejnego providera po błędzie dekrypcji; derywacja klucza cache'owana per parametry KDF.
- Lokalny `.bak` powstaje co 24h nawet przy zalockowanym vaulcie, z retencją 3 i bez dotykania plików spoza wzorca `\d{8}_\d{6}`.
- Zdegradowana baza (inodes lub DEK spadły do zera) nie awansuje `latest.db.enc`, ale nadal produkuje timestampowany snapshot; baseline w `system_config` nie przesuwa się do czasu powrotu liczników.
- Bramka: `cargo fmt --all --check` + `clippy --workspace --all-targets` (oba tryby) + `build --release --workspace` + suite angeld zielony (174 + nowe).
