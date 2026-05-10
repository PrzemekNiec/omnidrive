# Wizard Redesign + Tray Icons Fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace all wizard step body HTML with approved single-column designs, fix missing tray icons in installer payload, and ship v0.3.7.

**Architecture:** Pure front-end change — only `angeld/static/wizard.js` `stepBody()` is touched. No Rust changes. Two new JS helper functions added. Payload sync + version bump + installer rebuild complete the release.

**Tech Stack:** Vanilla JS, Tailwind CSS (CDN), existing glass-panel CSS in `wizard.html`, Inno Setup 6 for installer.

---

## Files Changed

| File | Change |
|---|---|
| `angeld/static/wizard.js` | Replace `stepBody()` + remove `statusClass()` + add `statusBadge()`, `modeDescription()`, `providerStatusBanner()` |
| `dist/installer/payload/static/wizard.js` | Sync from `angeld/static/wizard.js` |
| `dist/installer/payload/icons/tray_icons/` | Create dir + copy 5 PNGs from `icons/tray_icons/` |
| `angeld/Cargo.toml` | `0.3.6` → `0.3.7` |
| `omnidrive-core/Cargo.toml` | `0.3.6` → `0.3.7` |
| `angelctl/Cargo.toml` | `0.3.6` → `0.3.7` |
| `omnidrive-tray/Cargo.toml` | `0.3.6` → `0.3.7` |
| `omnidrive-shell-ext/Cargo.toml` | `0.3.6` → `0.3.7` |
| `omnidrive-cli/Cargo.toml` | `0.3.6` → `0.3.7` |
| `installer/omnidrive.iss` | `AppVersion=0.3.6` → `0.3.7` |
| `dist/installer/payload/*.exe` | Rebuilt binaries |

---

## Task 1: Add helper functions to wizard.js

**Files:**
- Modify: `angeld/static/wizard.js` — add 3 helpers before `stepBody()`, remove `statusClass()`

- [ ] **Step 1: Open `angeld/static/wizard.js` and locate the helpers block**

Find the `statusClass` function (around line 182) and the `stepBody` function (around line 240). We will replace `statusClass` with `statusBadge`, and add `modeDescription` and `providerStatusBanner` right before `stepBody`.

- [ ] **Step 2: Replace `statusClass()` with `statusBadge()` and add new helpers**

Replace the entire `statusClass` function block:
```js
  function statusClass(status) {
    const normalized = String(status || "").toUpperCase();
    if (normalized === "ERROR") return "status-error";
    if (normalized === "WARN") return "status-warn";
    return "status-ok";
  }
```

With these three functions:
```js
  function statusBadge(status) {
    const s = String(status || "").toUpperCase();
    const base = "ml-1 inline-flex rounded-full border px-1.5 py-0.5 text-[10px] font-semibold";
    if (s === "OK")    return `<span class="${base}" style="background:rgba(16,185,129,0.12);border-color:rgba(16,185,129,0.3);color:#6ee7b7">OK</span>`;
    if (s === "ERROR") return `<span class="${base}" style="background:rgba(239,68,68,0.12);border-color:rgba(239,68,68,0.3);color:#fca5a5">ERR</span>`;
    return `<span class="${base}" style="background:rgba(234,179,8,0.12);border-color:rgba(234,179,8,0.3);color:#fde047">${escape(status || "—")}</span>`;
  }

  function modeDescription(mode) {
    if (mode === "local") return "OmniDrive startuje z działającym lokalnym Skarbcem. Dostawcy chmurowi i shared-vault rozszerzają bazę zamiast ją blokować.";
    if (mode === "cloud") return "Zweryfikuj R2, B2 lub Scaleway. OmniDrive zsynchronizuje Skarbiec z wybranymi dostawcami.";
    if (mode === "join")  return "OmniDrive pobierze zaszyfrowaną migawkę metadanych, odszyfruje ją lokalnie i przeprowadzi grafting tożsamości zdalnego Skarbca.";
    return "Wybierz tryb żeby zobaczyć opis.";
  }

  function providerStatusBanner(p) {
    const status = (p.validation?.status || p.last_test_status || "").toUpperCase();
    if (status === "OK") return `
      <div class="rounded-xl px-4 py-3" style="background:rgba(16,185,129,0.08);border:1px solid rgba(16,185,129,0.25);">
        <p class="text-xs font-semibold text-emerald-300">${escape(providerHeadline(p))}</p>
        <p class="mt-1 text-xs text-slate-400">${escape(providerDetails(p))}</p>
        <p class="mt-1 text-[10px] text-slate-500">Ostatni test: ${formatTs(p.last_test_at)}</p>
      </div>`;
    if (status === "ERROR") return `
      <div class="rounded-xl px-4 py-3" style="background:rgba(239,68,68,0.08);border:1px solid rgba(239,68,68,0.25);">
        <p class="text-xs font-semibold text-rose-300">${escape(providerHeadline(p))}</p>
        <p class="mt-1 text-xs text-slate-400">${escape(providerDetails(p))}</p>
      </div>`;
    return `
      <div class="rounded-xl px-4 py-3" style="background:rgba(255,255,255,0.03);border:1px solid rgba(255,255,255,0.08);">
        <p class="text-xs text-slate-400">${escape(providerDetails(p))}</p>
        <p class="mt-1 text-[10px] text-slate-500">Ostatni test: ${formatTs(p.last_test_at)}</p>
      </div>`;
  }
```

