# Faza L — Sesja D: OAuth Frontend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate the Phase K Google OAuth2 backend with the OmniDrive web UI — login button, profile in topbar, clean logout, and post-login onboarding guard.

**Architecture:** New global `AUTH_STATE` object in `index.html` stores the OAuth token from `localStorage`; `consumeOauthTokenFromHash()` extracts the token from the URL hash on page load; `refreshUserProfile()` populates the topbar from `/api/auth/session`; the backend session endpoint is extended to JOIN with the `users` table and return `email`+`display_name`. The wizard lives on `/legacy`, so L.4 redirects there when onboarding is incomplete.

**Tech Stack:** Rust/Axum (`api/auth.rs`), Vanilla JS/HTML (`index.html`, `wizard.js`), SQLite via `db::get_user()`.

---

## File Map

| File | Role |
|------|------|
| `angeld/src/api/auth.rs:139-155` | Extend `get_auth_session` — JOIN users table, return `email`+`display_name` |
| `angeld/static/index.html:860` | Add `#googleLoginBtn` element next to `#oauthStatusBadge` |
| `angeld/static/index.html:976-979` | Add `AUTH_STATE` global declaration |
| `angeld/static/index.html:2641-2663` | Replace `loadUstawieniaSession()` — auth headers + show/hide button |
| `angeld/static/index.html:2928-2939` | Extend logout handler — clear OAuth token |
| `angeld/static/index.html:~2910` | Add `consumeOauthTokenFromHash`, `refreshUserProfile`, `oauthPostLoginGuard` |
| `angeld/static/index.html:~2920` | Wire new functions into init IIFE |
| `angeld/static/wizard.js:251-255` | Add Google login panel to step 0 |

---

## Task 1: Backend — extend `/api/auth/session`

**Files:**
- Modify: `angeld/src/api/auth.rs:139-155`

- [ ] **Step 1.1 — Replace `get_auth_session` body**

Open `angeld/src/api/auth.rs`. Replace the entire `get_auth_session` function (lines 139–155):

```rust
async fn get_auth_session(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    match extract_session(&state.pool, &headers).await {
        Some(session) => {
            let user = db::get_user(&state.pool, &session.user_id).await.ok().flatten();
            Json(serde_json::json!({
                "valid": true,
                "user_id": session.user_id,
                "device_id": session.device_id,
                "expires_at": session.expires_at,
                "email": user.as_ref().and_then(|u| u.email.as_deref()),
                "display_name": user.as_ref().map(|u| u.display_name.as_str()),
            }))
        },
        None => Json(serde_json::json!({
            "valid": false,
            "error": "invalid_or_expired_session",
        })),
    }
}
```

- [ ] **Step 1.2 — Verify with cargo check**

```bash
cargo check --manifest-path angeld/Cargo.toml
```

Expected: `Finished` with 0 errors. `db::get_user` already exists in `db.rs`; no new imports needed.

---

## Task 2: HTML — add `#googleLoginBtn` element

**Files:**
- Modify: `angeld/static/index.html:860`

- [ ] **Step 2.1 — Add hidden button next to badge**

In `angeld/static/index.html`, find line 860:
```html
            <span id="oauthStatusBadge" class="px-3 py-1 rounded-full text-[10px] font-bold tracking-widest uppercase border border-white/10 text-slate-400">—</span>
```

Replace with:
```html
            <span id="oauthStatusBadge" class="px-3 py-1 rounded-full text-[10px] font-bold tracking-widest uppercase border border-white/10 text-slate-400">—</span>
            <a id="googleLoginBtn" href="/api/auth/google/start" style="display:none" class="flex items-center gap-2 px-4 py-2 rounded-xl text-sm font-bold bg-primary/10 text-primary border border-primary/20 hover:bg-primary/20 transition-colors"><span class="material-symbols-outlined text-lg">account_circle</span>&nbsp;Zaloguj przez Google</a>
```

The button is hidden by default (`style="display:none"`). `loadUstawieniaSession()` will toggle visibility in Task 5.

---

## Task 3: JS — `AUTH_STATE` global

**Files:**
- Modify: `angeld/static/index.html:978-979`

- [ ] **Step 3.1 — Insert AUTH_STATE after the bootstrap comment**

In `angeld/static/index.html`, find lines 977-979:
```js
    // OmniDrive — Skarbiec Console (frontend bootstrap)
    // F.3 audit log ✅; F.4 recovery alert ✅; F.5 shard status ✅; F.6 status pill ✅; F.7 router ✅; G.* stats
    // ============================================================
```

Replace with:
```js
    // OmniDrive — Skarbiec Console (frontend bootstrap)
    // F.3 audit log ✅; F.4 recovery alert ✅; F.5 shard status ✅; F.6 status pill ✅; F.7 router ✅; G.* stats
    // ============================================================

    // ── Auth state — OAuth token (Faza L) ─────────────────────────
    const AUTH_STATE = {
      oauthToken: localStorage.getItem('omnidrive.oauthToken') || null,
      expiresAt: parseInt(localStorage.getItem('omnidrive.oauthExpiresAt') || '0', 10),
      user: null,
    };
```

