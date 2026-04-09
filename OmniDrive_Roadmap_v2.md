# OMNIDRIVE
### Local-First, Zero-Knowledge Storage

---

## Execution Roadmap v2.0
**Epiki 32.5 → 35 → 33 → 34**

*Po Red Teamingu architektonicznym (v0.1.11+) · Kwiecień 2026 · POUFNE*

---

## 1. Kontekst architektoniczny

OmniDrive to lokalny daemon (Windows background service), który agreguje przestrzeń z zewnętrznych bucketów S3 (Cloudflare R2, Backblaze B2, Scaleway), szyfruje wszystko lokalnie i zarządza tym z poziomu wirtualnego dysku `O:\`. Architektura opiera się na zasadzie Zero-Knowledge: klucze szyfrujące nigdy nie opuszczają urządzenia użytkownika.

**Aktualny stan (v0.1.11):** Stabilny system jedno/dwu-urządzeniowy z działającymi ścieżkami `LOCAL_ONLY`, `SINGLE_REPLICA` i `EC_2_1`. Hot-reload providerów, P2P LAN sync, join vault i conflict resolution na bazie lineage tracking.

---

## 2. 4 Poziomy Ochrony

Użytkownik wybiera poziom ochrony prawym przyciskiem myszy w Eksploratorze Windows:

| Poziom | Polityka | Opis | Chmury |
|--------|----------|------|--------|
| **1. LOKALNIE** | `LOCAL_ONLY` | LAN Sync między urządzeniami | Brak chmury |
| **2. COMBO** | `SINGLE_REPLICA` | Pełny plik na dysku + 1 kopia R2 | 1 provider |
| **3. CHMURA** | `Sharding R2+B2` | Plik znika z dysku (widmo), dane w 2 chmurach | 2 providery |
| **4. FORTECA** | `EC_2_1` | Widmo + Erasure Coding na 3 providerach (R2+B2+Scaleway) | 3 providery |

---

## 3. Sekwencja wykonania

Kolejność jest krytyczna — każda faza buduje na fundamencie poprzedniej. Zmiana kolejności grozi refaktoryzacją kryptografii pod presją czasową.

| Faza | Epik/Task | Zakres | Estymacja |
|------|-----------|--------|-----------|
| **Faza 0** | **Checkpoint 0** | Specyfikacja kryptograficzna: algorytmy, długości kluczy, KDF, parametry Argon2id. Dokument decyzyjny. | 1 tydzień |
| **Faza 1** | **Epic 32.5** | Envelope Encryption + migracja formatu bazy danych. Fundament dla wszystkich kolejnych epików. | 2–3 tygodnie |
| **Faza 2a** | **Task 35.0** | cfapi.dll PoC — izolowany test: plik → widmo → hydracja z lokalnego cache. Zero logiki chmurowej. | 1–2 tygodnie |
| **Faza 2b** | **Tasks 35.1–35.2** | Pełny Ghost Shell: Ingest State Machine, EC pipeline, Context Menu, Shell Extension, ikony overlay. | 4–6 tygodni |
| **Faza 3** | **Epic 33** | Zero-Knowledge Link Sharing: Fragment URI, Web Receiver, streaming deszyfrowania w przeglądarce. | 3–4 tygodnie |
| **Faza 4** | **Epic 34** | Family Cloud: OAuth2, asymetryczne Key Wrapping (X25519/ECDH), ACL, rewokacja, recovery keys. | 4–6 tygodni |

---

## 4. Faza 0: Checkpoint kryptograficzny

Zanim pojawi się pierwsza linia kodu Envelope Encryption, musi powstać dokument decyzyjny (1–2 strony) definiujący jedno źródło prawdy dla całej hierarchii kluczy. Decyzje kryptograficzne podejmowane ad hoc podczas implementacji to przepis na niespójności.

### Wymagane decyzje

- **Algorytmy:** AES-256-GCM (DEK), X25519/ECDH P-256 (asymetria), Argon2id (KDF).
- **Parametry KDF:** Ilość iteracji Argon2id, zużycie pamięci, równoległość — balans bezpieczeństwo vs. czas odblokowania na słabszych maszynach.
- **Format DEK wrapping:** AES-256-KW vs AES-256-GCM-SIV do owijania kluczy DEK.
- **Kompatybilność z WebCrypto:** Jeśli Epic 33 używa WebCrypto API w przeglądarce, wybór algorytmów musi być kompatybilny z `window.crypto.subtle` (np. ECDH P-256 zamiast X25519, jeśli przeglądarki nie wspierają).
- **Strategia wersjonowania:** Schemat `vault_format_version` i ścieżka forward-compatibility.

---

## 5. Faza 1: Epic 32.5 — Cryptographic Foundation

> **EPIC 32.5: ENVELOPE ENCRYPTION**

Przebudowa hierarchii kluczy z płaskiego modelu (Master Passphrase → chunki) na trójpoziomowy model kopertowy. Ta zmiana MUSI nastąpić przed implementacją linków publicznych (Epic 33) i współdzielenia vaultów (Epic 34), aby uniknąć refaktoryzacji pod presją.

### Task 32.5.1: Wdrożenie hierarchii kluczy (Envelope Encryption)

| | |
|---|---|
| **Cel** | Zamiana szyfrowania chunków bezpośrednio kluczem z KDF na trójpoziomową hierarchię DEK → Vault Key → KDF. |
| **Zakres** | • DEK (Data Encryption Key): losowy AES-256 per plik, szyfruje chunki danych. |
| | • Vault Key: klucz główny Skarbca, szyfruje WYŁĄCZNIE klucze DEK (key wrapping). |
| | • Master Passphrase → Argon2id → odblokowuje lokalnie Vault Key. |
| | • Zapis wrapped DEK w SQLite obok metadanych pliku. |
| **Wynik** | Rotacja Vault Key wymaga jedynie re-wrappingu małych kluczy DEK, NIE re-szyfrowania terabajtów chunków w chmurze. |
| **Ryzyko** | 🔴 **WYSOKIE** |

### Task 32.5.2: Migracja formatu bazy (vault_format_version)

| | |
|---|---|
| **Cel** | Bezpieczna ścieżka aktualizacji z obecnego formatu metadanych na nowy schemat Envelope Encryption. |
| **Zakres** | • Dodanie pola `vault_format_version` do SQLite. |
| | • Migrator: odszyfrowanie chunków starym kluczem → wygenerowanie DEK → ponowne zaszyfrowanie → aktualizacja metadanych. |
| | • Logika wznowienia (resumable migration): jeśli laptop się wyłączy w połowie, daemon wznawia od ostatniego checkpointa. |
| | • Rollback path: możliwość powrotu do starego formatu jeśli migracja się nie powiedzie. |
| **Wynik** | Zero utraty danych przy zmianie wersji formatu. Mieszanka starego i nowego formatu jest niemożliwa. |
| **Ryzyko** | 🔴 **WYSOKIE** |

---

## 6. Faza 2: Epic 35 — The Ghost Shell

> **EPIC 35: NATIVE EXPLORER EXPERIENCE**

Integracja z powłoką Windows (`cfapi.dll`). Użytkownik klika prawym przyciskiem myszy dowolny plik, wybiera poziom ochrony, a daemon „wchłania" plik: szyfruje, tnie na chunki (EC), wysyła do chmur i zostawia widmo (placeholder 0 bajtów).

### Task 35.0: cfapi.dll — Minimalny PoC (izolacja ryzyka)

| | |
|---|---|
| **Cel** | Weryfikacja bindingów FFI Rust ↔ Windows PRZED dodaniem logiki chmurowej. Najwyższe ryzyko techniczne w całej roadmapie. |
| **Zakres** | • Czysto lokalny mechanizm: plik → placeholder → hydracja z ukrytego folderu Cache. |
| | • Walidacja progresywnego strumieniowania `CfExecute` z `CF_OPERATION_TYPE_TRANSFER_DATA`. |
| | • Test interakcji z Windows Defender (Mark of the Web na „zmaterializowanych" plikach). |
| | • Architektura: Shell Extension DLL jako cienki klient (named pipe/localhost HTTP do `angeld`). Crash DLL = crash Explorer.exe — dlatego minimum logiki w DLL. |
| **Wynik** | Go/No-Go gate: jeśli PoC nie działa stabilnie, Epic 35 wymaga alternatywnej strategii (np. ProjFS). |
| **Ryzyko** | 🔴 **WYSOKIE** |

### Task 35.1: Ingest State Machine + Erasure Coding

| | |
|---|---|
| **Cel** | Transakcyjne „wchłanianie" plików odporne na awarie z pełnym cyklem życia. |
| **Zakres** | • Stany: `PENDING → CHUNKING → UPLOADING → GHOSTED` (+ `HYDRATING` + `FAILED` z diagnostyką). |
| | • Atomowa podmiana oryginału na widmo TYLKO po pełnym potwierdzeniu WSZYSTKICH chunków. Błąd = rollback. |
| | • Graceful Degradation: jeśli 2 z 3 providerów niedostępne, szybki timeout + ikona „niedostępne" w Eksploratorze. |
| | • HYDRATING: logika retry, timeout, partial-failure handling przy odtwarzaniu widma z chmury. |
| **Wynik** | Pełny transakcyjny cykl życia pliku: od surowego pliku do widma i z powrotem, bez możliwości utraty danych. |
| **Ryzyko** | 🔴 **WYSOKIE** |

### Task 35.2: Context Menu + Shell Extension (4 Poziomy Ochrony)

| | |
|---|---|
| **Cel** | Rejestracja rozszerzenia powłoki Windows z menu kontekstowym dla 4 poziomów ochrony. |
| **Zakres** | • Menu: LOKALNIE (`LOCAL_ONLY`), COMBO (`SINGLE_REPLICA`), CHMURA (Sharding), FORTECA (`EC_2_1`). |
| | • Ikony overlay w Eksploratorze dla każdego stanu (synced, uploading, ghost, error). |
| | • Architektura cienki klient: DLL robi minimum — tylko wysyła komendy do daemona `angeld`. Cała logika po stronie `angeld`. |
| | • Obsługa stanu offline: kliknięcie widma bez sieci → ikona „niedostępne", nie zawieszenie Explorera. |
| **Wynik** | Użytkownik widzi i kontroluje politykę ochrony każdego pliku bezpośrednio z poziomu Eksploratora. |
| **Ryzyko** | 🟡 **ŚREDNIE** |

---

## 7. Faza 3: Epic 33 — Zero-Knowledge Link Sharing

> **EPIC 33: ZERO-KNOWLEDGE SHARING**

Udostępnianie plików na zewnątrz bez wysyłania kluczy deszyfrujących do serwerów OmniDrive. Klucz jest częścią fragmentu URI (`#`) i nigdy nie trafia na serwer.

