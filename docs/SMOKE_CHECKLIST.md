# OmniDrive — Smoke Test Checklist

> **Cel:** Protokół ~45 punktów do odhaczania przed KAŻDYM buildem `OmniDrive-Setup-vX.Y.Z.exe` który ma trafić na Dell albo dalej. Nie zastępuje testów automatycznych — uzupełnia je o rzeczy, których cargo test nie wyłapie (cross-device, UI, security w runtime, integracja Windows API).
>
> **Wersja:** v1 (2026-05-17) — w trakcie Fazy 0 Task 3.
> **Adresuje:** P1-001+P1-005 (safety numbers Dell↔Lenovo), P1-006 (logout-locks-vault), P2-004 (auto-lock), P2-005 (Zeroize). Punkty oznaczone `🚨 EXPECTED-FAIL` to znane luki — przejdą dopiero po implementacji α.0a/0b/0c.
>
> **Jak używać:**
> 1. Skopiuj ten plik do `docs/smoke-runs/SMOKE-vX.Y.Z-YYYY-MM-DD.md` przed sesją.
> 2. Odhaczaj `[ ]` → `[x]` w trakcie. Pisz wynik / log line / screenshot path obok każdego punktu.
> 3. Każdy `🚨 EXPECTED-FAIL` musi zostać udokumentowany — "tak, nadal nie działa, patrz P1-006" — nie pomijać.
> 4. Po skończeniu: zapisz plik w `docs/smoke-runs/`, commit, podsumowanie w STATUS.md.
>
> **Wynik buildu:** `PASS` (zero failów poza expected) / `PASS-with-known-gaps` (tylko EXPECTED-FAIL) / `FAIL` (cokolwiek innego).
>
> **Środowisko testowe:**
> - **Lenovo (PN-THINKPAD)** = dev box, daemon uruchamiany z `target/release/angeld.exe` (memory: nie używamy instalatora na Lenovo).
> - **Dell (PN-OFFICE)** = QA target, instalowany przez `OmniDrive-Setup-vX.Y.Z.exe`.
> - **Hasło vault:** zapisane w sesji (memory: hasło z sesji 2026-05-10 z dwukrotną wymuszoną rotacją klucza).

---

## A. Build / Instalacja (5 pkt)

- [ ] **A1.** `cargo clippy --workspace -- -D warnings` → exit 0 (to jest dokładnie krok CI z `.github/workflows/ci.yml`)
- [ ] **A2.** `cargo clippy --workspace --all-targets -- -D warnings` → exit 0 (defense-in-depth: łapie linty w testach których lib-only przepuszcza, P2-003)
- [ ] **A3.** `cargo fmt --all -- --check` → exit 0 (po implementacji Task 5; obecnie 63 pliki brudne — EXPECTED-FAIL do czasu fmt commitu)
- [ ] **A4.** `cargo build --release --workspace` → buduje wszystkie 5 crateów (angeld, angelctl, omnidrive-cli, omnidrive-tray, omnidrive-shell-ext) bez warningów
- [ ] **A5.** Inno Setup `dist/installer/build-installer.ps1` → produkuje `OmniDrive-Setup-vX.Y.Z.exe` w `dist/installer/output/`. Wersja w nazwie pliku = wersja z `Cargo.toml` (memory: ZAWSZE bump przed releasem)

## B. Nowy vault (świeża maszyna, np. Dell przed Join) (6 pkt)

- [ ] **B1.** Świeży install na czystej maszynie (usunąć `%LOCALAPPDATA%\OmniDrive` jeśli zostało) → start daemona z menu Start → tray icon się pojawia, Web UI otwiera się na `http://127.0.0.1:8787/`
- [ ] **B2.** Wizard wykrywa brak vaultu w chmurze → ekran "Utwórz nowy vault" (NIE "Dołącz do istniejącego" — to byłby bug)
- [ ] **B3.** Wpisanie hasła ≥12 znaków, powtórzenie, klik "Utwórz" → ekran sukcesu → `/api/vault/safety-numbers` (curl z `Authorization: Bearer <token>`) zwraca JSON z `safety_numbers` (60 cyfr w 12 grupach po 5) + `mnemonic` (12 słów English BIP-39) + `identicon` (base64 PNG)
- [ ] **B4.** Zapisać safety numbers + mnemonic na fizycznej kartce (real-world: do recovery)
- [ ] **B5.** `/api/diagnostics/health` → status `"ok"`. Providerzy: B2 zielony, R2 zielony lub `warn` (P1-004 ConnReset jest znany), Scaleway zielony lub `warn` (P1-003 403 znany). Dwa `fail` = blokada releasu, jeden = tolerowane.
- [ ] **B6.** `POST /api/vault/lock` → `/api/vault/safety-numbers` zwraca `null` (vault zablokowany). `POST /api/unlock` z tym samym hasłem → `safety_numbers` ZNOWU identyczne z B3 (kluczowe: NIE wygenerowały się ponownie!)