---

## Task 4: JS — three helper functions before the init IIFE

**Files:**
- Modify: `angeld/static/index.html` — insert block just before `(function () {` (around line 2915)

- [ ] **Step 4.1 — Find the init IIFE opening line**

Search for the exact text `    (function () {` near the end of the script. It appears once, just after the `initUstawieniaActions` and Skarbiec view functions.

- [ ] **Step 4.2 — Insert three functions immediately before `(function () {`**

```js
    // ── L: OAuth token extraction from URL hash ────────────────
    function consumeOauthTokenFromHash() {
      const raw = location.hash.startsWith('#') ? location.hash.slice(1) : '';
      const params = new URLSearchParams(raw);
      const token = params.get('oauth_token');
      if (!token) return false;
      const expiresAt = params.get('expires_at') || '0';
      localStorage.setItem('omnidrive.oauthToken', token);
      localStorage.setItem('omnidrive.oauthExpiresAt', expiresAt);
      AUTH_STATE.oauthToken = token;
      AUTH_STATE.expiresAt = parseInt(expiresAt, 10);
      // Remove token from URL — do not leak to browser history or referer
      history.replaceState(null, '', '/#przeglad');
      return true;
    }

    // ── L: Update topbar profile from /api/auth/session ───────
    async function refreshUserProfile() {
      const nameEl = document.getElementById('userName');
      const roleEl = document.getElementById('userRole');
      if (!nameEl || !roleEl || !AUTH_STATE.oauthToken) return;
      try {
        const res = await fetch('/api/auth/session', {
          headers: { Accept: 'application/json', Authorization: `Bearer ${AUTH_STATE.oauthToken}` },
        });
        if (!res.ok) return;
        const data = await res.json();
        if (!data.valid) return;
        AUTH_STATE.user = { user_id: data.user_id, email: data.email || null, display_name: data.display_name || null };
        nameEl.textContent = data.email || data.display_name || data.user_id || 'Local';
        roleEl.textContent = data.email ? 'Google' : 'Lokalna sesja';
      } catch (_) {}
    }

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

---

## Task 5: JS — update `loadUstawieniaSession` (L.1 przycisk)

**Files:**
- Modify: `angeld/static/index.html:2641-2663`

- [ ] **Step 5.1 — Replace entire loadUstawieniaSession function**

Find and replace the whole function (from `async function loadUstawieniaSession()` through its closing `}`):

```js
    async function loadUstawieniaSession() {
      const line     = document.getElementById('oauthStatusLine');
      const badge    = document.getElementById('oauthStatusBadge');
      const loginBtn = document.getElementById('googleLoginBtn');

      const showLogin = () => {
        if (badge) badge.style.display = 'none';
        if (loginBtn) loginBtn.style.display = '';
        if (line) line.textContent = 'Brak aktywnej sesji Google';
      };
      const showConnected = (email) => {
        if (badge) {
          badge.textContent = 'Połączono';
          badge.className = 'px-3 py-1 rounded-full text-[10px] font-bold tracking-widest uppercase border border-secondary/30 text-secondary';
          badge.style.display = '';
        }
        if (loginBtn) loginBtn.style.display = 'none';
        if (line) line.textContent = `Zalogowano jako ${email}`;
      };

      try {
        const headers = { Accept: 'application/json' };
        if (AUTH_STATE.oauthToken) headers['Authorization'] = `Bearer ${AUTH_STATE.oauthToken}`;
        const res = await fetch('/api/auth/session', { headers });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data = await res.json();
        const email = data.valid ? (data.email || null) : null;
        email ? showConnected(email) : showLogin();
      } catch (_) {
        showLogin();
      }
    }
```

---

## Task 6: JS — extend logout handler (L.3)

**Files:**
- Modify: `angeld/static/index.html:2928-2939`

- [ ] **Step 6.1 — Replace the wyloguj handler block**

Find this exact block (inside the nav click forEach, `if (view === 'wyloguj')` branch):

```js
          if (view === 'wyloguj') {
            const tok = VAULT_STATE.sessionToken;
            if (tok) {
              fetch('/api/auth/logout', {
                method: 'POST',
                headers: { 'Authorization': `Bearer ${tok}` },
              }).catch(() => {});
            }
            VAULT_STATE.sessionToken = null;
            VAULT_STATE.unlocked = null;
            location.reload();
            return;
          }
```

Replace with:

```js
          if (view === 'wyloguj') {
            // Logout OAuth session
            const oauthTok = AUTH_STATE.oauthToken;
            if (oauthTok) {
              fetch('/api/auth/logout', {
                method: 'POST',
                headers: { 'Authorization': `Bearer ${oauthTok}` },
              }).catch(() => {});
            }
            localStorage.removeItem('omnidrive.oauthToken');
            localStorage.removeItem('omnidrive.oauthExpiresAt');
            AUTH_STATE.oauthToken = null;
            AUTH_STATE.expiresAt = 0;
            AUTH_STATE.user = null;
            // Logout vault session (unchanged)
            const tok = VAULT_STATE.sessionToken;
            if (tok) {
              fetch('/api/auth/logout', {
                method: 'POST',
                headers: { 'Authorization': `Bearer ${tok}` },
              }).catch(() => {});
            }
            VAULT_STATE.sessionToken = null;
            VAULT_STATE.unlocked = null;
            location.reload();
            return;
          }
