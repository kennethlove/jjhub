# e24s02 — Reset request: GET /reset, POST /reset

## 1. Story

As a visitor who forgot my password I follow "Forgot your
password?", enter my address, and get the same "Check your inbox"
answer whether or not the account exists. The reset link is logged
server-side (alpha delivery; mailer later).

## 2. Business value

Password recovery exists end to end without leaking account
existence: known and unknown addresses return byte-identical
responses, and the one-hour token is stored hashed. Login rate
limiting (Action::Login) also covers the new POST route.

## 3. Type

feat

## 4. Risk / context

risk: P2 (auth surface, anti-enumeration contract) — context:
migration `0010_reset_tokens.sql`, `token.rs` (generate/hash
reuse), `routes.rs` reset handlers + rate-limited `/reset` route,
`reset.html`/`reset_card.html`/`reset_sent{,_card}.html` shells,
reset link printed with `eprintln!` to server log.

## 5. Verification

`cargo test --workspace` (auth_reset_request, auth_htmx,
rate_limit_login); live proof: log line contains a `jjh_` token
URL and a `reset_tokens` row with `expires_at ≈ now + 1h`;
`ux_audit` + CDP UAT on the sent state.