## C. Join Existing Vault (cross-device, Dell dołącza do Lenovo) (6 pkt)

> **Pre-warunki:** Lenovo wgrało ≥1 plik (sekcja D) i ma działający vault. Snapshot metadanych jest w B2 (lub R2/Scaleway).

- [ ] **C1.** Dell instaluje świeżą wersję (po B1-B5 wykonanych jako alternatywny scenariusz bootstrap → wyczyść Dell i zacznij od czystej maszyny tutaj). Wizard wykrywa istniejący vault w chmurze → ekran "Dołącz do istniejącego vault"
- [ ] **C2.** Wpisanie hasła z Lenovo + klik "Dołącz" → snapshot fetchuje się (logi: `[ONBOARDING] downloaded snapshot from B2 bytes=...`), graft do lokalnej `omnidrive.db` (logi: `[ONBOARDING] grafted N rows from snapshot`)
- [ ] **C3.** 🚨 **P1-001+P1-005 OBOWIĄZKOWY**: `curl http://127.0.0.1:8787/api/vault/safety-numbers` na Dellu == identyczny output na Lenovo. **3 elementy muszą się zgadzać:**
  - `safety_numbers` — 60 cyfr identyczne
  - `mnemonic` — 12 słów identyczne
  - `identicon` — SHA256 bytes identyczne
  - **Jeśli różne — `db.rs::graft_restored_metadata_snapshot` nadal pomija `vault_state.encrypted_vault_key`/`vault_key_generation`. EXPECTED-FAIL do α.4.**
- [ ] **C4.** `/api/vault/status` na Dellu: `members_count >= 2`, `key_generation == ` (z Lenovo, np. 4 jeśli Lenovo gen=4), `vault_id` identyczne z Lenovo
- [ ] **C5.** MultiDevice tab w UI Dell widzi OBA urządzenia (PN-THINKPAD + PN-OFFICE). Po refresh w UI Lenovo — czy Lenovo widzi Dell? **EXPECTED-FAIL do β.1 (P1-002 — brak periodic snapshot fetch worker; Lenovo zobaczy Dell dopiero po manualnym restart).**
- [ ] **C6.** Plik wgrany przez Lenovo (D7) widoczny na Dellu jako `ghost` placeholder w `O:\` (CFAPI status: `IO_REPARSE_TAG_CLOUD`)

## D. Upload / Download / Sync (8 pkt)

- [ ] **D1.** Lenovo: wgranie pliku `test-small.bin` ~512KB (`head -c 524288 /dev/urandom > test-small.bin` lub równoważnie w PowerShell) do `O:\` → po <5s pojawia się w `/api/files`
- [ ] **D2.** Logi `angeld.log` zawierają `[UPLOAD] target completed pack=... provider=B2` w ciągu <60s od D1
- [ ] **D3.** Logi: analogicznie `provider=R2`. Jeśli `ConnectionReset` — sprawdź P1-004 czy nadal otwarte, oznacz EXPECTED-FAIL
- [ ] **D4.** Logi: analogicznie `provider=Scaleway`. Jeśli `403 AccessDenied` — sprawdź P1-003, EXPECTED-FAIL
- [ ] **D5.** Restart daemona Lenovo (`taskkill /F /IM angeld.exe && start angeld.exe`) → po unlock plik nadal widoczny w `O:\` i `/api/files`
- [ ] **D6.** Dell (już po C): hydrate `test-small.bin` (kliknięcie 2× w Explorerze) → pobiera z B2 + dekryptuje. Stan z `ghost` na `hydrated` w cfapi (`Get-Item test-small.bin` → no `Offline` attribute)
- [ ] **D7.** Dell: `Get-FileHash test-small.bin -Algorithm SHA256` == hash oryginału z Lenovo. **Jeśli różne lub `aes-gcm operation failed` w logach — graft DEK nie zadziałał (P1-001), EXPECTED-FAIL do α.4.**
- [ ] **D8.** Lenovo: wgranie pliku `test-large.bin` ~50MB → upload kończy się <5min, multi-pack jeśli >chunk_size, wszystkie packi `COMPLETED_HEALTHY`

## E. UI (Web) (5 pkt)

- [ ] **E1.** `http://127.0.0.1:8787/` → ładuje się wizard (świeży vault) albo Status (po unlock). Brak białej strony, brak błędu w DevTools console
- [ ] **E2.** Sidebar links wszystkie działają (klik nie przenosi na 404, nie odświeża strony): **Przegląd**, **Pliki**, **MultiDevice**, **Diagnostyka**, **Udostępnione**, **Maintenance**, **Wyloguj**
- [ ] **E3.** Gramatyka PL (CLAUDE.md zakaz): grep `mies\.|MB/s|sek\.` w `dist/installer/payload/static/index.html` + `wizard.js` + `views/*` → **brak matchy.** Używamy "miesięcy", "megabajtów na sekundę", "sekund".
- [ ] **E4.** Onboarding: identicon (24×24 PNG) + 12 słów + 60 cyfr (12 grup po 5) wyświetlają się na ekranie sukcesu. Kopiowanie do schowka działa.
- [ ] **E5.** Diagnostyka: wszystkie sekcje wyrenderowane (Vault, Providers B2/R2/Scaleway, Cloud Guard, Smart Sync, Watcher, Disaster Recovery, Cache). Brak `404` ani `[object Object]`.

