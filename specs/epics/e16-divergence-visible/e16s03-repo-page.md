# e16s03 — Repo page marks divergent merge requests

## 1. Story

As a repository visitor, I see which open merge requests have diverged
directly on the repository page — so divergence is visible before I
even open a review.

## 2. Business value

Completes the scope's "MR + repo surfaces" choice: the repo page's
Open MRs section is the browse surface; a divergent change waiting
there gets seen without drilling into each MR detail.

## 3. Type

feat

## 4. Risk / context

risk: P3 (BCP 3, one context computation + one mark in an existing
section) — context: landing page, Tera, e12 tokens.

## 5. Context

Zoom-out on the modules this touches:
- **`repo_page` (routes.rs)** — already queries open MRs (`mrs`
  context, SELECT with reviewer counts) and hosts e13's Files section
  and e10s02's protected-refs block; this story computes divergence
  per listed MR (bounded by that query's cap) via e16s01's helper and
  adds a flag to the `mrs` context entries.
- **`repo.html` Open MRs section** — marks divergent MRs with the
  same class/notice pattern e16s02 defines (one shared class across
  surfaces — divergence is one concept, one treatment).
- **Precedent** — same shape as the landing page additions from e13
  (entry flags via context) and e08s03 (flag rendered from one
  computed boolean); no JS, no new routes.

## 6. Requirements

#### ADDED: Divergence mark on the repo page MR surface
Open MRs whose revisions diverge carry the shared divergence mark on
the repository page; linear MRs render as before.

## 7. Actors

Any visitor on public repos.

## 8. Preconditions

e16s01 (helper), e16s02 (shared mark definition), e13s01 (Open MRs
section rendering).

## 9. Postconditions

All three scope surfaces — MR detail, MR list, repo page — agree on
divergence, from one predicate and one visual treatment.

## 10. Business rules

- Mark appears only for divergent MRs; linear MRs byte-unchanged.
- Same class string as e16s02 (single source, no surface-local
  variants).

## 11. Domain model touchpoints

**Repository** landing view, **MergeRequest** read-only.

## 12. UI/UX notes

Reuses e16s02's token-styled mark; no new tokens; ux audit before
handoff.

## 13. API contracts

None.

## 14. Data model

None.

## 15. Non-functional requirements

Divergence computed only for open MRs already fetched for the page —
no extra queries beyond the ancestry helper calls (bounded by the
section cap).

## 16. Out of scope

Closed/abandoned MR marks (open surface only), other repo-page
changes, notifications, badges beyond the mark itself.

## 17. Acceptance criteria

- [ ] `cargo test -p server divergence_repo_page` — repo with one
      divergent + one linear open MR: page marks only the divergent
      one; same mark class as the MR list. (e16s03t1)
- [ ] `cargo test --workspace` green; ux audit passes. (e16s03t2)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: repo page marks the divergent MR
  Given a public repo with a divergent open MR and a linear open MR
  When I GET the repository page
  Then the divergent MR carries the divergence mark
  And the linear MR carries no mark

Scenario: repo without divergence stays quiet
  Given a public repo with only linear open MRs
  When I GET the repository page
  Then no divergence mark renders anywhere
```

## 19. Verification script (manual)

1. Open a seeded repo page — divergent MR marked, linear MR clean;
   the mark matches the MR list's.

## 20. Traceability

`// story: e16s03` in the repo-page divergence rendering and the
`divergence_repo_page` test.
