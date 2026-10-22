# e08s02 — Conflict-aware blob and tree browsing

## 1. Story

As a reviewer, I open a conflicted file in jjhub and see the conflict sides
clearly marked, because browsing a repo that hosts conflicts must show the
conflict, not a silently wrong first side.

## 2. Business value

Completes the display half of "Repos whose commits carry unresolved jj
conflicts are stored and served like any other … and pages". The browse
pages from e07 currently show the plain-git view (first side minus
`.jjconflict*` entries), which reads as the file's content while hiding the
conflict — a trust bug in a forge.

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 3, presentation over shipped transport) — context: UI (git
plumbing reads, argv-only, HTML-escaped).

## 5. Context

Zoom-out on the modules this touches:
- **browse.rs purpose** — read-only git access for pages: `list_tree`
  (`git ls-tree -z`), `read_blob` (`git show`, NUL sniffing →
  `Blob::Text`/`Blob::Binary`), `validate_path`, `resolve_commit`.
  **Callers** — routes.rs `tree_page` (1260), `tree_page_at` (1268),
  `blob_page` (1378), `render_tree` (1280). **Contracts** — `Option`/`Result`
  returns mapped to 404; argv-only subprocess; content HTML-escaped at
  render.
- **routes.rs purpose** — every HTTP page handler; tests in
  crates/server/tests assert HTML substrings.

Conflict detection reads jj's git representation directly (Prior Art,
adopted): `jj:trees` header (`JJ_TREES_COMMIT_HEADER`, git_backend.rs:104)
via `extract_root_tree_from_commit`-style parsing (git_backend.rs:591 —
odd-count merge of tree ids), then the per-path side trees under
`.jjconflict-base-N` / `.jjconflict-side-N` (write path:
`write_tree_conflict`, git_backend.rs:1519) with the README entry
describing the labels. A blob page therefore does a three-side read (base +
side terms) for that path; a tree page lists the directory with the
`.jjconflict*` dirs grouped into one conflict entry instead of raw names.

Discovery (jj-lib 0.41.0): the `jj:conflict-labels` header
(`JJ_CONFLICT_LABELS_COMMIT_HEADER`, git_backend.rs:105) carries the
conflict labels when present — the page renders them as side headings.

## 6. Requirements

#### ADDED: Conflict-aware blob page
A blob whose tree at the given ref is a jj conflict renders the conflict:
base/side content with `<<<<<<<` / `=======` / `>>>>>>>` markers (labels
from `jj:conflict-labels` when present), fully HTML-escaped; never the
first side presented as content.

#### ADDED: Conflict-aware tree page
A tree containing `.jjconflict-base-N` / `.jjconflict-side-N` directories
renders them as a single "conflict" indicator row; the raw `.jjconflict*`
names are not linked as ordinary directories.

#### ADDED: Plain files unaffected
Commits without a `jj:trees` conflict header render exactly as today —
no behavior change for the e07 pages on clean repos.

## 7. Actors

Any visitor who can read the repository.

## 8. Preconditions

e07s01 (tree/blob pages), e08s01 (conflicted repo exists and serves).

## 9. Postconditions

A conflicted file and its directory are honestly displayed at any ref;
reviewers see the conflict instead of a plausible wrong file.

## 10. Business rules

- Read-only: argv-only git subprocess calls; `validate_path` rules
  unchanged (no `..`, no leading `/`, no NUL).
- Escape-then-render on every side's content (reflected data is code).
- Conflict rendering is per-path: only paths inside a conflict get
  markers; siblings render normally.
- Binary/conflict collision: binary side content shows the e07 binary
  note per side, no marker inflation.

## 11. Domain model touchpoints

**Repository**, read-only git object access, **Revision** (conflicted
revision displayed, not mutated).

## 12. UI/UX notes

Server-rendered inline HTML on the e07 page pattern. Conflict marker lines
styled distinctly (monospace, tinted background via existing Tailwind-ish
classes); tree page shows one conflict entry with a count of sides. No JS.

## 13. API contracts

HTML pages only — existing `/blob/{ref}/{*path}` and `/tree/{ref}/{*path}`
routes; no new endpoints.

## 14. Data model

None.

## 15. Non-functional requirements

Conflict parse cost is one header read + one extra `git show` per
conflicted path; non-conflicted paths keep the single-call path. Render
caps (1 MiB, e07) apply per side.

## 16. Out of scope

Conflict *editing* or resolution, side-by-side diff UI, per-hunk
navigation, conflict display on MR diff pages (e08s03 flags state only),
jj-side conflict metadata beyond the git representation.

## 17. Acceptance criteria

- [x] `cargo test -p server conflict_browse` — blob page on a conflicted
      path renders markers with all sides and escaped content; a
      non-conflicted path renders unchanged; binary sides show the note.
      (e08s02t1) — 4 passed.
- [x] `cargo test -p server conflict_tree` — tree page groups
      `.jjconflict*` dirs into one conflict entry, does not link them as
      directories, and lists ordinary entries unchanged. (e08s02t2) — 2 passed.
- [x] `cargo test --workspace` green. — dispatch gates green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: conflicted blob shows markers
  Given a public repo with a conflicted file at a ref
  When I GET the blob page for that path
  Then the body contains "<<<<<<<" and "=======" markers
  And every side's content is HTML-escaped

Scenario: tree page groups conflict dirs
  Given a public repo with a conflicted directory at a ref
  When I GET the tree page for its parent
  Then the ".jjconflict" directories render as one conflict entry
  And no ".jjconflict-side-0" directory link is present

Scenario: clean repo unaffected
  Given a public repo without conflicts
  When I GET its blob and tree pages
  Then the output matches the e07 behavior exactly
```

## 19. Verification script (manual)

1. Open the conflicted file's blob page on the fixture repo — markers and
   sides visible, no raw `<` from content.
2. Open the parent tree page — one conflict entry, no `.jjconflict*` links.
3. Open a clean repo's pages — identical to before.

## 20. Traceability

`// story: e08s02` in the conflict-read helpers, the tree/blob render
changes, and the `conflict_browse` / `conflict_tree` tests.
