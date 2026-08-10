# OmniDrive — opis projektu (onboarding dla AI / nowego współpracownika)

> Dokument wprowadzający. Czytasz to, bo zaczynasz pracę nad OmniDrive „od zera".
> Stan na: **2026-06-04**, wersja workspace **0.3.27**, branch `main` (faza α.C.b).

> ⚠️ **Ten dokument nie był odświeżany od 2026-06-04 i w kilku miejscach opisuje stan,
> którego już nie ma.** Traktuj go jako wprowadzenie do *idei* projektu, nie jako źródło
> prawdy o kodzie. Znane rozjazdy (stan na 2026-08-10, wersja 0.3.30):
> - `db.rs`, `smart_sync.rs` i `downloader.rs` zostały zdekomponowane na moduły
>   (`db/`, `smart_sync/`, `downloader/`) — sekcja 3 opisuje je jako pojedyncze pliki.
> - `shell_integration.rs` leży w `angeld/src/`, nie w `angeld/src/cfapi/`.
> - `angelctl/` nie jest działającym CLI (Z10-07 w rejestrze).
> - Roadmapa (sekcja 6) jest sprzed przeglądu całego kodu. Aktualny stan i kolejność prac:
>   „PRZEGLĄD ZAMKNIĘTY" w `docs/ARCHITECTURE.md` + plan naprawy
>   `docs/superpowers/plans/2026-08-02-plan-naprawy-przegladu.md`. Faza 0 planu zamknięta
>   2026-08-10, trwa Faza 1.
>
> **Źródła prawdy:** `CLAUDE.md` (reguły), `docs/ARCHITECTURE.md` (stan i rejestr znalezisk),
> `docs/crypto-spec.md` (krypto).

---

## 1. Czym jest OmniDrive

OmniDrive to **zero-knowledge cloud vault** — prywatny dysk w chmurze, w którym
operator chmury (Backblaze B2, Cloudflare R2, Scaleway, dowolne S3-kompatybilne)
**nigdy nie widzi plaintextu ani kluczy**. Całe szyfrowanie i deszyfrowanie dzieje
się lokalnie, na urządzeniu użytkownika. Chmura przechowuje wyłącznie zaszyfrowane,
pofragmentowane bloby (packi) bez metadanych pozwalających je odtworzyć bez kluczy.

Model mentalny: **„Twój własny Dropbox/OneDrive, ale serwer jest matematycznie
ślepy"**. Użytkownik na Windowsie widzi wirtualny dysk `O:\` (oraz folder
`OmniSync`), który zachowuje się jak zwykły dysk, a pod spodem pliki są dzielone na
chunki, szyfrowane kluczem per-plik, kodowane nadmiarowo (erasure coding) i
wysyłane do chmury.

Kluczowe właściwości:
- **Zero-Knowledge**: serwer nie ma kluczy; utrata hasła = utrata danych (stąd
  recovery keys).
- **Local-First**: daemon działa lokalnie, nasłuchuje tylko na loopback + LAN.
  Nie ma centralnego serwera aplikacyjnego ani tunelu. `skarbiec.app` to wyłącznie
  statyczny content.
- **Odporność na awarie**: erasure coding pozwala odtworzyć plik nawet gdy część
  packów zniknie/uszkodzi się w chmurze.
- **Cross-device**: nowe urządzenie dołącza do istniejącego vaulta i odtwarza stan
  z chmury (priorytet nad P2P/LAN mesh).

---

## 2. Stack technologiczny

| Warstwa            | Technologia                                                        |
| ------------------ | ------------------------------------------------------------------ |
| Język / Core       | **Rust** (Edition 2024)                                            |
| Async runtime      | **Tokio**                                                          |
| Baza danych        | **SQLite** przez `sqlx` (+ `rusqlite` miejscami)                   |
| Krypto             | AES-256-GCM, AES-KW (key wrap), Argon2id (KDF), X25519, Erasure Coding |
| Integracja OS      | Windows API (`windows-rs`), **Cloud Files API** (`cfapi.dll`)      |
| Chmura             | S3-kompatybilne (Backblaze B2, Cloudflare R2, Scaleway), własny podpis AWS SigV4 |
| Frontend (Web UI)  | Vanilla JS/HTML + Tailwind, glassmorphism, serwowane z pamięci daemona |
| HTTP server        | `axum` (lokalne API daemona)                                       |
| Build / Instalator | Cargo workspace + **Inno Setup** (`dist/installer/`)               |

