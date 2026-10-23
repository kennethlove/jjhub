# e16s01 — Divergence detection over pushed revisions

## 1. Story

As a reviewer, I need jjhub to know when the revisions of a merge
request diverge (two heads carrying the same change, neither containing
the other) — so the surfaces built on top of e16 can show it.

## 2. Business value

Review identity is jjhub's core claim (ADR-0001). The MR already
anchors to a change-id; this story supplies the one predicate the
product has never had: "this change has diverged." Everything visible
in e16s02/e16s03 renders from it.

## 3. Type

feat

## 4. Risk / context

risk: P3 (BCP 2, read-only computation, no schema change) — context:
new helper over the existing git plumbing; detection is pure graph
arithmetic.

## 5. Context

Zoom-out on the modules this touches:
- **revisions table** — rows are pushed revisions of an MR
  (`SELECT commit_id FROM revisions WHERE mr_id = ...`, already read
  by `mr_detail_page`); every pushed revision of the change.
- **Divergence in jj's model** — same change-id, two heads where
  neither is an ancestor of the other. On the server this is a git
  ancestry question over a fixed set of commit ids.
- **Git ancestry plumbing** — the repo already shells git for
  everything (`crates/server/src/transport.rs`, browse, jj_meta);
  ancestry = `git merge-base --is-ancestor A B` over commit pairs.
  Env discipline matches existing helpers (GIT_CONFIG_GLOBAL/SYSTEM
  neutralized).
- **Where it lives** — a small read-only helper in the server crate
  (e.g. `divergence.rs` or next to the existing receive helpers),
  taking the bare repo path plus a set of commit ids and returning the
  diverging head pairs (or none). Render code in e16s02/e16s03 calls
  it; no handler changes in this story.
- **jj parity** — no jj-lib call is required: the pushed graph in
  bare git IS the source of truth the HTTP/SSH paths both admit
  through.

## 6. Requirements

#### ADDED: Detection helper
A helper detects divergence among a set of pushed commit ids for one
change: two or more heads where no head is an ancestor of another.
Ancestry errors (missing object, bad input) resolve as not-divergent —
same defensive posture as `is_ancestor` in receive_guard.

#### ADDED: Contract test fixtures
A seeded repo can hold two sibling commits carrying the same
change-id header (the state a history rewrite across clones produces)
so tests assert both the positive and negative cases.

## 7. Actors

Render paths (e16s02/e16s03) and their tests.

## 8. Preconditions

A public MR exists with at least two pushed revisions (the revisions
table is populated by pushes — e03/e10 paths unchanged).

## 9. Postconditions

One function answers "does this change diverge, and on which heads"
for any commit set; no store, no schema, no new subprocess pattern
beyond existing git calls.

## 10. Business rules

- Linear revision sets (every head an ancestor of the newest) are NOT
  divergent — normal re-push stacks stay clean.
- Single-revision MRs are never divergent.
- Detection is per change (per MR), never global across MRs.
- Ancestry failures fail safe: report not-divergent, never a 500.

## 11. Domain model touchpoints

**MergeRequest** (change_id anchor), **Revision** (pushed commit ids),
read-only.

## 12. UI/UX notes

None in this story — rendering lands in e16s02/e16s03.

## 13. API contracts

None — internal helper. (A future API surface can expose it; not
now.)

## 14. Data model

None.

## 15. Non-functional requirements

Bounded work: revision sets are small (capped by the existing page),
pairwise ancestry only; no background jobs.

## 16. Out of scope

Rendering any flag (e16s02/e16s03), push-time rejection or marking of
divergent pushes, operation-log events (audit surface was explicitly
not chosen at scope interview), cross-MR divergence.

## 17. Acceptance criteria

- [ ] `cargo test -p server divergence_detect` — two siblings with the
      same change-id: detection returns the diverging heads; adding
      their child (a merge) keeps ancestors-only sets non-divergent;
      a single revision is non-divergent; missing objects do not
      error. (e16s01t1)
- [ ] `cargo test --workspace` green. (e16s01t2)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: two siblings diverge
  Given a bare repo with two commits sharing one change-id header
    and neither commit is an ancestor of the other
  When I ask whether the change diverges
  Then the answer is divergent with both heads

Scenario: a linear stack does not diverge
  Given a bare repo with parent and child commits sharing a change-id
  When I ask whether the change diverges
  Then the answer is not divergent

Scenario: ancestry errors fail safe
  Given a commit id that does not exist in the repo
  When I ask whether the change diverges
  Then the answer is not divergent and no error surfaces
```

## 19. Verification script (manual)

None — helper-only story; tests are the proof.

## 20. Traceability

`// story: e16s01` in the detection helper and the
`divergence_detect` test.
