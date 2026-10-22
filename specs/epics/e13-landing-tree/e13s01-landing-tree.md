# e13s01 — Root file tree on the landing page

## 1. Story

As a visitor to a repository, the landing page shows the file tree at the
default revision, so I see the repository's shape where I arrive instead
of clicking through to browse first.

## 2. Business value

Delivers the SCOPE success criterion: "The repo landing page shows the
file tree at the default revision." The scope states why: "Landing on
README-only hides the repository's shape; browsing should start where the
user arrives." Reuses e07's `browse::list_tree` wholesale — the v0.2.0
prior-art table records "browse list_tree/read_blob | extend | reused for
landing tree" — so the tree logic is built once.

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 3, presentation over shipped plumbing) — context: UI
(server-rendered page, no JS).

## 5. Context

Zoom-out on the modules this touches:
- **browse.rs purpose** — read-only git plumbing: `list_tree(bare,
  git_ref, path) -> Option<Vec<(String, bool)>>` (entry name, is_dir) and
  `read_blob`. **Callers** — tree/blob page handlers in `routes.rs`.
  **Contracts** — argv-only git subprocesses; `None` = unknown ref.
- **repo_page purpose** — the landing page at `GET /{owner}/{name}`
  (`routes.rs::repo_page`). **Callers** — lib.rs router. **Contracts** —
  axum handler; tests assert HTML substrings.
- The landing page gains a **Files** section rendered from
  `list_tree(bare, trunk, "")` — the repo's `trunk` column as the default
  revision, root path, non-recursive (depth 1). Entry names are HTML-
  escaped with the existing helper.
- Sequencing: this story renders inside the future Tera base layout —
  e15s01 owns templates and the tree lands after it (release order: e15
  before e13). Steps target the extracted template once it exists;
  handler and test shape are unchanged either way.

## 6. Requirements

#### ADDED: Root tree on the landing page
`GET /{owner}/{name}` lists the root entries of the repo's trunk —
directories and files, one level deep — from `browse::list_tree`, with
escaped names. Unknown/absent revision renders the section's empty state,
not an error.

#### ADDED: Default revision is the trunk column
The tree reads the repo row's `trunk` value; no new ref parameter.

## 7. Actors

Any visitor on public repos (same visibility as the repo page today).

## 8. Preconditions

e07s01 (`list_tree`/`read_blob` exist), e02s01 (bare storage, trunk
column), e15s01 (base layout template — tree renders inside it).

## 9. Postconditions

The landing page answers "what is in this repository" without a second
navigation.

## 10. Business rules

- Read-only: argv-only git, no writes, names escaped (XSS).
- One level deep on the landing page; deeper navigation belongs to the
  existing tree pages.
- Section honors the repo page's existing visibility rules.

## 11. Domain model touchpoints

**Repository** (trunk as default revision), read-only git tree access.

## 12. UI/UX notes

Plain list inside the e12 token system / e15 layout; no JS. Entry links
land in e13s02.

## 13. API contracts

Same route, richer HTML — no new endpoints.

## 14. Data model

None.

## 15. Non-functional requirements

One `list_tree` subprocess per landing render; root listing adds no
per-entry subprocess calls.

## 16. Out of scope

Entry links to tree/blob pages (e13s02), empty-repo onboarding state
(e13s02), README rendering, nested listings on the landing page, blob
contents, sorting beyond `list_tree` order, tree-page changes.

## 17. Acceptance criteria

- [x] `cargo test -p server landing_tree` — seeded repo: the landing page
      lists root entries (directories and files) at the trunk revision,
      with escaped names. (e13s01t1) — 3 passed.
- [x] `cargo test --workspace` green. — dispatch gates green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: landing page lists the root tree
  Given a public repo "o/r" with docs/ and README.md at trunk
  When I GET "/o/r"
  Then the page lists "docs" and "README.md"

Scenario: entry names are escaped
  Given a public repo with a file named "<script>.md" at trunk
  When I GET "/o/r"
  Then the response body contains no raw "<script>" tag
```

## 19. Verification script (manual)

1. Push a repo with a nested structure.
2. Open the landing page — root entries listed at the trunk revision.

## 20. Traceability

`// story: e13s01` in the landing-page tree block (or its template) and
the `landing_tree` test.
