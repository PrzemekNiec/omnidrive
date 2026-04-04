(function () {
  const overlay = document.getElementById("onboardingWizardOverlay");
  if (!overlay) return;

  const ui = {
    kicker: document.getElementById("onboardingWizardStepKicker"),
    title: document.getElementById("onboardingWizardStepTitle"),
    desc: document.getElementById("onboardingWizardStepDescription"),
    counter: document.getElementById("onboardingWizardStepCounter"),
    bar: document.getElementById("onboardingWizardProgressBar"),
    draft: document.getElementById("onboardingWizardDraftBanner"),
    error: document.getElementById("onboardingWizardError"),
    body: document.getElementById("onboardingWizardBody"),
    back: document.getElementById("onboardingWizardBackButton"),
    next: document.getElementById("onboardingWizardNextButton"),
    secondary: document.getElementById("onboardingWizardSecondaryButton"),
  };

  const STORAGE_KEY = "omnidrive.onboardingWizard.v1";
  const PROVIDERS = {
    "cloudflare-r2": { short: "R2", name: "Cloudflare R2", region: "auto" },
    "backblaze-b2": { short: "B2", name: "Backblaze B2", region: "" },
    "scaleway": { short: "SCW", name: "Scaleway", region: "" },
  };
  const ORDER = Object.keys(PROVIDERS);
  const st = {
    step: 0,
    mode: null,
    busy: false,
    selectedProvider: ORDER[0],
    onboarding: null,
    draftApplied: false,
    identity: { device_name: "", device_id: null },
    security: { passphrase: "", confirm: "", passphraseProvided: false },
    providers: Object.fromEntries(ORDER.map((name) => [name, {
      provider_name: name, endpoint: "", region: PROVIDERS[name].region, bucket: "",
      force_path_style: false, enabled: true, access_key_status: "MISSING",
      secret_key_status: "MISSING", last_test_status: null, last_test_error: null,
      last_test_at: null, validation: null, draft_source: null, busy: false,
    }])),
    secrets: Object.fromEntries(ORDER.map((name) => [name, { access_key_id: "", secret_access_key: "" }])),
    drafts: {},
  };

  ui.back.addEventListener("click", () => {
    if (st.busy || st.step === 0) return;
    hideError();
    if (st.step === 3) clearProviderSecrets();
    if (st.step === 5 || st.step === 4) clearSecuritySecrets();
    st.step -= 1;
    saveSession();
    render();
  });
  ui.next.addEventListener("click", () => void onPrimary());
  ui.secondary.addEventListener("click", () => {
    if (ui.secondary.dataset.action === "draft") {
      applyDrafts();
      render();
    }
  });

  void init();

  async function init() {
    restoreSession();
    try {
      const status = await api("/api/onboarding/status");
      st.onboarding = status;
      mergeStatus(status);
      if (String(status.onboarding_state || "").toUpperCase() === "COMPLETED") {
        hideWizard();
        return;
      }
      st.step = resolveInitialStep(status);
      showWizard();
      render();
    } catch (error) {
      console.error("wizard init failed", error);
    }
  }

  function restoreSession() {
    try {
      const raw = sessionStorage.getItem(STORAGE_KEY);
      if (!raw) return;
      const saved = JSON.parse(raw);
      st.step = Math.max(0, Math.min(5, Number(saved.step || 0)));
      st.mode = saved.mode || null;
      st.selectedProvider = saved.selectedProvider || st.selectedProvider;
      st.draftApplied = Boolean(saved.draftApplied);
      if (saved.identity) Object.assign(st.identity, saved.identity);
      if (saved.providers) {
        for (const name of ORDER) {
          if (saved.providers[name]) Object.assign(st.providers[name], saved.providers[name]);
        }
      }
    } catch (_) {}
  }

  function saveSession() {
    const providers = Object.fromEntries(ORDER.map((name) => [name, {
      endpoint: st.providers[name].endpoint,
      region: st.providers[name].region,
      bucket: st.providers[name].bucket,
      force_path_style: st.providers[name].force_path_style,
      enabled: st.providers[name].enabled,
      access_key_status: st.providers[name].access_key_status,
      secret_key_status: st.providers[name].secret_key_status,
      last_test_status: st.providers[name].last_test_status,
      last_test_error: st.providers[name].last_test_error,
      last_test_at: st.providers[name].last_test_at,
      validation: st.providers[name].validation,
      draft_source: st.providers[name].draft_source,
    }]));
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify({
      step: st.step,
      mode: st.mode,
      selectedProvider: st.selectedProvider,
      draftApplied: st.draftApplied,
      identity: st.identity,
      providers,
    }));
  }

  function mergeStatus(status) {
    st.identity.device_name = status.device_name || st.identity.device_name || "This device";
    st.identity.device_id = status.device_id || st.identity.device_id;
    st.mode ||= String(status.onboarding_mode || "").toUpperCase() === "CLOUD_ENABLED" ? "cloud" : "local";
    st.drafts = {};
    for (const provider of status.providers || []) {
      const slot = st.providers[provider.provider_name];
      if (!slot) continue;
      slot.endpoint ||= provider.endpoint || "";
      slot.region ||= provider.region || "";
      slot.bucket ||= provider.bucket || "";
      slot.force_path_style = Boolean(provider.force_path_style);
      slot.enabled = Boolean(provider.enabled);
      slot.access_key_status = provider.access_key_status || slot.access_key_status;
      slot.secret_key_status = provider.secret_key_status || slot.secret_key_status;
      slot.last_test_status = provider.last_test_status || slot.last_test_status;
      slot.last_test_error = provider.last_test_error || slot.last_test_error;
      slot.last_test_at = provider.last_test_at || slot.last_test_at;
      slot.draft_source = provider.draft_source || null;
      if (provider.draft_source === ".env") st.drafts[provider.provider_name] = provider;
    }
    saveSession();
  }

  function resolveInitialStep(status) {
    if (st.step > 0) return st.step;
    const step = String(status.current_step || "welcome").toLowerCase();
    if (step === "identity") return 2;
    if (step === "providers") return 3;
    if (step === "security") return 4;
    if (step === "completed") return 5;
    return 0;
  }

  function showWizard() {
    overlay.classList.remove("hidden", "wizard-hidden");
    overlay.classList.add("wizard-visible");
    overlay.setAttribute("aria-hidden", "false");
  }

  function hideWizard() {
    overlay.classList.remove("wizard-visible");
    overlay.classList.add("wizard-hidden");
    overlay.setAttribute("aria-hidden", "true");
    setTimeout(() => overlay.classList.add("hidden"), 320);
  }

  function hideError() {
    ui.error.classList.add("hidden");
    ui.error.textContent = "";
  }

  function showError(message) {
    ui.error.textContent = message;
    ui.error.classList.remove("hidden");
  }

  function statusClass(status) {
    const normalized = String(status || "").toUpperCase();
    if (normalized === "ERROR") return "status-error";
    if (normalized === "WARN") return "status-warn";
    return "status-ok";
  }

  function modeLabel(mode) {
    if (mode === "local") return "Utwórz nowy lokalny Skarbiec";
    if (mode === "cloud") return "Podłącz dostawców chmurowych";
    if (mode === "join") return "Dołącz do istniejącego Skarbca";
    return "Nie wybrano";
  }

  function providerHeadline(provider) {
    if (provider.busy) return "Testowanie połączenia...";
    if (provider.validation?.status === "OK") return "Połączenie zweryfikowane pomyślnie";
    if (provider.validation?.status === "ERROR") return `${provider.validation.error_kind || "ProviderError"}: walidacja nieudana`;
    if (provider.draft_source === ".env") return "Zaimportowano szkic z .env";
    return "Walidacja nie została jeszcze uruchomiona.";
  }

  function providerDetails(provider) {
    return provider.validation?.message
      || provider.last_test_error
      || "Użyj „Testuj połączenie”, aby uruchomić testy reachability, auth, list, put i delete.";
  }

  function render() {
    hideError();
    renderDraft();
    const meta = stepMeta();
    ui.kicker.textContent = meta.kicker;
    ui.title.textContent = meta.title;
    ui.desc.textContent = meta.desc;
    ui.counter.textContent = `Krok ${st.step + 1} / 6`;
    ui.bar.style.width = `${((st.step + 1) / 6) * 100}%`;
    ui.body.innerHTML = stepBody();
    ui.back.classList.toggle("invisible", st.step === 0 || st.busy);
    ui.back.disabled = st.step === 0 || st.busy;
    ui.next.disabled = st.busy;
    ui.next.textContent = st.busy ? "Przetwarzanie..." : meta.primary;
    bindStep();
  }

  function stepMeta() {
    const items = [
      { kicker: "Kreator pierwszego uruchomienia", title: "Witaj w OmniDrive", desc: "Domyślnie local-first, z chmurą gdy jej potrzebujesz. Ten kreator przygotuje lokalny Skarbiec, dostawców chmurowych albo dołączenie do wspólnego Skarbca.", primary: "Dalej" },
      { kicker: "Krok 2", title: "Wybierz tryb pracy", desc: "Wybierz ścieżkę dla tego urządzenia. Możesz pozostać lokalnie, podłączyć dostawców albo dołączyć do istniejącego Skarbca.", primary: "Dalej" },
      { kicker: "Krok 3", title: "Tożsamość urządzenia", desc: "Nazwa urządzenia pojawia się w diagnostyce, kartach peerów i historii konfliktów.", primary: "Zapisz tożsamość" },
      { kicker: "Krok 4", title: "Dostawcy chmurowi", desc: st.mode === "local" ? "Konfiguracja chmury jest opcjonalna w trybie lokalnym. Możesz ją pominąć teraz albo zweryfikować dostawców na później." : "Zweryfikuj prawdziwe dane S3 zanim OmniDrive zacznie z nich korzystać.", primary: "Dalej" },
      { kicker: "Krok 5", title: "Bezpieczeństwo", desc: st.mode === "local" ? "Opcjonalne przy czysto lokalnym użyciu w tej wersji." : st.mode === "join" ? "Wymagane do odszyfrowania zdalnej migawki metadanych i podpięcia tego urządzenia do tego samego Skarbca." : "Wymagane dla procesu onboarding cloud-backed. Pozostaje wyłącznie w pamięci przeglądarki do kolejnego kroku backendu.", primary: "Dalej" },
      { kicker: "Krok 6", title: "Finalizacja", desc: st.mode === "join" ? "Odtwórz metadane od wybranego dostawcy, przeprowadź grafting tożsamości zdalnego Skarbca i przełącz O: do trybu sync-root z placeholderami." : "Sprawdź wybrany tryb, tożsamość i zweryfikowanych dostawców, a następnie uruchom OmniDrive.", primary: st.mode === "join" ? "Dołącz do istniejącego Skarbca" : "Uruchom OmniDrive" },
    ];
    return items[st.step];
  }

  function stepBody() {
    if (st.step === 0) {
      return `
        <div class="grid gap-5 lg:grid-cols-[1.15fr,0.85fr]">
          <article class="glass-muted rounded-[28px] p-6">
            <p class="text-xs uppercase tracking-[0.22em] text-slate-500">Local-first</p>
            <h3 class="mt-3 text-2xl font-semibold text-white">Twój Skarbiec działa jeszcze przed konfiguracją chmury.</h3>
            <p class="mt-4 text-sm leading-7 text-slate-300">OmniDrive startuje z działającym lokalnym Skarbcem i aktywnym dashboardem. Dostawcy chmurowi i tryb shared-vault rozszerzają bazę, zamiast ją blokować.</p>
          </article>
          <article class="glass-muted rounded-[28px] p-6">
            <p class="text-xs uppercase tracking-[0.22em] text-slate-500">Aktualny stan</p>
            <div class="mt-4 grid gap-3">
              <div class="glass-panel rounded-2xl p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">Skarbiec</p><p class="mt-3 text-lg font-semibold text-white">${escape(st.onboarding?.onboarding_state || "INITIAL")}</p></div>
              <div class="glass-panel rounded-2xl p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">Tryb</p><p class="mt-3 text-lg font-semibold text-white">${escape(modeLabel(st.mode))}</p></div>
              <div class="glass-panel rounded-2xl p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">Urządzenie</p><p class="mt-3 text-lg font-semibold text-white break-words">${escape(st.identity.device_name || "To urządzenie")}</p><p class="mt-2 text-sm text-slate-400">${escape(st.identity.device_id || "ID zostanie nadane po zapisaniu tożsamości")}</p></div>
            </div>
          </article>
        </div>`;
    }

    if (st.step === 1) {
      return `<div class="grid gap-4 xl:grid-cols-3">${["local","cloud","join"].map((mode) => {
        const title = modeLabel(mode);
        const desc = mode === "local"
          ? "Pozostań w local-first. Dysk O: działa od razu."
          : mode === "cloud"
            ? "Zweryfikuj teraz R2, B2 lub Scaleway dla synchronizacji opartej o dostawców."
            : "Odtwórz metadane ze Skarbca cloud-backed i dołącz to urządzenie do tej samej przestrzeni nazw.";
        const selected = st.mode === mode;
        return `<button type="button" data-mode="${mode}" class="glass-muted ${selected ? "border-white/25 bg-white/10 ring-1 ring-white/20" : "border-white/10"} rounded-[28px] border px-6 py-6 text-left transition hover:border-white/20 hover:bg-white/10"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">${mode === "local" ? "Najszybsza ścieżka" : mode === "cloud" ? "Gotowe na chmurę" : "Wspólny Skarbiec"}</p><h3 class="mt-3 text-xl font-semibold text-white">${escape(title)}</h3><p class="mt-3 text-sm leading-7 text-slate-300">${escape(desc)}</p><p class="mt-4 text-xs uppercase tracking-[0.22em] ${selected ? "text-white" : "text-slate-500"}">${selected ? "Wybrano" : "Kliknij, aby wybrać"}</p></button>`;
      }).join("")}</div>`;
    }

    if (st.step === 2) {
      return `
        <div class="grid gap-5 xl:grid-cols-[1.1fr,0.9fr]">
          <article class="glass-muted rounded-[28px] p-6">
            <label class="text-xs uppercase tracking-[0.22em] text-slate-500" for="wizardDeviceNameInput">Nazwa urządzenia</label>
            <input id="wizardDeviceNameInput" type="text" value="${escape(st.identity.device_name || "")}" class="mt-4 w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-base text-white outline-none transition focus:border-white/20" placeholder="Przemek-Laptop" maxlength="80" />
            <p class="mt-3 text-sm text-slate-400">Ta nazwa pojawia się w kartach peerów LAN, historii rewizji i kopiach konfliktowych.</p>
          </article>
          <article class="glass-muted rounded-[28px] p-6">
            <p class="text-xs uppercase tracking-[0.22em] text-slate-500">Tożsamość urządzenia</p>
            <p class="mt-4 text-lg font-semibold text-white break-all">${escape(st.identity.device_id || "ID urządzenia zostanie nadane po zapisaniu tożsamości.")}</p>
            <p class="mt-3 text-sm leading-7 text-slate-300">OmniDrive utrzymuje stabilną tożsamość urządzenia dla każdej instalacji.</p>
          </article>
        </div>`;
    }

    if (st.step === 3) {
      const p = st.providers[st.selectedProvider];
      const s = st.secrets[st.selectedProvider];
      const validated = ORDER.filter((name) => st.providers[name].enabled && String(st.providers[name].last_test_status || "").toUpperCase() === "OK").length;
      return `
        <div class="grid gap-5 xl:grid-cols-[0.95fr,1.25fr]">
          <div class="grid gap-3">
            ${ORDER.map((name) => {
              const provider = st.providers[name];
              return `<button type="button" data-provider="${name}" class="glass-muted ${name === st.selectedProvider ? "border-white/20 bg-white/10 ring-1 ring-white/15" : "border-white/10"} rounded-[24px] border px-4 py-4 text-left transition hover:border-white/20 hover:bg-white/10"><div class="flex items-start justify-between gap-3"><div class="flex items-center gap-3"><div class="flex h-11 w-11 items-center justify-center rounded-2xl border border-white/10 bg-white/5 text-sm font-semibold text-white">${PROVIDERS[name].short}</div><div><p class="text-sm font-semibold text-white">${escape(PROVIDERS[name].name)}</p><p class="mt-1 text-xs text-slate-400">${escape(provider.bucket || "Brak skonfigurowanego bucketu")}</p></div></div><span class="inline-flex rounded-full border px-3 py-1 text-[11px] font-medium ${statusClass(provider.last_test_status || "WARN")}">${escape(provider.last_test_status || "Szkic")}</span></div><p class="mt-3 text-xs text-slate-400">${escape(provider.last_test_status ? `${provider.last_test_status} · ${formatTs(provider.last_test_at)}` : provider.draft_source === ".env" ? "Wykryto szkic" : "Jeszcze nie zweryfikowano")}</p></button>`;
            }).join("")}
            <article class="glass-muted rounded-2xl p-4 text-sm text-slate-300"><p class="font-medium text-white">${validated > 0 ? "Walidacja dostawcy jest gotowa." : "Żaden dostawca nie przeszedł jeszcze walidacji."}</p><p class="mt-2">${st.mode === "local" ? "W trybie local-only możesz pominąć chmurę albo zweryfikować dostawców już teraz." : "Przynajmniej jeden włączony dostawca musi przejść walidację przed przejściem dalej."}</p></article>
          </div>
          <article class="glass-muted rounded-[28px] p-6">
            <div class="flex items-start justify-between gap-4"><div><p class="text-xs uppercase tracking-[0.22em] text-slate-500">${escape(PROVIDERS[st.selectedProvider].name)}</p><h3 class="mt-2 text-xl font-semibold text-white">Połączenie z dostawcą</h3></div><span class="inline-flex rounded-full border px-3 py-1 text-xs font-medium ${statusClass(p.last_test_status || "WARN")}">${escape(p.last_test_status || "Szkic")}</span></div>
            <div class="mt-5 grid gap-4 md:grid-cols-2">
              ${field("Endpoint dostawcy","wizardProviderEndpoint",p.endpoint,"https://<account>.r2.cloudflarestorage.com")}
              ${field("Bucket","wizardProviderBucket",p.bucket,"omnidrive-prod")}
              ${field("Region","wizardProviderRegion",p.region,PROVIDERS[st.selectedProvider].region || "eu-west-1")}
              ${field("Access Key","wizardProviderAccessKey",s.access_key_id,p.access_key_status === "SET" ? "Zapisany access key [SET]" : "AKIA...")}
              <label class="block text-sm text-slate-300 md:col-span-2"><span class="text-xs uppercase tracking-[0.22em] text-slate-500">Secret Key</span><input id="wizardProviderSecretKey" type="password" value="${escape(s.secret_access_key || "")}" class="mt-3 w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none transition focus:border-white/20" placeholder="${p.secret_key_status === "SET" ? "Zapisany sekret [SET]" : "Wklej secret access key"}" autocomplete="new-password" /></label>
            </div>
            <div class="mt-4 grid gap-3 md:grid-cols-2">
              <label class="glass-panel flex items-center gap-3 rounded-2xl px-4 py-3 text-sm text-slate-200"><input id="wizardProviderEnabled" type="checkbox" class="h-4 w-4 rounded border-slate-700 bg-slate-900" ${p.enabled ? "checked" : ""} />Włącz dla tego Skarbca</label>
              <label class="glass-panel flex items-center gap-3 rounded-2xl px-4 py-3 text-sm text-slate-200"><input id="wizardProviderForcePathStyle" type="checkbox" class="h-4 w-4 rounded border-slate-700 bg-slate-900" ${p.force_path_style ? "checked" : ""} />Wymuś adresowanie path-style</label>
            </div>
            <div class="mt-5 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between"><p class="text-sm text-slate-400">Sekrety nie opuszczają przeglądarki poza żądaniem walidacji, a status onboardingu nigdy nie zwraca ciphertextów.</p><button id="wizardTestProviderButton" class="inline-flex items-center justify-center rounded-xl border border-white/10 bg-white/10 px-4 py-2 text-sm font-medium text-white transition hover:border-white/20 hover:bg-white/15 disabled:cursor-not-allowed disabled:opacity-60" ${p.busy ? "disabled" : ""}>${p.busy ? "Testowanie połączenia..." : "Testuj połączenie"}</button></div>
            <div class="mt-5 rounded-2xl border px-4 py-4 ${p.validation?.status === "ERROR" ? "border-rose-500/30 bg-rose-500/10 text-rose-100" : p.validation?.status === "OK" ? "border-emerald-500/20 bg-emerald-500/10 text-emerald-100" : "border-white/10 bg-white/5 text-slate-300"}"><p class="text-sm font-medium">${escape(providerHeadline(p))}</p><p class="mt-2 text-sm">${escape(providerDetails(p))}</p><p class="mt-2 text-xs text-slate-400">Ostatni test: ${formatTs(p.last_test_at)}</p></div>
          </article>
        </div>`;
    }

    if (st.step === 4) {
      return `
        <div class="grid gap-5 xl:grid-cols-[1.15fr,0.85fr]">
          <article class="glass-muted rounded-[28px] p-6">
            <label class="block text-sm text-slate-300"><span class="text-xs uppercase tracking-[0.22em] text-slate-500">Hasło główne (Master Passphrase)</span><input id="wizardPassphrase" type="password" value="${escape(st.security.passphrase)}" class="mt-3 w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none transition focus:border-white/20" placeholder="${st.mode === "local" ? "Opcjonalne na teraz" : "Wpisz hasło główne"}" autocomplete="new-password" /></label>
            <label class="mt-4 block text-sm text-slate-300"><span class="text-xs uppercase tracking-[0.22em] text-slate-500">Potwierdź hasło</span><input id="wizardPassphraseConfirm" type="password" value="${escape(st.security.confirm)}" class="mt-3 w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none transition focus:border-white/20" placeholder="Powtórz hasło" autocomplete="new-password" /></label>
          </article>
          <article class="glass-muted rounded-[28px] p-6">
            <p class="text-xs uppercase tracking-[0.22em] text-slate-500">Uwagi bezpieczeństwa</p>
            <div class="mt-4 space-y-3 text-sm text-slate-300">
              <div class="rounded-2xl border border-white/10 bg-white/5 px-4 py-4"><p class="font-medium text-white">${escape(st.mode === "local" ? "Opcjonalne w trybie local-only w tej wersji." : st.mode === "join" ? "Wymagane do odszyfrowania metadanych z istniejącego Skarbca." : "Wymagane przed finalizacją onboardingu cloud-backed.")}</p><p class="mt-2">Hasło pozostaje wyłącznie w pamięci przeglądarki podczas sesji kreatora i jest wysyłane tylko raz do kroku restore/finalize.</p></div>
              <div class="rounded-2xl border border-white/10 bg-white/5 px-4 py-4"><p class="font-medium text-white">Co stanie się dalej?</p><p class="mt-2">${escape(st.mode === "join" ? "OmniDrive pobierze zaszyfrowaną migawkę metadanych, odszyfruje ją lokalnie, przeprowadzi grafting tożsamości zdalnego Skarbca i od razu zmaterializuje placeholdery na O:." : "Hasło przygotowuje zaszyfrowane kopie metadanych i przyszłe odzyskiwanie w konfiguracjach cloud-backed.")}</p></div>
            </div>
          </article>
        </div>`;
    }

    return `
      <div class="grid gap-5 xl:grid-cols-[1.05fr,0.95fr]">
        <article class="glass-muted rounded-[28px] p-6">
          <p class="text-xs uppercase tracking-[0.22em] text-slate-500">Podsumowanie uruchomienia</p>
          <div class="mt-4 grid gap-3 md:grid-cols-2">
            <div class="glass-panel rounded-2xl p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">Wybrany tryb</p><p class="mt-3 text-lg font-semibold text-white">${escape(modeLabel(st.mode))}</p><p class="mt-2 text-sm text-slate-400">${st.mode === "join" ? "Restore metadanych uruchomi się teraz." : "Gotowe do uruchomienia"}</p></div>
            <div class="glass-panel rounded-2xl p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">Urządzenie</p><p class="mt-3 text-lg font-semibold text-white break-words">${escape(st.identity.device_name || "Nienazwane urządzenie")}</p><p class="mt-2 text-sm text-slate-400">${escape(st.identity.device_id || "Tożsamość nie została jeszcze zapisana")}</p></div>
            <div class="glass-panel rounded-2xl p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">Zweryfikowani dostawcy</p><p class="mt-3 text-lg font-semibold text-white">${ORDER.filter((name) => st.providers[name].enabled && String(st.providers[name].last_test_status || "").toUpperCase() === "OK").length}</p><p class="mt-2 text-sm text-slate-400">${escape(ORDER.filter((name) => st.providers[name].enabled && String(st.providers[name].last_test_status || "").toUpperCase() === "OK").map((name) => PROVIDERS[name].short).join(", ") || "Brak")}</p></div>
            <div class="glass-panel rounded-2xl p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">Hasło</p><p class="mt-3 text-lg font-semibold text-white">${st.security.passphraseProvided ? "Gotowe w pamięci" : "Nie podano"}</p><p class="mt-2 text-sm text-slate-400">${st.mode === "local" ? "Opcjonalne dla local-only" : st.mode === "join" ? "Wymagane do restore metadanych" : "Wymagane dla cloud-backed flow"}</p></div>
          </div>
        </article>
        <article class="glass-muted rounded-[28px] p-6">
          <p class="text-xs uppercase tracking-[0.22em] text-slate-500">Kontrola końcowa</p>
          <div class="mt-4 space-y-3 text-sm text-slate-300">
            <div class="rounded-2xl border border-white/10 bg-white/5 px-4 py-4"><p class="font-medium text-white">Lokalny dashboard pozostaje aktywny</p><p class="mt-2">Health, logi, utrzymanie i diagnostyka są dostępne pod overlayem kreatora.</p></div>
            <div class="rounded-2xl border ${st.mode === "join" ? "border-cyan-500/30 bg-cyan-500/10 text-cyan-100" : "border-emerald-500/20 bg-emerald-500/10 text-emerald-100"} px-4 py-4"><p class="font-medium ${st.mode === "join" ? "text-cyan-100" : "text-emerald-100"}">${st.mode === "join" ? "Gotowe do dołączenia do istniejącego Skarbca." : "Gotowe do uruchomienia OmniDrive."}</p><p class="mt-2">${st.mode === "join" ? "OmniDrive odtworzy zaszyfrowane metadane od wybranego dostawcy, przeprowadzi grafting współdzielonej tożsamości Skarbca i przemontuje O: do odtworzonego widoku sync-root." : "Zakończenie tego kroku płynnie ukryje kreator i pozostawi dashboard uruchomiony w wybranym trybie onboardingu."}</p></div>
          </div>
        </article>
      </div>`;
  }

  function renderDraft() {
    if (!st.onboarding?.draft_env_detected || st.step !== 0) {
      ui.draft.classList.add("hidden");
      ui.draft.innerHTML = "";
      ui.secondary.classList.add("hidden");
      ui.secondary.dataset.action = "";
      return;
    }
    const names = Object.keys(st.drafts).map((name) => PROVIDERS[name]?.name || name).join(", ");
    ui.draft.innerHTML = `<div class="flex flex-col gap-3 md:flex-row md:items-center md:justify-between"><div><p class="font-medium text-cyan-50">Wykryto konfigurację deweloperską (.env)</p><p class="mt-1 text-sm text-cyan-100/90">OmniDrive znalazł importowalne ustawienia dostawców${names ? ` dla ${escape(names)}` : ""}. Formularze mogą zostać uzupełnione bez ujawniania zapisanych sekretów.</p></div><button id="onboardingWizardDraftButton" class="inline-flex items-center justify-center rounded-xl border border-cyan-300/20 bg-cyan-500/15 px-4 py-2 text-sm font-medium text-cyan-50 transition hover:border-cyan-200/30 hover:bg-cyan-500/20">Automatycznie wczytaj z .env</button></div>`;
    ui.draft.classList.remove("hidden");
    ui.secondary.dataset.action = "draft";
    ui.secondary.textContent = "Użyj wykrytego szkicu";
    ui.secondary.classList.remove("hidden");
  }

  function bindStep() {
    document.getElementById("onboardingWizardDraftButton")?.addEventListener("click", () => { applyDrafts(); render(); });
    document.querySelectorAll("[data-mode]").forEach((button) => button.addEventListener("click", () => { st.mode = button.dataset.mode; saveSession(); render(); }));
    document.getElementById("wizardDeviceNameInput")?.addEventListener("input", (e) => { st.identity.device_name = e.target.value; saveSession(); });
    document.querySelectorAll("[data-provider]").forEach((button) => button.addEventListener("click", () => { st.selectedProvider = button.dataset.provider; saveSession(); render(); }));
    bindInput("wizardProviderEndpoint", (v) => { st.providers[st.selectedProvider].endpoint = v; saveSession(); });
    bindInput("wizardProviderBucket", (v) => { st.providers[st.selectedProvider].bucket = v; saveSession(); });
    bindInput("wizardProviderRegion", (v) => { st.providers[st.selectedProvider].region = v; saveSession(); });
    bindInput("wizardProviderAccessKey", (v) => { st.secrets[st.selectedProvider].access_key_id = v; });
    bindInput("wizardProviderSecretKey", (v) => { st.secrets[st.selectedProvider].secret_access_key = v; });
    document.getElementById("wizardProviderEnabled")?.addEventListener("change", (e) => { st.providers[st.selectedProvider].enabled = Boolean(e.target.checked); saveSession(); });
    document.getElementById("wizardProviderForcePathStyle")?.addEventListener("change", (e) => { st.providers[st.selectedProvider].force_path_style = Boolean(e.target.checked); saveSession(); });
    document.getElementById("wizardTestProviderButton")?.addEventListener("click", () => void testProvider());
    bindInput("wizardPassphrase", (v) => { st.security.passphrase = v; }, false);
    bindInput("wizardPassphraseConfirm", (v) => { st.security.confirm = v; }, false);
  }

  function bindInput(id, handler) {
    document.getElementById(id)?.addEventListener("input", (e) => handler(e.target.value));
  }

  function clearProviderSecrets() {
    for (const name of ORDER) {
      st.secrets[name] = { access_key_id: "", secret_access_key: "" };
    }
  }

  function clearSecuritySecrets() {
    st.security.passphrase = "";
    st.security.confirm = "";
  }

  function applyDrafts() {
    for (const [name, draft] of Object.entries(st.drafts)) {
      const slot = st.providers[name];
      if (!slot) continue;
      slot.endpoint = draft.endpoint || slot.endpoint;
      slot.region = draft.region || slot.region;
      slot.bucket = draft.bucket || slot.bucket;
      slot.force_path_style = Boolean(draft.force_path_style);
      slot.enabled = Boolean(draft.enabled);
      slot.access_key_status = draft.access_key_status || slot.access_key_status;
      slot.secret_key_status = draft.secret_key_status || slot.secret_key_status;
      slot.draft_source = draft.draft_source || slot.draft_source;
    }
    st.draftApplied = true;
    saveSession();
  }

  async function onPrimary() {
    hideError();
    if (st.busy) return;
    try {
      if (st.step === 0) st.step = 1;
      else if (st.step === 1) {
        if (!st.mode) throw new Error("Wybierz tryb pracy przed przejściem dalej.");
        if (st.mode === "local") await api("/api/onboarding/bootstrap-local", { method: "POST" });
        st.step = 2;
      } else if (st.step === 2) {
        const name = String(st.identity.device_name || "").trim();
        if (!name) throw new Error("Podaj nazwę urządzenia przed przejściem dalej.");
        st.busy = true; render();
        const response = await api("/api/onboarding/setup-identity", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ device_name: name }) });
        st.identity.device_name = response.device_name;
        st.identity.device_id = response.device_id;
        st.busy = false;
        st.step = 3;
      } else if (st.step === 3) {
        const validatedProviders = ORDER.filter((name) => st.providers[name].enabled && String(st.providers[name].last_test_status || "").toUpperCase() === "OK");
        if (st.mode !== "local" && validatedProviders.length === 0) {
          throw new Error("Przed przejściem dalej zweryfikuj co najmniej jednego włączonego dostawcę.");
        }
        if (st.mode !== "local" && !validatedProviders.includes(st.selectedProvider)) {
          st.selectedProvider = validatedProviders[0];
        }
        clearProviderSecrets();
        st.step = 4;
      } else if (st.step === 4) {
        if ((st.mode !== "local" || st.security.passphrase || st.security.confirm) && !st.security.passphrase) throw new Error("Podaj hasło główne przed przejściem dalej.");
        if (st.security.passphrase !== st.security.confirm) throw new Error("Potwierdzenie hasła nie zgadza się.");
        st.security.passphraseProvided = Boolean(st.security.passphrase);
        st.step = 5;
      } else {
        st.busy = true; render();
        if (st.mode === "join") {
          if (!st.security.passphrase) throw new Error("Podaj hasło główne przed dołączeniem do istniejącego Skarbca.");
          const response = await api("/api/onboarding/join-existing", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              passphrase: st.security.passphrase,
              provider_id: st.selectedProvider,
            }),
          });
          if (!response.restore || response.restore.status !== "OK") {
            throw new Error("Przywracanie Skarbca nie zwróciło pomyślnego wyniku.");
          }
        } else {
          await api("/api/onboarding/complete", { method: "POST" });
        }
        clearProviderSecrets();
        clearSecuritySecrets();
        sessionStorage.removeItem(STORAGE_KEY);
        hideWizard();
        st.busy = false;
        if (typeof window.loadDashboard === "function") window.loadDashboard().catch(console.error);
        return;
      }
      saveSession();
      render();
    } catch (error) {
      st.busy = false;
      render();
      showError(error.message || String(error));
    }
  }

  async function testProvider() {
    hideError();
    const provider = st.providers[st.selectedProvider];
    const secret = st.secrets[st.selectedProvider];
    if (!provider.endpoint.trim() || !provider.bucket.trim() || !provider.region.trim()) {
      showError("Endpoint, bucket i region są wymagane przed testem połączenia z dostawcą.");
      return;
    }
    if (!secret.access_key_id.trim() && provider.access_key_status !== "SET") {
      showError("Wklej access key albo użyj już zapisanego klucza przed testem dostawcy.");
      return;
    }
    if (!secret.secret_access_key.trim() && provider.secret_key_status !== "SET") {
      showError("Wklej secret key albo użyj już zapisanego sekretu przed testem dostawcy.");
      return;
    }
    provider.busy = true;
    render();
    try {
      const response = await api("/api/onboarding/setup-provider", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          provider_name: provider.provider_name,
          endpoint: provider.endpoint.trim(),
          region: provider.region.trim(),
          bucket: provider.bucket.trim(),
          force_path_style: provider.force_path_style,
          enabled: provider.enabled,
          access_key_id: secret.access_key_id.trim() || undefined,
          secret_access_key: secret.secret_access_key.trim() || undefined,
        }),
      });
      provider.access_key_status = response.access_key_status || provider.access_key_status;
      provider.secret_key_status = response.secret_key_status || provider.secret_key_status;
      provider.last_test_status = response.validation?.status || null;
      provider.last_test_error = response.validation?.status === "ERROR" ? response.validation.message : null;
      provider.last_test_at = response.validation?.last_run || null;
      provider.validation = response.validation || null;
      if (provider.validation?.status === "OK") st.secrets[st.selectedProvider] = { access_key_id: "", secret_access_key: "" };
      saveSession();
    } catch (error) {
      provider.validation = { status: "ERROR", message: error.message || "Walidacja dostawcy nie powiodła się.", last_run: Date.now(), error_kind: "ValidationError" };
      provider.last_test_status = "ERROR";
      provider.last_test_error = provider.validation.message;
      provider.last_test_at = provider.validation.last_run;
      showError(provider.validation.message);
    } finally {
      provider.busy = false;
      render();
    }
  }

  async function api(url, options = {}) {
    const response = await fetch(url, options);
    if (!response.ok) {
      let message = `${response.status} ${response.statusText}`;
      try {
        const payload = await response.json();
        if (payload.human_readable_reason) message = payload.human_readable_reason;
        else if (payload.message) message = payload.message;
        else if (payload.error) message = payload.error;
      } catch (_) {}
      throw new Error(message);
    }
    return response.json();
  }

  function field(label, id, value, placeholder) {
    return `<label class="block text-sm text-slate-300"><span class="text-xs uppercase tracking-[0.22em] text-slate-500">${escape(label)}</span><input id="${id}" type="${id === "wizardProviderSecretKey" ? "password" : "text"}" value="${escape(value || "")}" class="mt-3 w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none transition focus:border-white/20" placeholder="${escape(placeholder)}" autocomplete="off" /></label>`;
  }

  function escape(value) {
    return String(value ?? "").replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;");
  }

  function formatTs(timestamp) {
    if (!timestamp) return "Nigdy";
    const date = new Date(Number(timestamp));
    return Number.isNaN(date.getTime()) ? String(timestamp) : date.toLocaleString();
  }
})();
