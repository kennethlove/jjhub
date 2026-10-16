# e03s03 — Merge: squash/merge-commit + cascade rebase of descendants

## 1. Story

As an owner, merging the bottom MR squashes it into trunk and the server rebases the rest of the stack, so merging is one click with no author-side fixup.

## 2. Business value

The stack merge is the jjhub headline behavior (spike/ADR model): merge bottom, server rebases descendants (invariants 4 and 7).

## 3. Type

feat

## 4. Risk / context

risk: P0 (history rewriting on trunk) — context: domain + infra.

## 5. Context

Merge button on MR detail (owner only). Executor: (1) squash or merge-commit the bottom change onto trunk per repo setting — forward-only (invariant 4); (2) mark MR merged; (3) in stack order, rebase each descendant MR's latest revision onto new trunk: create a new commit (same change-id header, same tree/author, new parent), append as new Revision on the descendant MR, push into bare repo. Ref updates done via a git update-ref transaction so partial failure can't corrupt (whole operation in pre-checked phases: verify clean, build commits, single ref transaction).

## 6. Requirements

#### ADDED: Merge execution
Owner-only POST /{owner}/{name}/merge-requests/{change_id}/merge. Bottom MR only (parent reachable from trunk); descendant MRs may not merge first (invariant 7 — test).

#### ADDED: Repo merge setting honored
squash (default) → single parent on trunk, change description as message; merge_commit → `Merge <description>` with two parents.

#### ADDED: Cascade rebase
After trunk advances, each descendant gets a rebased revision appended; MR stays open; newest revision commit contains change-id header.

#### ADDED: Forward-only trunk (invariant 4)
Merge fails cleanly if remote trunk moved concurrently (re-check inside ref transaction; 409 on mismatch).

## 7. Actors

Owner (merge), reviewer (observes).

## 8. Preconditions

e03s01, e03s02 shipped. Repo merge_setting column exists (e02s01).

## 9. Postconditions

Trunk advanced by exactly one merge; descendants rebased server-side; MR state = merged for bottom.

## 10. Business rules

- Merged MR cannot reopen by push (its change-id now on trunk; e03s01 reopen only for abandoned — encode: merged is terminal).
- Cascade stops at first rebase failure; remaining MRs untouched (test with injected failure).
- Merge operations serialized per repo (in-process mutex per repo id).

## 11. Domain model touchpoints

**Merge Request.state** (merged terminal), **Revision** append-only, **Trunk**, repo **merge_setting**.

## 12. UI/UX notes

Merge button on detail page with repo-setting label (squash/merge-commit); disabled for non-bottom MRs with tooltip "merge bottom first".

## 13. API contracts

POST merge endpoint (form), redirect back to detail page.

## 14. Data model

No new columns; revisions appended; optional merge_commit_sha on MR.

## 15. Non-functional requirements

Merge p95 <2s for stacks ≤10. Never leaves trunk mid-transaction (single ref update).

## 16. Out of scope

CI gating before merge (statuses exist but no policy), revert, squash-and-ff options, interactive rebase.

## 17. Acceptance criteria

- [ ] Bottom squash merge: trunk tree = change content, one new trunk commit (test via git log).
- [ ] merge_commit setting produces two-parent merge (test).
- [ ] Descendant MRs gain rebased revisions; change-ids preserved (test).
- [ ] Non-owner merge → 403; merging non-bottom first → 409 (tests).
- [ ] Mid-cascade failure leaves trunk advanced and untouched descendants — no partial refs (test with fault injection).

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: cascade merge
  Given stack A -> B -> C, all open, A targets trunk
  When the owner merges A
  Then trunk contains A's content
  And B and C each have a new rebased revision
  And merging B before its turn is rejected
```

## 19. Verification script (manual)

1. Push 3-stack, merge bottom via UI.
2. `git -C bare log trunk` — squash commit present; `jj log` in meta dir shows rebased descendants.
3. MR pages: A merged, B/C open with fresh revisions.

## 20. Traceability

`// story: e03s03` in implementing code and tests.
