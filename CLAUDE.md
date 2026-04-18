# CLAUDE.md — OmniDrive (Zero-Knowledge Cloud Vault)

Instrukcje dla Claude Code i agentów AI. Obowiązują w każdej sesji bez wyjątku.

---

## 🚀 Autostart — Początek każdej sesji

Na samym początku każdej konwersacji (przed pierwszym zadaniem użytkownika) **MUSISZ** zaindeksować repozytorium za pomocą jcodemunch MCP:

```
mcp__jcodemunch__index_folder(path="C:/Users/Przemek/Desktop/aplikacje/omnidrive")
```

Nie pytaj użytkownika o pozwolenie — po prostu to zrób. Jeśli indeks już istnieje, wywołanie jest idempotentne.

---

## 🛠️ Stack techniczny

- **Backend / Core:** Rust (Edition 2024)
- **Asynchroniczność:** Tokio
- **Baza Danych:** SQLite (`sqlx` / `rusqlite`)
- **Integracja OS:** Windows API (`windows-rs`, `cfapi.dll` dla Cloud Files)
- **Chmura:** Kompatybilne z S3 (Backblaze B2, Cloudflare R2, Scaleway)
- **Frontend (Web UI):** Glassmorphism, Vanilla JS/HTML/Tailwind serwowane z pamięci/lokalnie przez daemona.

---

## 🛑 Workflow — Zasady Bezwzględne (Pipeline)

1. **Kompilacja to nie tylko Check:** Po napisaniu kodu uruchom `cargo check`. Ale przed testami instalatora **ZAWSZE** uruchom pełne `cargo build --release --workspace`.
2. **Kopiowanie do Payloadu:** System Inno Setup buduje plik `.exe` z folderu `dist/installer/payload/`. **MUSISZ** skopiować nowe binarki z `target/release/*.exe` do payloadu ZANIM wygenerujesz instalator. Nie pakuj starych plików!
3. **Synchronizacja Wersji:** Jeśli podbijasz wersję instalatora (np. `0.1.14`), podbij wersję we wszystkich plikach `Cargo.toml` w całym workspace (`angeld`, `omnidrive-core`, `angelctl`).
4. **Zero-Knowledge Rule:** Nigdy nie loguj do konsoli (`tracing::info!`, `println!`) plaintextowych haseł, kluczy DEK, Vault Keys ani tokenów OAuth. Używaj `[REDACTED]`.

---

## 📂 Architektura i Repozytorium

| Moduł / Plik               | Rola i Zastosowanie                                                 |
| -------------------------- | ------------------------------------------------------------------- |
| `omnidrive-core/`          | Silnik kryptograficzny (EC_2_1, Erasure Coding, AES-GCM, Argon2id). |
| `angeld/src/db.rs`         | Główny interfejs bazy danych SQLite i migracje schematu.            |
| `angeld/src/onboarding.rs` | Logika `Join Existing Vault`, odtwarzanie metadanych (Grafting).    |
| `angeld/src/cfapi/`        | Integracja z Windows Cloud Files (Ghost Shell).                     |
| `dist/installer/`          | Skrypty Inno Setup (`.iss`) i folder `payload/`.                    |
| `docs/crypto-spec.md`      | Single Source of Truth dla Envelope Encryption i formatu V2.        |

---

## 🪤 Pułapki Rusta i Windowsa (Gotchas)

### 1. Typowanie SQLite (Schema Mismatches)

- **Zero tolerancji dla hacków z `CAST`.** Jeśli SQLite na innym urządzeniu przechowuje kolumnę (np. `mtime` lub `mode`) jako `INTEGER` lub może ona przyjąć `NULL`, w Ruście **MUSI** to być zmapowane jako `Option<i64>`.
- Używaj `Option<T>` dla każdego pola, które w schemacie nie ma `NOT NULL`. Błędy dekodowania typu blokują całą aplikację.

### 2. File Locks i Windows Defender

- Operacje na plikach na dysku `C:` i `O:` są często blokowane przez Eksplorator Windows lub Antywirus.
- **Zawsze używaj retry loop** z backoffem (`tokio::time::sleep`), np. 3-5 prób co 500ms, dla operacji takich jak: kasowanie bazy, podpinanie SyncRoot, modyfikowanie `omnidrive.db`.

### 3. Zamykanie Połączeń Bazy Danych

- Przed wykonaniem komendy `ATTACH DATABASE` dla pobranego snapshotu upewnij się, że wszelkie poprzednie operacje testowe na tym pliku zamknęły uchwyt (explicite dzwoń `drop(conn)`). Zapobiega to błędom `(code: 1) database restored is locked`.

---

## 🖥️ UI, UX i Diagnostyka

### 1. Gramatyka Polska (Web UI)

- **KATEGORYCZNY ZAKAZ** używania skrótów: `mies.`, `MB/s`, `sek.`.
- UI ma być profesjonalne. Używaj pełnych słów: "sekund", "bajtów", "miesięcy".
- Statusy mają jasne wagi: `OK` (Zdrowy), `WARN` (Ostrzeżenie/Idle), `FAILED` (Błąd krytyczny).

### 2. Statusy Architektury

- `O:\` — Domniemany wirtualny dysk Skarbca.
- `SyncRoot` — `C:\Users\{User}\AppData\Local\OmniDrive\OmniSync`.
- Nie używamy hacków w rejestrze do podmiany ikon wirtualnego dysku (rezygnacja z mystyfikacji). Status daemona i sterowanie systemem ma odbywać się z poziomu zasobnika systemowego (**Task 35.3: System Tray Companion**).

---

## 🔒 Roadmapa Krypto (Decyzje Architektoniczne)

- **Phase 0:** Przejście na Envelope Encryption (Klucze Kopertowe).
- **Vault Key:** Używamy klucza `Vault Key` do wrapowania (AES-KW) kluczy DEK (Data Encryption Keys) na poziomie poszczególnych plików/chunków.
- **WebCrypto:** Zachowaj kompatybilność algorytmów z API przeglądarek, dla przyszłej wymiany kluczy (Epic 33).
- Odtworzenie stanu z Backblaze B2/R2 ma priorytet nad siecią P2P (LAN Mesh) na starcie świeżego urządzenia.
