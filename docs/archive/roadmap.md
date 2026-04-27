# OmniDrive (Skarbiec) — Strategiczna Roadmapa v1.0

> Ostatnia aktualizacja: 2026-04-19 | Status: **ZATWIERDZONA** (decyzje D1/D1a/D2/D3/D4/D6 podjęte 2026-04-19)
> Plan taktyczny (per-faza TODO, commity, testy): `plan.md`

Dokument strategiczny dla faz M.6 → v0.3.0 → Epic 33 Tryb B → v0.4.0 → mobile (P→Q→R→S). Zawiera decyzje architektoniczne, analizę alternatyw i całościową mapę priorytetów.

---

## Decyzje zatwierdzone (2026-04-19)

- **D1 = TAK + D1a = Cloudflare Pages** — domena `skarbiec.app` wraca do użycia **wyłącznie jako host statycznej treści** (CF Pages dla share decryptora Trybu B + landing page). Daemon dalej na `127.0.0.1` only. Auto-deploy przez Cloudflare Pages Git integration (push do `main` = deploy automatyczny). Nowe repo: `omnidrive/share-site` podpięte w dashboardzie Cloudflare Pages. Domena już jest w CF → zero dodatkowej konfiguracji DNS.
- **D2 = Hybrid** — testy E2E w Fazie N: mockito/minio w CI + manual smoke test na realnym B2/R2 przed tagiem v0.3.0 (Lenovo test user).
- **D3 = Desktop polish first** — po v0.3.0 idziemy w v0.4.0 = Epic 33 Tryb B + Faza O (quota + VFS). Mobile (P→Q→R→S) zaczyna się dopiero po v0.4.0.
- **D4 = Opcja C (read-only snapshot)** — mobile V1 czyta snapshot `index.json.enc` wygenerowany przez daemon (daemon writes, mobile reads). Tryb write (CRDT/LWW) — osobna decyzja dopiero po V1.
- **D6 = TAK** — prosty landing page na root domeny `skarbiec.app` w tle v0.3.0 (What is it / Screenshots / Download / GitHub link), deploy razem z share-site.
- **D5** — stack mobile (UniFFI vs Flutter) odłożony do konsultacji przed rozpoczęciem fazy mobilnej (po v0.4.0).

### KRYTYCZNA POPRAWKA M.6.1 (2026-04-19)

**NIE dodajemy `skarbiec.app` do CORS allowlist w lokalnym daemonie.** Daemon pozostaje głuchy na zewnętrzny internet. Uzasadnienie:
- W Trybie A (LAN Share) dekryptor jest serwowany z tego samego daemona — same-origin, brak CORS issue
- W Trybie B (Public Share) dekryptor z GH Pages czyta **bezpośrednio z B2/R2** — daemon w ogóle nie uczestniczy w downloadzie
- Dodanie publicznej domeny do CORS = attack surface (XSS na GH Pages → fetch do LAN-owego daemona z ukradzioną sesją)

**CORS allowlist = tylko loopback + prywatne zakresy LAN.** `share_cors_layer()` w `angeld/src/api/mod.rs:221-235` zostanie uproszczony (usunięcie gałęzi `b"https://skarbiec.app"`).

---

## 0. Kluczowa decyzja architektoniczna: skarbiec.app + Cloudflare Pages dla share-linków

**Pytanie:** „Czy możemy wykorzystać własną domenę skarbiec.app z Cloudflare Pages by ładniejsze linki wysyłać do udostępnionych plików?"

**Odpowiedź: TAK — to jest ARCHITEKTONICZNIE INNY scenariusz niż ten, który wcześniej odrzuciliśmy.**

### Co wcześniej odrzuciliśmy vs co zatwierdzamy teraz

