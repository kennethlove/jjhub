# e11s02 — Search UI scoped to the repository

## 1. Story

As a visitor to a repository page, I type a revset in a search box and see
the matching revisions on a results page, so history search is part of the
site, not just an API.

## 2. Business value

Delivers the "and UI" half of the SCOPE_LATEST acceptance: "a revset
expression returns matching revisions through API and UI." The endpoint
from e11s01 becomes reachable by anyone who can read the repo, with a
default query and a friendly empty state instead of a blank JSON blob.

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 3, presentation over shipped API) — context: UI
(server-rendered pages, no JS framework).

## 5. Context

Zoom-out on the modules this touches:
- **routes.rs purpose** — every HTTP page handler. **Callers** — lib.rs
  router. **Contracts** — axum handler signatures; tests assert HTML
  substrings.
- New page `GET /{owner}/{name}/search?q=<revset>`: a search box prefilled
  with `q` (default `open()`) plus a results list rendered server-side
  from the e11s01 endpoint's query function (same process — call the
  jj_meta query helper directly, not over HTTP). Each result row links the
  change-id to the MR page when an open MR exists, and shows the short
  commit id and a one-line description. Empty result set renders a
  dedicated empty state suggesting a broader query, not a bare table.
- The repo detail page gains a **Search** link next to Files; the search
  page reuses the repo-detail chrome (nav, repo header).

## 6. Requirements

#### ADDED: Search page with query box
`GET /{owner}/{name}/search` renders a search box (GET form, `q` param)
and a results list; every result links to its MR detail page when one is
open for the change.

#### ADDED: Default query
Absent or empty `q` runs `open()` — a first paint that shows something
useful on any repo with open changes.

#### ADDED: Empty state
A valid query with zero matches renders an empty-state block naming the
query and suggesting a broader one; never a bare table or raw JSON.

#### ADDED: Repo-page entry point
The repo detail page links Search whenever the viewer can read the repo.

## 7. Actors

Any user who can read the repository.

## 8. Preconditions

e11s01 (query function + endpoint), e06s01 (repo chrome/nav), e07 (Files
link pattern).

## 9. Postconditions

A reader opens any repo, types a revset, and sees matching revisions as
pages — no curl, no client-side jj.

## 10. Business rules

- Same authz as e11s01: private repos invisible to unauthorized viewers.
- The page reuses the e11s01 query path — one query implementation, no
  duplicated parser wiring.
- Query text is HTML-escaped on render (reflected-input XSS).
- The 50-result cap from e11s01 applies here too; page states the cap
  when it truncates.

## 11. Domain model touchpoints

**Repository** (scope), **Change**/**Revision** (result rows), **Merge
Request** (result links when open).

## 12. UI/UX notes

Server-rendered page on the repo-detail pattern (Tera if e15 has landed,
inline HTML otherwise). GET form — the query is shareable as a URL.
Results: one row per commit — change-id, short commit id, description —
escape everything from the query and the data.

## 13. API contracts

`GET /{owner}/{name}/search?q=<revset>` — HTML page (200); unknown repo
or unauthorized private repo → 404 (repo-detail behavior). Consumes the
e11s01 query helper; adds no new JSON surface.

## 14. Data model

None — reads through the e11s01 path.

## 15. Non-functional requirements

One server round-trip per search; no client-side JS required for the flow;
escape-then-render on every reflected value.

## 16. Out of scope

Cross-repo / global search, search filters beyond the raw revset, query
autocomplete, recent-queries history, pagination beyond the fixed cap.

## 17. Acceptance criteria

- [ ] `cargo test -p server revset_ui` — search page renders with query
      box and result rows; results link to open MRs; query text is
      escaped. (e11s02t1)
- [ ] `cargo test -p server revset_default` — empty `q` runs `open()`;
      zero matches render the empty state. (e11s02t2)
- [ ] `cargo test --workspace` green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: search returns rendered results
  Given a public repo "o/r" with open changes
  When I GET "/o/r/search?q=open()"
  Then the page shows one row per matching change
  And each row links to the change's MR page

Scenario: default query on first paint
  Given a public repo "o/r"
  When I GET "/o/r/search"
  Then the query box contains "open()"
  And open changes are listed

Scenario: empty result set
  Given a public repo "o/r"
  When I GET "/o/r/search?q=description(nope-nothing)"
  Then the page shows the empty state, not a table

Scenario: reflected query is escaped
  Given a public repo "o/r"
  When I GET "/o/r/search?q=%3Cscript%3E"
  Then the response body contains no raw "<script>" tag
```

## 19. Verification script (manual)

1. `cargo run` with a seeded repo.
2. Open `localhost:3000/o/r/search` — search box prefilled with `open()`,
   rows for open changes, rows link to MR pages.
3. Search a distinctive description term — only matching rows.
4. Search `description(zzz-unlikely)` — empty state, no bare table.
5. Repo page shows the Search link next to Files.

## 20. Traceability

`// story: e11s02` in the search handler, the search template/HTML block,
the repo-page Search link, and the `revset_ui` / `revset_default` tests.
