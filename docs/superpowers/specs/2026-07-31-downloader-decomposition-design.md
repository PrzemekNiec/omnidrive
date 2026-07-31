# Dekompozycja `angeld/src/downloader.rs` — design

**Data:** 2026-07-31
**Baza:** HEAD `02c9fb2` origin/main, v0.3.28
**Typ:** refaktor mechaniczny — zero zmian zachowania, zero migracji, zero bumpu wersji
**Poprzedniki:** `2026-07-31-db-decomposition-design.md`, `2026-07-31-smart-sync-decomposition-design.md`

---

## 1. Stan wyjściowy

`angeld/src/downloader.rs` = **1 730 linii**, 59 bloków top-level. Rozkład jest inny niż w dwóch poprzednich dekompozycjach:

| Element | Linie |
|---|---|
| `#![allow(dead_code)]` + 34 importy | 37 |
| Typy: `Downloader`, `DownloadProvider`, `RestoredPackSource`, `RestoreResult`, `DownloaderError` + 7 `impl` | ~135 |
| **`impl Downloader` — jeden blok, 17 metod** | **988** |
| `EncryptedChunkBytes` + `impl`, `impl DownloadProvider` | ~60 |
| Wolne funkcje: `reconstruct_ciphertext`, `build_manifest_bytes`, `decrypt_chunk_record`, konwertery, helpery env | ~210 |
| `mod tests` | 295 |

**To zmienia charakter zadania.** Przy `db.rs` i `smart_sync.rs` podział sprowadzał się do rozdania bloków top-level do plików. Tutaj 57% pliku to **jeden blok** — rozdanie bloków zostawiłoby plik z metodą 988-linijkową i niczego by nie rozwiązało.

## 2. Metoda: podział bloku `impl`

Rust pozwala rozbić inherent `impl` na wiele bloków w różnych modułach tego samego crate'a. Każdy plik dostaje własny `impl Downloader { … }` z podzbiorem metod. Z zewnątrz typ zachowuje się identycznie — `Downloader::read_range(…)` rozwiązuje się tak samo, niezależnie od tego, w którym pliku leży definicja.

Koszt jest ten sam, co przy `smart_sync.rs`: metody prywatne wołane spoza swojego nowego pliku muszą dostać `pub(super)`. Z inwentarza wynika, że dotyczy to 7 z 17 metod (`load_plaintext_chunk`, `try_fetch_chunk_from_peer`, `maybe_schedule_prefetch`, `prefetch_chunks`, `download_pack`, `probe_latency`, `download_shard`) plus część wolnych funkcji pomocniczych. Dokładna lista jest wyliczana z referencji, nie zgadywana.

## 3. Cel i zakres

**Cel:** `angeld/src/downloader/` z 8 plikami, żaden powyżej ~470 linii.

**W zakresie:** przeniesienie 1:1 metod i funkcji; podział `impl Downloader` na 4 bloki; `pub(super)` wymuszone podziałem; redystrybucja importów; rozdzielenie testów.

**Poza zakresem:** zmiany logiki pobierania, dekrypcji, rekonstrukcji EC, prefetchu, obsługi błędów; nowe testy; bump wersji; zmiany w 9 miejscach konsumujących `downloader::Downloader`.

## 4. Docelowa struktura

```
angeld/src/downloader/
  mod.rs        ~190  #![allow(dead_code)], typy + DownloaderError + konwersje From,
                      deklaracje modułów
  read.rs       ~450  impl Downloader: restore_file, read_range, read_range_streamed,
                      read_plaintext_chunk_by_id, get_encrypted_chunk_bytes,
                      load_plaintext_chunk
  pack.rs       ~290  impl Downloader: download_pack, download_shard,
                      try_fetch_chunk_from_peer
  chunk.rs      ~210  EncryptedChunkBytes + impl, reconstruct_ciphertext,
                      build_manifest_bytes, decrypt_chunk_record, vec_to_*
  provider.rs   ~180  impl DownloadProvider + impl Downloader: from_env,
                      from_provider_configs, reload_active_providers_from_db,
                      has_remote_providers, set_peer_client, probe_latency
  prefetch.rs   ~110  impl Downloader: maybe_schedule_prefetch, prefetch_chunks
  util.rs        ~30  env_path, duration_from_env, to_usize, to_u64, format_error_details
  test_support.rs      (cfg(test)) helpery wspólne, jeśli testy ich potrzebują
```