| Scenariusz | Dlaczego ryzyko | Status |
|---|---|---|
| **Odrzucony:** `skarbiec.app` → Cloudflare Tunnel → **`angeld` daemon** | Daemon (Rust service, unlock state, sesje, cała logika zero-knowledge) publicznie wystawiony → attack surface, CF widzi TLS, session hijack, RCE possible | ❌ Złamanie Zero-Knowledge |
| **Zatwierdzony:** `skarbiec.app` → Cloudflare Pages → **tylko statyczny decryptor HTML** | Tylko static HTML+JS, brak serwera z sekretami. Fragment URL (DEK) nigdy nie leci w HTTP request. B2/R2 ma tylko szyfrowane chunki (Zero-Knowledge jak dziś) | ✅ Zgodne z Zero-Knowledge |

**Różnica fundamentalna:** wcześniej domena miała hostować **działający serwer** (daemon z tajemnicami). Teraz ma hostować **statyczną stronę bez tajemnic**. Cloudflare Pages jest CDN-em dla publicznego HTML-a — tam nie ma czego zhackować oprócz ewentualnej podmiany bundla (co można zmitygować). Domena jest już w Cloudflare → integracja CF Pages = jeden krok w dashboardzie.

### Zero-Knowledge check (formalny dowód)

Co widzi każda warstwa:

1. **DNS (Cloudflare DNS):** widzi tylko, że ktoś odpytuje `skarbiec.app` — nie widzi pliku, nie widzi DEK
2. **Cloudflare Pages:** widzi `GET /share/{id}` — dostarcza publiczny HTML, nie widzi `#fragment` (browser go nie wysyła), nie widzi co się potem dzieje
3. **B2/R2 (bucket użytkownika):** widzi żądania GET na encrypted chunki — jak dziś, już Zero-Knowledge
4. **Browser Boba:** parsuje `#fragment` lokalnie → DEK zostaje w pamięci JS, nigdy nie wychodzi
5. **Alice's daemon:** może być offline przez cały proces downloadu

**DEK nigdy nie opuszcza przeglądarki odbiorcy.** To jest Zero-Knowledge.

### Koszty i ryzyka tego rozwiązania

**Plusy:**
- Ładne, krótkie, markowe linki: `https://skarbiec.app/s/abc123#dek@...`
- Profesjonalny wizerunek (ważne dla adopcji)
- Odbiorca ufa domenie bardziej niż `omnidrive.github.io`
- $10-15/rok — wydatek śmieszny w skali projektu

**Minusy i mitygacje:**
- **Ryzyko:** ktoś kompromituje repo → podmienia decryptor na złośliwy → exfiltruje DEK z fragmentu
  **Mitygacja:** a) repo protected + 2FA + signed commits, b) decryptor ma **Subresource Integrity (SRI)** dla wszystkich zewnętrznych skryptów, c) opcjonalnie: publikowany SHA-256 hash decryptora w `README.md` + w GitHub Releases, użytkownik techniczny może zweryfikować
- **Ryzyko:** CF blokuje konto / domenę pod naciskiem (censorship — CF ma historię usuwania treści)
  **Mitygacja:** fallback — repo jest publiczne na GitHub, `omnidrive.github.io/share` = alternatywny hosting; stary link `skarbiec.app/s/abc` można replay'ować ze zmienionym origin, fragment zachowany
- **Operacyjne:** jeszcze jedna rzecz do utrzymania (DNS, renewal)
  **Mitygacja:** Cloudflare Registrar przy cenie wholesale + DNS → „set and forget" na 10 lat; domena i Pages w tym samym dashboardzie CF

### Macierz dopuszczonych użyć domeny

| Użycie | Decyzja |
|---|---|
| Hosting `angeld` daemona przez tunel (jak odrzucone wcześniej) | ❌ **ZABRONIONE** — złamałoby Zero-Knowledge |
| Landing page / strona marketingowa projektu | ✅ OK |
| Hosting statycznego decryptora share-linków | ✅ OK (Tryb B Epic 33) |
| Hosting statycznego decryptora przez Cloudflare Pages | ✅ OK (zatwierdzone D1a) |
| Fallback `omnidrive.github.io/share` gdyby CF odpadło | ✅ OK (Wariant awaryjny) |
| Publikacja signed release'ów (download link dla instalatora) | ✅ OK |