### Task 33.1: Fragment-Based Cryptography

| | |
|---|---|
| **Cel** | Architektura linków opartych na DEK w fragmencie URI. |
| **Zakres** | • Format: `https://skarbiec.app/{file_id}#{DEK_key}`. |
| | • Fragment URI (`#`) jest ignorowany przez serwery HTTP — klucz pozostaje lokalny. |
| | • Decyzja projektowa: per-file DEK (jeden klucz na cały plik, niezależnie od ilości chunków EC). Dokumentacja tej decyzji na wypadek przyszłej zmiany na per-chunk DEK. |
| | • Opcjonalny TTL (czas życia linku) i jednorazowe linki (burn-after-read). |
| **Wynik** | Link, który można bezpiecznie wysłać mailem — nawet jeśli ktoś przechwyci URL, serwer nie ma klucza. |
| **Ryzyko** | 🟡 **ŚREDNIE** |

### Task 33.2: Export API + Web Receiver

| | |
|---|---|
| **Cel** | Frontend do deszyfrowania plików w przeglądarce odbiorcy (WebCrypto API). |
| **Zakres** | • JavaScript w przeglądarce odbiorcy używa WebCrypto API i klucza DEK z URL do deszyfracji. |
| | • Streaming: `ReadableStream` + `TransformStream` do progresywnego deszyfrowania dużych plików (limit RAM). |
| | • Limit rozmiaru: explicite ograniczenie lub chunked download z progresywnym deszyfrowaniem dla plików >500 MB. |
| | • UX: pasek postępu deszyfrowania + przycisk „Zapisz jako...". |
| **Wynik** | Odbiorca bez konta OmniDrive może pobrać i odszyfrować plik wyłącznie w przeglądarce. |
| **Ryzyko** | 🟡 **ŚREDNIE** |

