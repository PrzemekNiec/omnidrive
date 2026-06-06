# OmniDrive — Formalny przegląd kryptograficzny (QG5)

**Data:** 2026-06-06
**Recenzent:** Claude (Opus 4.8) — formalny review wewnętrzny, NIE audyt zewnętrzny
**Zakres:** cały stos krypto Fazy α (α.A–α.C), workspace v0.3.27, HEAD `74fc236`
**Gate:** QG5 z `STATUS.md §12.3` — *„Formalny crypto review (Claude) — patrz Faza α DoD"*
**Spec referencyjny:** `docs/crypto-spec.md` (§1–§15, zaktualizowany w α.D.a)

> **Granica zakresu (D11):** dla v0.4 QG5 = formalny przegląd wewnętrzny. **Audyt zewnętrzny krypto jest gate'em v5.0** (gdy w grę wchodzą cudze pliki). Ten dokument NIE jest audytem zewnętrznym ani dowodem formalnym — jest ustrukturyzowanym przeglądem inżynierskim łańcucha kluczy, niezmienników zero-knowledge i pokrycia threat-modelu §12.1.

---

## 1. Metodyka

Przegląd oparty na czytaniu kodu produkcyjnego (`omnidrive-core/src/{crypto,hybrid,pqkem}.rs`, `angeld/src/{vault,identity,db,downloader}.rs`) + specyfikacji `crypto-spec.md`. Dla każdej warstwy oceniono: (a) poprawność konstrukcji, (b) separację domen kluczy, (c) niezmienniki zero-knowledge, (d) obsługę błędów/tamperingu. Findings sklasyfikowano: **Critical** (utrata/wyciek danych w modelu zagrożeń) / **High** / **Medium** / **Low** (defense-in-depth) / **Info**.

---

## 2. Łańcuch kluczy (pełny pipeline)

```
passphrase
  │  Argon2id v0x13 (parameter_set 2: m=256 MiB, t=3, p=1, salt per-vault)
  ▼
master_key (256-bit, uniform)
  ├─ HKDF-Expand("kek-v2")                 → KEK ───────────────┐
  ├─ HKDF-Expand("vault-key-v1")           → V1 vault_key (legacy read)
  ├─ HKDF-Expand("manifest-mac-key-v1")    → manifest MAC
  ├─ HKDF-Expand("lease-mac-key-v1")       → lease MAC
  ├─ HKDF-Expand("local-anchor-key")       → local anchor
  └─ HKDF-Expand("omnidrive-identity-kek-v1") → identity-KEK ──┐
                                                               │
KEK ──AES-KW-Unwrap(encrypted_vault_key)──► Vault Key (256-bit, RANDOM)
                                                  │
                                                  ├─AES-KW──► DEK per-inode (RANDOM) ──AES-256-GCM──► chunki
                                                  └─(device wrap, §14): X25519 i/lub X25519+ML-KEM-768 hybrid
identity-KEK ──AES-256-GCM(seal)──► X25519 priv (60 B) + ML-KEM-768 decaps (sealed)
```

**Ocena separacji domen:** wszystkie wyprowadzenia HKDF używają **rozłącznych etykiet `info`** (domain separation poprawna). `expand_labeled_key`/`derive_identity_kek` to HKDF-**Expand-only** (`from_prk`) — prawidłowe per RFC 5869 §3.3, bo IKM (`master_key` z Argon2id, losowy VK) jest już jednorodnie losowy. `derive_wrapping_key` (X25519) i combiner hybrydowy używają HKDF-**Extract+Expand** (`new(salt, ikm)`) — prawidłowe, bo ich IKM (ECDH point / konkatenacja ss) NIE jest jednorodne. Asymetria jest zamierzona i poprawna.

---

## 3. Analiza per-warstwa