**Warunek nienaruszalny:** Pod domeną `skarbiec.app` **NIGDY nie uruchamia się żaden proces serwerowy z dostępem do danych użytkownika**. Tylko statyczne zasoby z Cloudflare Pages. Każde URL = static file. Zero runtime, zero sesji, zero tajemnic.

---

## 1. Analiza fazowa i decyzje techniczne

### Faza M.6 — Local-First Lock-in (P0, 1-2 dni)

Cel: zamknąć architektonicznie fakt, że **daemon nie komunikuje się z publicznym internetem**.

- **M.6.1 (CORS) — FINALNA DECYZJA:** **USUNĄĆ** `https://skarbiec.app` z allowlist w `share_cors_layer()` (`angeld/src/api/mod.rs:221-235`). Daemon słucha tylko loopback + LAN, CORS allowlist zawiera wyłącznie origins z tych zakresów. W Trybie A decryptor serwowany same-origin z daemona. W Trybie B decryptor z GH Pages rozmawia **tylko z B2/R2**, nigdy z daemonem. Aktualizować też komentarz `/// Allows cross-origin access from skarbiec.app and localhost (dev).` żeby nie wprowadzać w błąd.
- **M.6.2 (OAuth):** Dodać assertion w `config.rs` — wywalić z kodu jakąkolwiek możliwość zastąpienia `http://127.0.0.1:` przez zewnętrzny URL. Dziś `OAUTH_REDIRECT_URL` jest env-configurable → zostawić jako escape hatch dla dev, ale w release buildzie wymusić host=127.0.0.1 (compile-time check albo startup assertion).
- **M.6.3 (Docs):** Lista plików do purge `skarbiec.app`:
  - `docs/crypto-spec.md:252`
  - `angeld/src/api/mod.rs:221` (komentarz)
  - `dist/share-site/index.html:169`
  - `plan.md:336`
  - `PROJECT_STATUS.md:613`
  - `OmniDrive_Roadmap_v2.md:150`
- **M.6.4 (Sanity grep):** `rg -n 'skarbiec\.app'` + `rg -n 'public|hosted|tunnel'` → weryfikacja, że żaden nowy kod nie nazywa wariantu „public" lub „hosted" bez odwołania się do static-only policy.
- **M.6.5 (README):** Sekcja **„Architektura sieci: 100% Local-First"** — jednoznaczne wyjaśnienie dla developerów/contributorów, że daemon słucha tylko na loopback i domena (jeśli zostaje) służy tylko do static content.
- **M.6.6 (dynamic share host):** `angeld/src/api/sharing.rs:172` — zamiast hardcoded `http://localhost:8787/share/{id}#{dek}`, wygenerować link z nagłówka `Host:` requestu albo z nowego configu `OMNIDRIVE_SHARE_HOST`. Dziś share działa **tylko same-device** (bo `localhost` to u Boba `localhost` = Bob's computer, nie Alice's). Potrzebne do Trybu A (LAN).

---

### Faza N — Stabilizacja v0.3.0 (P0, 2-3 dni)

- **N.1 (Dead code):** `#[allow(dead_code)]` na poziomie funkcji/modułu musi mieć **komentarz** `// reserved for Epic X` — bez tego dead-code audit w przyszłości straci kontekst.
- **N.2 (E2E) — D2 Hybrid:**
  - Unit/integration: mockito (istniejący w dev-deps) dla S3 API roundtrip
  - Manual smoke na real B2/R2 przed tagiem v0.3.0 (Lenovo): unlock → create file → observe encrypted chunk upload → lock → unlock → read back