---

## 8. Faza 4: Epic 34 — The Family Cloud

> **EPIC 34: SHARED VAULTS & IDENTITY**

Przejście z aplikacji czysto lokalnej na produkt z tożsamością, zachowując pełną separację tożsamości autentykacyjnej (OAuth) od tożsamości kryptograficznej (X25519). Serwer OmniDrive dystrybuuje zaszyfrowane bloby — nigdy nie widzi kluczy.

### Task 34.1: OAuth2 Identity Layer

| | |
|---|---|
| **Cel** | Uwierzytelnianie użytkowników przez Google OAuth, całkowicie niezależne od derywacji kluczy kryptograficznych. |
| **Zakres** | • Google Login = „to jest ten użytkownik", nie „to jest jego klucz". |
| | • Zarządzanie sesjami i JWT na poziomie panelu daemona. |
| | • Klucz prywatny X25519 jest generowany lokalnie i derivowany z własnego hasła użytkownika — NIE z tokena Google. |
| | • Przejęcie konta Google NIE daje dostępu do danych w vaulcie. |
| **Wynik** | Użytkownik loguje się wygodnie, ale klucze kryptograficzne są w pełni niezależne od dostawcy tożsamości. |
| **Ryzyko** | 🟡 **ŚREDNIE** |

### Task 34.2: Asymmetric Key Wrapping (Zero-Knowledge Handoff)

