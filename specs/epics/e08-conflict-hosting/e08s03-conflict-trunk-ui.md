# e08s03 — Conflict state in trunk resolution and MR views

## 1. Story

As a repository owner, I see which commits on my landing and merge-request
pages carry unresolved conflicts, so trunk resolution degrades into a clear
state instead of an error page.

## 2. Business value

Finishes the SCOPE_LATEST conflict line: "conflict state stops being a
rejection case for … pages." jj_meta currently hard-fails when the trunk
root tree is a conflict (`jj_meta.rs:138` — `trunk root tree is a
conflict`), and nothing on the UI flags a conflicted commit — so a hosted
conflicted repo still breaks where users look.

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 3, presentation + one shared read path) — context: UI +
domain (jj_meta trunk resolution used by every page).

## 5. Context

Zoom-out on the modules this touches:
- **jj_meta.rs purpose** — the only jj-lib access point (settings,
  `Workspace::load`, `spawn_blocking`); `load_trunk_file_names`
  (jj_meta.rs:97) resolves the trunk bookmark to root tree names.
  **Callers** — repo pages, tree root, MR pages, kata sidecar.
  **Contracts** — async fns returning `Result<T, String>`; already skips
  conflicted *bookmarks* (`target.as_normal()` guard, jj_meta.rs:116).
- **routes.rs purpose** — HTTP page handlers (`repo_page` 398,
  `mr_detail_page` 648). **Tests** — crates/server/tests assert HTML
  substrings.
- **merge.rs purpose** — MR landing checks (`merge_bottom` merge.rs:103,
  trunk ref `refs/heads/{trunk}` merge.rs:153, `merge-base --is-ancestor`
  merge.rs:162).

Conflict state detection reuses e08s02's read (Prior Art, adopted): the
`jj:trees` header marks a conflicted commit (git_backend.rs:104), so both
the trunk helper and the pages derive the flag from one helper instead of
re-parsing twice. When the trunk root tree *is* a conflict, trunk
resolution returns a defined "trunk conflicted" state that pages render —
not a `Result` error bubcling into a 500.

## 6. Requirements

#### ADDED: Graceful conflicted-trunk resolution
`load_trunk_file_names` on a trunk whose root tree is a conflict returns a
defined state (trunk unavailable, conflict flagged) instead of the
`trunk root tree is a conflict` error; callers render their existing empty
state plus the flag. Conflicted trunk bookmarks stay skipped
(jj_meta.rs:116 behavior unchanged).

#### ADDED: Conflicted-commit flag helper
One helper reports whether a revision's commit carries jj's conflict
header; shared by the trunk path and the UI so the flag means one thing.

#### ADDED: Landing page flags conflicted commits
The repo landing page marks a conflicted commit (badge/note naming the
conflict) wherever the commit is listed.

#### ADDED: MR pages flag conflicted commits
MR detail (and the landing view inside it) marks conflicted revisions with
the same flag; no page errors on a repo containing conflicted commits.

## 7. Actors

Repo owner, collaborators, and any visitor who can read the pages.

## 8. Preconditions

e06s01 (site shell), e07s01 (pages exist), e08s01 (conflicted repos
hosted), e08s02 (conflict read helper).

## 9. Postconditions

A repo with conflicted commits renders every page — with visible conflict
state — and trunk resolution never turns a conflict into a server error.

## 10. Business rules

- Conflict flag = jj's git representation (`jj:trees` header), same
  source as e08s02 — one definition, no second parser.
- Conflicted bookmarks remain excluded from trunk resolution.
- Flag is presentation only: MR merge/landing checks (merge.rs:162)
  behavior is unchanged in this story; resolving conflicts stays out of
  scope.
- Existing authz and HTML escaping rules apply to all new markup.

## 11. Domain model touchpoints

**Repository** (trunk), **Revision**/**Change** (conflict flag on
commits), **Merge Request** (flagged revisions).

## 12. UI/UX notes

Server-rendered inline HTML on existing patterns: a conflict badge next to
the flagged commit; when trunk is conflicted, the landing page shows its
empty state plus the flag — never a 500 or a blank table. No JS.

## 13. API contracts

HTML pages only; no endpoint changes. `load_trunk_file_names` keeps its
signature (defined-state return), so callers handle the new state without
a contract break.

## 14. Data model

None — flag derived from commit headers; no tables, no migrations.

## 15. Non-functional requirements

Flag lookup rides an already-fetched commit — no extra subprocess per
row; trunk resolution cost unchanged for non-conflicted repos.

## 16. Out of scope

Conflict resolution, editing, or merge execution on conflicted revisions;
conflict markers in MR *diff* views; changes to `merge-base` landing
checks; trunk auto-retargeting.

## 17. Acceptance criteria

- [ ] `cargo test -p server conflict_meta` — conflicted trunk root tree
      returns the defined state (no error string, no 500); conflicted
      trunk bookmarks still skipped; clean repos resolve trunk exactly as
      before. (e08s03t1)
- [ ] `cargo test -p server conflict_ui` — landing page and MR detail
      render the conflict flag on the conflicted commit; a repo with
      conflicts renders all pages without error. (e08s03t2)
- [ ] `cargo test --workspace` green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: conflicted trunk degrades gracefully
  Given a repo whose trunk bookmark points at a commit with a conflicted root tree
  When I GET the landing page
  Then I get 200 with the empty state and a conflict flag
  And no "trunk root tree is a conflict" error is surfaced

Scenario: conflicted commit flagged on landing page
  Given a public repo with a conflicted commit
  When I GET the repo page
  Then the conflicted commit carries the conflict badge

Scenario: MR detail flags conflicted revisions
  Given a public MR whose commits include a conflicted one
  When I GET the MR detail page
  Then the conflicted revision is marked
  And the page renders without error

Scenario: clean repo unchanged
  Given a repo without conflicts
  When I GET its landing and MR pages
  Then no conflict badge appears
```

## 19. Verification script (manual)

1. Host the conflicted fixture repo; open the landing page — flag visible,
   no 500.
2. Point trunk at the conflicted commit — landing shows empty state plus
   flag.
3. Open an MR containing the conflicted commit — badge on the revision.
4. Clean repo — no badges anywhere.

## 20. Traceability

`// story: e08s03` in the trunk-resolution change, the conflict-flag
helper, the landing/MR markup, and the `conflict_meta` / `conflict_ui`
tests.
