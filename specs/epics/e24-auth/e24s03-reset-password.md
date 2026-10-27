# e24s03 — Set a new password from a reset link

## 1. Story

As a visitor I open the reset link from my inbox and choose a new
password. The form renders only while the link is unexpired and
unused; mismatched confirmations keep the link alive, a successful
change burns it, and a replay is refused.

## 2. Business value

Completes recovery: the account's password changes through a
single-use, one-hour token without any account-enumeration
regression on the way in.

## 3. Type

feat

## 4. Risk / context

risk: P2 (credential change) — context: `token.rs`
`find_reset`/`consume_reset` (argon2 candidate verify, atomic
burn), `routes.rs` `reset_password_form`/`set_new_password`,
`password_reset.html`, `/reset-password` route (no rate layer —
192-bit token). Validation reuses `domain::MIN_PASSWORD_LEN` and
the pinned lowercase error strings.

## 5. Verification

`cargo test --workspace`; live loop: reset → GET form → POST
password → 303 to /login → old password 401 → new password 303 →
replay 400; CDP UAT at 1440 + 390 (zero overflow).
