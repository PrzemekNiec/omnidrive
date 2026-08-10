# SMOKE β.a — Cross-device E2E (Dell ↔ Lenovo) — v0.3.28

> Weryfikuje wszystkie P1 z Dell smoke v0.3.23 naraz: **P1-001** (hydration/graft), **P1-002** (fetch worker), **P1-003/004** (cloud redundancy). Live smoke = osobna akceptacja operacyjna; NIE bramkuje DONE kodu.
>
> **Admin:** daemon z `target/release` + przeglądarka = zwykły PowerShell (NIE admin). Instalator na Dellu = zwykły user (`PrivilegesRequired=lowest`). Web UI: `http://127.0.0.1:8787`. `curl.exe` (nie alias PS).

---

## ⬛ PREREQ

| # | Gdzie | Co | Admin |
|---|---|---|---|
| P1 | **Scaleway konsola** | ✅ ZROBIONE — klucz ma FullAccess (P1-003 odblokowane). | — |
| P2 | **Lenovo PS** | `cd C:\Users\Przemek\Desktop\aplikacje\omnidrive` ; `cargo build --release --workspace` (v0.3.28) | NIE |
| P3 | **Dell** | Zainstaluj **OmniDrive-Setup-0.3.28.exe** (z `dist\installer\output\` na Lenovo — skopiuj na Dell pendrivem/siecią, uruchom dwuklikiem). Instalator ubije stary daemon, podmieni binarki (angeld+tray+cli), doda autostart. | NIE |
| P4 | obie | Lenovo i Dell w tej samej LAN. | — |

---

## 🟦 A — Lenovo: źródłowy vault + plik testowy

| # | Gdzie | Komenda / akcja | Admin |
|---|---|---|---|
| A1 | LENOVO PS | `cd C:\Users\Przemek\Desktop\aplikacje\omnidrive` ; `.\target\release\angeld.exe` (zostaw otwarte) | NIE |
| A2 | LENOVO przeglądarka | `http://127.0.0.1:8787` → **odblokuj** vault hasłem | NIE |
| A3 | LENOVO PS (2. okno) | `curl.exe -s http://127.0.0.1:8787/api/health` → `{"status":"ok"...}` | NIE |
| A4 | LENOVO PS | `"smoke betaA $(Get-Date -Format o)" | Out-File O:\smoke-betaA.txt` ; `Get-FileHash -Algorithm SHA256 O:\smoke-betaA.txt` → **zapisz HASH** | NIE |
| A5 | LENOVO przeglądarka | **Diagnostyka** → poczekaj aż `smoke-betaA.txt` = zsynchronizowany (chunk uploaded). **Multi-Device** → zanotuj **safety numbers**. | NIE |

---

## 🟩 B — Dell: dołączenie + Hydration (P1-001 / P1-005)

| # | Gdzie | Komenda / akcja | Admin |
|---|---|---|---|
| B1 | DELL | Daemon wstaje sam po instalacji (autostart). Jeśli nie: Menu Start → OmniDrive Daemon. | NIE |
| B2 | DELL przeglądarka | `http://127.0.0.1:8787` → **„Join Existing Vault"** → hasło. Dell pobiera snapshot z chmury + graft (α.C.b). | NIE |
| B3 | DELL przeglądarka | Po join **odblokuj**. Multi-Device → **safety numbers MUSZĄ = A5** (P1-005). | NIE |
| B4 | DELL | Otwórz `O:\smoke-betaA.txt` (Eksplorator/Notatnik) → wymusza hydration z chmury + grafted DEK. | NIE |
| B5 | DELL PS | `Get-FileHash -Algorithm SHA256 O:\smoke-betaA.txt` → **HASH MUSI = A4** (P1-001). `aes-gcm operation failed` w logu = FAIL. | NIE |

---

## 🟧 C — Redundancja snapshotu (P1-003 / P1-004)

| # | Gdzie | Komenda / akcja | Admin |
|---|---|---|---|
| C1 | DELL przeglądarka | **Diagnostyka** → „Wykonaj backup teraz" *(API: `POST /api/metadata-backup/backup-now`)* | NIE |
| C2 | DELL przeglądarka | Diagnostyka → metadata-backup status → **B2=COMPLETED, R2=COMPLETED (P1-004, brak ConnReset), Scaleway=COMPLETED (P1-003 — IAM odblokowany)** | NIE |
| C3 | DELL PS (log) | Brak `ConnectionReset (os error 10054)`. Jeśli Scaleway 403 → log: „IAM/bucket-policy denial...". | NIE |
| ✅ DoD | — | **3/3 zielone** (po FullAccess Scaleway). Min. akceptowalne = 2/3. | — |

---

## 🟪 D — Lenovo zauważa Della (P1-002 Fetch Worker)

> ⚡ **Natychmiastowo** dzięki nowemu triggerowi (`POST /api/metadata-backup/fetch-now` wpięty pod przycisk Odśwież) — bez czekania 1h.

| # | Gdzie | Komenda / akcja | Admin |
|---|---|---|---|
| D1 | (warunek) | Dell po B2 i C1 = jego device + roster są w chmurze. Lenovo (A1) działa + odblokowane. | NIE |
| D2 | LENOVO przeglądarka | **Multi-Device tab → kliknij „Odśwież" (ikona refresh)** → POST fetch-now pobiera snapshot, roster-merge-additive grafuje, lista się przeładowuje → ✅ **Dell (PN-OFFICE) widoczny** (P1-002). | NIE |
| D3 | (kontrola data-safety) | Po D2 otwórz dowolny istniejący plik z O:\ na Lenovo → ✅ czytelny (fetch NIE ruszył lokalnych DEK — roster-merge-only). | NIE |

---

## ✅ Kryteria PASS
- **P1-001:** hash B5 == hash A4.
- **P1-005:** safety numbers Dell == Lenovo (B3).
- **P1-004:** R2 COMPLETED, brak ConnReset (C2/C3).
- **P1-003:** Scaleway COMPLETED (C2).
- **P1-002:** Lenovo widzi Della po kliknięciu Odśwież (D2); lokalne pliki Lenovo nadal czytelne (D3).

## Po PASS
STATUS §12.6: β.a live smoke → potwierdzone. Opcjonalnie bump/release notes. Brak zmian kodu wymaganych.
