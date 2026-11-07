---
bug_id: BUG-003
status: fixed
severity: medium
scope: mail
title: Verification mail carries a relative /verify path — recipient cannot click through
---

# BUG-003: verification mail carries a relative /verify path

## Symptom

Staging run for e36s03 t2 (real relay, `JJHUB_EMAIL_PROVIDER=smtp`)
delivered a verification mail whose whole body is:

    Open /verify/vfy_ad21c4cd98c3d5ee886e3dc9925ab0793ce0e835b4baa2931e1e80f40120d9f2 once to confirm your address. It expires in 24 hours.

The path has no origin, so a mail client has nothing to open: the
link is not clickable and the recipient must retype the path onto the
site by hand. Expected: an absolute link
`http(s)://<site>/verify/<token>` like the password-reset mail
already sends (e34s02 built `http://{host}/reset/{token}`).

## RCA (4-phase)

1. **Reproduce** — the staging send itself (e36s03 t2): received mail
   body quoted above; relay delivered fine, body carries a bare path.
2. **Isolate** — three verification call sites build bodies from
   `email::verify_link`, which returns only the path segment; the
   bodies are handed to the mailer verbatim. The reset mail is the
   one body that already derives an origin, and it does so inline in
   its own handler — the verify sites never had that step.
3. **Hypothesize** — the path-form link decision (MAIL-LATEST:
   tokens in the link path, never a query string) was read as
   "send the path", dropping the distinction between path form in
   routes and a usable link in a mail. No origin was ever attached
   on the verify path.
4. **Verify** — code read: `email::verify_link` formats
   `/verify/{token}` only; signup, resend, and login-resend pass it
   straight into `Mailer::send`; no base/origin env or helper exists
   anywhere in that path; the received staging mail matches the
   format string exactly. Root cause verified, not hypothesized.

## Security impact: LOW

Origin is not configured — it must come from the request. Deriving it
from the `Host` header (the reset mail's shipped pattern) lets a
forged `Host` on an attacker-triggered send put an attacker-origin
link into a victim's mail. For the verify path the token only
completes an address check on an account the attacker signed up
themselves, so the gain is small; the same pattern on the reset mail
is the higher-value target and already ships that way. Fix uses one
shared origin helper for every mail so a later hardening (canonical
origin) lands in one place, not four.

## Fix approach

One origin derivation, shared by all mail bodies:

- `email::origin(headers)` → `scheme://host`, host from the `Host`
  header (fallback `localhost:3000`), scheme from
  `X-Forwarded-Proto` when a proxy sets it, else `http` — keeps the
  reset mail's plain-http behavior when no proxy header is present.
- The three verify call sites prefix the path with `origin` (headers
  reach them: `register`, `login`, and `verify_resend` gain/forward
  the `HeaderMap` extractor).
- The reset handler switches to the same helper (same output as
  today for plain-host requests; gains proxy-aware scheme).

Risk: Low — mail bodies only; routes and token handling untouched.

## TDD Fix Plan

1. **RED**: test — signup through `register` with a `Host` header
   records a mail body containing `http://<host>/verify/<token>`
   (absolute), and the token still verifies the address.
   **GREEN**: `email::origin` helper + `HeaderMap` on `register`;
   signup body built from `origin + path`.
   **verify**: `cargo test -p server --test email_confirmation`

2. **RED**: test — `POST /verify-resend` with a `Host` header records
   an absolute verify link in the resend body.
   **GREEN**: `HeaderMap` on `verify_resend`; body from the shared
   helper.
   **verify**: `cargo test -p server --test email_confirmation`

3. **RED**: test — unverified login (which auto-resends) with a
   `Host` header records an absolute link.
   **GREEN**: `HeaderMap` on `login`, passed into
   `login_resend_verification`.
   **verify**: `cargo test -p server --test email_confirmation`

**REFACTOR**: reset handler uses `email::origin` too (behavior
preserved under existing reset tests).

## Acceptance Criteria

- [ ] Verify mail bodies carry an absolute link built from the
      request origin; token form stays path-form.
- [ ] Existing reset mail tests still pass unchanged.
- [ ] Full suite, fmt, clippy, build green.

## Resolution

<!-- filled in by validate-fix -->
