# e03s02 — Stack derivation + MR pages (list, detail, revision diffs)

## 1. Story

As a reviewer, I see the repo's open MRs as a stack, open one, and read its revision diffs — so I review the change, not the branch.

## 2. Business value

First user-visible surface of the review model. Pages read MRs from e03s01; stack structure comes from commit parents (invariant 3).

## 3. Type

feat

## 4. Risk / context

risk: P1 — context: domain + UI (Tera/htmx).

## 5. Context

Repo page gains an MR list grouped into stacks: bottom MR targets the repo's trunk bookmark, each descendant targets its parent MR — derived by walking commit parents at render time (never stored). MR detail page: change-id, state, revision list (newest first), and per-revision file diffs computed server-side from the bare repo objects (git diff-tree via subprocess, argv-only). Link to kata review lands in e05s04.

## 6. Requirements

#### ADDED: Stack derivation (invariant 3)
Parentage derived from commit parents of latest revisions; trunk-targeting bottom = MR whose parent commit is reachable from trunk. Never persisted.

#### ADDED: MR list page
`GET /{owner}/{name}/merge-requests` shows open MRs grouped in stack order (bottom first).

#### ADDED: MR detail + revisions
`GET /{owner}/{name}/merge-requests/{change_id}` shows state, stack position, revision list.

#### ADDED: Revision diffs
Detail page renders per-revision file diffs (unified text diff, binary marked) computed from repo objects.

## 7. Actors

Reviewer (any logged-in user on public repos; per invariant 6).

## 8. Preconditions

e03s01 shipped. Trunk bookmark exists (default `trunk`).

## 9. Postconditions

A pushed stack renders as an ordered chain; every revision's diff viewable.

## 10. Business rules

- Change-id in URLs is the jj change-id string (letters only); validated before use.
- Diff output escaped in templates (no raw HTML from repo content).
- Empty-diff revisions still listed, marked "no changes".

## 11. Domain model touchpoints

**Merge Request**, **Revision**, **Stack** (tech-stack.md), **Trunk**.

## 12. UI/UX notes

Tera templates + htmx for revision expansion; Tailwind. Keep pages server-rendered, no JS framework.

## 13. API contracts

HTML pages only (routes above).

## 14. Data model

Read-only over merge_requests/revisions + git objects. No schema change.

## 15. Non-functional requirements

List page <200ms for 100 MRs. Diff generation cached per commit-id (on-disk cache dir OK).

## 16. Out of scope

Merge buttons (e03s03), abandon UI (e03s04), kata links (e05s04), side-by-side diff view.

## 17. Acceptance criteria

- [ ] Pushed 3-stack renders bottom→top with parent links (test, HTML assertions).
- [ ] MR detail lists revisions newest-first with change-id and commit id (test).
- [ ] Diff view shows added/removed lines for a revision; repo content is HTML-escaped (XSS test).
- [ ] Unknown change-id → 404.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: stack renders in order
  Given pushed stack A -> B -> C (A targets trunk)
  When I open /{owner}/{repo}/merge-requests
  Then MRs show in order A, B, C with parent links
```

## 19. Verification script (manual)

1. Push 3-change stack; open /{owner}/{repo}/merge-requests — three cards, ordered.
2. Open bottom MR — revision list, diff of pushed file visible.

## 20. Traceability

`// story: e03s02` in implementing code and tests.
