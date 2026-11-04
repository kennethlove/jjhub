# e29s01 — operator designation and web suspension at /admin

## 1. Story

As a public-node operator I designate an operator and suspend an
abuser from a site-level `/admin` page: the page lists accounts,
lets me suspend or unsuspend one, and every web path the account
touches — its live session, its next login — refuses it until I
unsuspend. Each action records actor, target, and time.

## 2. Business value

No moderation surface exists at any layer today (v0.5.0 scope
e29): blocking an abuser is the minimum viability test for running
a public node. Enforcement lands at the two places a session
becomes a user — `middleware::require_session` (middleware.rs:33
existence query) and the login handler (routes.rs:434 credential
query) — plus the review-proxy session branch (review_proxy.rs:56),
so one status column covers every web entry. Transport refusal is
e29s02.

## 3. Type

feat

## 4. Risk / context

risk: P0 — auth/security boundary (trust-boundary tests required
on the operator check itself). Decisions (user-confirmed):

- **Bootstrap:** effective operator = `users.is_operator` OR the
  handle named by env `JJHUB_OPERATOR` (checked at query time, so
  it works before any account exists and never races signup).
- **Surface:** site-level `/admin` page, operator-only (404 for
  non-operators), session + CSRF like every other POST; modeled on
  the settings page.
- **Lockout guard:** operators cannot be suspended (covers self
  and the last operator — the role stays lockout-able never).
- **Storage:** migration 0016 — `users.is_operator`,
  `users.suspended_at`, `moderation_actions (id, actor_id, target_id,
  action, created_at)`.
- Suspension is reversible: unsuspend clears `suspended_at`, the
  same account and sessions return; public repos stay readable.

## 5. Steps

1. RED suite `crates/server/tests/operator_moderation.rs`: env
   bootstrap names an operator; a column operator is effective;
   ordinary user gets 404 on GET/POST /admin; operator suspends a
   target (303) and a `moderation_actions` row lands with actor,
   target, action; the target's live session is rejected on its
   next request; its login attempt is refused; operator targets
   and self-suspend are refused (403) with no row; unsuspend
   restores the same session without re-registration; POST needs
   session and CSRF → verify: `cargo test -p server --test
   operator_moderation` (expected RED: relation/users columns and
   routes missing)
2. Add migration `0016_moderation.sql` (columns + audit table) →
   verify: `cargo test -p server --test operator_moderation` (RED
   continues: routes absent; migration no longer errors)
3. Effective-operator helper (column OR `JJHUB_OPERATOR` handle)
   and operator gate on GET /admin → verify: `cargo test -p
   server --test operator_moderation`
4. POST /admin/suspend and /admin/unsuspend behind session + CSRF +
   operator gate, audit row per action, lockout guard → verify:
   `cargo test -p server --test operator_moderation`
5. Enforce suspension where sessions become users: require_session
   existence query, login credential query, review-proxy session
   branch → verify: `cargo test -p server --test
   operator_moderation && cargo test -p server --test
   auth_middleware` (existing session tests stay green)
6. `/admin` template on DESIGN.md tokens (settings-page pattern,
   suspend/unsuspend forms, suspended badge) → verify: `ux_audit`
   on the absolute `crates/server/assets/app.css` path
7. Full gates + CDP UAT of /admin at 1440 and 390 → verify: `cargo
   fmt --all --check && cargo clippy --workspace --all-targets --
   -D warnings && cargo test && cargo build`

## Verification Script

`cargo test -p server --test operator_moderation` (new suite) +
`cargo test -p server --test auth_middleware` (existing session
contract unchanged); manual: with `JJHUB_OPERATOR=<handle>` set,
open `/admin` as that operator, suspend a second account, watch its
live session bounce to /login and its login fail, unsuspend, same
account works again.

## Out of scope

- Transport refusal (e29s02), per-owner blocks, takedown, appeals,
  audit export — v0.5.0 SCOPE out_of_scope.

## Risks

- Forgetting the review-proxy session branch leaves one web hole —
  covered by an explicit suite case.
- Suspended-state leakage: login must answer like a bad password,
  not "suspended" (anti-enumeration) — suite asserts identical
  refusal status.