- **N.3 (M.5 validation) — ZATWIERDZONE:** Manualny test cross-device zgodności Identicon + mnemonik BIP-39 między Lenovo a Dellem:
  1. Na Lenovo: unlock vault → zapisz SVG identicon + 12-słowny mnemonik z ekranu Bezpieczeństwo
  2. Na Dellu: `Join Existing Vault` z tym samym vault keyem → unlock
  3. Assert: ten sam SVG (byte-identyczny, bo `fingerprint()` jest deterministyczne) + ten sam mnemonik
  4. Wynik testu trafia do `CHANGELOG.md` v0.3.0 jako potwierdzenie M.5
- **N.4 (Release artifacts):**
  - Kopia binarek do `dist/installer/payload/` (CLAUDE.md rule — kategoryczne!)
  - Synchronizacja wersji we wszystkich `Cargo.toml` + `installer/omnidrive.iss`
  - Git tag `v0.3.0` + push
  - `CHANGELOG.md` wpis dla v0.3.0
- **N.5 (SHA-256 publikacja):** SHA-256 sum instalatora opublikowany w GitHub Releases + w `README.md` — element **trust** dla aplikacji bezpieczeństwa.

---

### Faza O — Architektura VFS (rozbita na O.1 + O.2+)

| Punkt | Realny zakres | Priorytet |
|---|---|---|
| **O.1 Quota fix** | ~1 dzień | P1 (po N, przed v0.3.1 lub w v0.3.0) |
| **O.2+ VFS Trait** | 2-4 tygodnie | P2 (enabler dla mobile Fazy P-R) |
| **O.3 Cache management** | Częściowo już w `smart_sync.rs` | backlog |

- **Faza O.1 — Quota Fix (standalone, ~1 dzień):** Poprawić raportowanie pojemności dysku O: z C: na faktyczny cloud quota (B2/R2).
- **Faza O.2+ — Cross-Platform VFS Foundation (osobny epic, ~2-4 tyg):** Trait `FileSystemAdapter`, refactor `cfapi` do implementacji traitu, prototyp FUSE adaptera dla Linux/macOS. ENABLER dla Fazy P-R.

---

### Epic 33 — System Udostępniania Zero-Knowledge

Dwa tryby: A (LAN) + B (public).

#### Tryb A — LAN Share (w v0.3.0)

- Zmiana: `sharing.rs:172` generuje link z dynamicznym hostem (Host header lub `OMNIDRIVE_SHARE_HOST`)
- UI: toggle „Link do mojego kompa (127.0.0.1)" vs „Link do mojej sieci LAN (192.168.x.x)"
- Ostrzeżenie w UI: „Odbiorca musi być w tej samej sieci Wi-Fi / LAN"
- Dla LAN: daemon musi słuchać na `0.0.0.0:8787` → dodać opcję w trayu „Udostępnij sieci lokalnej" (ON/OFF) — domyślnie OFF (bezpieczniej)

#### Tryb B — Public Share (osobny epic, 33.2)

**Architektura:**

```
Alice's angeld                           Cloudflare Pages / skarbiec.app
  ├─ genruje share_id, DEK                  └─ static decryptor (share/index.html)
  ├─ upload manifest.json do B2
  ├─ upload encrypted chunków do B2         B2/R2 (chmura Alice)
  ├─ presigned URLs z TTL 7 dni             └─ manifest.json + chunk-{i}.bin (encrypted)
  └─ zwraca link:
     https://skarbiec.app/s/{id}#{dek}@{b2_base}
        ↓
  Bob klika link
        ↓
  Browser Boba
     ├─ GET skarbiec.app/s/{id} → static HTML z CF Pages
     ├─ JS parsuje #fragment → DEK + B2 URL
     ├─ GET {b2_base}/manifest.json
     ├─ GET {b2_base}/chunk-{i}.bin (x N)
     ├─ WebCrypto decrypt (DEK, AES-GCM)
     └─ File stream → download
```