Platforma docelowa dziś: **Windows 11**. Mobile (Android-first, UniFFI) jest
zaplanowane po desktopie (patrz roadmapa).

---

## 3. Architektura — crates w workspace

Workspace ma 6 crate'ów (`Cargo.toml` → `members`):

| Crate                 | Rola                                                                   |
| --------------------- | ---------------------------------------------------------------------- |
| `omnidrive-core/`     | **Silnik kryptograficzny** — `crypto.rs` (AES-GCM, Argon2id, envelope), `layout.rs` (erasure coding / fragmentacja), `payloads.rs`, `ffi.rs` (UniFFI dla przyszłego mobile). Czysta logika, bez I/O. |
| `angeld/`             | **Daemon** — serce aplikacji. Baza danych, workery, lokalne API HTTP, integracja z Windows (cfapi, wirtualny dysk, sesje). Nazwa historyczna „angel daemon". |
| `angelctl/`           | CLI do sterowania daemonem (control plane).                            |
| `omnidrive-cli/`      | Dodatkowe narzędzie CLI.                                               |
| `omnidrive-tray/`     | **System Tray Companion** (zasobnik systemowy) — status i sterowanie. |
| `omnidrive-shell-ext/`| Rozszerzenie powłoki Windows (Explorer) — natywne stany cfapi (Ghost Shell). |

### Warstwy wewnątrz `angeld/src`

```
main.rs            → bootstrap daemona, restore bazy, projekcja sync root, spawn workerów
db.rs              → schemat SQLite + migracje + cały dostęp do danych (367 symboli, największy plik)
vault.rs           → logika vaulta: klucze, lock/unlock, envelope encryption, generacje
identity.rs        → tożsamość użytkownika/urządzenia (user_id, device_id, keypair X25519)
device_identity.rs → tożsamość urządzenia
onboarding.rs      → „Join Existing Vault", grafting (odtwarzanie metadanych z chmury)
recovery.rs        → recovery keys (odzyskanie po utracie hasła)
disaster_recovery.rs → odtworzenie całej bazy/stanu z chmury na świeżym urządzeniu

— Pipeline danych (upload) —
watcher.rs         → obserwuje zmiany plików w sync root (DOMYŚLNIE OFF / DRY_RUN podczas dev!)
ingest.rs          → przyjęcie pliku do pipeline'u
packer.rs          → chunking + erasure coding + budowa packów
uploader.rs        → wysyłka packów do S3
downloader.rs      → pobieranie i rekonstrukcja plików z packów

— Utrzymanie / integralność —
scrubber.rs        → okresowa weryfikacja integralności packów w chmurze (deep verify)
repair.rs          → naprawa brakujących/uszkodzonych packów (erasure recovery)
gc.rs              → garbage collection osieroconych packów
cloud_guard.rs     → strażnik operacji chmurowych (limity, egress accounting)

— Windows / OS —
cfapi/ (virtual_drive.rs, shell_state.rs, shell_integration.rs) → wirtualny dysk O:, Cloud Files, placeholdery
smart_sync.rs      → logika selektywnej synchronizacji / projekcji sync root
auto_lock.rs       → monitor bezczynności (wait-free AtomicU64, idle timeout)
lock_flow.rs       → DRY helper force_lock_and_dismount (logout/idle/Win+L → lock + audit + dismount)
win_session.rs     → zdarzenia sesji Windows
windows_hello.rs   → odblokowanie przez Windows Hello
win_acl.rs / acl.rs / secure_fs.rs → ACL i bezpieczne operacje plikowe
autostart.rs       → autostart daemona

— Sieć / P2P —
peer.rs            → LAN mesh (P2P między urządzeniami w tej samej sieci)
pipe_server.rs     → named pipe IPC (komunikacja z tray/shell-ext)
aws_http.rs        → klient HTTP + podpis AWS SigV4 do S3

— API (axum) —
api/mod.rs         → ApiServer (from_env, run), rate limiting, CORS (tylko loopback + RFC-1918), serwowanie Web UI
```

