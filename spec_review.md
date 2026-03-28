# OmniDrive Spec Review

Status: review dokumentu bazowego vs stan faktyczny implementacji  
Data: 2026-03-28  
Punkt odniesienia: [omnidrive_vfs_technical_spec_v1.md](C:/Users/Przemek/Desktop/aplikacje/omnidrive/omnidrive_vfs_technical_spec_v1.md)  
Stan projektu: [PROJECT_STATUS.md](C:/Users/Przemek/Desktop/aplikacje/omnidrive/PROJECT_STATUS.md)

## Executive Summary

OmniDrive został dowieziony bardziej jako **realny produkt desktopowy dla Windows** niż jako literalna implementacja storage-engine baseline z `omnidrive_vfs_technical_spec_v1.md`.

Najważniejszy wniosek:
- **produktowo jesteśmy dalej niż spec**, bo mamy:
  - Smart Sync / CFAPI,
  - `O:\`,
  - installer,
  - autostart,
  - secure local runtime,
  - diagnostics,
  - pełne E2E dla recovery, reconciliation i self-healing,
- ale **architektonicznie nie wdrożyliśmy jeszcze całego formalnego repo modelu ze spec**, zwłaszcza:
  - `superblock`,
  - `manifest envelope`,
  - `canonical MsgPack object graph`,
  - `lease/fencing token single-writer model`,
  - pełnego modelu „cloud metadata as canonical head”.

Ocena wysokopoziomowa:
- **co jest lepsze niż w spec**
  - używalność produktu,
  - integracja z Windows,
  - gotowość operacyjna,
  - recovery i self-healing,
  - installer i bootstrap clean-machine
- **co jest gorsze niż w spec**
  - formalna czystość modelu repozytorium,
  - zgodność z normatywnym formatem obiektów,
  - model lease/quorum/fencing,
  - rozdzielenie SQLite od canonical cloud metadata

W praktyce:
- jeśli celem jest **działający prywatny desktop vault**, obecny kierunek jest bardzo dobry,
- jeśli celem jest **pełna zgodność z oryginalną spec v1**, potrzebny będzie osobny etap architektoniczny, który domknie repo-format i concurrency model.

Najkrótsza synteza:
- **implementacja wygrała produktowo**
- **specyfikacja nadal wygrywa formalnością storage-engine**

## Management Summary

### Obecny status
- projekt jest już **realnym produktem desktopowym**, a nie tylko prototypem silnika storage
- najważniejsze funkcje użytkowe działają:
  - instalator
  - autostart
  - `O:\`
  - Smart Sync
  - recovery
  - self-healing
- największe luki są dziś głównie **architektoniczne**, a nie produktowe

### Co zostało dowiezione najlepiej
- integracja z Windows i Explorerem
- odporność operacyjna:
  - diagnostics
  - logi
  - E2E
  - recovery
  - repair
- gotowość do testów clean-machine i dalszego rolloutu desktopowego

### Gdzie nadal jest dług techniczny
- brak pełnego modelu `superblock / manifest / canonical repository head`
- brak lease / fencing token / single-writer control
- zbyt silna rola SQLite względem tego, co zakłada spec v1

### Ocena biznesowa
- jeśli celem jest **dowieźć działającą prywatną aplikację desktopową**, obecny kierunek jest właściwy
- jeśli celem jest **pełna zgodność z oryginalną spec storage-engine**, potrzebny będzie dodatkowy etap architektoniczny

### Rekomendacja
- krótkoterminowo:
  - dalej domykać jakość produktu desktopowego i clean-machine stability
- średnioterminowo:
  - zdecydować, czy OmniDrive ma pozostać pragmatycznym produktem desktopowym,
  - czy wracamy do pełnego wdrożenia formalnego repo-format i concurrency model ze spec v1

## Najważniejsze odchylenia od specyfikacji

1. **Zamiast repo-object model first, zbudowaliśmy desktop product first.**
   Lepsze: mamy działający produkt z `O:\`, Smart Sync, installerem i E2E.
   Gorsze: nie mamy jeszcze literalnie wdrożonego modelu `superblock -> manifest graph -> pack catalog` z dokumentu v1.

2. **SQLite jest dziś silniejszym źródłem prawdy operacyjnej niż zakłada spec.**
   Lepsze: prostsza implementacja, szybkie iteracje, realnie działający system.
   Gorsze: nadal nie jesteśmy w pełni przy modelu „cloud metadata defines canonical repository head”.

3. **IPC poszło w lokalne HTTP API zamiast gRPC/named pipes.**
   Lepsze: szybsza integracja z UI, diagnostyką i testami E2E.
   Gorsze: odbiega od stricte zdefiniowanego baseline’u procesu Angel w spec v1.

4. **Windows Cloud Files API stało się rdzeniem produktu wcześniej niż zakładał dokument.**
   Lepsze: natywna integracja z Explorerem i realne Files-On-Demand.
   Gorsze: większa złożoność platformowa i więcej problemów systemowych niż w czysto repozytoryjnym baseline.

5. **Storage engine rozwijał się ewolucyjnie, nie jako jednorazowe wdrożenie „canonical v1 object repository”.**
   Lepsze: udało się dowieźć `EC_2_1`, `SINGLE_REPLICA`, `LOCAL_ONLY`, reconciliation i repair bez zatrzymywania produktu.
   Gorsze: architektura jest bardziej hybrydowa niż czysty model z dokumentu.

6. **Nie wdrożyliśmy jeszcze formalnego lease/fencing token single-writer model.**
   Lepsze: obecny scope desktopowy tego jeszcze nie wymuszał.
   Gorsze: to pozostaje istotna luka względem specyfikacji, zwłaszcza przed mocniejszym multi-device.

7. **Nie ma jeszcze canonical MsgPack envelopes jako normatywnego formatu wszystkich manifestów.**
   Lepsze: uniknęliśmy dużego kosztu implementacyjnego na wczesnym etapie.
   Gorsze: nie ma jeszcze pełnej deterministycznej warstwy repo-format opisanej w spec.

8. **Disaster Recovery zostało dowiezione wcześniej i praktyczniej niż zakładał dokument.**
   Lepsze: mamy działający encrypted metadata backup, restore, auto-restore i pełne E2E.
   Gorsze: nadal jest to DR oparte o aktualny model runtime, a nie docelowy manifest/superblock repo-format.

9. **Security runtime poszło szerzej niż w samym baseline repo-format.**
   Lepsze: mamy `secrecy`, HKDF separation, encrypted cache, ACL hardening, secure spool cleanup.
   Gorsze: część problemów była bardzo platformowo-specyficzna i zwiększyła koszt integracji Windows.

10. **Observability i E2E są dużo dojrzalsze niż sugerowałaby wczesna specyfikacja v1.**
    Lepsze: log rotation, diagnostics API, E2E recovery/reconciliation/self-healing.
    Gorsze: testujemy dojrzały runtime, który architektonicznie nie jest jeszcze 1:1 z repo baseline spec.

11. **Installer i per-user bootstrap to nowy kierunek, którego spec v1 praktycznie nie obejmowała.**
    Lepsze: OmniDrive przestaje być projektem developerskim i staje się aplikacją.
    Gorsze: ta warstwa wprowadza nowe wymagania operacyjne, których dokument v1 nie modelował.

12. **Local-only setup mode to świadome odejście od założenia „cloud-first canonical head”.**
    Lepsze: clean install działa bez natychmiastowej konfiguracji providerów.
    Gorsze: to formalnie rozluźnia model źródła prawdy opisany w spec.

13. **Część roadmapy ze spec v1 została już funkcjonalnie pokryta inną drogą implementacyjną.**
    Lepsze: dostarczyliśmy rezultat użytkowy szybciej.
    Gorsze: dokument i kod nie są dziś w pełni izomorficzne architektonicznie.

14. **Największa obecna przewaga implementacji nad specyfikacją to dojrzałość produktu desktopowego.**
    Lepsze: `O:\`, Explorer, installer, autostart, diagnostics, policy modes, self-healing.
    Gorsze: największa przewaga specyfikacji nad implementacją to formalność modelu repo i concurrency guarantees.

## 1. Cel review

Ten dokument porównuje:
- założenia z [omnidrive_vfs_technical_spec_v1.md](C:/Users/Przemek/Desktop/aplikacje/omnidrive/omnidrive_vfs_technical_spec_v1.md),
- stan faktycznie osiągniętej implementacji,
- oraz to, co nadal planujemy wdrożyć, aby porównanie było kompletne.

To nie jest rewrite specyfikacji. To jest uczciwe podsumowanie:
- co zostało dowiezione,
- co zostało dowiezione inaczej,
- czego jeszcze nie ma,
- i które elementy roadmapy zamkną największe rozbieżności.

## 2. Charakter pierwotnej specyfikacji

Spec v1 opisuje system jako:
- **Angel-first runtime**,
- z **jednym writerem** i formalnym **lease/fencing token model**,
- z **canonical repository format** opartym o:
  - `superblock`,
  - `manifest envelope`,
  - `packfile`,
- gdzie:
  - cloud metadata jest docelowym canonical head,
  - SQLite jest tylko derived operational database,
  - a klient VFS i CLI są cienkimi klientami sterowanymi przez Angel.

To jest specyfikacja bardzo „storage-engine first”.

## 3. Co osiągnęliśmy zgodnie z duchem specyfikacji

Mimo odchyleń, kilka najważniejszych idei ze specyfikacji zostało realnie osiągniętych:

1. **Angel-first runtime faktycznie istnieje.**
   `angeld` jest centralnym procesem systemu i kontroluje runtime, SQLite, background workers, Smart Sync oraz recovery.

2. **CLI jest klientem, nie właścicielem stanu.**
   `omnidrive` korzysta z API / daemonowych flow i nie pełni roli bezpośredniego storage authority.

3. **Namespace jest inode-based.**
   To jest zgodne z bazową ideą dokumentu.

4. **Pack/chunk model istnieje i działa.**
   Mamy packer, downloader, chunk references, pack metadata, shard metadata i read path.

5. **Recovery po utracie lokalnej bazy zostało faktycznie dowiezione.**
   To jest bardzo ważna zgodność funkcjonalna z ideą „SQLite is not the only durable state”.

6. **Storage durability i self-healing istnieją.**
   Deep scrubber, degrade detection i repair worker są realnie wdrożone i przetestowane E2E.

7. **Background state machine faktycznie działa.**
   Nie jako literalna implementacja wszystkich nazw stanów ze spec, ale jako realny system workerów, retry, repair, reconciliation i upload scheduling.

## 4. Co dowieźliśmy ponad spec v1

W kilku obszarach implementacja poszła dalej niż dokument bazowy:

1. **Windows Smart Sync / CFAPI**
- placeholder projection
- hydration callbacks
- pin/unpin
- virtual drive `O:\`
- Explorer integration

2. **Runtime hardening**
- `secrecy` dla kluczy w RAM
- HKDF separation dla cache key
- encrypted local cache
- secure spool cleanup
- ACL hardening

3. **Observability**
- structured logging
- rotating logs
- diagnostics API
- worker status visibility

4. **E2E verification**
- happy path
- sync-root bootstrap
- disaster recovery
- policy reconciliation
- scrubber -> degrade -> repair

5. **Installer / installed-mode architecture**
- per-user install
- runtime path resolver
- autostart
- auto-bootstrap local vault
- clean-machine checklist

Spec v1 tego nie modelowała jako priorytet, a dziś jest to centralna część produktu.

## 5. Główne rozbieżności architektoniczne

### 5.1 Cloud source of truth vs obecny runtime

Spec v1:
- committed cloud metadata ma być canonical repository head,
- SQLite ma być tylko derived index + journal.

Stan obecny:
- SQLite jest nadal bardzo silnym elementem operacyjnym,
- DR potrafi ją odbudować z chmurowego metadata backupu,
- ale nie mamy jeszcze pełnego modelu:
  - `superblock`
  - `root_manifest`
  - `pack_catalog_manifest`
  - quorum-based canonical head resolution.

Ocena:
- **gorsze od spec** pod kątem formalności repo-modelu,
- **lepsze produktowo** pod kątem tempa dostarczenia działającego systemu.

### 5.2 Canonical binary repository format

Spec v1 normatywnie definiuje:
- `ODSB`
- `ODMF`
- `ODPK`
- inline tail index
- canonical MsgPack rules

Stan obecny:
- pack i chunk model istnieje,
- ale nie wdrożyliśmy jeszcze pełnego formalnego object format baseline dokładnie w tej postaci,
- szczególnie brakuje pełnego manifest/superblock envelope model.

Ocena:
- **niedowiezione względem spec literalnie**,
- ale część praktycznych potrzeb została pokryta przez aktualny model packów i SQLite metadata.

### 5.3 Lease / fencing / single-writer control

Spec v1:
- bardzo mocno normuje lease object i fencing token.

Stan obecny:
- nie ma jeszcze pełnej implementacji lease quorum i fencing token publication model.

Ocena:
- to jest jedna z najistotniejszych nierozwiązanych rozbieżności,
- szczególnie ważna przed silniejszym multi-device i conflict handling.

### 5.4 IPC model

Spec v1:
- gRPC + UDS / named pipe later.

Stan obecny:
- lokalne HTTP API z `axum`.

Ocena:
- **lepsze praktycznie** dla UI, testów i debugowania,
- **gorsze względem zgodności z baseline spec**.

### 5.5 VFS model

Spec v1:
- FUSE / później WinFsp.

Stan obecny:
- Windows Cloud Files API + wirtualny dysk `O:\`.

Ocena:
- dla Windows desktop product to jest prawdopodobnie **lepsza droga** niż literalne trzymanie się starego planu.

## 6. Osiągnięta architektura OmniDrive na dziś

Na dziś OmniDrive jest najlepiej opisywać tak:

### 6.1 Product shape

OmniDrive jest:
- lokalnym daemonem `angeld`,
- z lokalną SQLite,
- z szyfrowanym runtime,
- z natywnym Windows `SyncRoot`,
- z wirtualnym dyskiem `O:\`,
- z background workerami dla:
  - uploadu,
  - repair,
  - scrub,
  - GC,
  - metadata backup,
  - watcher,
- oraz z lokalnym HTTP API dla CLI i UI.

### 6.2 Storage model

Obecnie wspieramy:
- `EC_2_1`
- `SINGLE_REPLICA`
- `LOCAL_ONLY`

To jest ważne, bo praktycznie poszerzyliśmy model ze specyfikacji o warstwy polityk i trybów pracy, których dokument v1 nie opisywał w tej formie.

### 6.3 Security model

Mamy:
- Argon2
- vault key lifecycle
- encrypted cache
- secure local runtime
- metadata backup encryption

Czyli security model operacyjny jest już znacznie dojrzalszy niż sam baseline repo-format.

### 6.4 Reliability model

Mamy:
- DR restore
- scrubber
- degrade detection
- repair
- reconciliation
- E2E coverage najważniejszych ścieżek

To jest bardzo silna część aktualnej implementacji.

## 7. Co jest dziś obiektywnie lepsze niż w spec v1

1. **Produktowość**
- mamy realny installer i installed-mode bootstrap

2. **Windows UX**
- `O:\`
- placeholdery
- Explorer integration

3. **Operacyjna obserwowalność**
- diagnostics API
- rotating logs
- test harnessy

4. **Self-healing w praktyce**
- degrade -> repair jest nie tylko opisane, ale przetestowane

5. **Local-only bootstrap**
- system może wystartować nawet bez natychmiastowej konfiguracji providerów

## 8. Co jest dziś obiektywnie gorsze albo niedomknięte względem spec v1

1. brak formalnego canonical repository object model
2. brak superblock / manifest envelope implementation
3. brak lease / fencing token model
4. brak strict cloud-head-as-truth architecture
5. brak pełnej deterministycznej canonical MsgPack layer
6. brak pełnego versioned immutable manifest graph w formie opisanej przez spec

To są najważniejsze różnice techniczne, które trzeba rozumieć uczciwie.

## 9. Co jeszcze planujemy wdrożyć i jak to wpływa na porównanie

### 9.1 Najbliższe epiki produktowe

Z obecnej roadmapy:
- `Epic 27.6`
  - clean-machine validation matrix
- `Epic 28`
  - self-healing shell integration
- `Epic 29`
  - storage cost and policy dashboard
- `Epic 30`
  - maintenance console
- `Epic 31`
  - P2P LAN cache
- `Epic 32`
  - sync conflict handling
- `Epic 33`
  - zero-knowledge link sharing
- `Epic 34`
  - auth / Google login

To oznacza, że produkt będzie dalej rósł bardziej w stronę:
- desktop operability,
- multi-device behavior,
- user-facing features,
niż w stronę literalnego wdrożenia całego repo-format baseline ze spec v1.

### 9.2 Elementy roadmapy nadal istotne dla zgodności ze spec

W roadmapie nadal są obszary, które częściowo zbliżają nas do oryginalnego ducha spec:
- bardziej formalny EC core
- read path finalization
- GC
- versioning
- dedup
- quota
- policy engine
- upload scheduling

Natomiast:
- nie wszystkie z tych pozycji są dziś rozwijane dokładnie tak, jak zakładała spec v1,
- część została już zastąpiona przez bardziej produktowy i praktyczny model.

## 10. Rekomendacja interpretacyjna

Najuczciwiej patrzeć na to tak:

1. `omnidrive_vfs_technical_spec_v1.md` było bardzo dobrym **engineering baseline**.
2. Implementacja nie poszła 1:1 za dokumentem.
3. Część odejść była świadoma i dobra.
4. Największe zyski to:
   - realny produkt desktopowy
   - Smart Sync
   - instalacja
   - E2E
   - operational hardening
5. Największe braki względem spec to:
   - canonical object repository format
   - superblocks/manifests
   - fencing/lease model
   - formalne rozdzielenie cloud head od SQLite runtime

## 11. Wniosek końcowy

OmniDrive nie jest dziś implementacją literalnie zgodną ze spec v1.

OmniDrive jest dziś:
- **bardziej kompletnym produktem desktopowym**,
- ale **mniej formalnie zgodnym z repozytoryjną specyfikacją storage-engine**.

Jeśli celem biznesowym jest:
- dowieźć używalny, prywatny desktop vault na Windows,
to obecny kierunek jest bardzo dobry.

Jeśli celem architektonicznym jest:
- pełna zgodność z oryginalnym repo-format v1,
to potrzebny byłby osobny etap refaktoryzacji lub druga generacja storage metadata layer.

Najkrótsze uczciwe podsumowanie:
- **produkt wyszedł lepszy niż dokument pod kątem używalności**
- **dokument pozostał lepszy niż produkt pod kątem formalnej czystości modelu repo**
