# Faza L — Sesja D: OAuth Frontend

**Data:** 2026-04-18  
**Status:** Zatwierdzone przez użytkownika — gotowe do implementacji  
**Commit docelowy:** `feat(ui): Sesja D — OAuth frontend + profil użytkownika`

---

## Zakres

Integracja backendu OAuth2 Google (Faza K) z interfejsem użytkownika. Cztery kroki: L.1 przycisk logowania, L.2 profil w topbarze, L.3 rozszerzony logout, L.4 guard po zalogowaniu.

---

## Architektura stanu autentykacji (Sekcja 1)

Nowy globalny obiekt `AUTH_STATE` w `index.html`, osobny od `VAULT_STATE`:

```js
const AUTH_STATE = {
  oauthToken: localStorage.getItem('omnidrive.oauthToken') || null,
  expiresAt: parseInt(localStorage.getItem('omnidrive.oauthExpiresAt') || '0'),
  user: null,  // { user_id, email, display_name } — wypełnia refreshUserProfile()
};
```

Ujednolicona funkcja `authHeaders()` (addytywna — nie zastępuje istniejących):

```js
function authHeaders(extra = {}) {
  const token = AUTH_STATE.oauthToken || VAULT_STATE.sessionToken;
  const h = { 'Accept': 'application/json', 'Content-Type': 'application/json', ...extra };
  if (token) h['Authorization'] = `Bearer ${token}`;
  return h;
}
```

Istniejące `vaultAuthHeaders()` i `mdAuthHeaders()` pozostają niezmienione.

---

## Zmiany backendowe (Sekcja 2)

### `GET /api/auth/session` — rozszerzenie odpowiedzi

Plik: `angeld/src/api/auth.rs`

Dodajemy JOIN z tabelą `users` przez `db::get_user()` (już istnieje). Nowe pola w odpowiedzi:

```json
{
  "valid": true,
  "user_id": "...",
  "device_id": "...",
  "expires_at": 1234567890,
  "email": "user@gmail.com",
  "display_name": "Jan Kowalski"
}
```

Brak migracji — tabela `users` i `db::get_user()` istnieją od Fazy K.

### Obsługa `#oauth_token` na frontendzie

Funkcja `consumeOauthTokenFromHash()` — wywołana raz przy DOMContentLoaded, przed `onHashChange`:

1. Parsuje `location.hash` jako `URLSearchParams`
2. Jeśli `oauth_token` obecny: zapisuje do `localStorage`, aktualizuje `AUTH_STATE`
3. `history.replaceState(null, '', '/#przeglad')` — token znika z URL i historii przeglądarki

Token nigdy nie jest logowany (Zero-Knowledge Rule — `[REDACTED]`).

---

## Interfejs użytkownika (Sekcja 3)

### L.1 — Przycisk "Zaloguj przez Google"

**Ustawienia (index.html):** `loadUstawieniaSession()` renderuje dynamicznie:
- Sesja z emailem → badge "Połączono" + "Zalogowano jako {email}"
- Brak sesji / sesja lokalna → przycisk "Zaloguj przez Google" (`window.location.href = '/api/auth/google/start'`)

**Wizard (wizard.js):** Link opcjonalny na kroku 0 w panelu "Aktualny stan" — `<a href="/api/auth/google/start">`.

### L.2 — Profil w topbarze

Nowa funkcja `refreshUserProfile()`:

| Warunek | `#userName` | `#userRole` |
|---------|------------|-------------|
| valid=true, email istnieje | email | "Google" |
| valid=true, brak email | display_name lub skrócony user_id | "Lokalna sesja" |
| valid=false lub błąd | "Local" | "Lokalna sesja" |

Avatar: ikona `account_circle` (material icon) — brak zmian dopóki backend nie zwróci `picture_url`.

### L.3 — Logout rozszerzony

Istniejący handler w nav click `wyloguj` — dopisujemy (nic nie usuwamy):

1. Jeśli `AUTH_STATE.oauthToken` istnieje: `POST /api/auth/logout` z Bearer OAuth token
2. `localStorage.removeItem('omnidrive.oauthToken')`
3. `localStorage.removeItem('omnidrive.oauthExpiresAt')`
4. Reset `AUTH_STATE` w pamięci
5. Istniejący cleanup `VAULT_STATE` pozostaje bez zmian
6. `location.reload()`

### L.4 — Guard po zalogowaniu OAuth

Funkcja `oauthPostLoginGuard()` — wywołana po `consumeOauthTokenFromHash()`, tylko gdy token właśnie wczytany:

1. Jeśli `AUTH_STATE.oauthToken` jest null → exit (nie po OAuth redirect)
2. `GET /api/onboarding/status`
3. Jeśli `onboarding_state !== 'COMPLETED'` → usuń `hidden` z `#onboardingWizardOverlay`
4. Wizard obsługuje swój stan autonomicznie przez własne `init()`

---

## Pliki do modyfikacji

| Plik | Zmiany |
|------|--------|
| `angeld/src/api/auth.rs` | `get_auth_session` → JOIN users, +email, +display_name |
| `angeld/static/index.html` | `AUTH_STATE`, `consumeOauthTokenFromHash`, `refreshUserProfile`, logout, guard, przycisk |
| `angeld/static/wizard.js` | Link "Zaloguj przez Google" na kroku 0 |

**Bez nowych endpointów. Bez migracji bazy. Zero ryzyka dla danych produkcyjnych.**

---

## Weryfikacja po implementacji

1. `cargo check` musi przejść bez błędów
2. Wizyta na `http://127.0.0.1:8787` → kliknięcie "Zaloguj przez Google" → redirect do Google
3. Po autoryzacji Google → powrót do `/#przeglad`, token w localStorage, profil w topbarze
4. Wylogowanie → localStorage wyczyszczony, reload, profil wraca do "Local"
5. Świeży onboarding (INITIAL) po OAuth → wizard wyskakuje automatycznie