**Kluczowe decyzje implementacyjne Epic 33.2:**

1. **Upload strategii:** Opcja a) Duplicate encrypted chunków pod publicznym prefixem (`shares/{share_id}/chunk-*.bin`) — łatwe revocation przez delete prefix, ×2 storage. **Rekomendacja: Opcja a)** dla prostoty i czystego revocation.
2. **Password protection client-side:** Daemon szyfruje `wrapped_dek = AES-KW(dek, PBKDF2(password, salt))` → manifest zawiera `wrapped_dek` + `salt` zamiast direct DEK. Link: `https://skarbiec.app/s/{id}#{salt}@{b2_base}`. Bob wpisuje hasło → PBKDF2(hasło, salt) → unwrap DEK → download.
3. **Revocation:** UI przycisk „Wycofaj link" → daemon DELETE prefix w B2 → Bob dostaje 404. Daemon trzyma mapping share_id → [object keys] w SQLite.
4. **Expiration:** Daemon przy tworzeniu ustawia `Object Lock` TTL albo periodic cleanup. Manifest zawiera `expires_at` (JS sprawdza przed fetchem, pokazuje komunikat zamiast błędu CORS).
5. **Audit:** Bob może opcjonalnie wysłać beacon do Alice (jeśli Alice online) → `share_downloaded` event w audit log. Default: OFF (Zero-Knowledge respect). Opt-in: UI toggle „Chcę wiedzieć kiedy odbiorca pobierze".

**Wymagania dla `share-site` repo (osobny projekt):**
- Struktura: `index.html` + `decryptor.js` + `style.css` (minimalne)
- CI/CD: Cloudflare Pages Git integration — push do `main` → deploy automatyczny (bez GitHub Actions, CF sam buduje)
- Custom domain: `skarbiec.app` podpięte w CF Pages dashboard (domena już w CF → zero dodatkowej konfiguracji DNS)
- Fallback URL: `omnidrive.github.io/share` (repo jest publiczne → GH Pages as backup)
- Bundling: wszystko inline w jednym HTML (bez external JS) → SRI dla ewentualnych dependencies
- Testy: headless browser test z przykładowym encrypted manifest

**Szacunek Epic 33.2:** 2-3 tygodnie.

---

### Faza P — Core Extraction (1-2 tyg)

- `omnidrive-core/` **już istnieje** jako osobny crate w workspace — ma crypto (AES-KW, Argon2, chunk format)
- **P.1 (Restrukturyzacja):** audit listy: co z `angeld/src/*.rs` jest „pure logic" (do `omnidrive-core`) vs „platform glue" (zostaje w `angeld`). Kandydaci do shared core:
  - `packer.rs`, `downloader.rs` (encryption flow)
  - `vault.rs` (po wyjęciu SQLite glue)
  - Część `db.rs` (schema definitions, types — nie connections)
  - S3 client abstrakcja
- **P.2 (index.json.enc) — D4 Opcja C ZATWIERDZONA:** Hybrid — SQLite pozostaje truth, `index.json.enc` generowane przy każdym sync jako **read-only snapshot** dla mobile (daemon pisze, mobile czyta). Mobile write = osobna decyzja (CRDT? LWW?) dopiero po V1 (prawdopodobnie S).
- **P.3 (DB portability):** SQLite działa na iOS/Android bez problemu. `sqlx` kompiluje się. Brak blokera.

---

### Faza Q — Mobile Bridge (2 tyg)

**D5 do konsultacji przed startem fazy mobilnej.**

| Podejście | Plusy | Minusy |
|---|---|---|
| **UniFFI (Mozilla)** | Native Swift+Kotlin bindings, highest quality | Duplikuje UI — dwa natywne codebases, dłuższy time-to-market |
| **Flutter + flutter_rust_bridge** | Jeden codebase Dart/Flutter, szybkie prototypowanie | Ciężki runtime Dart, trudniejsza integracja z File Provider/SAF |
| **React Native + Rust FFI** | Duży talent pool, szybki rozwój | Perf niższa niż native, JS runtime = attack surface |
| **Tauri Mobile (eksperymentalne)** | Ten sam stack co desktop | Niestabilne, młody projekt |

