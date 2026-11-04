# e29s02 — suspended accounts refused on smart HTTP and SSH

## 1. Story

As a public-node operator, suspending an abuser kills their
transport access too: the token they already hold stops cloning
over smart HTTP and stops pushing over SSH, while every active
account's tokens behave exactly as before.

## 2. Business value

The token is today's only non-interactive credential and it does
not go through session or login. e22 built one admission decision
shared by both transports (`token::authenticate` →
`transport::authorize` / `ssh` mapping, transport.rs:535,
ssh.rs:245); suspension is one column checked inside
`token::authenticate` (token.rs:109), so both transports and the
review-proxy token path refuse from a single point instead of a
per-route checklist.

## 3. Type

feat

## 4. Risk / context

risk: P0 — auth boundary at the transport entry. The check is
fail-closed: a suspended account's token authenticates to `None`,
which HTTP already maps to 401 and SSH already maps to a
disconnect before any git process spawns. No changes to
`TransportAuth`, `transport_gate`, or `authorize` contracts; e22
scope/expiry/pinning behavior for active accounts is untouched and
its suites are the regression net.

## 5. Steps

1. RED cases in `crates/server/tests/operator_moderation.rs`: a
   suspended account's token returns `None` from
   `token::authenticate` (direct, per `token_restrictions.rs:135`
   pattern); its smart-HTTP clone and push are refused (401, per
   `git_transport.rs` pattern); its SSH push is refused before git
   spawns (per `ssh_push_admission.rs` pattern); an active
   account's token still authenticates on both transports →
   verify: `cargo test -p server --test operator_moderation`
   (expected RED: suspension not checked in authenticate)
2. GREEN: check `suspended_at` where `token::authenticate` loads
   the account — fail closed to `None` → verify: `cargo test -p
   server --test operator_moderation`
3. Regression: e22 suites and shared admission stay green for
   active accounts → verify: `cargo test -p server --test
   token_restrictions && cargo test -p server --test
   ssh_push_admission && cargo test -p server --test
   git_transport`
4. Full gates → verify: `cargo fmt --all --check && cargo clippy
   --workspace --all-targets -- -D warnings && cargo test && cargo
   build`

## Verification Script

`cargo test -p server --test operator_moderation` proves all three
refusals plus active-account passthrough; the three existing
transport suites prove nothing regressed.

## Out of scope

- Session/login refusal (e29s01), IP bans, token revocation by
  operators — suspension is one status, not credential deletion.

## Risks

- Putting the check in `authorize` instead of `authenticate` would
  leave token probing and review-proxy token paths alive — suite
  asserts at `authenticate` (the shared seam) first.
