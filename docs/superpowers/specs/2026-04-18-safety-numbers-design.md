# Safety Numbers (Faza M) — Design Spec

**Date:** 2026-04-18  
**Session:** E  
**Status:** Approved

---

## Goal

Add Signal-style Safety Numbers to OmniDrive: a 60-digit cryptographic fingerprint derived from `envelope_vault_key` + `user_id`, exposed via API and displayed in the UI with a QR code for cross-device verification.

---

## Architecture

### Key Material

**Input:** `envelope_vault_key` (current generation, random V2 VK) concatenated with `user_id` bytes.  
Rationale: fingerprint reflects the actual encryption key state — rotates with the key, prompting re-verification. Signal-like determinism within a key generation.

**NOT used:** `master_key` (passphrase-derived — would not change on rotation).

### Hash Algorithm

```
input  = envelope_vault_key_bytes || user_id.as_bytes()
hash   = SHA-256(input)                          // 32 bytes
blocks = hash[0..23] split into 12 × u16 (BE)   // 12 values, each 0–65535
digits = each u16 zero-padded to 5 decimal chars  // "01234"
output = 12 blocks joined by space               // 60 chars total
```

### Security Logging (Święta Zasada)

```rust
tracing::info!("[SAFETY_NUMBERS] generated for user={} [key_material: REDACTED]", user_id);
```

Raw key bytes **never** appear in logs.

---

## Components

### M.1 — Backend: `safety_numbers()` in `vault.rs`

New public method on `UnlockedVaultKeys`:

```rust
pub fn safety_numbers(&self, user_id: &str) -> String
```

- Uses `self.envelope_vault_key` (current generation only, not `previous_envelope_vault_key`)
- Uses `sha2::Sha256` from the `sha2` crate (already in workspace)
- Returns 12 space-separated 5-digit blocks: `"01234 56789 ..."`

### M.2 — API: `GET /api/vault/safety-numbers`

**File:** `angeld/src/api/vault.rs` — new route added to existing router.

**Auth:** Requires valid vault session (`Authorization: Bearer <token>`). `user_id` taken from session — no additional header needed.

**Response 200:**
```json
{
  "safety_numbers": "01234 56789 11111 22222 33333 44444 55555 66666 77777 88888 99999 00000",
  "key_generation": 3
}
```

`key_generation` from `VaultParams` — client uses it to detect if re-verification is needed after `rotate-key`.

**Response 401:** Vault locked or no valid session:
```json
{ "error": "vault_locked_or_no_session" }
```

### M.3 — UI: "Bezpieczeństwo" section in Ustawienia

**File:** `angeld/static/index.html`

New collapsible section after the "Sesja" section in Ustawienia. Loaded by `loadSafetyNumbers()` called from the Ustawienia tab init.

Layout:
```
┌──────────────────────────────────────────────┐
│  Bezpieczeństwo — Safety Numbers             │
│                                              │
│  Generacja klucza: 3                         │
│                                              │
│  01234  56789  11111  22222  33333  44444    │
│  55555  66666  77777  88888  99999  00000    │
│                                              │
│  [canvas QR — 180×180px via qrious CDN]     │
│                                              │
│  [✓ Oznacz urządzenie jako zweryfikowane]    │
│   Ostatnia weryfikacja: 2026-04-18 14:32     │
└──────────────────────────────────────────────┘
```

**QR library:** qrious 4.0.2 via cdnjs CDN (5KB, no dependencies):
```html
<script src="https://cdnjs.cloudflare.com/ajax/libs/qrious/4.0.2/qrious.min.js"></script>
```

QR value = raw 60-digit string without spaces (digits only, no separators).

**JS flow:**
1. `loadSafetyNumbers()` — `GET /api/vault/safety-numbers` with Bearer token
2. Renders `key_generation` and formatted digit blocks (`font-mono`, glassmorphism card)
3. `new QRious({ element: canvas, value: digits.replace(/ /g, ''), size: 180 })`
4. Shows last verification date from `safety_numbers_verified_at` if present

### M.4 — DB Migration: `safety_numbers_verified_at` column

**File:** `angeld/src/db.rs`

New column on `devices` table:
```sql
ALTER TABLE devices ADD COLUMN safety_numbers_verified_at INTEGER;
```

Added via `ensure_column_exists(pool, "devices", "safety_numbers_verified_at", "INTEGER")` — same pattern as existing lazy migrations.

**New API:** `POST /api/vault/devices/{device_id}/verify`  
Sets `safety_numbers_verified_at = epoch_secs()` for the given device. Requires valid session. Returns `{ "verified_at": <timestamp> }`.

---

## Error Handling

| Condition | Behavior |
|-----------|----------|
| Vault locked (no vault_keys) | 401 `vault_locked_or_no_session` |
| No session token | 401 `vault_locked_or_no_session` |
| Device not found (verify endpoint) | 404 |
| `envelope_vault_key` missing (should not happen when unlocked) | 500 internal — log error, no key material exposed |

---

## Testing

- Unit test for `safety_numbers()`: fixed key bytes + fixed user_id → assert expected 12-block string
- Unit test: output is exactly 59 chars (12 × 5 digits + 11 spaces)
- API test: GET returns 200 with `safety_numbers` and `key_generation` fields when vault unlocked
- API test: GET returns 401 when no session
- Manual: rotate key → safety_numbers change; `verified_at` clears conceptually (UI shows "Zweryfikuj ponownie")

---

## Out of Scope

- P2P / cross-device QR scanning (future)
- Automatic re-verification prompts on key rotation (Faza N cleanup)
- Safety Numbers in mobile app
