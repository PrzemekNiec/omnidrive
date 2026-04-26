# Wizard Onboarding — Nowa strona wizard.html

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Zastąpić zależność od `/legacy` dla onboardingu dedykowaną stroną `/wizard` w stylu Stitch (ciemny gradient, glassmorphism, max-w-xl), nie zmieniając logiki w `wizard.js`.

**Architecture:** Nowy plik `angeld/static/wizard.html` zawiera pełną stronę onboardingu z identycznym zestawem ID DOM, którego oczekuje `wizard.js`. Backend dostaje route `GET /wizard`. `index.html` wymienia dwa `location.replace('/legacy')` na `/wizard`. `wizard.js` — zero zmian.

**Tech Stack:** Tailwind CDN, Inter font, Material Symbols Outlined, Rust/Axum (`include_str!`), `wizard.js` (istniejący)

---

## Pliki

| Akcja | Ścieżka | Co robi |
|---|---|---|
| Utwórz | `angeld/static/wizard.html` | Pełna strona onboardingu — layout B |
| Modyfikuj | `angeld/src/api/mod.rs` | Dodanie route `GET /wizard` |
| Modyfikuj | `angeld/static/index.html` | 2× zamiana `/legacy` → `/wizard` |

---

## Task 1: Utwórz `wizard.html`

**Files:**
- Create: `angeld/static/wizard.html`

- [ ] **Krok 1.1: Napisz plik `wizard.html`**

Stwórz `angeld/static/wizard.html` z poniższą treścią. Plik jest kompletny — zawiera wszystkie ID wymagane przez `wizard.js`, layout B (dedykowana ciemna strona, karta `max-w-xl`, kropki nawigacji) i guard przekierowujący na `/` gdy onboarding już ukończony.