### Moduły API (`angeld/src/api/`)

`files.rs`, `vault.rs`, `onboarding.rs`, `recovery.rs`, `sharing.rs`, `oauth.rs`,
`auth.rs`, `settings.rs`, `stats.rs`, `diagnostics.rs`, `maintenance.rs`,
`audit.rs`, `auto_lock.rs`.

Web UI serwowane z pamięci: `index.html`, `wizard.html` + `wizard.js`,
`share.html`, plus `material-symbols-outlined.ttf`, `qrcode.min.js`.

---

## 4. Jak to działa — model kryptograficzny (Envelope Encryption)

Single source of truth: `docs/crypto-spec.md`. Skrót:

1. **Hasło użytkownika** → przez **Argon2id** → klucz pochodny.
2. Klucz pochodny **wrapuje (AES-KW) Vault Key** (klucz główny vaulta). Vault Key
   trzymany jest tylko zaszyfrowany; w pamięci żyje krótko, jest zeroizowany.
3. Każdy plik/chunk ma własny **DEK (Data Encryption Key)** — szyfrowanie treści
   przez **AES-256-GCM**.
4. DEK jest **wrapowany Vault Key** (AES-KW) i przechowywany w bazie obok metadanych.
5. Zaszyfrowane chunki przechodzą przez **erasure coding** (kodowanie nadmiarowe) →
   pliki dzielone na fragmenty z parzystością, pakowane w **packi**.
6. Packi trafiają do chmury S3. Chmura widzi tylko nieczytelne bloby.

Dodatkowo:
- **Recovery keys** — alternatywna ścieżka odzyskania Vault Key bez hasła.
- **Vault generations / legacy_read_key** — wsparcie rotacji kluczy i odczytu
  starych danych po rotacji.
- **Tożsamość**: `user_id` + `device_id` + para kluczy **X25519** (do przyszłej
  wymiany kluczy / family cloud / WebCrypto compat).
- **Zero-Knowledge Rule (twarda zasada)**: NIGDY nie logować plaintextowych haseł,
  DEK, Vault Key ani tokenów OAuth. Zawsze `[REDACTED]`.

---

## 5. Co już osiągnięto (zrealizowane)

- ✅ **Rdzeń krypto** — envelope encryption (Vault Key wrapuje DEK), AES-GCM,
  Argon2id, erasure coding. Format V2 ustabilizowany.
- ✅ **Pełny pipeline upload/download** — watcher → ingest → packer → uploader →
  S3, oraz downloader z rekonstrukcją z erasure codingu.
- ✅ **Utrzymanie integralności** — scrubber (deep verify), repair (erasure
  recovery), gc, cloud_guard z egress accounting. (Po rozwiązaniu „B2 bleeding":
  orphaned pack retry storm + broken egress accounting.)
