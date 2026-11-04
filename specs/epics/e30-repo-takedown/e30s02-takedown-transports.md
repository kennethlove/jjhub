# e30s02 — taken-down repo refused on smart HTTP and SSH

## 1. Story

As a public-node operator, a taken-down repository refuses
git traffic too: smart-HTTP fetch and push are refused for
non-owners, SSH refuses before any git process spawns, while
the owner and operator keep cloning and pushing exactly as
before.

## 2. Business value

A takedown that only walls the HTML view is cosmetic — the
content still clones. e22 built one admission decision shared
by both transports (`token::authorize` / ssh mapping), so the
takedown predicate extends from the web seam of e30s01 into
transport admission at one point instead of a per-transport
checklist. SCOPE v0.6.0 in_scope e30, story 2 of 2.

## 3. Type

feat

## 4. Risk / context

risk: P0 — auth boundary at the transport entry. The check
composes with e22's existing gates (token validity, scope,
suspension) rather than replacing any: an unaffected repo's
admission is byte-for-byte today's behavior, proven by the
existing transport suites as regression net. Fail-closed to
the same refusals transports already map (HTTP 401/404,
SSH disconnect before spawn).

## 5. Steps

1. RED cases in `crates/server/tests/repo_takedown.rs`:
   taken-down repo's smart-HTTP info-refs and push refused for
   a normal account (404), owner and operator still pass; SSH
   push refused before git spawns (per
   `ssh_push_admission.rs` pattern); an untouched repo passes
   on both → verify: `cargo test -p server --
   test repo_takedown` (expected RED: transport admission
   unaware of takedown)
2. GREEN: takedown check where both transports resolve the
   repo — one predicate, both entries, owner/operator bypass →
   verify: `cargo test -p server --test repo_takedown`
3. Regression: e22 suites and shared admission stay green for
   unaffected repos → verify: `cargo test -p server --test
   token_restrictions && cargo test -p server --test
   ssh_push_admission && cargo test -p server --test
   git_transport`
4. Full gates → verify: `cargo fmt --all --check && cargo
   clippy --workspace --all-targets -- -D warnings && cargo
   test && cargo build`

## Verification Script

`cargo test -p server --test repo_takedown` proves both
transport refusals plus owner/operator bypass; the three
existing transport suites prove unaffected repos never
regressed.

## Out of scope

- Web seam (e30s01), review-proxy visibility (it proxies
  views behind auth, not git content), token scope changes.

## Risks

- Checking inside `token::authenticate` (where e29 put
  suspension) would refuse the account, not the repo — the
  wrong dimension; the suite asserts at repo resolution /
  authorize level so account-level gates stay untouched.
