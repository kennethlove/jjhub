# e34s01 — mailer core and address verification

## 1. Story

As a new user, signing up sends an address-verification mail
through one mailer; with no provider configured the mail is
recorded locally and never leaves the box while every flow still
succeeds in-page; a visitor who never got the mail can ask for a
resend that is cooldown-limited and answered neutrally either
way; verification links use path form, not a query string.

## 2. Business value

The foundation the rest of v0.7.0 stands on: one selection
mechanism (explicit env, LogMailer default), the phase gate
honest by construction, and the verification loop closed for a
public node — SCOPE v0.7.0 in_scope e34 (mailer core, phase
gates, verification resend with 60s cooldown).

## 3. Type

feat

## 4. Risk / context

risk: P1 — signup is the growth path, so an unset provider must
change nothing (byte-for-byte current behavior when
JJHUB_EMAIL_PROVIDER is unset or unknown); the resend endpoint
is unauthenticated (an unverified user has no session — e23
gates login), so it must stay neutral (no enumeration) and
cooldown-bound per address. The link form switch must not break
the e23 verify route: old `?token=` links keep working.

## 5. Steps

1. RED cases in `crates/server/tests/mail_core.rs`: unset
   provider → signup creates the account and the verification
   delivery is recorded with a path-form `/verify/<token>` link
   (never `?token=`); unknown provider value behaves like unset;
   POST /resend-verification with a known address records one
   delivery and returns the same neutral response as an unknown
   address; a second request inside 60s records no second
   delivery; the mailer never being reachable cannot 500 the
   signup or the resend → verify: `cargo test -p server --test
   mail_core` (expected RED: LogMailer default and resend
   endpoint missing, links still query-form)
2. GREEN: explicit provider selection with LogMailer as the
   default for unset/unknown; `verify_link` switches to path
   form while the old query form stays accepted; resend
   endpoint with a 60s per-address cooldown and an always-identical
   neutral response → verify: `cargo test -p server --test
   mail_core`
3. Resend affordance on the verify-sent page (same form-on-DESIGN.md-tokens
   pattern) → verify: `cargo test -p server --test mail_core` +
   CDP overflow probe at 1440 and 390
4. Full gates → verify: `cargo fmt --all --check && cargo
   clippy --workspace --all-targets -- -D warnings && cargo
   test && cargo build`

## Verification Script

`cargo test -p server --test mail_core` proves the default arm
records rather than sends, links are path-form, the cooldown
holds, and responses never vary by address existence; full
preflight proves no regression.

## Out of scope

- SMTP arm and moderation notices (e34s03), password reset
  (e34s02), JSONL retention (e35), login/reset CAPTCHA (e31
  stays signup-only).

## Risks

- Making resend authenticated would lock out exactly the users
  it exists for; the suite asserts a no-cookie request works.
- A per-address cooldown keyed by raw address in a log would
  leak the address — the cooldown key must be a hash, matching
  the no-secret-actions rule.
