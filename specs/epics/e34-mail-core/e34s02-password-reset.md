# e34s02 — password reset

## 1. Story

As a user who forgot a password, I request a reset with my
address and always receive the same neutral response; a
delivered link `/reset/<token>` works once inside one hour,
sets a new password, and retires the old one; a mail problem
never turns into a 500 or an enumeration oracle.

## 2. Business value

A public node without password reset strands accounts on the
first forgotten password; the neutral response and single-use
token keep the recovery path honest — SCOPE v0.7.0 in_scope e34
(reset single-use 1h, always-neutral, never 500).

## 3. Type

feat

## 4. Risk / context

risk: P1 — reset is the classic enumeration surface: the
response must be identical for known, unknown, and
already-used requests, and mail failure must not change it.
The token contract (single-use, 1h, stored hashed) is
`token::issue_email` / `token::verify` — reused, not reinvented.
The current reset flow (auth_reset_password suite) is asserted
against this contract rather than assumed.

## 5. Steps

1. RED cases in `crates/server/tests/reset_mail.rs`: request
   with a known address → identical neutral response and one
   recorded delivery whose link is path-form `/reset/<token>`;
   unknown address → same response, no delivery; token older
   than 1h refused; second use of a consumed token refused;
   after a successful reset the old password fails and the new
   one passes; LogMailer default keeps all of it in-box →
   verify: `cargo test -p server --test reset_mail` (expected
   RED: reset does not deliver through the mailer / links not
   path-form)
2. GREEN: reset request wired through the mailer with the
   neutral-always response, path-form link, 1h single-use token
   semantics, mail errors swallowed to the same response →
   verify: `cargo test -p server --test reset_mail`
3. Full gates → verify: `cargo fmt --all --check && cargo
   clippy --workspace --all-targets -- -D warnings && cargo
   test && cargo build`

## Verification Script

`cargo test -p server --test reset_mail` proves the response
never varies, the link delivers once and dies twice (expiry and
reuse), and the password actually rotates; full preflight proves
no regression.

## Out of scope

- Recovery codes, account-recovery appeals, CAPTCHA on the
  request form (e31 is signup-only this cycle), SMS or
  third-party recovery providers.

## Risks

- If the existing reset flow already differs from this contract,
  the RED run names the gap; the fix aligns to this story, not
  the other way round.
