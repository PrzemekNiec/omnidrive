# Wizard Redesign — Design Spec
**Date:** 2026-04-28  
**Status:** Approved by user

---

## Problem

Wizard screens (`wizard.html` + `wizard.js`) were "mega squished" on Dell during v0.3.6 smoke test. Root cause: all `stepBody()` functions use viewport-based Tailwind breakpoints (`xl:grid-cols-*`, `lg:grid-cols-*`) that fire at 1280px/1024px viewport width, but the wizard panel is `max-w-xl` (560px). On a 1440px monitor the multi-column grids trigger inside a 560px container — each column becomes ~170px, unreadable.

Additionally `icons/tray_icons/` PNGs were never copied to the installer payload, causing `omnidrive-tray.exe` to panic on startup.

---

## Scope

1. **Wizard UI redesign** — `angeld/static/wizard.js` `stepBody()` functions, no changes to `wizard.html` shell or backend
2. **Tray icons payload fix** — copy `icons/tray_icons/` into `dist/installer/payload/icons/tray_icons/`
3. **Version bump** — 0.3.6 → 0.3.7, rebuild installer

---

## Design Decisions

### Layout (Option C — approved)
- Keep narrow panel: `max-w-xl` (560px), unchanged `wizard.html`
- Redesign every `stepBody()` for **single-column** — no side-by-side grids
- Use only `grid-cols-2` where the container comfortably fits two items (e.g., pairs of short inputs, 2×2 summary cards)
- No `lg:` / `xl:` viewport breakpoints in step body content

### Step 1 — Powitanie
- Status cards in `grid grid-cols-2 gap-3`: Skarbiec, Tryb, Urządzenie, Konto Google
- Info strip below (single card with descriptive text)

### Step 2 — Tryb pracy (Option B — approved)
- Radio list in a `glass-muted` container: three `<button>` rows each with radio circle + title + subtitle + icon
- Selected item highlighted with cyan border + background
- Description card below with left cyan border showing expanded info for selected mode

### Step 3 — Tożsamość urządzenia
- Single text input for device name
- Device ID card below (read-only, shows assigned ID or placeholder)

### Step 4 — Dostawcy chmurowi (Option A — approved)
- Tab bar: R2 | B2 | SCW — each tab shows provider short name + status badge
- Active tab highlighted cyan, form below updates on tab switch
- Form fields: Endpoint (full width), Bucket + Region (grid-cols-2), Access Key + Secret Key (grid-cols-2), Enable + Path-style checkboxes (grid-cols-2)
- Status banner (green/red/neutral) at bottom of form
- "Testuj połączenie" button aligned right

### Step 5 — Bezpieczeństwo
- Two password inputs stacked (Hasło główne, Potwierdź hasło)
- Two info cards below: security note + what happens next

### Step 6 — Finalizacja
- Summary in `grid grid-cols-2 gap-3`: Wybrany tryb, Urządzenie, Zweryfikowani dostawcy, Hasło
- Confirm banner (green for local/cloud, cyan for join mode)
- Primary button label: "Dołącz do istniejącego Skarbca" (join) or "Uruchom OmniDrive" (other)

---

## Tray Icons Fix

`omnidrive-tray.exe` calls `load_icons()` which panics if PNGs not found. It searches `{exe_dir}/icons/tray_icons/`. The installer copies `{PayloadDir}\icons\*` but `tray_icons/` subfolder was never in the payload.

**Fix:** Before running Inno Setup, copy `icons/tray_icons/` → `dist/installer/payload/icons/tray_icons/`. This is a manual build step (same as copying `.exe` files).

---

## Files Changed

| File | Change |
|---|---|
| `angeld/static/wizard.js` | Replace all `stepBody()` switch cases with single-column designs |
| `dist/installer/payload/icons/tray_icons/` | Add 5 PNG files from `icons/tray_icons/` |
| All `Cargo.toml` + `omnidrive.iss` | Bump 0.3.6 → 0.3.7 |
| `dist/installer/payload/*.exe` | Rebuild from `cargo build --release --workspace` |

---

## Out of Scope
- `wizard.html` shell (header, progress bar, nav buttons — untouched)
- Backend API (`angeld/src/api/onboarding.rs` — untouched)
- Dashboard (`index.html`) — separate Epic 36 work