| | |
|---|---|
| **Cel** | Bezpieczne przekazywanie dostępu do Skarbca między użytkownikami bez łamania zasady Zero-Knowledge. |
| **Zakres** | • Każdy użytkownik/urządzenie generuje parę kluczy X25519 (lub ECDH P-256 dla kompatybilności z WebCrypto). |
| | • Key Wrapping: `HKDF(ECDH(sender_priv, recipient_pub)) → AES-256-KW(Vault_Key)`. |
| | • Serwer OmniDrive przechowuje i dystrybuuje wyłącznie zaszyfrowane bloby kluczy. |
| | • Vault Key nigdy nie jest transmitowany w postaci jawnej. |
| **Wynik** | Serwer jest „ślepym pośrednikiem" — przekazuje koperty, ale nie ma dostępu do zawartości. |
| **Ryzyko** | 🔴 **WYSOKIE** |

### Task 34.3: ACL, Rewokacja i Recovery

| | |
|---|---|
| **Cel** | System uprawnień oraz bezpieczna procedura usuwania dostępu i odzyskiwania w razie utraty klucza. |
| **Zakres** | • Zaproszenia: właściciel generuje wrapped Vault Key dla nowego członka. |
| | • Rewokacja: usunięcie użytkownika → automatyczna rotacja Vault Key → re-wrap TYLKO kluczy DEK. Chunki danych niezmienione. |
| | • Recovery: Shamir's Secret Sharing lub papierowe recovery keys (np. 24 słowa BIP-39) — opcjonalne, ale wyraźnie komunikowane w onboardingu. |
| | • UX: jasny komunikat „zapisz ten klucz, bo nikt Ci nie pomoże" przy tworzeniu vaultu. |
| **Wynik** | Pełny cykl życia dostępu: zaproszenie → korzystanie → usunięcie → awaryjne odzyskanie. |
| **Ryzyko** | 🔴 **WYSOKIE** |

---

## 9. Rejestr ryzyk i środki zaradcze