- ✅ **Windows Cloud Files (Ghost Shell)** — wirtualny dysk `O:\`, placeholdery,
  natywne stany cfapi, shell extension, system tray (Epic 35). Bez własnych
  overlay icon — tylko natywne stany cfapi.
- ✅ **Onboarding / cross-device** — kreator (wizard), „Join Existing Vault",
  grafting metadanych z chmury, disaster recovery (odtworzenie bazy z B2/R2).
- ✅ **Auto-lock** — monitor bezczynności (AtomicU64), lock przy idle/logout/Win+L
  z audytem i dismountem (faza α.A.b).
- ✅ **Recovery keys, audit trail, sharing, OAuth** — Family Cloud (Epic 34).
- ✅ **Instalator** Inno Setup (Dell); na Lenovo uruchamiamy wprost z
  `target/release`.
- ✅ **Faza α.C.a** — realne generowanie par kluczy X25519 (zamknięta 2026-06-04).

---

## 6. Co przed nami (roadmapa)

### Aktualnie w toku
- **α.C.b — Graft identity-bundle** (TRWA): odtwarzanie tożsamości (user/device +
  klucze) przy dołączaniu urządzenia. Tabele już graftowane: `vault_state`
  (envelope key + generation + legacy_read_key), `data_encryption_keys`,
  `vault_recovery_keys`. Zostaje finalny krok (testy) + bramka + push.
  *To fundament cross-device PRZED PQC — świadoma repriorytetyzacja.*

### Następne fazy krypto
- **α.B.b — ML-KEM hybrid (post-quantum)**: hybrydowa wymiana kluczy X-Wing
  (X25519 + ML-KEM/Kyber), osobne kolumny na klucze Kyber, crate `ml-kem`
  (RustCrypto, pure-Rust). Zatwierdzone architektonicznie.
- **α.D.a — QG5** oraz dalsze utwardzanie krypto.

### Większe epiki / kierunki
- **Epic 33** — wymiana kluczy w przeglądarce (WebCrypto compat).
- **Epic 34** — Family Cloud (dalej: rozbudowa).
- **Epic 35** — Ghost Shell / IPC / natywne stany cfapi (dokończenie).
- **Epic 36** — redesign UI (layout „Stitch", glassmorphism).
- **Mobile (po v0.4.0)**: Android-first + UniFFI (`omnidrive-core/ffi.rs`),
  klucze pochodne z QR (nie raw key), snapshot SQLite, Inbox upload. Strategia:
  **desktop 100% gotowy przed mobile**.

---

## 7. Zasady pracy w repo (KRYTYCZNE — przeczytaj przed edycją kodu)

Pełne reguły: `CLAUDE.md`. Najważniejsze:

1. **🛡️ Bezpieczeństwo danych ponad wszystko** — to **maszyna produkcyjna**
   (Lenovo), nie izolowany sandbox. Błąd w krypto = utrata prywatnych plików.
   - Operacje zapisu/szyfrowania/usuwania **tylko** w ścieżce `SYNC_PATH`.
   - `watcher.rs` i auto-sync **domyślnie OFF lub DRY_RUN** podczas pracy nad
     UI/API.
   - Każda modyfikacja pliku poprzedzona logiem `tracing::info!` (pełna ścieżka +
     typ operacji).
   - Wątpliwość co do bezpieczeństwa → **zatrzymaj się i zapytaj**.

2. **✂️ Chirurgiczne, minimalne zmiany** — dotykaj tylko linii wynikających z
   zadania. Bez refaktorów „przy okazji", bez przedwczesnych abstrakcji, bez
   feature-flag „na zapas".

3. **Zakaz komentarzy** w kodzie produkcyjnym (poza krótkim `///` nad publicznym
   API, gdy WHY jest nieoczywiste). Historię trzyma git, nie komentarze.

4. **TDD** — najpierw test definiujący sukces, potem implementacja. Plany w
   `docs/superpowers/plans/` — odhaczaj checkboxy na bieżąco.

5. **Pipeline buildu** — `cargo check` → `cargo build --release --workspace` →
   kopiowanie binarek do `dist/installer/payload/` PRZED generowaniem instalatora.
   Bump wersji we **wszystkich** `Cargo.toml` jednocześnie.

6. **Gotchas Rust/Windows**:
   - SQLite: kolumny bez `NOT NULL` mapuj jako `Option<T>`, zero hacków z `CAST`.
   - File locks (Defender/Explorer na `C:`/`O:`) → retry loop z backoffem.
   - `ATTACH DATABASE` → wcześniej `drop(conn)`, żeby nie było „database locked".

7. **Polski w UI** — pełne słowa („sekund", nie „sek."; „bajtów", nie „B").

---

## 8. Słownik pojęć

- **Vault / Skarbiec** — zaszyfrowany kontener danych użytkownika.
- **Vault Key** — klucz główny vaulta; wrapuje DEK-i.
- **DEK** — Data Encryption Key, klucz per-plik/chunk (AES-GCM).
- **Pack** — zaszyfrowany, pofragmentowany blob wysyłany do chmury.
- **Grafting** — odtwarzanie metadanych vaulta na nowym urządzeniu z chmury.
- **Ghost Shell** — integracja z Windows Cloud Files (wirtualny dysk + placeholdery).
- **Sync Root** — `C:\Users\{User}\AppData\Local\OmniDrive\OmniSync`.
- **O:\** — wirtualny dysk skarbca.
- **angeld** — daemon (proces tła; serce aplikacji).
- **B2 bleeding** — historyczny bug nadmiarowego egress/retry storm (rozwiązany).