```

---

## Task 7: JS — wire new functions in init IIFE

**Files:**
- Modify: `angeld/static/index.html` — inside `(function () {`

- [ ] **Step 7.1 — Add `_oauthJustExtracted` at top of IIFE**

Inside `(function () {`, immediately before `const _savedInterval = ...`, insert:

```js
      const _oauthJustExtracted = consumeOauthTokenFromHash();
```

- [ ] **Step 7.2 — Call `refreshUserProfile` and guard after `onHashChange()`**

Find the line `      // Initial route` followed by `      onHashChange();`. After `onHashChange();`, insert:

```js
      refreshUserProfile();
      if (_oauthJustExtracted) oauthPostLoginGuard();
```

---

## Task 8: wizard.js — Google login panel on step 0 (L.1)

**Files:**
- Modify: `angeld/static/wizard.js:251-255`

- [ ] **Step 8.1 — Add Google panel after the Urządzenie panel in step 0**

In `wizard.js`, inside `stepBody()`, find the step 0 return block. Locate the Urządzenie `glass-panel` div (contains `st.identity.device_name`):

```js
              <div class="glass-panel rounded-2xl p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">Urządzenie</p><p class="mt-3 text-lg font-semibold text-white break-words">${escape(st.identity.device_name || "To urządzenie")}</p><p class="mt-2 text-sm text-slate-400">${escape(st.identity.device_id || "ID zostanie nadane po zapisaniu tożsamości")}</p></div>
```

Replace with (adds the Google panel immediately after):

```js
              <div class="glass-panel rounded-2xl p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">Urządzenie</p><p class="mt-3 text-lg font-semibold text-white break-words">${escape(st.identity.device_name || "To urządzenie")}</p><p class="mt-2 text-sm text-slate-400">${escape(st.identity.device_id || "ID zostanie nadane po zapisaniu tożsamości")}</p></div>
              <div class="glass-panel rounded-2xl p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">Konto Google</p><a href="/api/auth/google/start" class="mt-3 flex items-center gap-2 text-sm text-primary hover:underline"><span class="material-symbols-outlined text-lg">account_circle</span>Zaloguj przez Google</a></div>
```

---

## Task 9: cargo check + commit

- [ ] **Step 9.1 — Run cargo check**

```bash
cargo check --manifest-path angeld/Cargo.toml
```

Expected: `Finished` with 0 errors and 0 new warnings.

- [ ] **Step 9.2 — Restart daemon (wczytaj .env z Google credentials)**

```bash
# Zatrzymaj bieżącą instancję angeld, a następnie uruchom ponownie
# (daemon wczyta GOOGLE_CLIENT_ID i GOOGLE_CLIENT_SECRET ze zmiennych środowiskowych)
```

- [ ] **Step 9.3 — Manual smoke test**

1. Otwórz `http://127.0.0.1:8787` w przeglądarce
2. Nawiguj do Ustawienia → sekcja "Połączenie z Google" — powinien widnieć przycisk "Zaloguj przez Google"
3. Kliknij przycisk → redirect do Google OAuth
4. Zatwierdź dostęp → powrót do `/#przeglad`, token w `localStorage`, topbar pokazuje email
5. Wyloguj przez sidebar "Wyloguj" → localStorage wyczyszczony, topbar wraca do "Local"
6. Test L.4: świeża instalacja (INITIAL) → po zalogowaniu Google → redirect do `/legacy` i wizard

- [ ] **Step 9.4 — Commit**

```bash
git add angeld/src/api/auth.rs angeld/static/index.html angeld/static/wizard.js docs/superpowers/specs/2026-04-18-oauth-frontend-design.md docs/superpowers/plans/2026-04-18-oauth-frontend.md
git commit -m "feat(ui): Sesja D — OAuth frontend + profil użytkownika"
```

---

## Self-Review Checklist

- [x] **L.1** przycisk Google → Task 2 (HTML) + Task 5 (JS toggle) + Task 8 (wizard)
- [x] **L.2** profil w topbarze → Task 3 (AUTH_STATE) + Task 4 (refreshUserProfile) + Task 7 (wire)
- [x] **L.3** logout rozszerzony → Task 6 (handler)
- [x] **L.4** guard po zalogowaniu → Task 4 (oauthPostLoginGuard) + Task 7 (wire) — redirect `/legacy`
- [x] Backend `email`+`display_name` → Task 1
- [x] `history.replaceState` (token nie zostaje w URL) → Task 4
- [x] `localStorage` cleanup przy logout → Task 6
- [x] Brak regresji `vaultAuthHeaders`/`mdAuthHeaders` — nie dotykamy tych funkcji