```html
<!DOCTYPE html>
<html class="dark" lang="pl">
<head>
  <meta charset="utf-8" />
  <meta content="width=device-width, initial-scale=1.0" name="viewport" />
  <title>OmniDrive — Konfiguracja</title>
  <script src="https://cdn.tailwindcss.com?plugins=forms,container-queries"></script>
  <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet" />
  <link href="https://fonts.googleapis.com/css2?family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&display=swap" rel="stylesheet" />
  <script id="tailwind-config">
    tailwind.config = {
      darkMode: "class",
      theme: {
        extend: {
          colors: {
            "primary": "#00daf3",
            "primary-container": "#00bcd2",
          },
          fontFamily: {
            "headline": ["Inter"],
            "body": ["Inter"],
            "label": ["Inter"]
          }
        }
      }
    };
  </script>
  <style>
    body {
      font-family: 'Inter', sans-serif;
      background: radial-gradient(ellipse at top left, #1e293b 0%, #0f172a 50%, #020617 100%);
      min-height: 100vh;
      color: #e2e8f0;
    }
    .glass-panel {
      background: rgba(255, 255, 255, 0.05);
      backdrop-filter: blur(20px);
      -webkit-backdrop-filter: blur(20px);
      border: 1px solid rgba(255, 255, 255, 0.1);
    }
    .glass-muted {
      background: rgba(255, 255, 255, 0.04);
      border: 1px solid rgba(255, 255, 255, 0.08);
    }
    .wizard-visible {
      opacity: 1;
      pointer-events: auto;
    }
    .wizard-hidden {
      opacity: 0;
      pointer-events: none;
    }
    .wizard-progress-track {
      background: rgba(255, 255, 255, 0.08);
    }
    .wizard-progress-fill {
      background: linear-gradient(90deg, #00daf3, #4edea3);
    }
    .step-dot {
      display: inline-block;
      width: 8px;
      height: 8px;
      border-radius: 50%;
      background: rgba(255, 255, 255, 0.2);
      transition: background 0.3s;
    }
    .step-dot.active {
      background: #00daf3;
    }
  </style>
</head>
<body class="flex min-h-screen flex-col items-center justify-center px-4 py-10">

  <!-- Guard: jeśli onboarding już ukończony → wróć do dashboardu -->
  <script>
    (async function () {
      try {
        const r = await fetch('/api/onboarding/status', { headers: { Accept: 'application/json' } });
        if (r.ok) {
          const s = await r.json();
          if (String(s.onboarding_state || '').toUpperCase() === 'COMPLETED') {
            location.replace('/');
          }
        }
      } catch (_) {}
    })();
  </script>

  <!-- Logo -->
  <header class="mb-8 flex flex-col items-center gap-3 text-center">
    <div class="flex h-12 w-12 items-center justify-center rounded-2xl bg-gradient-to-br from-[#00daf3] to-[#4edea3]">
      <span class="material-symbols-outlined text-[#020617] text-2xl" style="font-variation-settings:'FILL' 1,'wght' 600">lock</span>
    </div>
    <div>
      <p class="text-xs uppercase tracking-[0.3em] text-slate-400">OmniDrive</p>
      <h1 class="mt-1 text-xl font-semibold text-white">Konfiguracja Skarbca</h1>
    </div>
  </header>

  <!-- Overlay wymagany przez wizard.js — startuje jako wizard-hidden, wizard.js go pokaże -->
  <div
    id="onboardingWizardOverlay"
    class="wizard-hidden w-full"
    aria-hidden="true"
  >
    <div class="mx-auto w-full max-w-xl">
      <section id="onboardingWizardPanel" class="glass-panel w-full rounded-[32px] p-6 md:p-8">
        <div class="flex flex-col gap-5">

          <!-- Nagłówek kroku -->
          <div class="flex flex-col gap-4 md:flex-row md:items-start md:justify-between">
            <div class="flex-1">
              <p id="onboardingWizardStepKicker" class="text-xs uppercase tracking-[0.3em] text-slate-400">Konfiguracja początkowa</p>
              <h2 id="onboardingWizardStepTitle" class="mt-2 text-2xl font-semibold text-white">Przygotowanie OmniDrive</h2>
              <p id="onboardingWizardStepDescription" class="mt-3 text-sm leading-6 text-slate-300">
                Ładowanie stanu pierwszego uruchomienia...
              </p>
            </div>
            <div class="glass-muted shrink-0 rounded-2xl px-4 py-3 text-right">
              <p class="text-xs uppercase tracking-[0.22em] text-slate-500">Postęp</p>
              <p id="onboardingWizardStepCounter" class="mt-2 text-lg font-semibold text-white">Krok 1 / 6</p>
            </div>
          </div>

          <!-- Pasek postępu -->
          <div class="wizard-progress-track h-2 overflow-hidden rounded-full">
            <div id="onboardingWizardProgressBar" class="wizard-progress-fill h-full rounded-full transition-all duration-300" style="width: 16.66%;"></div>
          </div>

          <!-- Banery stanu -->
          <div id="onboardingWizardDraftBanner" class="hidden rounded-2xl border border-cyan-400/20 bg-cyan-500/10 px-4 py-4 text-sm text-cyan-100"></div>
          <div id="onboardingWizardError" class="hidden rounded-2xl border border-rose-500/30 bg-rose-500/10 px-4 py-4 text-sm text-rose-100"></div>

          <!-- Treść kroku (renderowana przez wizard.js) -->
          <div id="onboardingWizardBody" class="min-h-[420px]"></div>

          <!-- Nawigacja -->
          <div class="flex flex-col gap-3 border-t border-white/10 pt-5 sm:flex-row sm:items-center sm:justify-between">
            <button
              id="onboardingWizardBackButton"
              class="inline-flex items-center justify-center rounded-xl border border-white/10 bg-white/5 px-4 py-2 text-sm font-medium text-slate-100 transition hover:border-white/20 hover:bg-white/10"
            >
              Wstecz
            </button>
            <div class="flex flex-col gap-3 sm:flex-row">
              <button
                id="onboardingWizardSecondaryButton"
                class="hidden inline-flex items-center justify-center rounded-xl border border-white/10 bg-white/5 px-4 py-2 text-sm font-medium text-slate-100 transition hover:border-white/20 hover:bg-white/10"
              >
                Akcja dodatkowa
              </button>
              <button
                id="onboardingWizardNextButton"
                class="inline-flex items-center justify-center rounded-xl bg-[#00daf3] px-5 py-2.5 text-sm font-semibold text-[#00363d] transition hover:bg-[#00bcd2]"
              >
                Dalej
              </button>
            </div>
          </div>

        </div>
      </section>
    </div>
  </div>

  <!-- Indykatory kroków -->
  <nav aria-label="Kroki" class="mt-6 flex items-center gap-2">
    <span class="step-dot active" data-step="0" aria-label="Krok 1"></span>
    <span class="step-dot" data-step="1" aria-label="Krok 2"></span>
    <span class="step-dot" data-step="2" aria-label="Krok 3"></span>
    <span class="step-dot" data-step="3" aria-label="Krok 4"></span>
    <span class="step-dot" data-step="4" aria-label="Krok 5"></span>
    <span class="step-dot" data-step="5" aria-label="Krok 6"></span>
  </nav>

  <!-- Sync kropek z aktualnym krokiem wizarda -->
  <script>
    (function () {
      const dots = document.querySelectorAll('.step-dot');
      function syncDots() {
        const counter = document.getElementById('onboardingWizardStepCounter');
        if (!counter) return;
        const m = counter.textContent.match(/Krok\s+(\d+)/i);
        if (!m) return;
        const current = parseInt(m[1], 10) - 1;
        dots.forEach((d, i) => d.classList.toggle('active', i === current));
      }
      const observer = new MutationObserver(syncDots);
      const target = document.getElementById('onboardingWizardStepCounter');
      if (target) observer.observe(target, { childList: true, subtree: true, characterData: true });
    })();
  </script>

  <script src="/wizard.js"></script>
</body>
</html>
```