| Ryzyko | Poziom | Środek zaradczy |
|--------|--------|-----------------|
| cfapi.dll bindingi niestabilne | 🔴 WYSOKIE | Task 35.0 jako izolowany PoC z go/no-go gate. Fallback: ProjFS. |
| Race conditions przy Ingest | 🔴 WYSOKIE | Transakcyjny state machine z rollbackiem. Blokada pliku na czas operacji. |
| Crash Shell Extension DLL = crash Explorera | 🔴 WYSOKIE | Architektura cienki klient (DLL robi minimum). Cała logika w `angeld`. |
| Migracja formatu przerywana (utrata prądu) | 🔴 WYSOKIE | Resumable migration z checkpointami. Rollback path do starego formatu. |
| WebCrypto OOM na dużych plikach | 🟡 ŚREDNIE | `ReadableStream` + `TransformStream`. Explicite limit rozmiaru sharingu. |
| Windows Defender blokuje hydrated pliki | 🟡 ŚREDNIE | Wczesne testy MotW. Ewentualnie signature pliku placeholder. |
| Koszty chmurowe zaskakują użytkownika | 🟡 ŚREDNIE | Cloud Guard + predykcyjny budżet („ten miesiąc ~X PLN"). Alerty progowe. |
| Utrata klucza prywatnego (jedyny właściciel) | 🔴 WYSOKIE | Shamir's Secret Sharing / papierowe recovery keys. Jasny UX onboarding. |
| Timeout hydracji widma (wolny provider) | 🟡 ŚREDNIE | EC_2_1 graceful degradation + adaptacyjne timeouty per provider. |

---

## 10. Definicja ukończenia (Definition of Done)

Każdy Task jest ukończony, gdy spełnia WSZYSTKIE poniższe kryteria:

1. **Testy integracyjne:** Pełny cykl życia (Ingest → Ghost → Hydrate → Delete) przechodzi bez błędów na dwóch fizycznych urządzeniach.
2. **Graceful degradation:** Symulacja awarii 1 z 3 providerów chmurowych nie powoduje utraty danych ani zawieszenia UI.
3. **Rollback:** Każda operacja stanowa ma zdefiniowany i przetestowany rollback path.
4. **Dokumentacja:** Decyzje kryptograficzne są udokumentowane w repozytorium (nie tylko w kodzie).
5. **Security review:** Krytyczne ścieżki (key wrapping, migration, sharing) przeszły peer review przed merge'em.

---

## 11. Faza 1+2: Refaktoring infrastruktury (CI, Cleanup, ApiError, Split api.rs)

**Status: ✅ UKOŃCZONE (2026-04-09)**

Przed dodaniem nowych feature'ów (OAuth2, per-folder permissions) — solidna baza techniczna.

### Krok 1: GitHub Actions CI ✅

- `.github/workflows/ci.yml` — `windows-latest` runner
- Pipeline: `cargo check` → `cargo clippy -D warnings` → `cargo test --test-threads=1`
- Cache: `target/`, `~/.cargo/registry`, `~/.cargo/git`

### Krok 2: Dead Code Cleanup ✅

- 85 clippy warnings → 0
- `#![allow(dead_code)]` na `db.rs`, `identity.rs`, `acl.rs` (funkcje Epic 34 czekające na użycie)
- `#[allow(clippy::should_implement_trait)]` na metodach `from_str` (nie-std konwencja)
- Fix: `&(impl Error)` → `&impl Error` w `gc.rs`, `repair.rs`, `scrubber.rs`
- Fix: zbędne casty `as *mut c_void` w `pipe_server.rs`, `omnidrive-shell-ext`
- Fix: `&PathBuf` → `&Path` w `omnidrive-tray`

### Krok 3: Unified ApiError ✅

- `angeld/src/api/error.rs` — 7 wariantów (BadRequest, Unauthorized, Forbidden, NotFound, Conflict, Locked, Internal)
- `impl IntoResponse for ApiError` — ujednolicony format JSON `{ "error": code, "message": msg }`
- `impl From<sqlx::Error>` + `impl From<std::io::Error>`
- Decyzja: `acl::require_role` zachowuje `Result<_, Response>` (26 call sites, ApiError w bin crate)

### Krok 4: Split api.rs → api/ directory ✅

- 5026 linii → 8 modułów + mod.rs (211 linii)
- Zero zmian w `main.rs` (`mod api;` rozwiązuje `api/mod.rs` automatycznie)

| Moduł | Trasy | Linii |
|-------|-------|-------|
| `mod.rs` | Router assembly, ApiState, ApiServer, shared helpers | 211 |
| `error.rs` | ApiError enum + impls | 76 |
| `onboarding.rs` | 7 tras bootstrap/setup/join/complete/reset | 786 |
| `auth.rs` | unlock, session, logout, renew | 230 |
| `diagnostics.rs` | health, shell, sync-root, storage, multidevice, transfers | 682 |
| `files.rs` | files CRUD, pin/unpin, filesystem, revisions, quota | 966 |
| `sharing.rs` | create/list/revoke/delete share, public endpoints | 554 |
| `vault.rs` | invite, join, devices, revoke, rewrap, health | 975 |
| `maintenance.rs` | scrub, repair, reconcile, backup, cache, ingest | 840 |

### E2E testy — aktualizacja auth ✅

- 6 plików testowych zaktualizowanych o session tokens (Bearer auth)
- 139 testów przechodzi (60 lib + 65 bin + 14 e2e)
- 3 pre-existing failures (provider config) — niezwiązane z refaktorem

---

## 12. Pozostałe zadania (backlog)

| Priorytet | Zadanie | Zależności |
|-----------|---------|------------|
| **P0** | Fix 3 e2e test failures (provider config w reconciliation, recovery, scrubber) | — |
| **P1** | Migracja handlerów na `Result<_, ApiError>` (26 call sites w acl pattern) | Krok 3 done |
| **P1** | Epic 34.3b: OAuth2 Identity Layer (Google Login) | Epic 34.0-34.4a done |
| **P1** | Epic 34.4b: Per-folder ACL permissions | Epic 34.4a done |
| **P2** | Epic 34.3c: Recovery Keys (Shamir/BIP-39) | Epic 34.3a done |
| **P2** | Epic 34.5: Audit Trail (kto co kiedy) | Epic 34.4a done |
| **P2** | Task 35.4: IPC + Icon Overlays (shell ext ↔ daemon) | Epic 35.2b done |

---

*OmniDrive © 2026*
