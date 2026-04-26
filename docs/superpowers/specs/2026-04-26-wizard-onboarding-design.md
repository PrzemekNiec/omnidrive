# Wizard Onboarding — Nowa strona wizard.html

**Data:** 2026-04-26
**Epic:** 36 (G.11 — usunięcie zależności od /legacy)

## Kontekst

Nowy `index.html` (Stitch redesign) nie posiada overlaya wizarda — przy braku skonfigurowanego Skarbca przekierowuje na `/legacy`. Celem jest wyeliminowanie tej zależności poprzez stworzenie dedykowanej strony `wizard.html` w stylu Stitcha.

`wizard.js` (573 linie) jest w pełni funkcjonalny i nie wymaga zmian — szuka stałego zestawu ID w DOM i renderuje do nich kroki wizarda.

## Decyzje projektowe

- **Layout:** Dedykowana pełnoekranowa strona (nie overlay na dashboardzie). Ciemny gradient jako tło, brak widocznego dashboardu za zasłoną.
- **Szerokość karty:** `max-w-xl` (~600px), wyśrodkowana. Krok wyboru trybu: 3 kolumny obok siebie.
- **Nawigacja kroków:** Kropki (6 kropek = 6 kroków) pod kartą. Aktualna kropka w kolorze primary (`#00daf3`).

## Architektura — zmiany

| Plik | Zmiana |
|---|---|
| `angeld/static/wizard.html` | Nowy plik — pełna strona onboardingu |
| `angeld/src/api/mod.rs` | Dodanie `GET /wizard` route |
| `angeld/static/index.html` | Zamiana 2× `'/legacy'` → `'/wizard'` |

`wizard.js` — bez zmian.

## Struktura wizard.html

```
<html> (dark, lang=pl)
  <head>
    Tailwind CDN + Inter font + Material Symbols
    Tailwind config (identyczny token set co index.html)
    <style> — glass-panel, wizard-progress, animacje
  </head>
  <body> — ciemny gradient radialny
    <!-- Logo + tytuł -->
    <header> — logo OmniDrive wyśrodkowane

    <!-- Karta wizarda -->
    <main>
      <section id="onboardingWizardPanel" class="glass-panel max-w-xl">
        <!-- Kicker + tytuł + opis (renderowane przez wizard.js) -->
        <p id="onboardingWizardStepKicker">
        <h1 id="onboardingWizardStepTitle">
        <p id="onboardingWizardStepDescription">

        <!-- Pasek postępu -->
        <div class="wizard-progress-track">
          <div id="onboardingWizardProgressBar">

        <!-- Bannery błędów/draftu -->
        <div id="onboardingWizardDraftBanner" class="hidden">
        <div id="onboardingWizardError" class="hidden">

        <!-- Treść kroku (renderowana przez wizard.js) -->
        <div id="onboardingWizardBody" class="min-h-[420px]">

        <!-- Nawigacja (Wstecz / secondary / Dalej) -->
        <footer>
          <button id="onboardingWizardBackButton">
          <button id="onboardingWizardSecondaryButton">
          <button id="onboardingWizardNextButton">
      </section>
    </main>

    <!-- Indykatory kroków (6 kropek) -->
    <nav aria-label="Kroki">
      6× <span class="step-dot" data-step="0..5">

    <!-- Overlay pełnoekranowy (wymagany przez wizard.js) -->
    <!-- Musi startować jako wizard-hidden — wizard.js wywołuje showWizard() po init() -->
    <!-- Edge case: jeśli status = COMPLETED, wizard.js woła hideWizard() i zwraca.     -->
    <!-- Dlatego dodajemy inline guard PRZED wizard.js: sprawdź status i redirect → /   -->
    <div id="onboardingWizardOverlay" class="wizard-hidden fixed inset-0 z-[9999] overflow-y-auto">

    <script src="/wizard.js">
  </body>
```

## Zachowane ID (wymagane przez wizard.js)

Wszystkie poniższe ID muszą istnieć w DOM w momencie uruchomienia `wizard.js`:

- `#onboardingWizardOverlay`
- `#onboardingWizardStepKicker`
- `#onboardingWizardStepTitle`
- `#onboardingWizardStepDescription`
- `#onboardingWizardStepCounter`
- `#onboardingWizardProgressBar`
- `#onboardingWizardDraftBanner`
- `#onboardingWizardError`
- `#onboardingWizardBody`
- `#onboardingWizardBackButton`
- `#onboardingWizardSecondaryButton`
- `#onboardingWizardNextButton`

## Styling

- Tło body: `radial-gradient(circle at top left, #1e293b, #0f172a, #020617)`
- Karta: klasa `.glass-panel` (identyczna co index.html: `bg-white/5 backdrop-blur-[20px] border border-white/10`)
- Primary color: `#00daf3` (identyczny token co index.html)
- Krok trybu: `grid grid-cols-3 gap-4` — 3 kafelki glassmorphism
- Kropki: `w-2 h-2 rounded-full`, aktywna = `bg-[#00daf3]`, nieaktywna = `bg-white/20`
- Pasek postępu: gradient `from-[#00daf3] to-[#4edea3]`

## Kryteria akceptacji

1. Świeże urządzenie (vault nie zainicjowany) → `index.html` przekierowuje na `/wizard`, nie `/legacy`
2. Po OAuth gdy `onboarding_state !== COMPLETED` → `/wizard`, nie `/legacy`
3. Wszystkie 6 kroków wizarda działają identycznie jak w `/legacy`
4. Po zakończeniu onboardingu wizard przekierowuje na `/` (dashboard)
5. `/legacy` nadal istnieje jako fallback — nie usuwamy go w tej sesji
