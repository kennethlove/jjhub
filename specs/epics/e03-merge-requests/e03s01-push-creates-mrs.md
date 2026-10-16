# e03s01 — Push receive creates/updates MRs anchored to change-id

## 1. Story

As a jj user, every pushed change with a diff appears as an open merge request keyed to its change-id, so review identity survives every rewrite.

## 2. Business value

The core jj-native review model (ADR-0001). Everything downstream (stack pages, merge, kata links) reads MRs.

## 3. Type

feat

## 4. Risk / context

risk: P0 (auth boundary, core model) — context: domain + infra. Builds on the e05s02 post-receive hook site in `transport.rs`.

## 5. Context

After a successful receive-pack, the server walks the pushed refs, reads each new head commit's `change-id` commit header (jj ≥0.30) and diff vs its parent, then upserts: an open MR per (repo, change-id) with an immutable Revision row per pushed commit. Invariant 1 enforced by a partial unique index. Invariant 2 (author email == MR owner) is enforced pre-receive by a `pre-receive` hook installed at repo creation: for updates that rewrite an existing open change-id, the new commit's author email must match the MR owner's — else the push is rejected before any ref moves.

## 6. Requirements

#### ADDED: MR creation on push
Every pushed head commit carrying a `change-id` header with a non-empty diff opens (or reopens) an MR. Commits identical to an existing revision create no duplicates.

#### ADDED: Revisions are immutable
Each pushed commit for a change appends a Revision (unique per mr+commit-id). Revisions are never deleted or rewritten.

#### ADDED: Invariant 1 — one open MR per (repo, change-id)
Partial unique index on (repo_id, change_id) WHERE state='open'.

#### ADDED: Invariant 2 — author email guard
Pre-receive hook rejects a push whose new commit rewrites an open MR's change-id with a different author email (403-class rejection; ref untouched).

#### ADDED: Reopen
Push to an abandoned MR's change-id flips state back to open (e03s04 defines abandon fully; reopen mechanics live here).

## 7. Actors

Pusher (owner), future reviewers.

## 8. Preconditions

e02s01/e02s02 shipped (transport + authz). e05s02's post-push hook site exists.

## 9. Postconditions

After any push: open MRs match pushed changes; revisions record every commit; invariants 1 and 2 hold (tests).

## 10. Business rules

- change-id parsed from git commit header `change-id:` (jj format); commits without it are ignored (plain git pushes).
- Diff computed vs first parent; root/empty diffs are skipped.
- Hook rejections leave every ref untouched.

## 11. Domain model touchpoints

**Merge Request**: id, repo_id, change_id, state(open/merged/abandoned), owner_id, created_at. **Revision**: id, mr_id, commit_id, pushed_at.

## 12. UI/UX notes

None (e03s02 renders pages).

## 13. API contracts

None new; git transport unchanged externally.

## 14. Data model

merge_requests + revisions tables; partial unique index; FKs to repositories/users.

## 15. Non-functional requirements

Hook runs in <50ms typical (caches open MRs per repo). No shell interpolation of ref data (pkt-line parsed in-process or env+argv only).

## 16. Out of scope

Stack page rendering (e03s02), merge execution (e03s03), abandon UI (e03s04), comment threads (kata).

## 17. Acceptance criteria

- [ ] Push of a 2-change stack → 2 open MRs with distinct change-ids (test).
- [ ] Re-push same change-id with amended commit → 1 new revision on existing MR; MR stays open (test).
- [ ] Duplicate open MR for same (repo, change-id) impossible — partial index test.
- [ ] Pre-receive rejects author-email mismatch on open change-id; refs unchanged (test).
- [ ] Plain git push without change-id header creates no MRs (test).

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: stack push opens MRs
  Given a fresh repo
  When I push two stacked changes with change-ids
  Then two open MRs exist, one per change-id

Scenario: rewrite adds revision
  Given open MR for change kpqr with revision c1
  When I push change kpqr at commit c2 with the same author email
  Then the MR has revisions c1 and c2
```

## 19. Verification script (manual)

1. Start server, create repo, push a jj stack (e2e-clone style).
2. `psql -c "SELECT state, change_id FROM repositories JOIN merge_requests ..."` — one open row per change.
3. Amend + re-push; revision count grows by one.

## 20. Traceability

`// story: e03s01` in implementing code and tests.
