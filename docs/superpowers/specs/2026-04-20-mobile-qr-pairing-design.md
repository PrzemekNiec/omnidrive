# OmniDrive Mobile — QR Pairing Spec + Fazy P/Q/R/S

**Data:** 2026-04-20  
**Sesja:** Architektoniczna Mobile  
**Status:** Zatwierdzona  
**Autorzy:** Claude + Przemek  

---

## 1. Cel

Zdefiniować:
1. Bezpieczny protokół parowania urządzenia mobilnego z istniejącym Skarbcem (QR Pairing Mini-Spec).
2. Zakres i kolejność Faz P → Q → R → S dla aplikacji mobilnej OmniDrive.

---

## 2. Zatwierdzone decyzje architektoniczne

Poniższe decyzje nie podlegają zmianie bez osobnej sesji architektonicznej:

| Decyzja | Wartość |
|---------|---------|
| Platforma pierwsza | Android (Kotlin + Jetpack Compose, `aarch64-linux-android`) |
| iOS | Bindingi Swift w przyszłości — Rust core musi być platform-agnostic |
| Fundament | Faza P (UniFFI Core Extraction) musi poprzedzać Q/R/S |
| QR Pairing | RAW Vault Key w QR = **niedopuszczalne**; tylko Opcja C (ECDH + SAS) |
| Przechowywanie klucza | Android Keystore (hardware-backed), biometria w Fazie R |
| Snapshot | SQLite-based (istniejący mechanizm), NIE index.json.enc |
| Anti-features | Brak tworzenia Skarbca na mobile, brak pełnego background sync |

---

## 3. QR Pairing Mini-Spec — Opcja C: ECDH + SAS

### 3.1 Przegląd

Signal-style pairing: ECDH bez potrzeby zewnętrznej biblioteki PAKE; ochrona przed MITM przez SAS (Short Authentication String) weryfikowany wizualnie przez użytkownika na obu ekranach.

### 3.2 Payload QR kodu

```
omnidrive://pair?
  vault_id=<UUID>
  &host=<daemon_ip>:<port>           // LAN-only, nigdy publiczny URL
  &dpk=<base64url(desktop_x25519_pubkey)>
  &nonce=<base64url(16B random)>
  &exp=<unix_epoch_seconds>          // expiry: now + 300 (5 minut)
  &v=1                               // wersja protokołu
```

**Wymagania payload:**
- `nonce` — jednorazowy, 16 bajtów losowych (`OsRng`)
- `exp` — QR ważny 5 minut; daemon odrzuca po upływie
- `dpk` — efemeryczny klucz X25519 wygenerowany dla tej sesji parowania; po użyciu (lub timeout) natychmiast niszczony

### 3.3 Protokół krok po kroku

```
DESKTOP                                    MOBILE
-------                                    ------
1. Generuj efemeryczną parę X25519
   (desktop_priv, desktop_pub)
   Generuj nonce (16B, OsRng)
   Zapisz (desktop_priv, nonce, exp) in-memory
   Pokaż QR z payload (3.2)
   Pokaż spinner "Czekam na telefon…"

                                    2. Skanuj QR
                                       Parsuj payload, sprawdź exp > now
                                       Generuj efemeryczną parę X25519
                                       (mobile_priv, mobile_pub)

                                    3. POST /api/mobile/pair-init
                                       { mobile_pub, nonce }

4. Sprawdź nonce (musi pasować, jednorazowy)
   Sprawdź exp > now
   ECDH(desktop_priv, mobile_pub) → session_key (X25519 shared secret)
   Zniszcz desktop_priv
   SAS = SHA-256(session_key || nonce)[0..2] → u16 → format "%04d"

                                    5. ECDH(mobile_priv, desktop_pub) → session_key
                                       Zniszcz mobile_priv
                                       SAS = SHA-256(session_key || nonce)[0..2] → u16 → format "%04d"

6. Daemon pokazuje SAS na ekranie UI (Web Dashboard)
   "Kod parowania: XXXX — czy zgadza się z telefonem?"
   Czeka na POST /api/mobile/pair-confirm lub timeout 60s

                                    7. Aplikacja pokazuje SAS na ekranie
                                       Użytkownik wizualnie porównuje kody
                                       Tap "Potwierdź" → POST /api/mobile/pair-confirm
                                       { nonce, confirm: true }

8. Daemon: nonce match + confirm=true
   Wygeneruj device_id (UUID v4) dla mobile
   Opakowuj Vault Key session_key'em:
     wrapped_vk = AES-256-KW(session_key, vault_key)
   Wyślij odpowiedź:
     { device_id, wrapped_vk: base64url, vault_key_generation }
   Zapisz device w tabeli devices (status: paired)
   Zniszcz session_key po stronie daemona

                                    9. Odbierz wrapped_vk
                                       Unwrap: vault_key = AES-256-KW-Unwrap(session_key, wrapped_vk)
                                       Zniszcz session_key
                                       Generuj hardware-backed AES-256 key w Android Keystore
                                         (alias: "omnidrive_vk_<device_id>")
                                       Re-wrap: keystore_wrapped_vk = Keystore.wrap(vault_key)
                                       Zniszcz vault_key z pamięci (zeroize)
                                       Zapisz keystore_wrapped_vk + device_id + vault_key_generation
                                         do lokalnego SQLite apki
                                       Pokaż ekran "Parowanie zakończone!"
```