**Rekomendacja:** Dla V1 Read-Only: **UniFFI** (native quality = lepsze dla security app). Pytanie strategiczne: czy mobile jest priorytetem w ogóle (Cryptomator latami tylko desktop).

---

### Faza R — Mobile V1 Read-Only (3-4 tyg)

- **R.2 (QR Handshake) — Opcja C rekomendowana:** Osobna tożsamość urządzenia mobilnego (jak multi-device join desktop-only) + własny key wrapped przez vault key Alice → mobile ma self-contained dostęp bez daemona, revokable przez Alice.
- **R.3 (Deszyfrowanie w locie):** Stream decrypt + progressive download (już mamy chunks-based decryption w packer/downloader).
- **R.4 (Biometrics + session):** Face ID / Touch ID dla lokalnej session → pieczęć na pamięci klucza. Platform-specific (Keychain iOS, KeyStore Android).
- **R.5 (Offline handling):** Mobile często bez sieci. Fetched pliki zostają w encrypted cache → możliwość „Pin file" (jak desktop cfapi).

---

### Faza S — Mobile V2 Read-Write (4-6 tyg)

- **S.1 (Upload z telefonu):** Photo/Video upload = killer feature. Zero-knowledge alternatywa dla iCloud/Google Photos. Wymaga:
  - Background upload (iOS BGTaskScheduler, Android WorkManager)
  - Resumable uploads (S3 multipart)
  - Chunked encryption w locie (nie trzymać całego filmu w RAM)
- **S.2 (File Provider / SAF):** iOS File Provider Extension + Android Storage Access Framework. Spójna UX z desktop cfapi.
- **S.3 (Conflict resolution) — najtrudniejsza część roadmapu:**
  - Opcja a) Last-Write-Wins (proste, data loss)
  - Opcja b) Per-file lock (complex)
  - Opcja c) Version branching (`file (Conflict from iPhone).pdf`)
  - **Rekomendacja:** Opcja c) dla V2, opcja a) dla V1 z ostrzeżeniem w UI

---

## 2. Całościowa mapa priorytetów

| Faza | Zakres | Szacunek | Priorytet |
|------|--------|----------|-----------|
| **M.6** | Local-First Lock-in + przekwalifikowanie domeny | 1-2 dni | P0 |
| **N** | Stabilizacja v0.3.0 (hybrid E2E, cross-device Identicon test) | 2-3 dni | P0 |
| **O.1** | Quota Fix (standalone) | 1 dzień | P1 |
| **Epic 33 Tryb A** | LAN Share (dopięcie dynamic host) | 0.5 dnia | P1 |
| **O.2+** | Cross-Platform VFS Foundation | 2-4 tyg | P2 |
| **Epic 33 Tryb B** | Public Share + CF Pages (skarbiec.app/s) | 2-3 tyg | P2 |
| **P** | Core Extraction (audit + selektywny move + snapshot) | 1-2 tyg | P2 |
| **Q** | Mobile Bridge (UniFFI) | 2 tyg | P3 |
| **R** | Mobile V1 Read-Only | 3-4 tyg | P3 |
| **S** | Mobile V2 Read-Write | 4-6 tyg | P4 |

**Łączny czas do pełnego ekosystemu:** 4-6 miesięcy soft (fulltime). Desktop v0.3.0: ~1 tydzień.

---

## 3. Analiza alternatyw dla statycznego decryptora (D1a)

