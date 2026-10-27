# e25s03 — Per-token revoke

## 1. Story

As a signed-in user I click Revoke on a ledger row and that token
stops working immediately; the row leaves the ledger on reload.
Revoke never touches another user's token.

## 2. Business value

The design's replacement for blanket rotation ships: scoped
revocation with ownership enforced in the SQL predicate, not the
handler.

## 3. Type

feat

## 4. Risk / context

risk: P2 (auth surface) — context: `token.rs::revoke` today
updates by id alone; the route scopes with `user_id`. Design shows
an `<a class="revoke">`; the real route is POST, driven by htmx
`hx-post` on the anchor (native fallback: small POST form). Test
in `tokens.rs` pins the existing helper — extend, don't break.

## 5. Verification

`cargo test --workspace` (revoke tests: own token gone, other
user's id is a no-op, revoked token rejected by
`token::authenticate`).