### 3.4 Bezpieczeństwo

| Właściwość | Mechanizm |
|-----------|-----------|
| Forward secrecy | Efemeryczne klucze X25519 — niszczone po wymianie |
| MITM ochrona | SAS weryfikowany wizualnie przez użytkownika |
| Replay protection | Jednorazowy nonce + expiry 5 minut |
| QR przechwycony | Bezwartościowy bez mobile_pub (atakujący musi być na LAN i połączyć się pierwszy) |
| Vault Key w QR | Nigdy — VK opuszcza daemon tylko jako wrapped (AES-KW) |
| Klucz na mobile | Wrappowany przez Android Keystore (hardware-backed); w Fazie R: wymaga biometrii |

### 3.5 Revocation

Desktop może unieważnić device z panelu "Multi-Device":
1. Usuń rekord z tabeli `devices` (lub ustaw `status = revoked`).
2. Przy następnej rotacji Vault Key — stare `wrapped_vk` na telefonie przestaje działać (klucz zmieniony).
3. Mobile nie może odtworzyć Vault Key bez ponownego parowania.

> Natychmiastowe unieważnienie (bez rotacji VK) jest możliwe tylko przez rotację klucza. Opcjonalne rozszerzenie: token revocation list w API (poza zakresem Fazy Q).

### 3.6 Nowe endpointy daemona

| Endpoint | Metoda | Opis |
|----------|--------|------|
| `/api/mobile/pair-start` | `GET` | Generuje payload QR, zwraca JSON + wyświetla QR w UI |
| `/api/mobile/pair-init` | `POST` | Mobile przesyła `mobile_pub` + `nonce`; daemon oblicza SAS |
| `/api/mobile/pair-confirm` | `POST` | Mobile potwierdza SAS; daemon wysyła `wrapped_vk` |
| `/api/mobile/pair-cancel` | `POST` | Anuluj aktywną sesję parowania |

### 3.7 Nowe tabele DB

```sql
-- Rozszerzenie tabeli devices (lazy migration przez ensure_column_exists)
ALTER TABLE devices ADD COLUMN platform TEXT;           -- 'android', 'ios', future
ALTER TABLE devices ADD COLUMN paired_at INTEGER;       -- epoch sekund
ALTER TABLE devices ADD COLUMN pairing_status TEXT DEFAULT 'active'; -- 'active', 'revoked'
ALTER TABLE devices ADD COLUMN vault_key_generation INTEGER; -- generacja VK w momencie parowania
```

---

## 4. Szkielet Faz P → Q → R → S

### 4.1 Faza P — Core Extraction (UniFFI)

**Cel:** Wyeksponować `omnidrive-core` jako bibliotekę Rust możliwą do użycia z Kotlina przez UniFFI. To absolutny fundament — bez P nie ma Q/R/S.