Testy (295 linii, 3 testy) rozdzielane do modułu, którego zachowanie asertują — wzorzec z `db.rs`.

## 5. Widoczność

Reguła bez zmian względem `smart_sync`: element zostaje prywatny, jeśli używa go wyłącznie własny moduł; `pub(super)` gdy używa go moduł siostrzany; już-`pub` bez zmian. Domknięcie przechodnie na typy wyciekające przez podniesione sygnatury. Lista raportowana w całości.

## 6. Weryfikacja zero-drift

Trzystopniowa, jak przy `smart_sync`, z jednym rozszerzeniem wymuszonym podziałem `impl`:

1. **Round-trip parsera** na całym pliku **i osobno na ciele `impl Downloader`** — rekonstrukcja musi dać baseline co do bajtu. ✅ *oba wykonane przed napisaniem tego spec-a.*
2. **Kompletność i treść** — każda metoda z baseline istnieje dokładnie raz w wynikowych blokach `impl Downloader`; treść identyczna albo różniąca się wyłącznie prefiksem `pub(super) `.
3. **Równoważność po `rustfmt`** — bloki, które `rustfmt` przełamał inaczej (dedent nie zmienia się tutaj, ale zmiana może wyjść przy imports/formatowaniu), muszą być identyczne z `rustfmt(baseline_block)`.
4. **Zakres diffu** — nic poza `angeld/src/downloader.rs` → `angeld/src/downloader/**` i `docs/`.

## 7. Ryzyka

| Ryzyko | Mitygacja |
|---|---|
| Podział `impl` gubi metodę | Kontrola: liczba i nazwy metod w wynikowych blokach `impl Downloader` == baseline (17) |
| Zbyt szerokie `pub(super)` | Wyliczane z referencji + domknięcie przechodnie; nigdy `pub(crate)` |
| Testy w pliku pokrywają tylko fragment | 3 testy na 1 730 linii — realnie potwierdzają `decrypt_chunk_record` i konwertery, **nie** ścieżkę pobierania. Ścieżkę sieciową pokrywają `angeld/tests/e2e_*` (`e2e_reconciliation`, `e2e_scrubber_repair`), które konsumują `Downloader` jako library consumer — muszą przejść |
| `read_range` / `read_range_streamed` to bliźniaki (110 linii każda) | Świadomie zostają w jednym pliku; ewentualna de-duplikacja to osobne zadanie, NIE ten refaktor |

## 8. Definition of Done

- [x] `angeld/src/downloader.rs` nie istnieje; `angeld/src/downloader/` z **7** plikami (`test_support.rs` okazał się zbędny — testy nie mają wspólnych helperów między modułami). ⚠️ **cel ~470 linii przekroczony w `read.rs` = 729**; kod produkcyjny to ~450, resztę stanowi przeniesiony tam test roundtrip (158 linii) z siedmioma helperami mock S3
- [x] fmt czysty; clippy `--all-targets -D warnings` czysty w obu trybach
- [x] `cargo build --release --workspace` OK; core **28**, angeld lib **199**
- [x] Testy e2e konsumujące `Downloader` **kompilują się** (`cargo test -p angeld --no-run`). ⚠️ **NIE zostały uruchomione** — wymagają mapowań `subst` i realnego środowiska, a sesja była bez nadzoru. Kompilacja dowodzi, że sygnatury konsumowane przez library consumer się nie zmieniły; nie dowodzi poprawności runtime
- [x] 17 metod `impl Downloader` obecnych dokładnie raz; treść identyczna modulo `pub(super)`
- [x] `git diff` poza `downloader/` i `docs/` pusty
- [x] Wpis `KNOWN_ISSUES.md` P2-009 + `STATUS.md` §12.7b
