# e30s01 — operator takes a repo down; visitor refused; restore works

## 1. Story

As a public-node operator, I take a repository down from the
web surface: anonymous visitors and normal accounts are refused
on every browse route, while the owner and the operator still
see it. Restore returns it for everyone. Both flips are one
moderation_actions row each.

## 2. Business value

Suspension cuts the person; takedown cuts the content. A public
node needs a reversible lever for doxxed or illegal material
where suspending the account is either too late or too blunt —
someone else may depend on the repo. History is preserved
untouched: reversible visibility, never deletion. SCOPE v0.6.0
in_scope e30, story 1 of 2 (web seam; transports follow in
e30s02).

## 3. Type

feat

## 4. Risk / context

risk: P0 — gates public content. The wall lives at the shared
visibility seam (`revsets::visible_repo` / repo resolution
paths), not per route, so the owner/operator bypass is one
named predicate instead of a wall per handler. The takedown
column is a timestamp (who and when derivable from
moderation_actions); rows, refs, and history are never
modified. Non-operators get 404 on the controls — the same
treatment as every operator surface e29 shipped.

## 5. Steps

1. RED cases in `crates/server/tests/repo_takedown.rs`:
   after the operator action, anonymous GET on the repo root
   and a normal account's GET are refused (404), owner and
   operator still render; a moderation_actions row exists for
   takedown and another for restore; restore puts the visitor
   back; non-operator POST to the controls → 404, no row →
   verify: `cargo test -p server --test repo_takedown`
   (expected RED: no column, no routes)
2. GREEN: migration adds the takedown timestamp to
   repositories; operator controls (takedown/restore forms
   with hidden CSRF) on the repo's admin-capable surface or
   /admin, behind session + CSRF + operator gate; visibility
   seam refuses taken-down repos for everyone except owner and
   operator; each flip inserts a moderation_actions row →
   verify: `cargo test -p server --test repo_takedown`
3. Owner notice placeholder is NOT built here (mail ships in
   v0.7.0) — only the audit row → verify: same suite
4. Full gates → verify: `cargo fmt --all --check && cargo
   clippy --workspace --all-targets -- -D warnings && cargo
   test && cargo build`

## Verification Script

`cargo test -p server --test repo_takedown` proves both
refusals, both bypasses, both audit rows, restore, and the
404; ux_audit + CDP probe cover the controls at 1440 and 390.

## Out of scope

- Transport refusal (e30s02), deletion or history rewriting,
  owner notification mail (v0.7.0), hiding repositories from
  their owners.

## Risks

- Checking only the repo page route would leave git endpoints
  and any other resolver-based surface leaking — suite asserts
  the shared seam first; e30s02 extends the same predicate to
  transport admission.
