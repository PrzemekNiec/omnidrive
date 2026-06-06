# Faza β — Task 2: P1-003 (Scaleway 403) & P1-004 (R2 ConnReset) — Cloud Redundancy

> **Cel QG (§12.6 β.c):** snapshot metadanych **zawsze w ≥1 sprawnym miejscu, docelowo 2/3 providerów**. Dziś: B2 OK, Scaleway 403, R2 ConnReset → tylko 1/3.
>
> **Charakter:** problemy sieciowe/chmurowe — częściowo NIE-TDD-owalne (wymagają live providerów / zmian IAM po stronie konsoli). Plan rozdziela **logikę testowalną in-process** (retry/backoff, klasyfikacja błędów, config) od **weryfikacji live (smoke)** i **akcji infra (IAM)**.
>
> **DECYZJE ZATWIERDZONE (Przemek 2026-06-06):** P1-003 = diagnostyka AccessDenied + graceful 2/3 degradation + action-item IAM (BEZ workaroundu konfigurowalnego prefiksu). P1-004 = krótki `pool_idle_timeout` na poziomie hyper buildera + adaptive RetryConfig + app-level `retry_with_backoff` (Opcja 1 — zachowuje perf hot-path packs, prune'uje martwe sockety). Zakaz wyłączania poola.

---

## ANALIZA (ugruntowana w kodzie)

### Architektura klienta S3 (stan obecny)
- `aws_http::load_shared_config` (`angeld/src/aws_http.rs`) — **jedyny** builder SdkConfig dla całego workspace: `HyperClientBuilder` (hyper_014) + `hyper_rustls` connector, **tylko `enable_http1()`**, webpki roots, `TimeoutConfig`. **BRAK jawnego `RetryConfig`** (używany domyślny SDK = standard, 3 próby). **BRAK tuningu connection-poolu** (`pool_idle_timeout` itd.).
- Pack-uploader `S3ProviderUploader` (`uploader.rs:196-219`): `load_shared_config` → `aws_sdk_s3::config::Builder` z `.endpoint_url(config.endpoint)` + `.region` + `.force_path_style(config.force_path_style)`. Per-provider `force_path_style` z DB (`provider_record.force_path_style`).
- Metadata-backup provider (`disaster_recovery.rs`): `Uploader::from_provider_config` (ten sam typ uploadera) + `MetadataBackupDownloadProvider` (`:1147-1174`) — **ten sam** `force_path_style` + `endpoint` per config. Snapshot upload przez `provider.upload_system_file(enc_path, snapshot_key)`.

### 🔑 P1-003 (Scaleway 403) — REWIZJA HIPOTEZY
`upload_system_file` → `upload_file` (`uploader.rs:249-303`) jest **bajt-w-bajt tym samym żądaniem** co `upload_pack` → `upload_file`: `put_object().bucket(self.bucket).key(key).body().content_length().content_type("application/octet-stream")`. **Żadnego ACL, storage_class, dodatkowych nagłówków.** Ten sam klient, ten sam bucket, ten sam `force_path_style`. **Jedyna różnica = prefiks klucza** (`packs/…` vs `_omnidrive/system/metadata/snapshots/…`).

**Wniosek:** skoro `packs/` PUT **działa** na Scaleway tym samym klientem, a `_omnidrive/system/` PUT zwraca 403 — to **NIE jest** problem `force_path_style` / `virtual_hosted_style` / formatu endpointu (te dawałyby 403 na WSZYSTKICH prefiksach, też `packs/`). Hipoteza path-style/endpoint = **ODRZUCONA dowodem** (współdzielony klient + działające packs).

**Najbardziej prawdopodobna przyczyna P1-003 = prefix-scoped IAM / bucket policy po stronie Scaleway** — klucz dostępowy ma `s3:PutObject` na `packs/*` ale NIE na `_omnidrive/system/*` (albo bucket policy blokuje ten prefiks). **To problem INFRASTRUKTURY, nie kodu** — nie da się go „załatać" w `.rs`.

### P1-004 (R2 ConnReset 10054) — keep-alive / connection pool
`os error 10054` = WSAECONNRESET (Windows). `enable_http1()` + domyślny hyper pool trzyma idle keep-alive connections; **R2 agresywnie zamyka idle połączenia** → następny PUT reużywa martwego socketu → RST mid-request. Klasyczna sygnatura stale-pooled-connection. Domyślny SDK retry (3×, standard) **powinien** klasyfikować ConnReset jako retryable, ale: (a) w oknie retry pool może oddawać ten sam martwy socket; (b) metadata `upload_metadata_backup` robi pojedynczą próbę per-provider i przy błędzie rejestruje FAILED — następny retry dopiero w kolejnym cyklu workera (1h+).

---

## PLAN (T2.1 – T2.4) — gdzie i jak modyfikujemy klienta

### T2.1 — connection-pool hygiene + jawny RetryConfig (root-cause P1-004) [aws_http.rs]
**Lokalizacja:** `angeld/src/aws_http.rs::load_shared_config` — jeden punkt, korzysta z niego CAŁY workspace (packs + metadata + wszyscy providerzy) → fix raz, działa wszędzie.
- Dodać do connectora/klienta hyper **`pool_idle_timeout`** krótszy niż R2 idle-close (proponuję 8–15 s; R2 zamyka ~idle po kilkudziesięciu s, ale konserwatywnie krótko = pruning martwych socketów zanim R2 je zerwie). Rozważyć też `pool_max_idle_per_host` (np. małe, lub 0 = bez reuse — kosztem nowego TLS handshake per request, ale eliminuje stale-reuse).
- Dodać jawny **`RetryConfig`** (`aws_config::retry::RetryConfig::adaptive()` lub `standard().with_max_attempts(5)`) do `aws_config::defaults(...)` — adaptive backoff dla transientów (ConnReset, timeout) na poziomie SDK.
- **HEDGE (do weryfikacji przy implementacji):** API `aws_smithy_http_client::hyper_014::HyperClientBuilder` może NIE eksponować `pool_idle_timeout` bezpośrednio — wtedy ustawić na poziomie `hyper_util`/`hyper` legacy `Client::builder()` przekazywanego do connectora, albo użyć nowszego smithy http-client API. Implementer potwierdza dostępną metodę PRZED kodowaniem; jeśli pool-config niedostępny w tej wersji smithy → fallback na samą RetryConfig + T2.2 app-retry.
- **Testowalność:** trudna in-process (sieć). Minimalny test: asercja że `load_shared_config` zwraca SdkConfig z ustawionym `retry_config` (jeśli API pozwala odczytać). Reszta = smoke live R2.

### T2.2 — exponential-backoff retry dla uploadu snapshotu (app-level resilience) [disaster_recovery.rs]
**Lokalizacja:** ścieżka `upload_metadata_backup` (`disaster_recovery.rs:546+`).
- Wyodrębnić **generyczny helper** `retry_with_backoff(max_attempts, base_delay, op)` (async, generic nad `FnMut() -> Future<Result<T, E>>`) z klasyfikacją: retryuj **tylko transienty** (ConnReset/timeout/5xx), NIE retryuj **permanentnych** (403 AccessDenied/401) — te od razu FAILED (P1-003 nie ma sensu retryować, to policy).
- Owinąć per-provider `upload_system_file` w `retry_with_backoff` (np. 3–4 próby, base 500 ms, exp backoff + jitter). Fresh attempt = nowe `put_object().send()` (SDK weźmie świeże/inne połączenie z poolu po pool-pruning z T2.1).
- **Testowalność (TDD CORE — to jest sedno in-process):** `retry_with_backoff` testowalny w pełni: (a) op fail×2 → ok → 3 próby, zwraca Ok; (b) op zawsze transient-fail → wyczerpuje próby → Err; (c) op zwraca permanentny błąd (AccessDenied) → **1 próba, brak retry**; (d) backoff rośnie (mierzony lub mockowany zegar). Klasyfikator błędów (`is_retryable(&err)`) też testowalny jednostkowo.

### T2.3 — P1-003: diagnostyka AccessDenied + graceful degradation + action-item IAM [disaster_recovery.rs + docs]
P1-003 to przede wszystkim **akcja infrastrukturalna**, ale kod może pomóc:
- **Kod (a):** klasyfikacja błędu — gdy provider zwraca `AccessDenied`/403 na upload snapshotu, log **actionable**: `"metadata snapshot upload denied by {provider} on prefix _omnidrive/system/ — check the access key's IAM/bucket policy grants PutObject/GetObject/ListBucket on this prefix"` (NIE generyczne "FAILED"). Distinguish 403 od transientów (wspólne z klasyfikatorem T2.2).
- **Kod (b):** upewnić się, że worker traktuje per-provider niezależnie — **2/3 sukces (B2 + R2) spełnia QG**; Scaleway-403 NIE blokuje całości (best-effort per provider, status w `metadata_backups`). Zweryfikować że obecna pętla już tak działa (z analizy: tak — per-provider status), ewentualnie domknąć.
- **Dokumentacja / action-item (NIE kod):** dopisać do `docs/KNOWN_ISSUES.md` P1-003, że root-cause = Scaleway IAM policy na prefiks `_omnidrive/system/*`; **wymagana akcja Przemka w konsoli Scaleway**: nadać kluczowi dostępowemu uprawnienia `s3:PutObject`+`s3:GetObject`+`s3:ListBucket` na `_omnidrive/system/*` (lub całość bucketu). Po zmianie IAM → live smoke potwierdza.
- **Opcja workaround (do decyzji):** jeśli Scaleway policy nieusuwalna, uczynić prefiks snapshotu konfigurowalnym (env/DB) by ominąć blokadę — ale to obejście, preferowany fix = IAM.

### T2.4 — weryfikacja: bramka + macierz smoke
- Bramka `--all-targets` (fmt+clippy oba tryby + build --release + core + angeld --lib) — testy jednostkowe `retry_with_backoff` + `is_retryable`.
- **Live smoke (osobna akceptacja, wymaga creds/Della):** (1) R2 PUT snapshotu po T2.1+T2.2 → sukces / brak ConnReset; (2) Scaleway PUT snapshotu po zmianie IAM → 200; (3) `metadata-backup status` pokazuje ≥2/3 zielone (cel 3/3).

---

## Macierz testowalności (uczciwie)
| Element | In-process TDD | Live smoke / infra |
|---|---|---|
| `retry_with_backoff` + `is_retryable` | ✅ pełny TDD | — |
| pool_idle_timeout / RetryConfig applied | ⚠️ asercja configu (jeśli API pozwala) | ✅ R2 PUT smoke |
| Scaleway 403 fix | ❌ (to IAM) | ✅ po zmianie IAM Przemka |
| 2/3 redundancy degradation | ✅ (per-provider status logic) | ✅ metadata-backup status |

---

## Sekwencja
T2.1 (aws_http pool+retry) → T2.2 (retry_with_backoff + wpięcie w upload_metadata_backup; **TDD core**) → T2.3 (AccessDenied diagnostyka + graceful + KNOWN_ISSUES action-item IAM) → T2.4 bramka. Tryb: subagent-driven. Bez bumpu wersji.

## Granice
- Scaleway IAM = akcja konsoli Przemka (kod tylko diagnozuje + degraduje gracefully).
- Bez zmian w EC/pack-distribution, share-linkach, FFI.
- Live smoke nie bramkuje DONE kodu (jak w β.b) — ale QG β.c (≥2/3) wymaga smoke do formalnego zamknięcia.
