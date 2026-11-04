# e31s01 — configured CAPTCHA refuses an unsolved signup

## 1. Story

As a public-node operator, I configure one verifier (sitekey +
secret) and signup shows a challenge: a missing or failing
token creates no account row and re-renders the form with a
clear retry message; a passing token signs up normally. With no
verifier configured, signup is unchanged.

## 2. Business value

e20 rate budgets bound abuse per IP; a distributed burst still
buys accounts cheaply. CAPTCHA bills each account human
attention — the one currency botnets cannot budget for. The
env-shaped opt-in keeps self-hosters free of an external
dependency, matching how mail and every other provider-shaped
feature behaves. SCOPE v0.6.0 in_scope e31.

## 3. Type

feat

## 4. Risk / context

risk: P1 — a fail-open verifier check (or one skipped when the
verifier is unreachable) makes the feature decorative; the
suite asserts fail-closed-to-retry whenever configured. A
fail-closed-when-unconfigured signup would lock the node, so
unconfigured = no challenge, verified by its own case. Signup
endpoint is already rate-limited (e20); the verifier call adds
one outbound HTTP per signup — bounded, timed out, never on
the login path.

## Prior art

Research-first, 2026-11-04 (before any code):

- **Choice: Cloudflare Turnstile.** Server-side validation is
  mandatory and simple: POST the `cf-turnstile-response` token
  plus `secret` to `https://challenges.cloudflare.com/turnstile/v0/siteverify`;
  response JSON `{success, ...}`. Tokens expire in 300s
  (enforced server-side). Free, privacy-friendly (no per-user
  ad-profile), managed mode = invisible-or-checkbox. Docs:
  developers.cloudflare.com/turnstile.
- **Rejected: Google reCAPTCHA** — third-party tracking on a
  self-hosted node's signup page, heavier challenges, and the
  only verifier of the three with a real telemetry footprint.
- **Rejected: hCaptcha** — same siteverify shape, but reward/
  ads model and a heavier widget; nothing jjhub needs that
  Turnstile lacks.
- **Rejected: built-in proof-of-work challenge** — no external
  dep, but we would own the arms race; the scope constraint
  already accepts one verifier call.
- Both supported verifiers share the siteverify contract, so
  the code targets that shape (one POST, one `success` field)
  and a second verifier would be config, not a rewrite.

## 5. Steps

1. Research record — this section exists with the choice and
   rejections above → verify: `grep -q 'Prior art'
   specs/epics/e31-captcha-signup/e31s01-captcha-signup.md`
2. RED cases in `crates/server/tests/captcha_signup.rs`:
   configured + missing token → no users row, form re-rendered
   with message; configured + verifier says success=false →
   same; configured + success=true → account created;
   unconfigured → today's behavior byte-for-byte → verify:
   `cargo test -p server --test captcha_signup` (expected RED:
   feature absent)
3. GREEN: widget rendered when
   `JJHUB_CAPTCHA_SITE_KEY`/`SECRET_KEY` set (Tera context,
   `cf-turnstile-response` field), server-side siteverify call
   with timeout before the account row, fail closed to the
   retry message when configured and rejected/unreachable →
   verify: `cargo test -p server --test captcha_signup`
4. Contract: siteverify pointed at a test double (the suite's
   stub server, per messagepit precedent) — no network in
   tests → verify: same suite
5. Full gates → verify: `cargo fmt --all --check && cargo
   clippy --workspace --all-targets -- -D warnings && cargo
   test && cargo build`

## Verification Script

`cargo test -p server --test captcha_signup` proves all four
cases plus the no-network contract; the existing auth suites
prove unconfigured signup and login never changed.

## Out of scope

- CAPTCHA on login, password reset, or any existing account
  (scope: registration only), challenge style options beyond
  the verifier's defaults, multi-verifier configuration.

## Risks

- Putting the check after the account row would leave created
  accounts behind failed challenges — suite asserts the users
  table is empty after every refusal, not just the response.
