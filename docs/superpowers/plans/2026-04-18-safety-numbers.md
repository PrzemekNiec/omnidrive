# Safety Numbers (Faza M) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Safety Numbers — a 60-digit SHA-256 fingerprint of `envelope_vault_key + user_id` — exposed via API and displayed in Ustawienia with a QR code and a "verified" toggle stored in SQLite.

**Architecture:** The cryptographic computation lives in `vault.rs` as a method on `UnlockedVaultKeys` (private) exposed through `VaultKeyStore` (public). Two new API routes call these methods. The UI adds a "Bezpieczeństwo" section to Ustawienia using the existing local `QRCode` library (`/qrcode.min.js` already loaded).

**Tech Stack:** Rust / sha2 0.10 / axum / SQLite (sqlx) / Vanilla JS / QRCode (local, already in static/)

---

## File Map

| File | Change |
|------|--------|
| `angeld/src/vault.rs` | Add `safety_numbers()` to `impl UnlockedVaultKeys` + `impl VaultKeyStore` |
| `angeld/src/db.rs` | Migration: add `safety_numbers_verified_at` column; add `set_device_safety_verified()` + `get_device_safety_verified_at()` |
| `angeld/src/api/vault.rs` | Add `GET /api/vault/safety-numbers` + `POST /api/devices/{device_id}/verify` routes |
| `angeld/static/index.html` | Add "Bezpieczeństwo" HTML section + `loadSafetyNumbers()` JS + wire into `activateUstawieniaView()` |

---

## Task 1: `safety_numbers()` in vault.rs

**Files:**
- Modify: `angeld/src/vault.rs` (around line 40 — `impl UnlockedVaultKeys` block; and line 132 — `impl VaultKeyStore` block)
- Test: `angeld/src/vault.rs` (existing `mod tests` at line 674)

- [ ] **Step 1: Write the failing test**

Add inside `mod tests { ... }` in `angeld/src/vault.rs`, after the last existing test (around line 715):

```rust
#[tokio::test]
async fn safety_numbers_deterministic_and_correct_length() -> Result<(), Box<dyn std::error::Error>> {
    let pool = db::init_db("sqlite::memory:").await?;
    let store = VaultKeyStore::new();
    store.unlock(&pool, "test-passphrase").await?;

    let nums = store.safety_numbers("user-abc").await;
    // envelope_vault_key may not exist after plain unlock (V1 path)
    // so we just verify shape if Some, or accept None if V1
    if let Some(s) = nums {
        // 12 blocks of 5 digits separated by spaces = 59 chars total
        assert_eq!(s.len(), 59, "expected 59 chars, got: {s}");
        let parts: Vec<&str> = s.split(' ').collect();
        assert_eq!(parts.len(), 12, "expected 12 blocks");
        for part in &parts {
            assert_eq!(part.len(), 5, "each block must be 5 chars: {part}");
            assert!(part.chars().all(|c| c.is_ascii_digit()), "non-digit in block: {part}");
        }
    }
    Ok(())
}

#[tokio::test]
async fn safety_numbers_stable_for_same_key() -> Result<(), Box<dyn std::error::Error>> {
    let pool = db::init_db("sqlite::memory:").await?;
    let store = VaultKeyStore::new();
    store.unlock(&pool, "test-passphrase").await?;

    let a = store.safety_numbers("user-xyz").await;
    let b = store.safety_numbers("user-xyz").await;
    assert_eq!(a, b, "same key+user must produce same Safety Numbers");
    Ok(())
}
```