- [ ] **Krok 1.2: Sprawdź że wszystkie wymagane ID są obecne**

```bash
```bash
for id in onboardingWizardOverlay onboardingWizardStepKicker onboardingWizardStepTitle \
           onboardingWizardStepDescription onboardingWizardStepCounter \
           onboardingWizardProgressBar onboardingWizardDraftBanner onboardingWizardError \
           onboardingWizardBody onboardingWizardBackButton \
           onboardingWizardSecondaryButton onboardingWizardNextButton; do
  grep -q "id=\"$id\"" angeld/static/wizard.html && echo "OK: $id" || echo "BRAK: $id"
done
```

Oczekiwany output: 12× `OK: ...` bez żadnego `BRAK`.

---

## Task 2: Dodaj route `GET /wizard` w Rust

**Files:**
- Modify: `angeld/src/api/mod.rs`

- [ ] **Krok 2.1: Dodaj handler `get_wizard` obok `get_legacy`**

W pliku `angeld/src/api/mod.rs`, po linii z `get_legacy` (około linii 191), dodaj nową funkcję:

```rust
async fn get_wizard() -> Html<&'static str> {
    Html(include_str!("../../static/wizard.html"))
}
```

- [ ] **Krok 2.2: Zarejestruj route w Router**

W tym samym pliku, w bloku `let app = Router::new()` (około linii 131-133), dodaj route tuż po `.route("/legacy", get(get_legacy))`:

```rust
.route("/wizard", get(get_wizard))
```

Tak żeby fragment wyglądał tak:

```rust
let app = Router::new()
    .route("/", get(get_index))
    .route("/legacy", get(get_legacy))
    .route("/wizard", get(get_wizard))
    .route("/wizard.js", get(get_wizard_js))
    // ...
```

- [ ] **Krok 2.3: `cargo check`**

```bash
cargo check --workspace 2>&1 | tail -5
```

Oczekiwany output: `Finished` bez żadnych `error`. Warningi są OK.

---

## Task 3: Przekieruj `index.html` na `/wizard` zamiast `/legacy`

**Files:**
- Modify: `angeld/static/index.html`

- [ ] **Krok 3.1: Zamień redirect po OAuth (linia ~3193)**

W `angeld/static/index.html` znajdź blok:

```javascript
    // ── L: Guard — redirect to /legacy if onboarding not done ─
    async function oauthPostLoginGuard() {
      try {
        const res = await fetch('/api/onboarding/status', { headers: { Accept: 'application/json' } });
        if (!res.ok) return;
        const status = await res.json();
        if (String(status.onboarding_state || '').toUpperCase() !== 'COMPLETED') {
          window.location.href = '/legacy';
        }
      } catch (_) {}
    }
```

Zmień `'/legacy'` na `'/wizard'`:

```javascript
    // ── L: Guard — redirect to /wizard if onboarding not done ─
    async function oauthPostLoginGuard() {
      try {
        const res = await fetch('/api/onboarding/status', { headers: { Accept: 'application/json' } });
        if (!res.ok) return;
        const status = await res.json();
        if (String(status.onboarding_state || '').toUpperCase() !== 'COMPLETED') {
          window.location.href = '/wizard';
        }
      } catch (_) {}
    }