## F. Recovery / Maintenance / Lock (5 pkt)

- [ ] **F1.** `POST /api/vault/lock` → 200 OK → `/api/vault/safety-numbers` zwraca `null` → `O:\` w Explorerze znika (CFAPI dismount, smart_sync `[LOCK] CF sync root torn down`)
- [ ] **F2.** `POST /api/unlock` z **dobrym** hasłem → 200 + session_token → safety_numbers wracają identyczne z B3
- [ ] **F3.** `POST /api/unlock` z **błędnym** hasłem 3× → 401 trzy razy, 4-ta próba → 429 Too Many Requests (rate limit `recent_unlock_failures` w api/mod.rs:60-65)
- [ ] **F4.** **Rotacja hasła:** `POST /api/vault/rotate-passphrase` z nowym hasłem → stare hasło już nie unlockuje (F2 z starym = 401), nowe unlockuje (F2 z nowym = 200). DEK re-wrap worker leci w tle (logi: `[VAULT] re-wrapping DEKs gen=X→Y, batch=...`).
- [ ] **F5.** **Recovery key** (BIP-39 24-word): `POST /api/recovery/generate` → 24 słowa. `POST /api/recovery/restore` z poprawnym 24-słownym mnemonicem + nowym hasłem → vault unlocked z nowym hasłem.

## G. Stabilność / Long-running (4 pkt)

- [ ] **G1.** Daemon idle 10 minut po unlocku (brak aktywności w `O:\`, brak otwartych zakładek UI) → `taskmgr`: CPU `angeld.exe` < 1% (SLA cel z roadmapy: <1% idle / <5% active). **Jeśli wyższe — sprawdź P2-001.**
- [ ] **G2.** Watcher stress: 100 małych plików kopiowanych szybko (`for /L %i in (1,1,100) do copy small.bin O:\test\file%i.bin`) → po <2min wszystkie w `/api/files` ze statusem `current_revision_id` ustawionym + active pack `COMPLETED_HEALTHY` (lub `UPLOADING` jeśli kolejka się jeszcze nie skończyła)
- [ ] **G3.** `taskmgr` po G2: brak zombie procesów `angeld.exe`, brak handle leaks (Handles count < 5000), brak memory growth >100MB ponad baseline B5
- [ ] **G4.** `/api/diagnostics/health` polling co 30s przez 1h → `status="ok"` cały czas, brak `failed`/`degraded`. (Można zostawić uruchomione w PowerShell tle: `while($true){ Invoke-RestMethod http://127.0.0.1:8787/api/diagnostics/health; Start-Sleep 30 }`)

## H. Zero-Knowledge / Security (11 pkt — najważniejsze)

> **Sekcja H pokrywa security gaps wykryte w Task 2 audytu Fazy 0 (P1-006, P2-004, P2-005). Punkty `🚨 EXPECTED-FAIL` przejdą dopiero po implementacji α.0a/0b/0c — do tego czasu są **dokumentowane jako znane** w każdym smoke runie.**

- [ ] **H1. 🚨 P1-006 (logout-locks-vault)**: unlock vault → `POST /api/auth/logout` → natychmiast `procdump -ma angeld.exe out.dmp` → `strings out.dmp | findstr /R "<hex-prefix-twojego-vault-key>"` → **NIE znaleziono** plain klucza. **EXPECTED-FAIL do α.0a** — obecnie `post_auth_logout` (api/auth.rs:189) tylko `delete_user_session`, klucze plain w RAM. Zacytuj fragment dmp pokazujący key.
- [ ] **H2. 🚨 P2-004 (auto-lock idle)**: unlock vault → zostaw maszynę bez ruchu 15 min → `/api/vault/safety-numbers` → zwraca `null` (vault auto-locked). **EXPECTED-FAIL do α.0b** — obecnie brak timera, vault unlocked po N godzinach.
- [ ] **H3. 🚨 P2-004 (Windows session lock)**: unlock vault → Win+L (zablokuj sesję Windows) → odblokuj sesję → `/api/vault/safety-numbers` → zwraca `null`. **EXPECTED-FAIL do α.0b** — brak `WM_WTSSESSION_CHANGE` hooka.
- [ ] **H4. 🚨 P2-005 (Zeroize)**: unlock vault → zapisz hex-prefix klucza z H1 → `POST /api/vault/lock` → natychmiast `procdump -ma angeld.exe out.dmp` → `strings out.dmp | findstr /R "<hex-prefix>"` → **NIE znaleziono.** **EXPECTED-FAIL do α.0c** — `expose_secret()` w vault.rs:77-91 zwraca un-zeroized copy, klucz zostaje na stosie wywołującego po lock.
- [ ] **H5.** `findstr /S /M "DEK\|VK\|master_key" %LOCALAPPDATA%\OmniDrive\logs\*.log` → tylko `[REDACTED]`, brak hex bytes klucza. (CLAUDE.md zero-knowledge rule).
- [ ] **H6.** Share-link tampering: `POST /api/share/create` na pliku → spróbuj decrypt z **zmodyfikowanym** DEK w URL fragment (zmień jedną literę) → browser pokazuje `aes-gcm tag verification failed`, brak partial plaintext, brak crash daemona.
- [ ] **H7.** **AAD audit sanity** (P3-001 doc-only — tu sprawdzamy że spec matches code): `decrypt_chunk_v2(dek, &nonce, &[], ciphertext, &gcm_tag)` z **niepustym** AAD (np. `b"x"`) → `aes-gcm tag verification failed` (czyli AAD jest faktycznie autentykowany, nie ignorowany).
- [ ] **H8.** **OAuth seal AAD**: encrypted Google refresh token w `oauth_tokens` table — próba odszyfrowania z **innym** `user_id` jako AAD → `decrypt_secret` returns `Err`. Wbudowane w `vault.rs::unseal_oauth_token` (linia 308 używa `user_id.as_bytes()` jako AAD).
- [ ] **H9.** SQLite forensics: `sqlite3 omnidrive.db "SELECT length(encrypted_vault_key), length(wrapped_vault_key) FROM vault_state"` → 40 bytes (AES-KW output). NIE 32. `SELECT length(wrapped_dek) FROM data_encryption_keys LIMIT 1` → 40 bytes. (Plain klucze byłyby 32, AES-KW dodaje 8 bytes integrity).
- [ ] **H10.** Memdump sanity podczas **unlocked**: H1 ale BEZ logout. `strings out.dmp | findstr /R "<key-hex-prefix>"` → **znaleziono** (klucz JEST w pamięci podczas unlocked — to oczekiwane, sanity check że memdump faktycznie łapie).
- [ ] **H11.** **CORS allowlist** (memory: never add public domains): `curl -H "Origin: https://evil.example.com" http://127.0.0.1:8787/api/files -v` → response brak `Access-Control-Allow-Origin: *` ani brak całego nagłówka. Dozwolone tylko loopback/RFC-1918 origins.

---

## Wynik

**Build:** `OmniDrive-Setup-vX.Y.Z.exe`
**Data:** YYYY-MM-DD HH:MM
**Tester:** Przemek + Claude
**Total odhaczone:** ___ / 50
**EXPECTED-FAIL:** ___ / 4 (H1, H2, H3, H4 do α; A3 do fmt commitu; ewentualnie C5/D3/D4/D7 do β/α.4)
**Other FAIL:** ___ (lista z opisem — każdy blokuje release, chyba że eskalowany do P0/P1 w KNOWN_ISSUES.md)

### Decyzja release

- [ ] **PASS** — wszystkie inne niż EXPECTED-FAIL przeszły → release zatwierdzony
- [ ] **PASS-with-known-gaps** — tylko EXPECTED-FAIL → release zatwierdzony z notką w changelogu
- [ ] **FAIL** — co najmniej jeden nieEXPECTED — release zablokowany, fix przed retest

### Notatki

(wolne miejsce na obserwacje, logs które chcesz zachować, screenshoty paths, hash niespodzianek, etc.)
