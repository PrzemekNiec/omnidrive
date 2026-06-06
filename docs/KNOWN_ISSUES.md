# OmniDrive — Known Issues Tracker

> **Single source of truth dla bugów.** Ten plik (nie GitHub Issues, nie STATUS.md) trzyma listę otwartych problemów z priorytetyzacją.
>
> **Ostatnia aktualizacja:** 2026-06-06
> **Aktualna wersja:** v0.3.27

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

### P1-001 + P1-005 (POŁĄCZONE) — Graft pomija krytyczne pola krypto vault_state + data_encryption_keys

- **Wykryto:** v0.3.22 (P1-001 hydration fail) + v0.3.23 wieczór (P1-005 safety numbers mismatch)
- **CONFIRMED 2026-05-10 wieczór:** porównanie `key_generation` przez `/api/vault/safety-numbers` na obu maszynach:
  - Lenovo: `key_generation=4`, mnemonic `town write alley critic unusual topple…`, safety `58975 53274 06638 …`
  - Dell:   `key_generation=1`, mnemonic `armed fiber cave strategy alert market…`, safety `03018 46227 27488 …`
  - user_id identyczny (`bb3cb95e-…`) — graft `users`/`devices`/`vault_members` z v0.3.23 OK
- **Root cause (jednoznaczny):** `db.rs::graft_restored_metadata_snapshot` (~linia 1830) kopiuje z `vault_state` TYLKO 3 pola: `master_key_salt`, `argon2_params`, `vault_id`. **Pomija `encrypted_vault_key` i `vault_key_generation`** (oraz wszelkie inne pola krypto w `vault_state`). Dell po grafcie używa swojego lokalnego gen=1 VK z bootstrap, nie Lenovo's gen=4 VK. KEK jest poprawny (salt+params grafted), ale unwrap zwraca losowy gen=1 VK Della, nie Lenovo's gen=4. Stąd:
  - Różny EVK → różne `safety_numbers` (P1-005)
  - DEK chunków (wrapped Lenovo's gen=4 VK) nie unwrap pod Dell's gen=1 → fallback tworzy gen=1 DEK → próba decrypt obcych chunków → `aes-gcm operation failed` (P1-001)
- **Impact:** **P0-style severity** dla single-user-multi-device (główny use case v0.4) — multi-device nie działa funkcjonalnie. UI pokazuje device w MultiDevice tab (cosmetic ok), ale ŻADEN plik nie da się pobrać na Dellu, a UI security verification (safety numbers) bezużyteczne. Nadal P1 bo pojedyncze urządzenie działa, tylko cross-device broken.
- **Fix scope (Faza α.C.b z roadmapy v0.4):** Rozszerzyć graft o pełen „identity bundle" krypto:
  1. `vault_state.encrypted_vault_key` + `vault_key_generation` + `previous_envelope_key` (cała tabela poza KDF params i vault_id)
  2. Cała tabela `data_encryption_keys` (DEK per-plik, wrapped pod source's VK)
  3. Cała tabela `recovery_keys` (BIP-39, jeśli istnieje, Sesja 34.6a)
  4. **Audit pozostałych tabel** w `docs/crypto-spec.md` żeby nie zostawić jeszcze jakiegoś pola
  5. Test e2e: Lenovo wgra plik → Dell join → Dell otwiera plik z O:\ → checksum match → safety numbers identyczne na obu
- **Status:** OPEN. **Faza α.C.b** roadmapy v0.4 (po Argon2id bump α.B.a, ML-KEM α.B.b, X25519 α.C.a). Faza α formalnie wystartuje po Fazie 0 (QA Foundation) — nie robimy hot-fix v0.3.24, bo trzeba to zrobić systematycznie z testem e2e (kluczowy flow F5).

### P1-006 — `/api/auth/logout` nie blokuje vaulta (klucze zostają w RAM)

- **Wykryto:** 2026-05-17, Task 2 Fazy 0 (audit AAD/zeroize/auto-lock).
- **Root cause:** `api/auth.rs::post_auth_logout` (linia 189) wykonuje TYLKO `db::delete_user_session(&state.pool, token).await` + audit log. **Brak `state.vault_keys.lock().await`.** Sesja HTTP w DB jest unieważniona, ale `VaultKeyStore.inner` w RAM nadal trzyma `UnlockedVaultKeys { master_key: SecretBox, vault_key: SecretBox, envelope_vault_key: ... }`. Dla porównania `api/vault.rs::post_vault_lock` (linia 915) wywołuje `state.vault_keys.lock().await` + dismount cfapi.
- **Impact:** **Zero-knowledge gap.** User klika "Wyloguj" myśląc, że jest bezpieczny — w rzeczywistości klucze plaintext są nadal w pamięci procesu `angeld.exe`. Atakujący z dostępem do procesu (debugger, ProcDump, core dump, lub atak fizyczny na zablokowany laptop z włączonym daemonem) może odczytać klucze i zdeszyfrować dane. UI semantyka jest kłamliwa.
- **Realny scenariusz:** Użytkownik kończy pracę → klika "Wyloguj" → odchodzi od komputera. Klucze nadal w pamięci. Atakujący z fizycznym dostępem (cold boot attack, Thunderbolt DMA, lub po prostu unlock systemu jeśli user nie zablokował Windows) widzi otwarty vault.
- **Fix scope:**
  1. `post_auth_logout` musi wywołać `state.vault_keys.lock().await` PRZED `delete_user_session` (analogicznie do `post_vault_lock`).
  2. Decyzja: czy logout = full vault lock (z dismount cfapi P0 sequence) czy tylko key zero? Pełny lock jest spójny z user mental model i security best practice.
  3. Audit innych endpointów: czy są inne ścieżki które kasują sesję ale nie lockują vaulta (np. session expiry timer w db, jeśli istnieje).
- **Status:** OPEN. **Faza α.A.a** (Crypto Hardening, grupa A hot-fixes). Może iść jako hot-fix v0.3.24 (mały scope, security high impact, low regression risk).

### P1-002 — Lenovo nie widzi Della w MultiDevice po join

- **Wykryto:** v0.3.23 Dell smoke test, MultiDevice tab Lenovo pokazuje tylko siebie
- **CONFIRMED 2026-05-10 wieczór:** Dell po v0.3.23 join-existing pokazuje OBA urządzenia (PN-THINKPAD + PN-OFFICE) ✅ — graft `devices` działa. Lenovo daemon zweryfikowany jako v0.3.23 (curl `/api/diagnostics` zwraca pełny JSON, endpoint dodany w v0.3.23). `members_count:1` w `/api/vault/status` na Lenovo — potwierdza że Lenovo nigdy nie pobrał zaktualizowanego snapshot z Della.
- **Symptom:** Dell po join-existing wgra zaktualizowany snapshot do chmury, ale Lenovo nigdy go nie pobiera, więc nie wie o nowym device
- **Hipoteza root cause:** Daemon ma snapshot **upload** worker (`MetadataBackupWorker`) ale nie ma symetrycznego snapshot **fetch** workera dla istniejących urządzeń. Tylko join-existing flow pobiera snapshot.
- **Impact:** Multi-device awareness jednokierunkowy. Gdy ktoś z rodziny dołącza nowy laptop (v5.0), admin nie zobaczy go bez restart daemona albo manual refresh.
- **Fix scope:** Periodic snapshot fetch worker (np. co 1h) w angeld. Decyzja: tylko gdy snapshot jest nowszy + lock wokół DB (nie nadpisuj jeśli były lokalne zmiany). Może wymagać per-device sequence number / lamport clock.
- **Status:** OPEN. Planowany w **Faza β.b** roadmapy v0.4.

### P1-005 → MERGED z P1-001 (2026-05-10 wieczór)

Diagnoza zakończona. Root cause potwierdzony empirycznie: `vault_state.encrypted_vault_key` + `vault_key_generation` nie kopiowane w grafcie, plus brak kopiowania `data_encryption_keys`. To ten sam underlying bug co P1-001 — patrz wpis P1-001+P1-005 (połączone) wyżej.

### P1-003 — Snapshot upload do Scaleway zwraca AccessDenied

- **Wykryto:** v0.3.23 Dell metadata-backup status — Scaleway 403 AccessDenied dla `_omnidrive/system/metadata/snapshots/*.db.enc`
- **Symptom:** B2 OK, R2 connection reset (osobny issue), Scaleway 403. Czyli z 3 providerów tylko jeden żywy.
- **Hipoteza:** Bucket policy / access key uprawnienia do prefix `_omnidrive/system/metadata/snapshots/` — może bucket nie pozwala PUT pod system/. Inny prefix (`packs/...`) działa wg logów.
- **Impact:** Brak redundancji metadanych: jedyna kopia snapshot na B2. Awaria B2 = utrata metadata, mimo że chunki są na 3 providerach.
- **Fix scope:** Sprawdzić Scaleway IAM policy + bucket policy + key permissions. Jeśli OK, zbadać dlaczego prefix `_omnidrive/system/` jest blokowany. Naprawić konfigurację albo udokumentować workaround.
- **Status:** OPEN. **Faza β.c** roadmapy v0.4 (snapshot redundancy fix). **Quality Gate 2.e** ("snapshot zawsze w ≥1 sprawnym miejscu") nie spełniony, ale technically B2 jest sprawny → tolerowalne tymczasowo. P1 bo bezpieczeństwo redundancji.

### P1-004 — Snapshot upload do R2 zwraca ConnectionReset

- **Wykryto:** v0.3.23 Dell metadata-backup status — R2 `ConnectionReset (os error 10054)` przy PUT
- **Symptom:** Brak 403, brak timeout — surowy reset połączenia od R2. Może być rate-limit / WAF / connection pool issue.
- **Hipoteza:** R2 hyper-1.x compatibility issue (memory: rustls/hyper consolidation odłożona). Może `keep-alive` pool trzyma wygasłe połączenie.
- **Impact:** Tak samo jak P1-003 — brak redundancji.
- **Fix scope:** Najpierw retry z fresh connection (`force-close` po 1 ConnReset). Drugorzędnie: Batch 7 C.3 (rustls/hyper consolidation z Backlog).
- **Status:** OPEN. **Faza β.c** roadmapy v0.4 (snapshot redundancy fix). Powiązany z C.3 (Backlog).

---

## P2 — Performance / SLA dług

### P2-001 — Watcher mieli CPU

- **Wykryto:** Subiektywna obserwacja Przemka, brak benchmarku
- **Symptom:** `angeld.exe` w taskmgr pokazuje wysokie CPU nawet w idle (do potwierdzenia liczbowego)
- **SLA cel:** < 1% CPU idle, < 5% active (per roadmap v0.4)
- **Fix scope:** (1) Mierzenie: profiling 60s idle + 60s active. (2) Audit `angeld/src/watcher.rs` (643 linie). Sprawdzić: polling vs event-driven? debounce? batch? file system event API (Windows ReadDirectoryChangesW)?
- **Status:** OPEN. **Faza β.d** (po pomiarach).

### P2-002 — VFS laguje przy dużych plikach

- **Wykryto:** Subiektywna obserwacja Przemka, brak benchmarku
- **Symptom:** Otwarcie dużego pliku (>50MB?) z O:\ trwa zauważalnie długo
- **SLA cel:** Cold fetch < 2s/10MB, < 10s/100MB; warm < 100ms (per roadmap v0.4)
- **Fix scope:** (1) Benchmark: cold fetch 1MB/10MB/100MB/1GB; warm fetch tych samych. (2) Audit `angeld/src/smart_sync.rs` (2197 linii — monolit do dekomponozycji). Sprawdzić: streaming hydration czy fetch-all-then-decrypt? EC reconstruction blokująca? Cache hit path?
- **Status:** OPEN. **Faza ε.a/β.e** (po pomiarach — dekompozycja smart_sync.rs).

### P2-004 — Brak auto-lock po idle

- **Wykryto:** 2026-05-17, Task 2 Fazy 0 (audit auto-lock).
- **Symptom:** Po `unlock` vault pozostaje otwarty dopóki user nie kliknie "Zablokuj vault" (POST `/api/vault/lock`) lub daemon nie umrze. Brak żadnego timera idle.
- **Werifikacja:** `grep -i "auto_lock|idle_timeout|idle_lock|lock_after|inactivity"` w całym `*.rs` workspace → **0 matches.**
- **Impact:** Zostawiony zalogowany laptop z odblokowanym vaultem = pełen dostęp do plików dla każdego, kto siada przy maszynie. Standardowa praktyka w password managerach (1Password, Bitwarden, KeePassXC) = auto-lock po 5/15/30 min idle, lub po zablokowaniu systemu Windows.
- **Fix scope:**
  1. Konfigurowalny timer (np. `vault.auto_lock_idle_minutes`, default 15).
  2. Reset timera przy każdym authenticated API call (file access, list, search, …).
  3. Hook na Windows session-lock event (`WM_WTSSESSION_CHANGE` / `SystemEvents.SessionSwitch`) — natychmiastowy lock przy zablokowaniu sesji Windows.
  4. UI: pasek statusu „odblokowany na 14:32 min" + warning przed auto-lock (np. 1 min wcześniej toast).
- **Impact:** Nie security data loss (klucze są w pamięci procesu, nie na dysku), ale user-facing security feature missing. Blokuje v0.4 (Faza ε UX/Stability).
- **Status:** OPEN. **Faza α.A.b** (security hot-fix, grupa A). Wymaga: hook Windows event API + config + UI element.

### P2-005 — Brak Zeroize na temp kopiach kluczy (klucze zostają w pamięci poza SecretBox)

- **Wykryto:** 2026-05-17, Task 2 Fazy 0 (audit key zeroization).
- **Werifikacja:** `grep "Zeroize|zeroize|ZeroizeOnDrop"` w całym workspace → **0 matches.** `crypto-spec.md §8` wymienia `zeroize` jako dep transitive przez `secrecy` ale wprost nie używamy.
- **Symptom:** `KeyBytes = [u8; 32]` (omnidrive-core/src/crypto.rs:28) nie ma `Zeroize` derive. `secrecy::SecretBox<KeyBytes>` zeruje wewnętrzną kopię na drop (dobre), ale każdy gettera `master_key()`, `vault_key()`, `envelope_vault_key()` w `vault.rs:77-91` zwraca KOPIĘ `KeyBytes` przez `*self.master_key.expose_secret()`. Te zwracane kopie żyją na stosie/stercie wywołującego, NIE są zeroize-on-drop. Po `lock()` w `VaultKeyStore.inner` klucze są wyzerowane, ale kopie u wywołujących (np. lokalne `let key = vault.require_key().await?;`) zostają.
- **Impact:** Memory dump po `lock()` może zawierać klucze pozostawione w pamięci wywołujących. Podważa zero-knowledge guarantee w przypadku coredumpa / hibernation / ataku DMA. Defense-in-depth gap.
- **Fix scope:**
  1. Dodać `zeroize = { workspace = true, features = ["zeroize_derive"] }` jako explicit dep w `omnidrive-core`.
  2. `KeyBytes` opakować w newtype z `#[derive(Zeroize, ZeroizeOnDrop)]` (zamiast type alias) — wymusi zeroize na każdej kopii.
  3. Audit call-sites `expose_secret()` w `vault.rs`, `downloader.rs`, `packer.rs`, `migrator.rs`, `sharing.rs` — zamienić plain copies na `SecretBox<KeyBytes>` lub krótkożyjące referencje.
  4. Test: po `vault.lock()`, memscan procesu nie znajduje znanych key patterns.
- **Status:** OPEN. **Faza α.A.c** (Crypto Hardening, grupa A hot-fixes). Powiązane z P1-006 (α.A.a) ale niezależne.

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
- **Status:** OPEN. P2 — blokuje v0.4 (clean architecture przed mobile). Decyzja Opcja A vs B vs C → Faza α lub β (wstawić jako β.f lub γ.a-pre, do decyzji).

### P2-006 — `revoke_device` nie NULLuje hybrydowego wrapu Vault Key (niekompletna rewokacja) — finding F-1 z QG5

- **Wykryto:** 2026-06-06, formalny crypto-review QG5 (`docs/superpowers/specs/2026-06-06-crypto-review.md`, finding **F-1**, severity Medium).
- **Root cause:** `db::revoke_device` czyści `devices.wrapped_vault_key` (wrap X25519, v2-x25519), ale **pozostawia** `devices.wrapped_vault_key_kyber` (wrap hybrydowy X25519+ML-KEM, v3-hybrid, dodany w α.B.b). Urządzenie zrewokowane, które zachowało lokalnie kopię bazy/snapshotu, wciąż posiada swój ML-KEM decapsulation key (`local_device_identity.encrypted_kyber_private_key`) i może odtworzyć Vault Key ścieżką hybrydową (`select_and_unwrap_vault_key` preferuje v3) — rewokacja jest obejściowa.
- **Eksploatowalność:** wymaga, by zrewokowane urządzenie retainowało kopię DB z hybrydowym blobem. **Hybrid multi-device NIE jest jeszcze aktywny live** (α.B.b zrealizował solo vault + best-effort wrap przy `accept_device`; pełne produkcyjne wpięcie `select_and_unwrap_vault_key` w onboarding = follow-up). Dlatego **NIE blokuje v0.4**, ale jest blokerem aktywacji live multi-device hybrid (post-v0.4).
- **Fix scope:**
  1. `db::revoke_device` musi NULLować OBIE kolumny (`wrapped_vault_key` **i** `wrapped_vault_key_kyber`) w tej samej operacji.
  2. Test: po `revoke_device` żadna ścieżka (`select_and_unwrap_vault_key`) nie odtwarza VK dla zrewokowanego urządzenia.
- **Status:** OPEN. **Dług techniczny — MUSI być naprawiony przed aktywacją LIVE multi-device hybrid.** Kandydat do fazy δ (Multi-User Infra) lub osobnego sub-taska przed Epic 33 sharing.

---

## P3 — Drobne UX / kosmetyka

### P3-001 — AAD pusty (`&[]`) na chunk encrypt/decrypt — niespecyfikowane w crypto-spec

- **Wykryto:** 2026-05-17, Task 2 Fazy 0 (audit AAD call-sites).
- **Status:** **świadoma decyzja, brak realnej luki** — wymaga jedynie dokumentacji w spec.
- **Faktyczne call-sites w prod (poza testami):**
  | Call-site | AAD | Komentarz |
  |---|---|---|
  | `packer.rs:264` `encrypt_chunk_v2(&dek, plaintext, &[])` | pusty | hot path — wszystkie nowe chunki |
  | `migrator.rs:163` `decrypt_chunk(vault_key, &chunk_id, &[], …)` | pusty | V1→V2 migration |
  | `migrator.rs:173` `encrypt_chunk_v2(&dek, &plaintext, &[])` | pusty | V1→V2 migration |
  | `downloader.rs:1353` `decrypt_chunk_v2(dek, &nonce, &[], …)` | pusty | V2 read path |
  | `downloader.rs:1357` `decrypt_chunk(vault_key, …, &[], …)` | pusty | V1 read path |
  | `vault.rs:298` `encrypt_secret(&derived, token, user_id.as_bytes())` | **`user_id`** ✓ | OAuth token sealing — wiąże ciphertext z tożsamością |
  | `vault.rs:308` `decrypt_secret(&derived, blob, user_id.as_bytes())` | **`user_id`** ✓ | symetrycznie |
- **Werdykt per kategoria:**
  - **OAuth secrets (`vault.rs`):** AAD = `user_id` — **POPRAWNE.** Cross-user tampering wykryty przez GCM tag.
  - **Chunki (`packer.rs`/`migrator.rs`/`downloader.rs`):** AAD = `&[]`. **Wymuszone przez WebCrypto compat** — share-link decryption w przeglądarce (`share.html`, komentarz w `crypto.rs:523`) musi mieć identyczny AAD jak encryption, a `crypto.subtle.decrypt` w browserze nie ma dostępu do `(inode_id, revision_id, chunk_index)`. Pusty AAD = jedyna opcja zgodna z architekturą Trybu B (statyczny dekryptor na skarbiec.app).
- **Defense-in-depth oportunity (poza scope obecnej architektury):** AAD mógłby wiązać chunk z `(inode_id, revision_id, chunk_index, dek_id)` — wykrywałby cross-file chunk swap attack (atakujący kopiuje encrypted chunk z pliku A na pliku B). Ale to łamie share-link compat (Tryb B). **Trade-off świadomie wybrany na rzecz share-link compat.**
- **Fix scope:** Dodać do `docs/crypto-spec.md` nową sekcję §12 "AAD semantics" wyjaśniającą:
  1. dlaczego `&[]` dla chunków (WebCrypto / Tryb B compat)
  2. dlaczego `user_id` dla OAuth secrets (cross-user tampering protection)
  3. udokumentowany trade-off cross-file swap protection vs share-link Tryb B
- **Status:** OPEN doc-only. **P3 (dokumentacja, brak code change).** Może iść razem z innym crypto-spec update.

### P3-002 — 23 prod unwrap/expect — triage

- **Wykryto:** 2026-05-17, Task 2 Fazy 0 (audit unwrap/expect).
- **Status raw vs prod:** 368 raw, ale po odfiltrowaniu `#[cfg(test)]` tail to **23** (audit report wcześniej zliczył 24, ale `downloader.rs:1582` jest w bloku testów). **Test count uzasadniony i konsekwentny — `unwrap` w testach OK.**
- **Triage:**
  - **11× UI tray binary** (`omnidrive-tray/src/main.rs`) — fail-fast na ładowaniu ikony, panic OK dla GUI app.
  - **3× mutex poison** (`cloud_guard.rs:185, 239, 273` — `.expect("session usage mutex poisoned")`) — idiomatic, mutex poison = bug w innym tasku, panic OK.
  - **3× post-guard / post-invariant**: `secure_fs.rs:72` (`.expect("retry loop must capture last error")`), `main.rs:1042` (analogicznie), `api/mod.rs:64` (`.first().copied().unwrap()` po `len() >= 3` guardzie), `device_identity.rs:51` (`.expect("local device identity must exist after upsert")`) — wszystkie po programowym invariancie.
  - **3× hardcoded Argon2** (`sharing.rs:46, 52, 75` — `.expect("valid argon2 params")` / `.expect("argon2 hash")`) — Argon2id z hardcoded params (8192, 2, 1, Some(32)). Niemożliwe do faili przy stałych parametrach + 32-byte output.
  - **2× ❗P2 (eskalacja)**:
    - `peer.rs:159` `reqwest::Client::builder().timeout(...).build().expect("peer client")` — startup crash przy nieprawidłowej konfiguracji reqwest (np. brak rustls feature, mismatched TLS backend). **Daemon nie wstanie.** Lepiej: `Result<Peer, PeerError>` propagation.
    - `ingest.rs:184` `.expect("ingest: packer initialization failed")` — analogicznie, packer init może faili przy złych params. Hot path = ingest pipeline, crash blokuje cały watcher.
- **Fix scope (per kategoria):**
  - UI tray + idiomy: nic do roboty.
  - Argon2 hardcoded: zostawić (sanity expect).
  - **Eskalowane (2):** zrefaktorować `peer.rs::Peer::new` i `ingest.rs::IngestWorker::new` do zwrotu `Result<Self, E>` zamiast panic. Wymaga zmiany sygnatury wywoływaczy (callerze już mają `?` Pattern).
- **Status:** OPEN dla 2 eskalowanych do P2 — pozostałe 21 udokumentowane jako świadome decyzje. **Eskalowane 2 → Faza β** (kandydat do β.f jako P2-003 quick wins batch, do decyzji).

### P3-003 — V2 chunk nie rekomputuje chunk_id po dekrypcji (parytet z V1) — finding F-2 z QG5

- **Wykryto:** 2026-06-06, formalny crypto-review QG5 (finding **F-2**, severity Low).
- **Root cause:** `omnidrive_core::crypto::decrypt_chunk_v2` (inaczej niż V1 `decrypt_chunk`) NIE rekomputuje `HMAC(DEK, plaintext)` po dekrypcji i nie weryfikuje go względem oczekiwanego chunk_id. Downloader (`downloader.rs:1323`) porównuje chunk_id z **prefiksu rekordu** (bajty z dysku) względem chunk_id z DB — to routing/sanity-check, NIE kryptograficzne wiązanie plaintext↔chunk_id. AAD=`&[]` (P3-001) nie wiąże chunk_id z ciphertextem.
- **Eksploatowalność:** **brak w modelu zero-knowledge §12.1(a).** Sfałszowanie chunka wymaga ważnego tagu GCM pod DEK, którego provider (jedyny adwersarz) NIE posiada. Wewnątrz-DEK substitution wymagałaby znajomości DEK. To wyłącznie luka defense-in-depth.
- **Fix scope (opcjonalny, defense-in-depth):** rekomputować chunk_id po dekrypcji V2 (parytet z V1 `decrypt_chunk`) **lub** związać oczekiwany chunk_id/ordinal jako AAD V2 (uwaga: AAD łamie share-link Tryb B compat — patrz P3-001 trade-off).
- **Status:** OPEN. Dług techniczny niski priorytet. Faza γ (Zero Data Loss Hardening) lub utrzymaniowa.

### P3-004 — Świeży vault tworzony na słabszym parameter_set Argon2id (migrowany przy 1. unlocku) — finding F-3 z QG5

- **Wykryto:** 2026-06-06, formalny crypto-review QG5 (finding **F-3**, severity Low).
- **Root cause:** Nowy vault tworzony jest na `DEFAULT` (`vault.rs`: parameter_set 1, m=64 MiB), a do `TARGET` (parameter_set 2, m=256 MiB Desktop High Security) migrowany dopiero przy pierwszym unlocku (`run_post_unlock_maintenance` → re-key migracja). Skutek: (a) okno, w którym świeży vault jest chroniony słabszym KDF (64 MiB zamiast 256 MiB) — istotne tylko jeśli atakujący zdobędzie DB między utworzeniem a pierwszym unlockiem; (b) podwójny koszt Argon2id (64 MiB + 256 MiB) przy pierwszym unlocku.
- **Fix scope:** tworzyć świeże vaulty od razu na `TARGET` parameter_set; zachować ścieżkę re-key migracji wyłącznie dla istniejących vaultów v1.
- **Decyzja Przemka 2026-06-06:** **doc-only dla Fazy α — NIE dotykamy kodu.** Pozostaje zarejestrowane jako dług techniczny do osobnej decyzji/fazy.
- **Status:** OPEN. Dług techniczny niski priorytet.



---

## Closed

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