| Opcja | Koszt/yr | Setup | Maintenance | Censorship | UX | Ocena |
|---|---|---|---|---|---|---|
| **Cloudflare Pages + custom domain** | 0 | Trywialny | Zero | Niska (CF terminacje historycznie rzadkie dla narzędzi) | ✅ | ⭐ **ZATWIERDZONE (D1a)** |
| GitHub Pages + custom domain | 0 | Trywialny | Zero | Średnia (DMCA) | ✅ | Fallback jeśli CF odpadnie |
| Self-host VPS (Hetzner/OVH) | $36-60 | Średni | Regularne OS updates | Wysoka | ✅ | Overkill |
| Installer bundle `file://` | 0 | Trywialny | Zero | Najwyższa | ❌ (Chrome blokuje crypto.subtle) | Tylko jako fallback lokalny |
| R2 public bucket + custom domain | ~$2-5 | Średni | Zero | Niska-średnia | ✅ | Elegant jeśli i tak R2 |

**Uzasadnienie CF Pages (D1a = ZATWIERDZONE):**
1. Zero kosztu poza domeną
2. Domena `skarbiec.app` już w Cloudflare → custom domain = jeden klik w dashboardzie CF Pages, zero dodatkowej konfiguracji DNS
3. Push do `main` = auto-deploy (CF Pages Git integration, bez GitHub Actions workflow)
4. Edge CDN globalny → szybsze ładowanie decryptora dla Boba niezależnie od lokalizacji
5. HTTPS automatyczny (CF zarządza)
6. Repo jest publiczne na GitHub → fallback `omnidrive.github.io/share` działa bez zmian gdyby CF odpadło
7. Jeden dashboard (CF) dla domeny + DNS + Pages + R2 → zero vendor switching

**Proponowana struktura repo `omnidrive/share-site`:**
```
share-site/
├── index.html      # decryptor (port z dist/share-site/index.html)
├── README.md       # SHA-256 hash decryptora dla weryfikacji + instrukcje self-host
└── (brak .github/workflows/ — CF Pages buduje bezpośrednio z repo)
```

**Ryzyko podmiany decryptora → mitygacje:**
- 2FA + signed commits na repo (CF Pages deployuje tylko z chronionego brancha)
- Branch protection (main tylko przez PR + review)
- SHA-256 decryptora publikowany w `README.md` + w GitHub Releases + w UI Skarbca → użytkownik techniczny może verify (`curl skarbiec.app/index.html | sha256sum`)
- Każdy potencjalny user może fork'ować repo i self-host'ować swój decryptor na własnym CF Pages / GH Pages

---

## 4. Najbliższa sesja — co robimy teraz?

1. **Implementacja M.6** (1-2 dni) — CORS cleanup, OAuth loopback assertion, docs purge, README, dynamic share host
2. **Epic 33 Tryb A dopięcie** (0.5 dnia, w M.6.6) — dynamic host w generowaniu linku
3. **Implementacja N** (2-3 dni) — dead code, hybrid E2E, cross-device Identicon test, release v0.3.0
4. **Implementacja O.1** (1 dzień) — quota fix (opcjonalnie przed v0.3.0 lub jako 0.3.1 hotfix)
5. **Release v0.3.0** — z landing page na skarbiec.app jako cherry on top
6. **Po v0.3.0:** Epic 33 Tryb B + Faza O.2+ → v0.4.0 (Desktop polish first, D3 zatwierdzone)

---

## 5. Świadome pominięcia

- **Epic 34 backlog (THREAT_MODEL, E2E multi-user):** dług techniczny po Fazie M — warto przypomnieć, ale nie priorytet przed v0.3.0
- **Bezpieczeństwo operacyjne (auditing, pen-testing, formal review):** warte osobnego tracka, ale to etap „po v1.0"
- **Płatności / monetyzacja:** OmniDrive jest projektem osobistym
- **i18n / l10n:** UI jest dziś po polsku, przyszłość angielska / inne — niepriorytetowe
- **Accessibility (a11y):** nigdzie nie pokryte — warto dodać jako P4 backlog
