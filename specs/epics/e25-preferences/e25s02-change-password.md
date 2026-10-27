# e25s02 — Change password

## 1. Story

As a signed-in user I follow "Change password" (the `#password`
state of the Account panel), enter current + new + confirm, and
get either the resting Account state or an inline error: wrong
current password, mismatched confirmations, or a short new
password.

## 2. Business value

The designed credential-change flow exists: `/settings/password`
verifies the current password with argon2, enforces the shared
10-character floor (`domain::MIN_PASSWORD_LEN`), and stores a new
argon2 hash.

## 3. Type

feat

## 4. Risk / context

risk: P2 (credential change) — context: `routes.rs`
`change_password` handler (Form last extractor), reuses
`password::verify_password`/`hash_password`; htmx form swaps the
`#password` state fragment; native POST path redirects to
`/settings#password` with a short-lived error cookie (the
`new_token` cookie pattern) so `:target` shows the state.

## 5. Verification

`cargo test --workspace` (new settings_password suite + login
still passes with the old password until changed).
