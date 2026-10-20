# e10s01 — Protected refs rejected at the push gate

## 1. Story

As a repository owner, a write that rewrites a protected ref is refused by
the server before storage, so history the server already accepted cannot be
silently replaced.

## 2. Business value

Delivers the SCOPE success criterion: "A write against a protected ref is
rejected server-side with a test proving the rejection." The v0.2.0 scope
states why this lives on the server: "Trust boundaries live at the server;
client-side politeness is not a guarantee for shared hosting." The v0.2.0
prior-art table names the insertion point — `receive_guard check_push`
(prior art: "extend | this repo receive_guard.rs:100 parses ref updates —
immutability insertion point").

## 3. Type

feat

## 4. Risk / context

risk: P0 (BCP 5; the push gate is the enforcement boundary for the whole
epic) — context: infra (pre-receive hook + guard).

## 5. Context

Zoom-out on the modules this touches:
- **receive_guard.rs purpose** — decisions that refuse a push before any
  ref moves. **Callers** — `main.rs::run_pre_receive` (the bare repo's
  pre-receive hook). **Contracts** — `check_push(pool, repo_id, pushed) ->
  Result<Vec<String>, sqlx::Error>`: returns human-readable rejection
  reasons; empty = allow; the hook exits 1 on any reason. Today it only
  rejects foreign-identity rewrites of open-MR changes (e03s01).
- **main.rs hook flow** — parses `<old> <new> <ref>` lines, resolves the
  repo row by canonicalized cwd, extracts commit identity per branch update
  with argv-only `git cat-file`, calls `check_push`, prints reasons to
  stderr, exits 1.

The immutability policy this story documents and enforces:

- **Content-addressed history.** Once the server records a protected ref
  pointing at a commit hash, that history must stay reachable. A protected
  ref may be created and advanced, never rewritten.
- **Protected set (config source).** Per repo: `refs/heads/{trunk}` (from
  `repositories.trunk`, default `trunk`) and every ref under `refs/tags/*`.
  One resolver function returns the set; e10s02 reuses it for display.
- **Rewrite definition.** A protected-ref update is a rewrite when `old`
  is not all-zeros and `new` does not contain `old` (non-fast-forward), or
  when `new` is all-zeros (deletion). Creation (old = zeros) and
  fast-forward advances pass.
- **Ancestry check.** Fast-forward-ness is computed in the hook with
  argv-only `git merge-base --is-ancestor <old> <new>` — same subprocess
  pattern and quarantine visibility as the existing `git cat-file` call.
- **Refusal shape.** Rejection = hook exits 1 with a reason naming the ref
  on stderr before any ref moves; no partial application.
- The hook classifies protection from ref names before commit-identity
  extraction, so tag updates and deletions (skipped today) reach the guard.
- Both guard rules run: a push may be refused by the protected-ref rule
  (this story) or the open-MR identity rule (e03s01).

Slopcheck: no external packages — std and the existing git subprocess
pattern only.

## 6. Requirements

#### MODIFIED: Push guard enforces protected-ref rewrites
**Before:** `check_push` only rejects foreign-identity rewrites of open-MR
changes (e03s01); branch rewrites and tag updates otherwise pass.
**After:** it additionally rejects any rewrite of a protected ref
(non-fast-forward advance or deletion) with a reason naming the ref.

#### ADDED: Protected-ref config source
One resolver returns the protected set for a repo — `refs/heads/{trunk}`
from the repo row plus `refs/tags/*` — used by the hook (this story) and
the settings listing (e10s02).

#### ADDED: Fast-forward allowance
Creation and fast-forward updates of protected refs pass; updates to
unprotected refs (change branches) keep exactly today's behavior.

## 7. Actors

Anyone who can push to the repository (owner or collaborator with a write
token).

## 8. Preconditions

e03s01 (pre-receive hook, `parse_ref_updates`, `check_push` exist),
e02s01 (bare storage, `repositories.trunk` column).

## 9. Postconditions

A refused push never moves a protected ref; the server is the enforcement
point, with the policy stated in one place.

## 10. Business rules

- argv-only git invocations; no shell interpolation of refs.
- Rejection reasons name the ref and the violation (pusher-readable).
- Protected set is derived (trunk + tags), not user-configurable in 0.2.0.
- The existing open-MR identity rule still applies to every pushed commit.

## 11. Domain model touchpoints

**Repository** (trunk, protection scope), ref updates (creation, advance,
rewrite, delete).

## 12. UI/UX notes

None — hook-level refusal over the git push protocol.

## 13. API contracts

None — refusals surface as git push rejection messages on stderr.

## 14. Data model

None — reads `repositories.trunk`; no new tables (e10s03 adds its own).

## 15. Non-functional requirements

At most one extra git subprocess per protected update; no new dependencies;
accepted pushes stay on the current hot path.

## 16. Out of scope

Settings listing (e10s02), audit recording (e10s03), per-ref user
configuration, GC of unreachable objects, ancestry policy for unprotected
refs.

## 17. Acceptance criteria

- [ ] `cargo test -p server protect_config` — resolver returns
      `refs/heads/{trunk}` + `refs/tags/*` for a seeded repo and honors a
      custom trunk. (e10s01t1)
- [ ] `cargo test -p server protect_reject` — non-fast-forward update to a
      protected ref and deletion of a protected ref each produce a
      rejection naming the ref. (e10s01t2)
- [ ] `cargo test -p server protect_allow` — creation and fast-forward of a
      protected ref pass; updates to unprotected change branches produce no
      protected-ref rejection. (e10s01t3)
- [ ] `cargo test --workspace` green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: rewrite of a protected ref is rejected
  Given a public repo "o/r" with trunk at commit A
  When I push an update moving trunk to a commit that does not contain A
  Then the push is refused with a message naming refs/heads/trunk

Scenario: fast-forward of a protected ref passes
  Given a public repo "o/r" with trunk at commit A
  When I push an update moving trunk to a descendant of A
  Then no protected-ref rejection is produced

Scenario: unprotected refs are unaffected
  Given a public repo "o/r"
  When I push a new change branch
  Then no protected-ref rejection is produced
```

## 19. Verification script (manual)

1. `cargo run` with a seeded repo.
2. Clone, move trunk to a sibling commit, `git push --force` — refused,
   reason names the ref, `git ls-remote` still shows the old head.
3. Push a new change branch — accepted.

## 20. Traceability

`// story: e10s01` in the protected-ref check in `receive_guard`, the
hook's protection classification, and the `protect_config` /
`protect_reject` / `protect_allow` tests.