**Zakres:**
- Konfiguracja Android NDK + toolchain `aarch64-linux-android` (i `x86_64-linux-android` na emulator)
- `uniffi` dependency w `omnidrive-core/Cargo.toml`
- UDL / proc-macro definition eksponujące typy kryptograficzne:
  - `decrypt_chunk(wrapped_dek, vault_key, ciphertext, nonce) -> Vec<u8>`
  - `verify_vault_identity(vault_id, safety_numbers_input) -> bool` (opcjonalnie)
- Build script → `libomni_core.so` dla `aarch64-linux-android`
- Generowanie bindingów Kotlin (`uniffi-bindgen generate`)
- Smoke test: wywołanie `decrypt_chunk` z Kotlina w unit teście (bez UI)

**Czas:** ~2-3 dni

**Nie wchodzi w P:** UI, networking, Keystore, parowanie.

---

### 4.2 Faza Q — Mobile Bridge & Handshake

**Cel:** Zbudować szkielet aplikacji Android + bezpieczne parowanie z Skarbcem przez QR.

**Zakres:**

**Q.1 — Android App Skeleton:**
- Nowy projekt Android (Kotlin + Jetpack Compose, `minSdkVersion 26` / Android 8.0)
- Podpięcie `libomni_core.so` + Kotlin bindingów z Fazy P
- Struktura nawigacji: Compose NavHost z ekranami: `Onboarding`, `Pairing`, `PairingConfirm`, `Home` (placeholder)
- Android Keystore setup — klucz AES-256 `omnidrive_vk_<device_id>` tworzony przy pierwszym parowaniu

**Q.2 — QR Scanning:**
- Integracja ML Kit Barcode Scanning (Google, bez własnego modelu)
- Parsowanie `omnidrive://pair?...` URL-a z QR
- Walidacja: `exp > now`, `v == 1`, obecność `host`, `dpk`, `nonce`

**Q.3 — ECDH + SAS Handshake (protokół z sekcji 3.3):**
- Strona Desktop: nowe endpointy `/api/mobile/pair-*` w `angeld/src/api/mobile_pairing.rs`
- Strona Desktop: UI w Web Dashboard (widok "Multi-Device" → przycisk "Sparuj telefon" → modal z QR)
- Strona Mobile: Kotlin coroutine do ECDH (library: `tink-android` lub `conscrypt`) + HTTP do daemona
- Strona Mobile: ekran SAS — duże cyfry, przyciski "Potwierdź" / "Anuluj"
- Strona Desktop: wyświetlenie SAS w modalu Web UI, czekanie na potwierdzenie

**Q.4 — Vault Key Storage:**
- Po udanym parowaniu: zapis `keystore_wrapped_vk` + metadane do lokalnego SQLite apki (`omnidrive_local.db`)
- Weryfikacja: po restarcie apki klucz jest odtwarzalny

**Czas:** ~5-7 dni

---

### 4.3 Faza R — Read-Only Vault Browser (V1)

**Cel:** Użytkownik może przeglądać i otwierać pliki ze Skarbca na telefonie. Odszyfrowanie w pamięci — nic nie zapisywane do storage telefonu (chyba że pinning).

**Zakres:**

**R.1 — BiometricPrompt Unlock:**
- Android Keystore key `omnidrive_vk_*` wymaga `BiometricPrompt` przed użyciem
- `BiometricManager.canAuthenticate()` — fallback do PIN urządzenia (klasa 2)
- Vault Key dostępny w pamięci tylko przez czas sesji (zeroize po 5 min inaktywności)

**R.2 — SQLite Snapshot:**
- `GET /api/snapshot/latest` z daemona → pobierz `omnidrive.db` snapshot (istniejący mechanizm)
- Zapisz lokalnie jako `vault_snapshot.db` (zaszyfrowana przez SQLCipher lub przechowywana w internal storage)
- SQLite reader w Kotlinie przez SQLDelight lub Room (odczyt katalogu plików)

**R.3 — File Browser UI:**
- Jetpack Compose: lista katalogów i plików z `inode` tabeli snapshotu
- Nawigacja przez katalogi
- Metadane: nazwa, rozmiar, data modyfikacji, typ (ikona)
- Wyszukiwarka (client-side po snapshie)