```

- [ ] **Krok 3.2: Zamień redirect przy braku vault (linia ~3370)**

W `angeld/static/index.html` znajdź blok:

```javascript
          } else if (data && data.initialized === false) {
            // Fresh device — no vault configured yet, go straight to wizard
            location.replace('/legacy');
          } else {
```

Zmień `'/legacy'` na `'/wizard'`:

```javascript
          } else if (data && data.initialized === false) {
            // Fresh device — no vault configured yet, go straight to wizard
            location.replace('/wizard');
          } else {
```

- [ ] **Krok 3.3: Weryfikacja — brak pozostałych redirectów do `/legacy` w index.html**

```bash
grep -n "'/legacy'" angeld/static/index.html
```

Oczekiwany output: **pusta linia** (zero wyników). Jeśli coś jest — popraw.

---

## Task 4: Build + weryfikacja kompilacji

**Files:** brak nowych

- [ ] **Krok 4.1: Pełny build release**

```bash
cargo build --release --workspace 2>&1 | tail -10
```

Oczekiwany output: `Finished release` bez `error[E...]`.

- [ ] **Krok 4.2: Sprawdź że `/wizard` jest w binarce**

```bash
strings target/release/angeld.exe | grep -c "Konfiguracja Skarbca"
```

Oczekiwany output: `1` (tekst z `wizard.html` jest osadzony w binarce przez `include_str!`).

---

## Task 5: Smoke test ręczny

> Nie ma testów automatycznych dla renderowania HTML — wykonaj ten task ręcznie z działającym daemonem.

- [ ] **Krok 5.1: Uruchom daemona w terminalu**

```bash
./target/release/angeld.exe
```

Daemon powinien wypisać `api server listening on http://127.0.0.1:PORT`.

- [ ] **Krok 5.2: Otwórz `http://127.0.0.1:PORT/wizard` w przeglądarce**

Sprawdź:
- [ ] Strona ładuje się (ciemne tło gradientowe, nie biała strona)
- [ ] Logo OmniDrive widoczne na górze
- [ ] Karta glassmorphism z tekstem "Przygotowanie OmniDrive" lub pierwszym krokiem
- [ ] 6 kropek nawigacji na dole, pierwsza podświetlona na `#00daf3`
- [ ] Przycisk "Dalej" w kolorze primary (cyan)
- [ ] Brak błędów w konsoli DevTools (F12)

- [ ] **Krok 5.3: Przejdź przez krok 1 → tryb wyboru**

Kliknij "Dalej" na Welcome. Sprawdź:
- [ ] Wyświetlają się 3 kafelki: Lokalny / Cloud-backed / Dołącz
- [ ] Aktywna kropka przesuwa się na pozycję 2
- [ ] Pasek postępu rośnie

- [ ] **Krok 5.4: Sprawdź że `http://127.0.0.1:PORT/legacy` nadal działa**

Otwórz `/legacy` — stary panel musi być dostępny (nie usuwamy go w tej sesji).

---

## Task 6: Commit

- [ ] **Krok 6.1: Staging i commit**

```bash
git add angeld/static/wizard.html angeld/src/api/mod.rs angeld/static/index.html docs/superpowers/specs/2026-04-26-wizard-onboarding-design.md docs/superpowers/plans/2026-04-26-wizard-onboarding.md
git commit -m "$(cat <<'EOF'
feat(ui): nowa strona /wizard — onboarding bez zależności od /legacy (Epic 36 G.11)

Tworzy angeld/static/wizard.html z layoutem B (dedykowana ciemna strona,
glassmorphism, max-w-xl, 6-krokowe kropki). wizard.js nie zmieniony —
szuka tych samych ID w DOM. index.html przekierowuje na /wizard zamiast /legacy.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Kryteria akceptacji (z spec)

1. Świeże urządzenie → `index.html` przekierowuje na `/wizard`, nie `/legacy`
2. Po OAuth gdy `onboarding_state !== COMPLETED` → `/wizard`, nie `/legacy`
3. Wszystkie 6 kroków wizarda działają (weryfikacja ręczna w Task 5)
4. `/legacy` nadal dostępny jako fallback
5. `cargo build --release --workspace` przechodzi bez błędów