- [ ] **Step 3: Also remove the `field()` helper** (no longer used in new stepBody)

Delete:
```js
  function field(label, id, value, placeholder) {
    return `<label class="block text-sm text-slate-300"><span class="text-xs uppercase tracking-[0.22em] text-slate-500">${escape(label)}</span><input id="${id}" type="${id === "wizardProviderSecretKey" ? "password" : "text"}" value="${escape(value || "")}" class="mt-3 w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none transition focus:border-white/20" placeholder="${escape(placeholder)}" autocomplete="off" /></label>`;
  }
```

---

## Task 2: Replace `stepBody()` — Step 0 (Powitanie)

**Files:**
- Modify: `angeld/static/wizard.js` — replace `if (st.step === 0)` branch

- [ ] **Step 1: Replace the step 0 return block**

Find and replace the entire `if (st.step === 0) { return \`...\`; }` block with:

```js
    if (st.step === 0) {
      return `
        <div class="flex flex-col gap-4">
          <div class="grid grid-cols-2 gap-3">
            <div class="glass-muted rounded-2xl p-4">
              <p class="text-[10px] uppercase tracking-[.18em] text-slate-500">Skarbiec</p>
              <p class="mt-2 text-base font-semibold text-white">${escape(st.onboarding?.onboarding_state || "INITIAL")}</p>
            </div>
            <div class="glass-muted rounded-2xl p-4">
              <p class="text-[10px] uppercase tracking-[.18em] text-slate-500">Tryb</p>
              <p class="mt-2 text-base font-semibold text-white">${escape(modeLabel(st.mode))}</p>
            </div>
            <div class="glass-muted rounded-2xl p-4">
              <p class="text-[10px] uppercase tracking-[.18em] text-slate-500">Urządzenie</p>
              <p class="mt-2 text-sm font-semibold text-white break-words">${escape(st.identity.device_name || "To urządzenie")}</p>
              <p class="mt-1 text-xs text-slate-500">${escape(st.identity.device_id || "ID zostanie nadane po zapisaniu tożsamości")}</p>
            </div>
            <div class="glass-muted rounded-2xl p-4">
              <p class="text-[10px] uppercase tracking-[.18em] text-slate-500">Konto Google</p>
              <a href="/api/auth/google/start" class="mt-2 flex items-center gap-2 text-sm text-primary hover:underline">
                <span class="material-symbols-outlined text-base">account_circle</span>Zaloguj przez Google
              </a>
            </div>
          </div>
          <div class="glass-muted rounded-2xl px-4 py-3">
            <p class="text-sm text-slate-300">OmniDrive startuje z działającym lokalnym Skarbcem i aktywnym dashboardem. Dostawcy chmurowi i tryb shared-vault rozszerzają bazę, zamiast ją blokować.</p>
          </div>
        </div>`;
    }
```

---

## Task 3: Replace `stepBody()` — Step 1 (Tryb pracy)

**Files:**
- Modify: `angeld/static/wizard.js` — replace `if (st.step === 1)` branch

- [ ] **Step 1: Replace the step 1 return block**

Find and replace the entire `if (st.step === 1) { return \`...\`; }` block with:

```js
    if (st.step === 1) {
      const modes = [
        { id: "local", icon: "computer",   label: "Utwórz lokalny Skarbiec",        subtitle: "Local-first · dysk O: od razu" },
        { id: "cloud", icon: "cloud_sync",  label: "Podłącz dostawców chmurowych",   subtitle: "R2, B2, Scaleway" },
        { id: "join",  icon: "link",        label: "Dołącz do istniejącego Skarbca", subtitle: "Restore metadanych" },
      ];
      return `
        <div class="flex flex-col gap-4">
          <div class="glass-muted rounded-[24px] p-1.5 flex flex-col gap-1">
            ${modes.map((m) => {
              const sel = st.mode === m.id;
              return `<button type="button" data-mode="${m.id}" class="flex items-center gap-4 rounded-[18px] px-4 py-3 text-left transition ${sel ? "" : "hover:bg-white/5"}" ${sel ? 'style="background:rgba(0,218,243,0.1);border:1px solid rgba(0,218,243,0.35);"' : ""}>
                <div class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full border-2" style="${sel ? "border-color:#00daf3;background:#00daf3;" : "border-color:rgba(255,255,255,0.2);"}">
                  ${sel ? '<div class="h-2 w-2 rounded-full bg-[#00363d]"></div>' : ""}
                </div>
                <div class="flex-1">
                  <p class="text-sm font-semibold text-white">${escape(m.label)}</p>
                  <p class="text-xs text-slate-400">${escape(m.subtitle)}</p>
                </div>
                <span class="material-symbols-outlined text-slate-500" style="font-size:16px;">${m.icon}</span>
              </button>`;
            }).join("")}
          </div>
          <div class="glass-muted rounded-[20px] px-5 py-4" style="border-left:3px solid #00daf3;">
            <p class="text-[10px] uppercase tracking-[.18em] text-[#00daf3]">Wybrany tryb</p>
            <p class="mt-1.5 text-sm font-semibold text-white">${escape(modeLabel(st.mode))}</p>
            <p class="mt-1 text-sm leading-6 text-slate-300">${escape(modeDescription(st.mode))}</p>
          </div>
        </div>`;
    }
```

---

## Task 4: Replace `stepBody()` — Step 2 (Tożsamość)

**Files:**
- Modify: `angeld/static/wizard.js` — replace `if (st.step === 2)` branch

- [ ] **Step 1: Replace the step 2 return block**

```js
    if (st.step === 2) {
      return `
        <div class="flex flex-col gap-4">
          <label class="flex flex-col gap-2">
            <span class="text-[10px] uppercase tracking-[.18em] text-slate-500">Nazwa urządzenia</span>
            <input id="wizardDeviceNameInput" type="text" value="${escape(st.identity.device_name || "")}" class="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-base text-white outline-none transition focus:border-white/20" placeholder="np. Dell-Laptop" maxlength="80" />
            <p class="text-xs text-slate-500">Widoczna w kartach peerów LAN, historii rewizji i kopiach konfliktowych.</p>
          </label>
          <div class="glass-muted rounded-2xl px-4 py-4">
            <p class="text-[10px] uppercase tracking-[.18em] text-slate-500 mb-2">Device ID</p>
            <p class="text-sm break-all ${st.identity.device_id ? "font-semibold text-white" : "text-slate-400"}">${escape(st.identity.device_id || "ID zostanie nadane po zapisaniu tożsamości.")}</p>
            <p class="mt-2 text-xs text-slate-500">OmniDrive utrzymuje stabilną tożsamość urządzenia dla każdej instalacji.</p>
          </div>
        </div>`;
    }
```

---

## Task 5: Replace `stepBody()` — Step 3 (Dostawcy)

**Files:**
- Modify: `angeld/static/wizard.js` — replace `if (st.step === 3)` branch

- [ ] **Step 1: Replace the step 3 return block**