### 3.1 KDF (Argon2id) — α.B.a
- Parameter_set 2: m=256 MiB / t=3 / p=1 — **przekracza** OWASP 2024 floor (m=46 MiB) i drugą rekomendację RFC 9106 (m=64 MiB, t=3). Mocny margines brute-force dla desktopu. **OK.**
- Re-key migracja atomowa (jedna tx, rollback) re-wrapuje TEN SAM Vault Key → `vault_key_generation` bez zmian → DEK/dane/safety-numbers nietknięte. Konstrukcja poprawna; `legacy_read_key` zachowuje czytelność V1. **OK.** (Obserwacja: F-3, F-5.)

### 3.2 Envelope (KEK → VK → DEK)
- AES-256-KW (RFC 3394) do wrappowania — brak nonce → brak ryzyka nonce-reuse; deterministyczny, co jest świadome i bezpieczne dla wrappowania kluczy. **OK.**
- VK i DEK losowe z `OsRng`. Rotacja VK = re-wrap DEK bez re-encryption. **OK.**

### 3.3 Szyfrowanie chunków (AES-256-GCM)
- V1: deterministyczny nonce+chunk_id (HMAC vault_key) — leak równości treści, **świadomie naprawione w V2** losowym nonce + chunk_id per-DEK.
- V2: `encrypt_chunk_v2` — random nonce (OsRng), chunk_id=HMAC(DEK,plaintext), AES-256-GCM. **OK.**
- Obserwacja integralności: patrz **F-2** (V2 nie rekomputuje chunk_id po dekrypcji; downloader porównuje chunk_id z prefiksu względem DB jako routing-check).

### 3.4 AAD (§12) — P3-001
- Chunki `&[]`, OAuth `user_id`, legacy_read_key `vault_id`, identity seal `&[]`. Każda decyzja uzasadniona w §12. Wiązanie OAuth↔user_id domyka confused-deputy w multi-user. **OK.** (Obserwacja defense-in-depth: F-4.)

### 3.5 Tożsamość urządzenia + device wrap (§14) — α.C.a + α.B.b
- X25519: ECDH z **guardem low-order point** (odrzut all-zero shared secret) → HKDF → AES-KW. **OK.**
- Hybrid X25519+ML-KEM-768: combiner **HKDF X-Wing** (NIGDY XOR), IKM = `x25519_ss‖mlkem_ss`, transkrypt **TLV length-prefixed** (anti-splice/downgrade/rebinding). Wersja w transkrypcie (downgrade-resistant). Implicit-rejection FIPS 203 obsłużony poprawnie (tamper łapany na AES-KW downstream). Deep crypto-review α.B.b.2 (2026-06-05) = APPROVED zero-Critical. **OK.**
- Selekcja v3→v2 + re-seal kluczy przy migracji KDF spójne. **OK.** (Obserwacje: F-1 revoke, F-6 wiring.)

### 3.6 Grafting tożsamości (§15) — α.C.b
- Grafuje stan *vaultu* (EVK, gen, legacy_read_key, DEK, recovery keys), NIE *urządzenia* (`local_device_identity`). Rozdział poprawny → dołączające urządzenie wyprowadza ten sam VK (P1-005) i unwrapuje istniejące DEK (P1-001). DoD in-process zielony. **OK.**

### 3.7 Higiena pamięci (§13) — α.A
- `KeyBytes` `ZeroizeOnDrop` + redacted Debug + non-Copy + buildery in-place; **dowód memdump H4** (after-lock = 0 trafień). Auto-lock (idle/Win+L/logout) minimalizuje okno ekspozycji. **OK** w granicach świadomej akceptacji §12.1(b) (F-7).

### 3.8 Session token (§11)
- Brak constant-time przy 256-bit tokenie OsRng na loopback/LAN — świadome, udokumentowane, z warunkiem rewizji przy ekspozycji publicznej. **OK** dla modelu local-first.

---

## 4. Pokrycie threat-modelu (§12.1)