**R.4 — Streaming Decrypt:**
- Tap na plik → `GET /api/files/{inode_id}/chunks` z daemona (LAN)
- Daemon serwuje zaszyfrowane chunki
- Mobile: pobierz chunk → `omnidrive_core::decrypt_chunk(dek, vault_key, ...)` → bufor w pamięci
- Otwórz przez Intent (ShareSheet) do odpowiedniej apki
- Limit: pliki > 50 MB wymagają ostrzeżenia (streaming, nie cache)

**R.5 — Offline Pinning (opcjonalnie):**
- Long-press → "Przypnij offline" → cache deszyfrowanego pliku w `files_dir` (internal storage)
- Wskaźnik dostępności offline w przeglądarce plików

**Czas:** ~7-10 dni

---

### 4.4 Faza S — Read-Write (V2)

**Cel:** Użytkownik może wysyłać pliki z telefonu do Skarbca oraz generować share-linki.

**Zakres:**

**S.1 — Inbox Upload:**
- Folder `Inbox/` w Skarbcu (dedykowany, nie koliduje z indeksem desktop)
- Android ShareSheet: "Udostępnij do OmniDrive" → `POST /api/mobile/inbox/upload`
- Daemon: przyjmuje plik, szyfruje V2 (nowy DEK, wrap VK), wstawia do `inode` i kolejkuje upload do B2/R2
- Mobile: progress indicator, potwierdzenie po zakończeniu

**S.2 — Share Links (Epic 33 Tryb B):**
- Tap na plik → "Utwórz link" → `POST /api/sharing/create`
- Daemon generuje `share_id` + DEK w URL fragment
- Mobile otwiera link w przeglądarce lub kopiuje do schowka
- Pełna kompatybilność z `skarbiec.app/s/<share_id>#<DEK>`

**S.3 — Camera Upload (opcjonalnie, V2 post-launch):**
- WorkManager job: zdjęcia z ostatnich 24h → auto-upload do `Inbox/Camera/`
- Wymaga osobnej zgody użytkownika (READ_MEDIA_IMAGES)

**Czas:** ~5-7 dni

---

## 5. Zależności między fazami

```
Faza P (UniFFI Core)
    └──→ Faza Q (Android Skeleton + QR Pairing)
              └──→ Faza R (Read-Only)
                        └──→ Faza S (Read-Write)
```

Faza N (v0.3.0 release) nie blokuje P/Q/R/S — może być rozwijana równolegle przez drugą osobę, ale QR Pairing (Q.3) wymaga zmian w daemonie (nowe endpointy), co powinno być mergowane do `main` przed implementacją Fazy R.

---

## 6. Anti-features (nigdy na mobile)

| Feature | Powód wykluczenia |
|---------|-------------------|
| Tworzenie nowego Skarbca | Wymaga pełnej konfiguracji B2/R2 + argon2 setup — tylko desktop |
| Pełny background sync | Zużycie baterii + złożoność konfliktu z indeksem desktop |
| Zmiana hasła / rotacja VK | Operacja administracyjna — tylko desktop z Web Dashboard |
| Zaawansowane ustawienia (GC, repair) | Poza zakresem V1/V2 mobile |
| SAF/FileProvider (Files app integration) | V3 — daleka przyszłość |

---

## 7. Krytyczne pliki (Fazy P-Q)

| Plik | Faza | Rola |
|------|------|------|
| `omnidrive-core/src/lib.rs` | P | Eksport UDL/UniFFI interface |
| `omnidrive-core/Cargo.toml` | P | Dependency `uniffi` |
| `angeld/src/api/mobile_pairing.rs` | Q | Endpointy `/api/mobile/pair-*` |
| `angeld/src/db.rs` | Q | Migracja `devices` tabeli (platform, paired_at, pairing_status) |
| `angeld/static/index.html` | Q | UI: przycisk "Sparuj telefon" + modal QR + SAS display |
| `mobile-android/` *(nowy katalog)* | Q | Projekt Android Kotlin |