```js
    if (st.step === 3) {
      const p = st.providers[st.selectedProvider];
      const s = st.secrets[st.selectedProvider];
      const validated = ORDER.filter((n) => st.providers[n].enabled && String(st.providers[n].last_test_status || "").toUpperCase() === "OK").length;
      return `
        <div class="flex flex-col gap-4">
          <div class="glass-muted rounded-2xl p-1 flex gap-1">
            ${ORDER.map((name) => {
              const pv = st.providers[name];
              const active = name === st.selectedProvider;
              return `<button type="button" data-provider="${name}" class="flex-1 rounded-xl px-3 py-2 text-sm transition ${active ? "font-semibold text-[#00daf3]" : "font-medium text-slate-400 hover:bg-white/5"}" ${active ? 'style="background:rgba(0,218,243,0.12);border:1px solid rgba(0,218,243,0.3);"' : ""}>
                <span class="text-xs uppercase tracking-[.12em]">${PROVIDERS[name].short}</span>${statusBadge(pv.last_test_status)}
              </button>`;
            }).join("")}
          </div>
          <div class="glass-muted rounded-[20px] p-4 flex flex-col gap-3">
            <div class="flex items-center justify-between">
              <p class="text-sm font-semibold text-white">${escape(PROVIDERS[st.selectedProvider].name)}</p>
              ${statusBadge(p.last_test_status)}
            </div>
            <label class="flex flex-col gap-1.5">
              <span class="text-[10px] uppercase tracking-[.18em] text-slate-500">Endpoint dostawcy</span>
              <input id="wizardProviderEndpoint" type="text" value="${escape(p.endpoint)}" class="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none transition focus:border-white/20" placeholder="https://&lt;account&gt;.r2.cloudflarestorage.com" autocomplete="off" />
            </label>
            <div class="grid grid-cols-2 gap-3">
              <label class="flex flex-col gap-1.5">
                <span class="text-[10px] uppercase tracking-[.18em] text-slate-500">Bucket</span>
                <input id="wizardProviderBucket" type="text" value="${escape(p.bucket)}" class="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none transition focus:border-white/20" placeholder="omnidrive-prod" autocomplete="off" />
              </label>
              <label class="flex flex-col gap-1.5">
                <span class="text-[10px] uppercase tracking-[.18em] text-slate-500">Region</span>
                <input id="wizardProviderRegion" type="text" value="${escape(p.region)}" class="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none transition focus:border-white/20" placeholder="${escape(PROVIDERS[st.selectedProvider].region || "eu-west-1")}" autocomplete="off" />
              </label>
            </div>
            <div class="grid grid-cols-2 gap-3">
              <label class="flex flex-col gap-1.5">
                <span class="text-[10px] uppercase tracking-[.18em] text-slate-500">Access Key</span>
                <input id="wizardProviderAccessKey" type="text" value="${escape(s.access_key_id)}" class="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none transition focus:border-white/20" placeholder="${p.access_key_status === "SET" ? "Zapisany access key [SET]" : "AKIA..."}" autocomplete="off" />
              </label>
              <label class="flex flex-col gap-1.5">
                <span class="text-[10px] uppercase tracking-[.18em] text-slate-500">Secret Key</span>
                <input id="wizardProviderSecretKey" type="password" value="${escape(s.secret_access_key)}" class="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none transition focus:border-white/20" placeholder="${p.secret_key_status === "SET" ? "Zapisany sekret [SET]" : "Wklej secret..."}" autocomplete="new-password" />
              </label>
            </div>
            <div class="grid grid-cols-2 gap-3">
              <label class="glass-panel flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm text-slate-200 cursor-pointer">
                <input id="wizardProviderEnabled" type="checkbox" class="h-4 w-4 rounded border-slate-700 bg-slate-900" ${p.enabled ? "checked" : ""} />Włącz dla Skarbca
              </label>
              <label class="glass-panel flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm text-slate-200 cursor-pointer">
                <input id="wizardProviderForcePathStyle" type="checkbox" class="h-4 w-4 rounded border-slate-700 bg-slate-900" ${p.force_path_style ? "checked" : ""} />Path-style
              </label>
            </div>
            ${providerStatusBanner(p)}
            <div class="flex items-center justify-between">
              <p class="text-xs text-slate-500">${validated > 0 ? `${validated} dostawca(ów) zweryfikowanych.` : "Żaden dostawca nie przeszedł jeszcze walidacji."}</p>
              <button id="wizardTestProviderButton" class="inline-flex items-center justify-center rounded-xl border border-white/10 bg-white/10 px-4 py-2 text-sm font-medium text-white transition hover:border-white/20 hover:bg-white/15 disabled:cursor-not-allowed disabled:opacity-60" ${p.busy ? "disabled" : ""}>${p.busy ? "Testowanie połączenia..." : "Testuj połączenie"}</button>
            </div>
          </div>
        </div>`;
    }
```