| # | Zagrożenie | Status | Mechanizm |
|---|---|---|---|
| (a) | Compromised provider | ✅ Pokryte | Zero-knowledge: provider widzi tylko ciphertext; EC_2_1; DEK nigdy nie opuszcza klienta |
| (b) | Compromised local OS | ⚠️ Częściowe (świadome) | DPAPI/secrecy persistowane; **RAM in-use = akceptacja ryzyka**; mitygacja auto-lock + ZeroizeOnDrop (F-7) |
| (d) | Recovery | ✅ Pokryte | BIP-39 + recovery keys (grafowane §15) |
| (e) | Brute force | ✅ Mocne | Argon2id m=256 MiB/t=3 (> OWASP floor) |
| (f) | Quantum-resistance | ✅ Pokryte | Hybrid X25519+ML-KEM-768 dla VK-wrap; AES-256-GCM/HMAC symetryczne post-quantum-safe (Grover → 128-bit) |

(c) compromised endpoint / Dead Man Switch — świadomie odłożone na v5.0.

---

## 5. Findings

| ID | Severity | Tytuł | Status |
|----|----------|-------|--------|
| F-1 | **Medium** | `revoke_device` nie NULLuje `wrapped_vault_key_kyber` | rekomendacja przed LIVE multi-device hybrid |
| F-2 | Low | V2 nie rekomputuje chunk_id po dekrypcji (parity z V1) | defense-in-depth |
| F-3 | Low | Świeży vault tworzony na parameter_set 1, migrowany do 2 przy 1. unlocku | uproszczenie |
| F-4 | Low | Seal klucza prywatnego urządzenia używa pustego AAD | defense-in-depth |
| F-5 | Info | `legacy_read_key` trzymany bezterminowo po migracji | higiena key-material |
| F-6 | Info | `select_and_unwrap_vault_key` jeszcze niewpięty w onboarding | kompletność |
| F-7 | Info | VK rezydentny w RAM podczas unlocku — zaakceptowane ryzyko v0.4 | udokumentowane |

### F-1 (Medium) — niekompletna rewokacja dla urządzeń hybrydowych
`db::revoke_device` czyści `wrapped_vault_key` (X25519), ale **pozostawia** `wrapped_vault_key_kyber` (hybrid). Urządzenie zrewokowane, które zachowało kopię bazy/snapshotu, wciąż posiada lokalnie swój ML-KEM decaps key i może odtworzyć Vault Key ścieżką hybrydową — rewokacja jest obejściowa.
**Eksploatowalność:** wymaga, by zrewokowane urządzenie retainowało kopię DB z hybrydowym blobem. **Hybrid multi-device NIE jest jeszcze aktywny live** (α.B.b zrobił solo + best-effort wrap na accept; pełne wpięcie = przyszłość), więc NIE blokuje v0.4.
**Rekomendacja:** `revoke_device` musi NULLować OBIE kolumny. Naprawić **przed** aktywacją live multi-device hybrid. Wpisać do `KNOWN_ISSUES.md`.

### F-2 (Low) — V2 chunk integrity bez rekomputacji chunk_id
`decrypt_chunk_v2` (inaczej niż V1 `decrypt_chunk`) NIE rekomputuje `HMAC(DEK, plaintext)` po dekrypcji; downloader (`downloader.rs:1323`) porównuje chunk_id z **prefiksu rekordu** (bajty z dysku) względem chunk_id z DB — to routing-check, nie kryptograficzne wiązanie plaintext↔chunk_id. AAD=`&[]` nie wiąże chunk_id z ciphertextem.
**Eksploatowalność:** **brak w modelu ZK.** Sfałszowanie chunka wymaga ważnego tagu GCM pod DEK, którego provider (jedyny adwersarz §12.1.a) NIE posiada. Wewnątrz-DEK substitution wymagałaby znajomości DEK.
**Rekomendacja (defense-in-depth):** rekomputować chunk_id po dekrypcji V2 (parytet z V1) **lub** związać oczekiwany chunk_id/ordinal jako AAD V2. Niski priorytet.

