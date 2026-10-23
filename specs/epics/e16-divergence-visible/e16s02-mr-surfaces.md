# e16s02 — MR surfaces show divergent siblings

## 1. Story

As a reviewer, when the change I'm reviewing has diverged, the merge
request detail page shows both siblings and says so, and the MR list
marks the MR — so I never review one head of a change while another
exists unnoticed.

## 2. Business value

Turns e16s01's predicate into honesty on the review surface. The MR
detail is where a reviewer decides; showing both heads with their
ancestry relation (which one contains the reviewed threads' context,
which is newer) keeps the change-id anchor truthful after a history
rewrite across clones.

## 3. Type

feat

## 4. Risk / context

risk: P2 (BCP 3, rendering + one context key, design tokens only) —
context: Tera templates, no JS, e12 tokens.

## 5. Context

Zoom-out on the modules this touches:
- **`mr_detail_page` (routes.rs)** — builds `sections`, `badges`,
  `revisions` context; this story adds one computed key
  (`divergent` + which heads) via e16s01's helper over the MR's
  revision commit ids.
- **`mr_detail.html` / `mr_revision.html` (Tera)** — flag placement:
  a divergence notice above the revision list (e12 token styling,
  same pattern as `.conflict-flag`), sibling heads listed with short
  ids linking to their own revision sections.
- **MR list rendering (`repo_page` context `mrs` + its template)** —
  a mark next to the MR title when divergent; detection runs per
  listed MR over its newest-pair of revisions (bounded: list caps at
  the existing page size).
- **Callers/contracts** — axum handler, tests assert HTML substrings;
  no handler signature changes, no JS.

## 6. Requirements

#### ADDED: Divergence notice on MR detail
A divergent MR renders a notice above its revisions naming both heads
(short ids) and linking each to its revision section; a non-divergent
MR renders no notice.

#### ADDED: Divergence mark in MR list
The MR list marks divergent MRs with the same design token treatment
(one class, e12 tokens); marks never appear for ordinary MRs.

## 7. Actors

Reviewers on public repos (viewer paths unchanged for private).

## 8. Preconditions

e16s01 (helper), e16's seed fixture (two siblings per change),
existing MR detail/list pages.

## 9. Postconditions

Divergence is visible wherever a reviewer chooses an MR — detail page
states it, list pre-warns; zero change to push/merge behavior.

## 10. Business rules

- The notice shows BOTH heads; never prefers one silently.
- Short-id display matches existing commit-id rendering conventions.
- Non-divergent pages are byte-identical to before this story (no
  extra badge classes, no extra context keys rendering output).

## 11. Domain model touchpoints

**MergeRequest** (change anchor), **Revision** (heads), read-only
render.

## 12. UI/UX notes

One notice class + one list mark, both e12 tokens (reuse the
`.conflict-flag` pattern's token discipline, distinct class — this is
not conflict state); role="status" for the notice; ux audit before
handoff.

## 13. API contracts

None — server-rendered pages.

## 14. Data model

None.

## 15. Non-functional requirements

List-page detection bounded to the listed MRs' revision sets; no
N+1 storm — one ancestry helper call per listed MR at most.

## 16. Out of scope

Repo-page surface (e16s03), API exposure, push-time behavior, audit
events, auto-resolution or divergence management actions.

## 17. Acceptance criteria

- [ ] `cargo test -p server divergence_mr_detail` — divergent seed MR:
      detail page contains the notice, both head short-ids, and links
      to both revision sections; same MR's non-divergent sibling
      fixture shows no notice. (e16s02t1)
- [ ] `cargo test -p server divergence_mr_list` — repo MR list marks
      the divergent MR only; ordinary MR lists are unchanged.
      (e16s02t2)
- [ ] `cargo test --workspace` green; ux audit passes. (e16s02t3)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: divergent MR says so
  Given a public repo whose MR has two sibling revisions
  When I GET the MR detail page
  Then a divergence notice names both heads
  And each head links to its revision section

Scenario: ordinary MR stays quiet
  Given a public repo whose MR revisions are linear
  When I GET the MR detail page
  Then no divergence notice renders
```

## 19. Verification script (manual)

1. Seed a divergent MR — open detail: notice above revisions, both
   short ids link correctly; list shows the mark.
2. Open any ordinary MR — no notice, no mark.

## 20. Traceability

`// story: e16s02` in the MR detail/list divergence rendering and the
`divergence_mr_detail` / `divergence_mr_list` tests.