---

## Task 6: Replace `stepBody()` — Step 4 (Bezpieczeństwo)

**Files:**
- Modify: `angeld/static/wizard.js` — replace `if (st.step === 4)` branch

- [ ] **Step 1: Replace the step 4 return block**

```js
    if (st.step === 4) {
      return `
        <div class="flex flex-col gap-4">
          <label class="flex flex-col gap-2">
            <span class="text-[10px] uppercase tracking-[.18em] text-slate-500">Hasło główne (Master Passphrase)</span>
            <input id="wizardPassphrase" type="password" value="${escape(st.security.passphrase)}" class="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none transition focus:border-white/20" placeholder="${st.mode === "local" ? "Opcjonalne na teraz" : "Wpisz hasło główne"}" autocomplete="new-password" />
          </label>
          <label class="flex flex-col gap-2">
            <span class="text-[10px] uppercase tracking-[.18em] text-slate-500">Potwierdź hasło</span>
            <input id="wizardPassphraseConfirm" type="password" value="${escape(st.security.confirm)}" class="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none transition focus:border-white/20" placeholder="Powtórz hasło" autocomplete="new-password" />
          </label>
          <div class="glass-muted rounded-2xl px-4 py-3">
            <p class="text-sm font-medium text-white">${escape(st.mode === "local" ? "Opcjonalne w trybie local-only w tej wersji." : st.mode === "join" ? "Wymagane do odszyfrowania metadanych z istniejącego Skarbca." : "Wymagane przed finalizacją onboardingu cloud-backed.")}</p>
            <p class="mt-2 text-xs text-slate-400">Hasło pozostaje wyłącznie w pamięci przeglądarki podczas sesji kreatora i jest wysyłane tylko raz do kroku restore/finalize.</p>
          </div>
          <div class="glass-muted rounded-2xl px-4 py-3">
            <p class="text-sm font-medium text-white">Co stanie się dalej?</p>
            <p class="mt-2 text-xs text-slate-400">${escape(st.mode === "join" ? "OmniDrive pobierze zaszyfrowaną migawkę metadanych, odszyfruje ją lokalnie, przeprowadzi grafting tożsamości zdalnego Skarbca i od razu zmaterializuje placeholdery na O:." : "Hasło przygotowuje zaszyfrowane kopie metadanych i przyszłe odzyskiwanie w konfiguracjach cloud-backed.")}</p>
          </div>
        </div>`;
    }
```

---

## Task 7: Replace `stepBody()` — Step 5 (Finalizacja)

**Files:**
- Modify: `angeld/static/wizard.js` — replace the final `return` block (step 5)

- [ ] **Step 1: Replace the step 5 return block**

The current code falls through to a bare `return \`...\`` at the end of `stepBody()`. Replace that final return with:

