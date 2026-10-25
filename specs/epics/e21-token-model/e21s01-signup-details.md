# e21s01 — Signup auto-issues token; one-time details screen

## 1. Story

As a new user, I finish signup and land on a details screen that
hands me my transport token — copy-once, never shown again — so I
can clone my own empty repo without hunting through settings.

## 2. Business value

Tracer-bullet slice of the token-model epic: signup mints the
credential the product is built around (transport tokens are the
only non-interactive credential) and delivers it where the user
already is. The e15s02 shown-once discipline carries over: hashed
storage, plaintext rendered exactly once.

## 3. Type

feat

## 4. Risk / context

risk: P0 (BCP 5 — signup flow is the front door; credential
delivery + new users columns) — context: users gains
`display_name` (nullable) and `theme` (default `system`) in one
migration; the details screen is a new route behind the session.

## 5. Context

Zoom-out on the modules this touches:
- **signup route** (routes.rs) — purpose: create the account and
  session. Callers: POST /signup. Contracts: after this story it
  mints one transport token (via token::create, token.rs:31) and
  redirects to the details screen instead of the old landing.
- **token module** (token.rs) — purpose: generate/hash/create
  transport tokens. Callers: settings token form today, signup
  after this story. Contracts: plaintext returned once by create;
  only the hash is stored.
- **session** (session.rs) — the details screen requires a signed
  session; completion marking rides on the users row.
- **Tera templates** — new details template; DESIGN.md is the
  source of truth for tokens, and the UI audit gate applies.

## 6. Requirements

#### ADDED: Signup mints one transport token
POST /signup creates the account, mints one transport token
(stored hashed, label "signup"), and records its id so the
details screen can show the plaintext exactly once.

#### ADDED: One-time details screen
After signup the user lands on a details screen showing the
plaintext token (copy-once control, no re-display after
completion). Completing it marks completion; the screen never
re-renders afterwards. Abandoned screens do not re-trigger on
later logins — regenerate (e21s03) covers users who never finish.

#### ADDED: users columns for display name and theme
Migration adds `users.display_name TEXT NULL` (UI fallback:
handle) and `users.theme TEXT NOT NULL DEFAULT 'system'`
(allowed: dark | light | system). Defaults keep every existing
row valid; no backfill.

## 7. Actors

New user, existing user (schema only).

## 8. Preconditions

Signup flow green; token module green; migration chain through
0009 applied.

## 9. Postconditions

A new account exists with exactly one active transport token; the
plaintext appeared once, on the details screen; completing it makes
the route return the ordinary landing forever after.

## 10. Business rules

- Exactly one auto-issued token per signup; label fixed
  ("signup"); scopes default read+write (e22 lands the column; if
  e22 ships first, the mint passes it explicitly).
- Plaintext never appears in logs, redirects, or re-renders.
- Existing zero-token users are NOT backfilled (SCOPE decision).

## 11. Domain model touchpoints

**User** (two new columns + details-completion marker), **Token**
(new row per signup).

## 12. UI/UX notes

New details screen: token copy-once control, completion button,
display-name and theme inputs (e21s02 wires their effects),
gravatar preview (e21s02). Must pass the DESIGN.md audit.

## 13. API contracts

POST /signup redirect target changes to the details route (new
GET/POST pair). Old landing remains for later logins.

## 14. Data model

Migration 0010: `users.display_name TEXT NULL`,
`users.theme TEXT NOT NULL DEFAULT 'system'`, plus the users-side
marker for details completion (column or one-row table — decided
in code).

## 15. Non-functional requirements

Token minting is in the signup transaction path — one extra INSERT,
no network; signup latency must not visibly regress.

## 16. Out of scope

Preference effects (e21s02), regenerate (e21s03), demoting the
repo-page form (e21s03), email delivery, backfill (SCOPE).

## 17. Acceptance criteria

- [ ] `cargo test -p server users_prefs_migration` — columns exist
      with correct defaults; existing rows valid. (e21s01t1)
- [ ] `cargo test -p server signup_auto_token` — signup mints one
      hashed token and redirects to the details route.
      (e21s01t2)
- [ ] `cargo test -p server details_once` — plaintext renders once;
      completion blocks re-render; abandonment does not re-trigger.
      (e21s01t3)
- [ ] `cargo test --workspace` green. (e21s01t4)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: signup hands over a token
  Given a visitor on the signup form
  When they register a new account
  Then they land on the details screen
  And one hashed transport token exists for their user

Scenario: the screen never re-renders
  Given a user who completed the details screen
  When they log in again
  Then the details route shows the ordinary landing
  And no plaintext token is ever rendered again

Scenario: abandoned screen does not re-trigger
  Given a user who signed up but never completed the screen
  When they log in later
  Then they land on the ordinary landing
  And settings regenerate remains available
```

## 19. Verification script (manual)

1. Sign up a fresh account.
2. Details screen shows one token with a copy control.
3. Complete it; log out; log in — the screen never reappears and
   the token is not shown anywhere.

## 20. Traceability

`// story: e21s01` in the signup handler, the details route and
template, migration 0010, and the `signup_auto_token` /
`details_once` tests.
