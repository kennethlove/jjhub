# e01s01 — Register, login, session-authenticated pages

## 1. Story

As a self-hoster, I register an account, log in, and stay authenticated across pages, so jjhub knows who I am before I create repos or tokens.

## 2. Business value

Foundation for every authenticated flow (repos, MRs, tokens). Without accounts nothing else works.

## 3. Type

feat

## 4. Risk / context

risk: P0 (auth boundary, BCP 5) — context: infra + domain

## 5. Context

jjhub is a greenfield axum service. This story boots the workspace: binary crate, router, Postgres pool via sqlx, migrations runner, plus users table, registration and login pages (Tera), argon2 password hashing, signed httpOnly session cookie, and an auth middleware.

## 6. Requirements

#### ADDED: User registration
A visitor submits email + password on /register. Email is unique (case-insensitive). Password hashed with argon2. Success → session issued, redirect to /settings.

#### ADDED: User login
A user submits email + password on /login. Wrong credentials → 401 page, no session. Success → session cookie set (signed, httpOnly, SameSite=Lax), redirect to /settings.

#### ADDED: Session persistence
Session survives navigation. Expired or absent session on a gated page → redirect to /login.

#### ADDED: Auth middleware
Routes under /settings require a valid session; all other seeded routes are public.

## 7. Actors

Visitor (unauthenticated), User (authenticated).

## 8. Preconditions

Postgres reachable (DATABASE_URL); migrations run on boot.

## 9. Postconditions

users table exists; /healthz serves; register/login/logout work end-to-end.

## 10. Business rules

- Email case-insensitive uniqueness.
- Minimum password length 10 chars.
- One session per login; logout clears cookie.

## 11. Domain model touchpoints

**User** (tech-stack.md): id, email, password_hash, created_at. No profile beyond account (out of scope).

## 12. UI/UX notes

Tera server-rendered forms, Tailwind styling, htmx not required for this story.

## 13. API contracts

None yet (HTML forms). /healthz returns 200 "ok".

## 14. Data model

users(id uuid pk, email citext unique, password_hash text, created_at timestamptz).

## 15. Non-functional requirements

Password hashing must be argon2id. Session cookie secret from config/env, never committed.

## 16. Out of scope

Password reset, email verification, OIDC, rate limiting on login (post-MVP), user profiles.

## 17. Acceptance criteria

- [ ] `cargo run` boots, /healthz returns 200.
- [ ] Register creates user; duplicate email rejected.
- [ ] Login with wrong password rejected; with right password sets session.
- [ ] /settings redirects to /login when logged out; renders when logged in.
- [ ] Logout ends session.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: register then login
  Given no user "a@example.com"
  When I register "a@example.com" with password "correct horse battery"
  Then a session is issued
  And I am redirected to /settings

Scenario: duplicate registration rejected
  Given user "a@example.com" exists
  When I register "a@example.com" again
  Then registration fails with a conflict

Scenario: gated page requires session
  Given I am logged out
  When I GET /settings
  Then I am redirected to /login
```

## 19. Verification script (manual)

1. `cargo run` in one terminal.
2. `curl -sf localhost:3000/healthz` → "ok".
3. Browser: register at /register, land on /settings.
4. Log out, log back in → /settings again.
5. `curl -i localhost:3000/settings` without cookie → 303 to /login.

## 20. Traceability

`// story: e01s01` in implementing code and tests.