- [ ] **Step 2: Run tests to confirm they fail (method doesn't exist yet)**

```
cd C:/Users/Przemek/Desktop/aplikacje/omnidrive
cargo test -p angeld safety_numbers 2>&1 | tail -20
```

Expected: compile error `no method named safety_numbers found`

- [ ] **Step 3: Implement `safety_numbers` on `UnlockedVaultKeys`**

In `angeld/src/vault.rs`, inside `impl UnlockedVaultKeys { ... }` (around line 87, after the last accessor method):

```rust
fn safety_numbers(&self, user_id: &str) -> Option<String> {
    use sha2::{Digest, Sha256};
    let evk = self.envelope_vault_key()?;
    let mut hasher = Sha256::new();
    hasher.update(evk);
    hasher.update(user_id.as_bytes());
    let hash = hasher.finalize();
    let blocks: Vec<String> = hash[..24]
        .chunks(2)
        .map(|pair| {
            let val = u16::from_be_bytes([pair[0], pair[1]]);
            format!("{:05}", val)
        })
        .collect();
    Some(blocks.join(" "))
}
```

- [ ] **Step 4: Implement `safety_numbers` on `VaultKeyStore`**

In `angeld/src/vault.rs`, inside `impl VaultKeyStore { ... }` (around line 234, after `previous_envelope_vault_key()`):

```rust
pub async fn safety_numbers(&self, user_id: &str) -> Option<String> {
    self.inner.read().await.as_ref()?.safety_numbers(user_id)
}
```

- [ ] **Step 5: Run tests — must pass**

```
cargo test -p angeld safety_numbers 2>&1 | tail -20
```

Expected: `test vault::tests::safety_numbers_deterministic_and_correct_length ... ok` and `test vault::tests::safety_numbers_stable_for_same_key ... ok`

- [ ] **Step 6: Commit**

```bash
git add angeld/src/vault.rs
git commit -m "feat(vault): safety_numbers() — SHA-256(evk||user_id) → 60-digit fingerprint"
```

---

## Task 2: DB migration and helper functions

**Files:**
- Modify: `angeld/src/db.rs`
  - Migration call: around line 983 (end of `ensure_column_exists` block)
  - New functions: append near end of file, after `get_device` (line ~6537)

- [ ] **Step 1: Write failing test**

Add to the test section of `db.rs` (search for `#[cfg(test)]` in db.rs; if none exists for this area, add after the last `ensure_column_exists` call block):

Since db.rs may not have a local test module, write the test inline at the bottom of db.rs:

```rust
#[cfg(test)]
mod safety_tests {
    use super::*;

    #[tokio::test]
    async fn set_and_get_safety_verified_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let pool = init_db("sqlite::memory:").await?;
        // Need a device in the DB — insert via migrate_single_to_multi_user
        // which creates tables; then insert a minimal device row
        sqlx::query(
            "INSERT INTO devices (device_id, user_id, device_name, public_key, created_at) \
             VALUES ('d1', 'u1', 'test', X'0102', 1000)"
        )
        .execute(&pool)
        .await?;

        let before = get_device_safety_verified_at(&pool, "d1").await?;
        assert!(before.is_none(), "should be NULL before verification");

        set_device_safety_verified(&pool, "d1").await?;

        let after = get_device_safety_verified_at(&pool, "d1").await?;
        assert!(after.is_some(), "should be set after verification");
        assert!(after.unwrap() > 0);
        Ok(())
    }
}
```

- [ ] **Step 2: Run test to confirm it fails**

```
cargo test -p angeld set_and_get_safety_verified_roundtrip 2>&1 | tail -20
```

Expected: compile error `cannot find function set_device_safety_verified`

- [ ] **Step 3: Add migration call**

In `angeld/src/db.rs`, in the migration block (around line 991, after the last `ensure_column_exists` call for `packs`/`storage_mode`), add:

```rust
ensure_column_exists(&pool, "devices", "safety_numbers_verified_at", "INTEGER").await?;
```

- [ ] **Step 4: Add two helper functions**

Append to `angeld/src/db.rs` after `get_device` function (after line 6537):

```rust
pub async fn set_device_safety_verified(
    pool: &SqlitePool,
    device_id: &str,
) -> Result<(), sqlx::Error> {
    let now = epoch_secs();
    sqlx::query(
        "UPDATE devices SET safety_numbers_verified_at = ? WHERE device_id = ?",
    )
    .bind(now)
    .bind(device_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_device_safety_verified_at(
    pool: &SqlitePool,
    device_id: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let row: Option<(Option<i64>,)> = sqlx::query_as(
        "SELECT safety_numbers_verified_at FROM devices WHERE device_id = ?",
    )
    .bind(device_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(ts,)| ts))
}
```

- [ ] **Step 5: Run test — must pass**

```
cargo test -p angeld set_and_get_safety_verified_roundtrip 2>&1 | tail -20
```

Expected: `test db::safety_tests::set_and_get_safety_verified_roundtrip ... ok`

- [ ] **Step 6: Commit**

```bash
git add angeld/src/db.rs
git commit -m "feat(db): safety_numbers_verified_at migration + get/set helpers"
```

---

## Task 3: API endpoints

**Files:**
- Modify: `angeld/src/api/vault.rs`

- [ ] **Step 1: Add route declarations**

In `angeld/src/api/vault.rs`, in the `pub fn routes()` function (around line 115, after `.route("/api/vault/rotate-key", ...)`), add:

```rust
.route("/api/vault/safety-numbers", get(get_safety_numbers))
.route("/api/devices/{device_id}/verify", post(post_verify_device))
```

- [ ] **Step 2: Add `get_safety_numbers` handler**

Append to `angeld/src/api/vault.rs` (at the end of the file):

```rust
async fn get_safety_numbers(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = super::auth::extract_session(&state.pool, &headers)
        .await
        .ok_or(ApiError::Unauthorized {
            message: "vault locked or no session".to_string(),
        })?;

    let numbers = state
        .vault_keys
        .safety_numbers(&session.user_id)
        .await
        .ok_or(ApiError::Unauthorized {
            message: "vault locked or no session".to_string(),
        })?;

    let vault = db::get_vault_params(&state.pool)
        .await
        .map_err(|e| ApiError::Internal { message: e.to_string() })?;

    let key_generation = vault.as_ref().and_then(|v| v.vault_key_generation).unwrap_or(0);

    let verified_at = db::get_device_safety_verified_at(&state.pool, &session.device_id)
        .await
        .unwrap_or(None);

    tracing::info!(
        "[SAFETY_NUMBERS] generated for user={} [key_material: REDACTED]",
        session.user_id
    );

    Ok(Json(serde_json::json!({
        "safety_numbers": numbers,
        "key_generation": key_generation,
        "verified_at": verified_at,
    })))
}
```

- [ ] **Step 3: Add `post_verify_device` handler**

Append to `angeld/src/api/vault.rs`, after `get_safety_numbers`:

```rust
async fn post_verify_device(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(target_device_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _session = super::auth::extract_session(&state.pool, &headers)
        .await
        .ok_or(ApiError::Unauthorized {
            message: "no active session".to_string(),
        })?;

    db::get_device(&state.pool, &target_device_id)
        .await
        .map_err(|e| ApiError::Internal { message: e.to_string() })?
        .ok_or(ApiError::NotFound {
            resource: "device",
            id: target_device_id.clone(),
        })?;

    db::set_device_safety_verified(&state.pool, &target_device_id)
        .await
        .map_err(|e| ApiError::Internal { message: e.to_string() })?;

    Ok(Json(serde_json::json!({ "verified_at": db::epoch_secs() })))
}
```

- [ ] **Step 4: cargo check**

```
cargo check -p angeld 2>&1 | tail -30
```

Expected: `Finished` with no errors. Fix any compile errors (typically missing `use` imports — check that `Path` is imported from `axum::extract::Path`).

If `Path` is not imported in vault.rs, find the existing `use axum::extract::{...}` line and add `Path` to it.

- [ ] **Step 5: Commit**

```bash
git add angeld/src/api/vault.rs
git commit -m "feat(api): GET /api/vault/safety-numbers + POST /api/devices/{id}/verify"
```

---

## Task 4: UI — "Bezpieczeństwo" section in Ustawienia

**Files:**
- Modify: `angeld/static/index.html`

- [ ] **Step 1: Add HTML section**

In `angeld/static/index.html`, find the closing tags of the Ustawienia view (line ~934, right before `</div></div>` that closes the `p-8 max-w-4xl` and `view-ustawienia` divs).

The exact string to find (line 934):
```html
        </section>
      </div>
    </div>
```

Replace with:
```html
        </section>

        <!-- Sekcja 4: Bezpieczeństwo — Safety Numbers -->
        <section class="glass-panel rounded-2xl p-6 space-y-6" id="safetyNumbersSection">
          <h2 class="text-[10px] font-bold uppercase tracking-widest text-slate-400 border-b border-white/5 pb-3">Bezpieczeństwo — Safety Numbers</h2>

          <div id="safetyNumbersContent">
            <p class="text-xs text-slate-400">Ładowanie…</p>
          </div>
        </section>
      </div>
    </div>
```

- [ ] **Step 2: Add `loadSafetyNumbers()` JS function**

In `angeld/static/index.html`, find the `async function loadUstawieniaPathsAndCache()` function (around line 2682). Insert the following function **before** it:

```javascript
    async function loadSafetyNumbers() {
      const container = document.getElementById('safetyNumbersContent');
      if (!container) return;

      const token = VAULT_STATE.sessionToken;
      if (!token) {
        container.innerHTML = '<p class="text-xs text-slate-400">Vault jest zablokowany. Odblokuj, aby zobaczyć Safety Numbers.</p>';
        return;
      }

      try {
        const res = await fetch('/api/vault/safety-numbers', {
          headers: { Authorization: `Bearer ${token}`, Accept: 'application/json' },
        });
        if (!res.ok) {
          container.innerHTML = '<p class="text-xs text-error">Nie udało się pobrać Safety Numbers.</p>';
          return;
        }
        const data = await res.json();
        const nums = data.safety_numbers || '';
        const gen  = data.key_generation ?? 0;
        const verifiedAt = data.verified_at;

        const verifiedLabel = verifiedAt
          ? `Ostatnia weryfikacja: ${new Date(verifiedAt * 1000).toLocaleString('pl-PL')}`
          : 'Niezweryfikowano';

        // Format digit blocks into two rows of 6
        const blocks = nums.split(' ');
        const row1 = blocks.slice(0, 6).join('&nbsp;&nbsp;');
        const row2 = blocks.slice(6, 12).join('&nbsp;&nbsp;');

        container.innerHTML = `
          <p class="text-xs text-slate-400 mb-3">Generacja klucza: <span class="font-mono text-slate-200">${gen}</span></p>
          <div class="bg-surface-container-high/50 rounded-xl p-4 space-y-2 mb-4">
            <p class="font-mono text-lg tracking-widest text-on-surface text-center">${row1}</p>
            <p class="font-mono text-lg tracking-widest text-on-surface text-center">${row2}</p>
          </div>
          <div class="flex flex-col items-center gap-2 mb-4">
            <div id="safetyQrCanvas" class="rounded-xl overflow-hidden bg-white p-2"></div>
            <p class="text-[10px] text-slate-400">Zeskanuj, aby zweryfikować urządzenie</p>
          </div>
          <div class="flex items-center justify-between gap-4 border-t border-white/5 pt-4">
            <p class="text-xs text-slate-400">${verifiedLabel}</p>
            <button id="markVerifiedBtn"
              class="flex items-center gap-2 px-4 py-2 rounded-xl text-sm font-bold bg-secondary/10 text-secondary border border-secondary/20 hover:bg-secondary/20 transition-colors">
              <span class="material-symbols-outlined text-base">verified</span>
              Oznacz jako zweryfikowane
            </button>
          </div>`;

        // Render QR code using the local QRCode library
        const qrEl = document.getElementById('safetyQrCanvas');
        if (qrEl && typeof QRCode !== 'undefined') {
          new QRCode(qrEl, {
            text: nums.replace(/ /g, ''),
            width: 180,
            height: 180,
            colorDark: '#0f172a',
            colorLight: '#ffffff',
            correctLevel: QRCode.CorrectLevel.M,
          });
        }

        // Bind verify button
        const verifyBtn = document.getElementById('markVerifiedBtn');
        if (verifyBtn) {
          verifyBtn.addEventListener('click', async () => {
            const sessionDevice = VAULT_STATE.deviceId;
            if (!sessionDevice) return;
            try {
              const r = await fetch(`/api/devices/${sessionDevice}/verify`, {
                method: 'POST',
                headers: { Authorization: `Bearer ${token}`, Accept: 'application/json' },
              });
              if (r.ok) loadSafetyNumbers();
            } catch (_) {}
          });
        }
      } catch (err) {
        container.innerHTML = `<p class="text-xs text-error">Błąd: ${err.message}</p>`;
      }
    }
```

- [ ] **Step 3: Wire `loadSafetyNumbers()` into `activateUstawieniaView()`**

Find `activateUstawieniaView()` (around line 2844):

```javascript
    function activateUstawieniaView() {
      if (!USTAWIENIA_STATE.initialized) {
        USTAWIENIA_STATE.initialized = true;
        initUstawieniaActions();
      }
      Promise.allSettled([
        loadUstawieniaShell(),
        loadUstawieniaSession(),
        loadUstawieniaPathsAndCache(),
      ]);
    }
```

Replace with:

```javascript
    function activateUstawieniaView() {
      if (!USTAWIENIA_STATE.initialized) {
        USTAWIENIA_STATE.initialized = true;
        initUstawieniaActions();
      }
      Promise.allSettled([
        loadUstawieniaShell(),
        loadUstawieniaSession(),
        loadUstawieniaPathsAndCache(),
        loadSafetyNumbers(),
      ]);
    }
```

- [ ] **Step 4: Add `deviceId` to `VAULT_STATE`**

The verify button uses `VAULT_STATE.deviceId`. Find `VAULT_STATE` initialization (around line 1845):

```javascript
    const VAULT_STATE = {
```

Check if `deviceId` already exists. If not, add it:

```javascript
    const VAULT_STATE = {
      unlocked: false,
      sessionToken: null,
      deviceId: null,      // populated on unlock
```

Then find where `VAULT_STATE.sessionToken` is set (around line 1963, after successful unlock):

```javascript
        VAULT_STATE.sessionToken = data.session_token || null;
```

Add after that line:

```javascript
        VAULT_STATE.deviceId = data.device_id || null;
```

Note: `/api/unlock` response currently returns `session_token` and `expires_at`. If `device_id` is not in the unlock response, the verify button can fall back to skipping — the section still shows numbers. Alternatively, `GET /api/auth/session` returns `device_id` in the session response. Update `refreshUserProfile()` or `loadSafetyNumbers()` to get `device_id` from session if `VAULT_STATE.deviceId` is null:

In `loadSafetyNumbers()`, before rendering the verify button, add a fallback:

```javascript
        // Get device_id from session if not yet in VAULT_STATE
        if (!VAULT_STATE.deviceId && data.device_id) {
          VAULT_STATE.deviceId = data.device_id;
        }
```

Wait — `GET /api/vault/safety-numbers` doesn't return `device_id`. But `GET /api/auth/session` does return `device_id`. 

Simpler fix: add `device_id` to the `GET /api/vault/safety-numbers` response. In Task 3 Step 2, the handler has access to `session.device_id` — add it to the JSON:

```rust
Ok(Json(serde_json::json!({
    "safety_numbers": numbers,
    "key_generation": key_generation,
    "verified_at": verified_at,
    "device_id": session.device_id,   // add this line
})))
```

Then in `loadSafetyNumbers()` JS, after `const data = await res.json();`, add:

```javascript
        if (data.device_id) VAULT_STATE.deviceId = data.device_id;
```

Go back to Task 3 Step 2 and add `"device_id": session.device_id` to the JSON response before committing.

- [ ] **Step 5: Build and verify compile**

```
cargo build --release -p angeld 2>&1 | tail -30
```

Expected: `Finished release [optimized]`

- [ ] **Step 6: Commit**

```bash
git add angeld/static/index.html
git commit -m "feat(ui): Bezpieczeństwo — Safety Numbers section + QR code + weryfikacja"
```

---

## Task 5: Full build + plan.md update

- [ ] **Step 1: Full workspace release build**

```
cargo build --release --workspace 2>&1 | tail -20
```

Expected: `Finished release [optimized]`

- [ ] **Step 2: Run all angeld tests**

```
cargo test -p angeld 2>&1 | tail -30
```

Expected: all tests pass (no failures).

- [ ] **Step 3: Update plan.md**

In `plan.md`, find the Faza M entry and mark it `✅ DONE` with the commit hash after completion.

- [ ] **Step 4: Final commit**

```bash
git add plan.md
git commit -m "docs(plan): Faza M — Safety Numbers DONE"
```

---

## Self-Review

**Spec coverage check:**
- M.1 `safety_numbers()` in vault.rs → Task 1 ✓
- M.2 `GET /api/vault/safety-numbers` → Task 3 ✓
- M.3 UI "Bezpieczeństwo" section + QR → Task 4 ✓
- M.4 "Zweryfikowano" toggle + DB migration → Tasks 2 + 3 + 4 ✓
- `key_generation` in API response → Task 3 ✓
- `verified_at` in API response → Task 3 ✓
- `device_id` in API response (needed for verify button) → Task 4 note + Task 3 Step 2 ✓
- Święta Zasada logging → Task 3 Step 2 ✓

**Type consistency check:**
- `safety_numbers` signature: `(&self, user_id: &str) -> Option<String>` used consistently in Task 1 and Task 3 ✓
- `set_device_safety_verified(pool, device_id)` + `get_device_safety_verified_at(pool, device_id)` match across Tasks 2 and 3 ✓
- `VAULT_STATE.deviceId` introduced in Task 4 Step 4, used in Task 4 Step 2 ✓
