# e13s02 — Landing tree navigation and empty state

## 1. Story

As a visitor, landing-tree entries navigate into the existing tree and
blob pages, and an empty repository shows an onboarding state instead of a
blank list — so browsing starts at the landing page and never dead-ends.

## 2. Business value

Turns the e13s01 listing into working navigation. Directory entries link
deeper into e07's tree pages, file entries open e07's blob pages, and the
first-push experience gets an invitation instead of an empty section —
matching the scope's "browsing should start where the user arrives."

## 3. Type

feat

## 4. Risk / context

risk: P2 (BCP 2, link wiring and display-only state) — context: UI
(server-rendered page, no JS).

## 5. Context

Zoom-out on the modules this touches:
- **browse.rs purpose** — `list_tree`/`read_blob` plumbing (see e13s01
  zoom-out; contracts unchanged).
- **repo_page purpose** — landing page at `GET /{owner}/{name}`.
  **Callers** — lib.rs router. **Contracts** — axum handler; tests assert
  HTML substrings.
- Landing-tree entries gain hrefs, reusing e07's URL shapes exactly:
  directories → `/{owner}/{name}/tree/{trunk}/{path}`, files →
  `/{owner}/{name}/blob/{trunk}/{path}`. Names stay HTML-escaped.
- Empty state: when the trunk ref has no commits (or `list_tree` returns
  `None`), the Files section renders a short onboarding block — "no files
  yet — push your first commit" — instead of an empty list. No dead links
  ever (same discipline as the e05s04 metadata guard).
- Same e15 sequencing as e13s01: template-owned rendering once e15s01
  lands; handler/test shape unchanged.

## 6. Requirements

#### ADDED: Entries link to tree and blob pages
Directory entries link to the corresponding e07 tree route at the trunk
revision; file entries link to the corresponding blob route; link targets
are built only from validated entry names (escaped, no `..` possible from
`ls-tree` output).

#### ADDED: Empty repo shows onboarding state
A repo whose trunk has no commits renders an onboarding block in the Files
section (invite to push), with no entry list and no dead links.

## 7. Actors

Any visitor on public repos.

## 8. Preconditions

e13s01 (root tree renders), e07s01 (tree/blob routes exist), e15s01
(base layout template).

## 9. Postconditions

Every landing-tree entry navigates to a working page; a fresh repo's
landing page invites the first push.

## 10. Business rules

- Link shape parity with e07 routes — the landing page never invents a
  second URL scheme.
- Empty state renders only when the trunk has no commits; an existing
  tree never shows it.
- Escaping applies to names inside link text and hrefs.

## 11. Domain model touchpoints

**Repository** (trunk), read-only git tree access.

## 12. UI/UX notes

Links and one empty-state paragraph inside the e12 token system; no JS;
focus-visible styles come from the e12 stylesheet.

## 13. API contracts

None — link targets are the existing e07 routes.

## 14. Data model

None.

## 15. Non-functional requirements

Still one `list_tree` subprocess per render; link building adds no I/O.

## 16. Out of scope

Breadcrumb on the landing page, README rendering, hover previews, icons,
sorting, empty states for other sections, tree-page changes.

## 17. Acceptance criteria

- [ ] `cargo test -p server landing_links` — seeded nested repo: directory
      entry links to the tree route at trunk, file entry links to the blob
      route; link text and hrefs escaped. (e13s02t1)
- [ ] `cargo test -p server landing_empty` — repo with no commits: the
      landing page renders the onboarding block, lists no entries, and
      contains no tree/blob links. (e13s02t2)
- [ ] `cargo test --workspace` green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: entries navigate
  Given a public repo "o/r" with docs/guide.md at trunk
  When I GET "/o/r"
  Then "docs" links to /o/r/tree/trunk/docs
  And "guide.md" links to /o/r/blob/trunk/docs/guide.md

Scenario: empty repo invites a push
  Given a public repo "o/r" with no commits
  When I GET "/o/r"
  Then the page shows the onboarding state
  And the page contains no tree or blob links
```

## 19. Verification script (manual)

1. Open a seeded repo's landing page — click a directory, then a file;
   both land on the existing tree/blob pages.
2. Open an empty repo's landing page — onboarding block, no links.

## 20. Traceability

`// story: e13s02` in the landing-tree link/empty-state rendering and the
`landing_links` / `landing_empty` tests.