### F-3 (Low) — świeży vault na słabszym parameter_set
Nowy vault tworzony jest na `DEFAULT` (parameter_set 1, m=64 MiB), a do `TARGET` (2, m=256 MiB) migrowany dopiero przy pierwszym unlocku. Skutek: (a) okno, w którym świeży vault jest chroniony słabszym KDF; (b) podwójny koszt Argon2id przy pierwszym unlocku.
**Rekomendacja:** tworzyć świeże vaulty od razu na `TARGET`. Zachować ścieżkę migracji dla istniejących v1.

### F-4 (Low) — pusty AAD przy seal klucza prywatnego
`encrypt_private_key`/`seal_secret_blob` używają pustego AAD. `local_device_identity` jest per-device i niegrafowane (§15), więc brak realnego kontekstu do podstawienia. **Rekomendacja:** AAD=`device_id` jako defense-in-depth.

### F-5 / F-6 / F-7 — Info
- **F-5:** po pełnej migracji wszystkich chunków do V2 `legacy_read_key` można wyczyścić (mniej key-material at rest).
- **F-6:** helper selekcji unwrap + testy istnieją; produkcyjne wpięcie w onboarding = follow-up (nie podatność).
- **F-7:** rezydencja VK w RAM podczas unlocku jest udokumentowanym, zaakceptowanym ryzykiem v0.4; mitygowana auto-lock + ZeroizeOnDrop. v5.0+ może dodać TPM/enclave.

---

## 6. Mocne strony (potwierdzone)

- Pełna separacja domen HKDF (rozłączne etykiety), poprawny wybór Extract-only vs Expand-only wg jednorodności IKM.
- AES-KW eliminuje ryzyko nonce-reuse przy wrappowaniu kluczy; random nonce V2 eliminuje content-equality leak.
- Hybrid combiner HKDF X-Wing z TLV transkryptem — anti-downgrade/splice/rebinding; implicit-rejection FIPS 203 obsłużone poprawnie; deep-review α.B.b.2 zero-Critical.
- Low-order point guard na ECDH.
- ZeroizeOnDrop udowodniony empirycznie (memdump H4 = 0 trafień).
- Argon2id znacząco powyżej floora OWASP.
- Zero-knowledge utrzymane: provider nigdy nie widzi VK/DEK/plaintextu; klucze nigdy nie logowane (`[REDACTED]`).

---

## 7. Werdykt QG5

**PASS (warunkowy) dla zakresu v0.4** (solo vault + single-user-multi-device infra).

- **Zero findings Critical/High.** Łańcuch kluczy, envelope, hybrid wrap, graft, zeroize i auto-lock są kryptograficznie poprawne i utrzymują niezmienniki zero-knowledge.
- **F-1 (Medium)** dotyczy ścieżki **jeszcze nieaktywnej live** (multi-device hybrid) → nie blokuje v0.4, ale **MUSI** zostać naprawiony przed jej aktywacją (post-v0.4). Wpisać do `KNOWN_ISSUES.md`.
- **F-2/F-3/F-4 (Low)** to rekomendacje defense-in-depth — nieblokujące, do rozważenia w fazie γ/utrzymaniowej.
- **F-5/F-6/F-7 (Info)** — higiena i kompletność.

**Warunki domknięcia QG5:** (1) akceptacja tego dokumentu przez Przemka; (2) zarejestrowanie F-1 (i opcjonalnie F-2) w `KNOWN_ISSUES.md` jako pozycji blokujących LIVE multi-device hybrid. Po akceptacji: `STATUS.md §12.5 α.D.a → DONE`, **Faza α ZAMKNIĘTA**.

**Live SMOKE** (Dell↔Lenovo, real provider egress) pozostaje osobną akceptacją operacyjną — NIE jest częścią gate'u QG5 (review kodu/spec).
