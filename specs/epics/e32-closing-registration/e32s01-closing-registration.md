# e32s01 — operator toggle refuses new signups, sessions untouched

## 1. Story

As a public-node operator, I close registration from the /admin
surface and new accounts stop being created node-wide, with a
clear message on the signup form; reopening restores signup, and
everyone already signed in keeps working throughout.

## 2. Business value

The cheapest inflow brake a public node can offer: for a private
party, a conference, or a node under signup flood, one toggle
stops the flood without touching a single existing account. It
reuses the operator gate, CSRF, and moderation_actions trail
e29 shipped, so the lever costs one settings row and one form —
SCOPE v0.6.0 in_scope e32.

## 3. Type

feat

## 4. Risk / context

risk: P1 — gates new-user acquisition by mistake, so the
refusal message must say the node is invite-free and closed
(not "error"), and the toggle must be operator-only (404 for
everyone else). Existing sessions, logins, and passwords are
untouched: the check lives only in the register handler. The
setting is a row (survives restarts), never an env var — the
scope says reversible from the surface, not from a deploy.

## 5. Steps

1. RED cases in `crates/server/tests/registration_gate.rs`:
   closed → POST /register returns the refusal and the users
   table has no new row; open → same request creates the
   account; an authenticated non-operator POSTs the toggle and
   gets 404 with no row change; a logged-in session keeps
   fetching /settings while closed → verify: `cargo test -p
   server --test registration_gate` (expected RED: no
   site_settings row, no route, register unaware)
2. GREEN: migration adds the registration-closed setting;
   `POST /admin/registration` (on/off) behind the existing
   protected router with session + CSRF; register handler
   checks it and re-renders the signup form with a clear
   message; both flips insert a moderation_actions row →
   verify: `cargo test -p server --test registration_gate`
3. Panel: /admin gets the registration section with state and
   toggle forms on DESIGN.md tokens → verify: `cargo test -p
   server --test registration_gate` + CDP overflow probe
4. Full gates → verify: `cargo fmt --all --check && cargo
   clippy --workspace --all-targets -- -D warnings && cargo
   test && cargo build`

## Verification Script

`cargo test -p server --test registration_gate` proves the
refusal creates no row, the non-operator gets 404, flips are
audited, and live sessions survive; full preflight proves no
regression.

## Out of scope

- Invite codes or waitlists (a closed door stays closed),
  closing login for existing users, per-form CAPTCHA (e31).

## Risks

- Checking in a middleware instead of the register handler
  would also wall password reset and login — the suite asserts
  exactly one refused route (register) so the blast radius
  stays named.