```js
    // step 5 — Finalizacja
    const verifiedProviders = ORDER.filter((n) => st.providers[n].enabled && String(st.providers[n].last_test_status || "").toUpperCase() === "OK");
    const verifiedNames = verifiedProviders.map((n) => PROVIDERS[n].short).join(", ");
    const isJoin = st.mode === "join";
    return `
      <div class="flex flex-col gap-4">
        <div class="grid grid-cols-2 gap-3">
          <div class="glass-panel rounded-2xl p-4">
            <p class="text-[10px] uppercase tracking-[.18em] text-slate-500">Wybrany tryb</p>
            <p class="mt-2 text-sm font-semibold text-white">${escape(modeLabel(st.mode))}</p>
            <p class="mt-1 text-xs text-slate-400">${isJoin ? "Restore metadanych uruchomi się teraz." : "Gotowe do uruchomienia"}</p>
          </div>
          <div class="glass-panel rounded-2xl p-4">
            <p class="text-[10px] uppercase tracking-[.18em] text-slate-500">Urządzenie</p>
            <p class="mt-2 text-sm font-semibold text-white break-words">${escape(st.identity.device_name || "Nienazwane urządzenie")}</p>
            <p class="mt-1 text-xs text-slate-400 break-all">${escape(st.identity.device_id || "Tożsamość nie zapisana")}</p>
          </div>
          <div class="glass-panel rounded-2xl p-4">
            <p class="text-[10px] uppercase tracking-[.18em] text-slate-500">Zweryfikowani dostawcy</p>
            <p class="mt-2 text-2xl font-semibold text-white">${verifiedProviders.length}</p>
            <p class="mt-1 text-xs text-slate-400">${escape(verifiedNames || "Brak")}</p>
          </div>
          <div class="glass-panel rounded-2xl p-4">
            <p class="text-[10px] uppercase tracking-[.18em] text-slate-500">Hasło</p>
            <p class="mt-2 text-sm font-semibold text-white">${st.security.passphraseProvided ? "Gotowe w pamięci" : "Nie podano"}</p>
            <p class="mt-1 text-xs text-slate-400">${escape(st.mode === "local" ? "Opcjonalne dla local-only" : st.mode === "join" ? "Wymagane do restore" : "Wymagane dla cloud-backed")}</p>
          </div>
        </div>
        <div class="rounded-2xl px-5 py-4" style="${isJoin ? "background:rgba(0,218,243,0.08);border:1px solid rgba(0,218,243,0.25);" : "background:rgba(16,185,129,0.08);border:1px solid rgba(16,185,129,0.2);"}">
          <p class="text-sm font-semibold ${isJoin ? "text-cyan-200" : "text-emerald-300"}">${isJoin ? "Gotowe do dołączenia do istniejącego Skarbca." : "Gotowe do uruchomienia OmniDrive."}</p>
          <p class="mt-2 text-sm text-slate-300">${escape(isJoin ? "OmniDrive odtworzy zaszyfrowane metadane od wybranego dostawcy, przeprowadzi grafting współdzielonej tożsamości Skarbca i przemontuje O: do odtworzonego widoku sync-root." : "Zakończenie tego kroku płynnie ukryje kreator i pozostawi dashboard uruchomiony w wybranym trybie onboardingu.")}</p>
        </div>
      </div>`;
```

- [ ] **Step 2: Commit wizard.js changes**

```bash
git add angeld/static/wizard.js
git commit -m "feat(ui): wizard single-column redesign — all 6 steps"
```

---

## Task 8: Test wizard in browser

**Files:** none (read-only verification)

- [ ] **Step 1: Start a test daemon with fresh DB on alternate port**

```bash
OMNIDRIVE_DRY_RUN=1 OMNIDRIVE_API_BIND=127.0.0.1:8788 OMNIDRIVE_DB_URL="sqlite:tmp-wizard-test.db" target/release/angeld.exe
```

If `target/release/angeld.exe` is stale, run `cargo build --release -p angeld` first (takes ~1-2 min).

- [ ] **Step 2: Open wizard in browser**

Open: `http://127.0.0.1:8788/wizard.html`

Walk through all 6 steps and verify:
- Step 1: 2×2 status grid + info strip
- Step 2: Radio list (click each option → description card updates)
- Step 3: Input field + device ID card
- Step 4: R2/B2/SCW tabs + form (click tabs → form updates)
- Step 5: Two password fields + two info cards
- Step 6: 2×2 summary + green/cyan banner

- [ ] **Step 3: Kill test daemon and delete temp DB**

```bash
taskkill /F /IM angeld.exe
del tmp-wizard-test.db
del tmp-wizard-test.db-wal
del tmp-wizard-test.db-shm
```

---

## Task 9: Sync wizard.js to payload + copy tray_icons

**Files:**
- Create: `dist/installer/payload/icons/tray_icons/` (directory + 5 PNGs)
- Modify: `dist/installer/payload/static/wizard.js`

- [ ] **Step 1: Sync wizard.js to payload**

```bash
cp angeld/static/wizard.js dist/installer/payload/static/wizard.js
```

- [ ] **Step 2: Create tray_icons directory in payload and copy PNGs**

```bash
mkdir -p dist/installer/payload/icons/tray_icons
cp icons/tray_icons/BASE_CLOUD.png   dist/installer/payload/icons/tray_icons/
cp icons/tray_icons/STATE_ERROR.png  dist/installer/payload/icons/tray_icons/
cp icons/tray_icons/STATE_LOCKED.png dist/installer/payload/icons/tray_icons/
cp icons/tray_icons/STATE_SYNCED.png dist/installer/payload/icons/tray_icons/
cp icons/tray_icons/STATE_SYNCING.png dist/installer/payload/icons/tray_icons/
```

