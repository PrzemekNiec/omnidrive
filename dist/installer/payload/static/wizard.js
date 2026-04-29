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
    ui.counter.dataset.currentStep = st.step + 1;
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

  function escape(value) {
    return String(value ?? "").replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;");
  }

  function formatTs(timestamp) {
    if (!timestamp) return "Nigdy";
    const date = new Date(Number(timestamp));
    return Number.isNaN(date.getTime()) ? String(timestamp) : date.toLocaleString();
  }
})();