- [ ] **Step 3: Verify**

```bash
ls dist/installer/payload/icons/tray_icons/
```

Expected output: `BASE_CLOUD.png  STATE_ERROR.png  STATE_LOCKED.png  STATE_SYNCED.png  STATE_SYNCING.png`

- [ ] **Step 4: Commit**

```bash
git add dist/installer/payload/static/wizard.js dist/installer/payload/icons/tray_icons/
git commit -m "fix(installer): add tray_icons PNGs to payload, sync wizard.js"
```

---

## Task 10: Version bump 0.3.6 → 0.3.7

**Files:** 6× `Cargo.toml` + `installer/omnidrive.iss`

- [ ] **Step 1: Bump all Cargo.toml files**

In each of these files, change `version = "0.3.6"` to `version = "0.3.7"`:
- `Cargo.toml` (workspace root — if present)
- `angeld/Cargo.toml`
- `omnidrive-core/Cargo.toml`
- `angelctl/Cargo.toml`
- `omnidrive-tray/Cargo.toml`
- `omnidrive-shell-ext/Cargo.toml`
- `omnidrive-cli/Cargo.toml`

- [ ] **Step 2: Bump installer version**

In `installer/omnidrive.iss`, change:
```
#define AppVersion "0.3.6"
```
to:
```
#define AppVersion "0.3.7"
```

- [ ] **Step 3: Verify no 0.3.6 remains**

```bash
grep -r "0\.3\.6" --include="Cargo.toml" --include="*.iss" .
```

Expected: no output.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml angeld/Cargo.toml omnidrive-core/Cargo.toml angelctl/Cargo.toml omnidrive-tray/Cargo.toml omnidrive-shell-ext/Cargo.toml omnidrive-cli/Cargo.toml installer/omnidrive.iss
git commit -m "chore(release): bump version 0.3.6 → 0.3.7"
```

---

## Task 11: Build release + copy binaries + build installer

**Files:** `dist/installer/payload/*.exe`, `dist/installer/output/OmniDrive-Setup-0.3.7.exe`

- [ ] **Step 1: Full release build**

```bash
cargo build --release --workspace
```

Expected: exits 0. Takes 3-5 min if nothing changed in Rust (only metadata touched).

- [ ] **Step 2: Copy binaries to payload**

```bash
cp target/release/angeld.exe            dist/installer/payload/angeld.exe
cp target/release/angelctl.exe          dist/installer/payload/angelctl.exe
cp target/release/omnidrive.exe         dist/installer/payload/omnidrive.exe
cp target/release/omnidrive-tray.exe    dist/installer/payload/omnidrive-tray.exe
```

- [ ] **Step 3: Build installer with Inno Setup**

```bash
"C:/Program Files (x86)/Inno Setup 6/ISCC.exe" installer/omnidrive.iss
```

Expected: `Successful compile (...)` and output at `dist/installer/output/OmniDrive-Setup-0.3.7.exe`.

- [ ] **Step 4: Commit payload binaries + installer output**

```bash
git add dist/installer/payload/angeld.exe dist/installer/payload/angelctl.exe dist/installer/payload/omnidrive.exe dist/installer/payload/omnidrive-tray.exe
git commit -m "chore(release): v0.3.7 — wizard redesign + tray icons fix"
```

---

## Self-Review

**Spec coverage:**
- ✅ Single-column stepBody for all 6 steps (Tasks 2–7)
- ✅ Step 2 radio list + description card (Task 3)
- ✅ Step 4 tabs + form + status banner (Task 5)
- ✅ Tray icons in payload (Task 9)
- ✅ Version bump (Task 10)
- ✅ Rebuild installer (Task 11)
- ✅ `wizard.html` shell untouched — no tasks needed

**Type consistency:**
- `statusBadge()` defined in Task 1, called in Tasks 5
- `modeDescription()` defined in Task 1, called in Task 3
- `providerStatusBanner()` defined in Task 1, called in Task 5
- `providerHeadline()`, `providerDetails()`, `formatTs()`, `escape()`, `modeLabel()` — all pre-existing, unchanged

**No placeholders:** all code blocks are complete.
