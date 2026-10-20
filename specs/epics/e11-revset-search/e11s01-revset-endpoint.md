# e11s01 — Revset query endpoint end-to-end

## 1. Story

As a jj user, I query a repository's history through jjhub with the revset
language I already know, so search speaks one dialect instead of teaching me
a second query syntax.

## 2. Business value

Closes the "Revset search" line in SCOPE_LATEST.yaml: "a revset expression
returns matching revisions through API and UI". This story builds the API
half — the parsing and evaluation surface every later story (search UI,
op-log views) reuses. No SQL full-text index to build, maintain, or diverge.

## 3. Type

feat

## 4. Risk / context

risk: P0 (BCP 5, query engine for the whole epic) — context: domain + infra.
New endpoint over read-only jj-lib calls; jj-lib 0.41 is already a server
crate dependency.

## 5. Context

Zoom-out on the modules this touches:
- **routes.rs purpose** — every HTTP handler. **Callers** — lib.rs router
  registrations. **Contracts** — axum handler signatures; tests assert
  status codes and body substrings.
- **jj_meta.rs purpose** — the only jj-lib access point (settings,
  `Workspace::load`, `spawn_blocking` around blocking jj calls).
  **Callers** — repo create (e05s01), push sync (e05s02), kata sidecar,
  browse (e07). **Contracts** — async fns returning `Result<T, String>`,
  meta workspace at `{storage}/meta/{owner}/{name}/`.

New route `GET /{owner}/{name}/revsets?q=<revset>`: parse `q` with
`jj_lib::revset_parser::parse_program`, resolve it with
`RevsetExpression::evaluate` against the repo's colocated meta workspace
(load via `Workspace::load` + `load_at_head`, after `sync_refs` so the
latest push is visible). Run the evaluation in `spawn_blocking` like every
other jj-lib call in jj_meta.rs. Results capped at 50 commits (server-side
limit; a revset can match thousands). Response is JSON: one object per
matching commit — `change_id`, `commit_id`, `description`, `is_open` — in
evaluation order.

Discovery (planning-context, v0.2.0 research): `parse_program`
(revset_parser.rs:447) plus `RevsetExpression::evaluate` (revset.rs:680)
cover the query engine — no hand-rolled parser, no new jj-lib
feature-branch PR. The archived wave-1 security review of the parser
surface stands; this story adds no parser code of its own.

## 6. Requirements

#### ADDED: Revset query endpoint
`GET /{owner}/{name}/revsets?q=<revset>` on an existing repo returns 200
with a JSON list of matching commits (change_id, commit_id, description,
is_open), newest evaluation order preserved, capped at 50.

#### ADDED: Query parsing via jj-lib
`q` is parsed with `revset_parser::parse_program` and evaluated with
`RevsetExpression::evaluate` in the repo's meta workspace after ref sync —
the jj dialect, not SQL, is the query language.

#### ADDED: Invalid query is a 400
A query that fails to parse or evaluate returns 400 with the parser's
message in the body; no partial results, no 500.

#### ADDED: Repo scoping and authz
Only repos the viewer may read (public, or private with owner/
collaborator session) resolve; unknown or unauthorized repos keep their
existing 404 behavior. Endpoint is read-only.

## 7. Actors

Any user who can read the repository (owner, collaborator, or anonymous
visitor on a public repo).

## 8. Preconditions

e02s01/e02s02 (repos + authz), e05s01 (colocated meta workspace exists
per repo), e05s02 (`sync_refs` site). jj-lib 0.41 in `crates/server`.

## 9. Postconditions

Any readable repo answers revset queries server-side; parse failures
surface as jj's own error messages; the search UI (e11s02) has its data
source.

## 10. Business rules

- jj's revset grammar is the contract — no jjhub-specific query sugar.
- Result list capped at 50 commits; the cap is a server constant, not a
  request parameter.
- Query evaluation is read-only: no transaction, no ref write.
- Private-repo queries never leak existence to unauthorized viewers
  (same 404 as the rest of the repo routes).

## 11. Domain model touchpoints

**Repository** (scoped query target), **Change** (change_id in results),
**Revision** (commit_id in results).

## 12. UI/UX notes

None — JSON API only; the search page lands in e11s02.

## 13. API contracts

`GET /{owner}/{name}/revsets?q=<revset>`
- 200: `[{"change_id": "...", "commit_id": "...", "description": "...", "is_open": true}, ...]`
- 400: parse/evaluate error, body carries the message text
- 404: unknown repo, unauthorized private repo, or missing `q`
- 405/404: route exists for GET only

## 14. Data model

None — reads through jj-lib; no new tables, no migrations.

## 15. Non-functional requirements

Evaluation runs off the async runtime (`spawn_blocking`), consistent with
every jj-lib call in jj_meta.rs. Query timeout bounded by the block_on
call; result cap keeps response size flat regardless of match count.

## 16. Out of scope

Search page, search box, default query, empty states (e11s02). Full-text
code search (SCOPE_LATEST out-of-scope). Caching, pagination, query
history, saved queries. Any write-side jj operation.

## 17. Acceptance criteria

- [x] `cargo test -p server revset_parse` — parse_program accepts valid
      revsets (`trunk()`, `open()`, `description(x)`, unions) and rejects
      garbage. (e11s01t1)
- [x] `cargo test -p server revset_query` — seeded repo: endpoint returns
      matching commits as JSON, capped at 50, scoped to the path repo.
      (e11s01t2)
- [x] `cargo test -p server revset_error` — unparseable query → 400 with
      message; unknown repo → 404; private repo unauthorized → 404.
      (e11s01t3)
- [x] `cargo test --workspace` green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: query returns matching commits
  Given a public repo "o/r" with pushed changes
  When I GET "/o/r/revsets?q=open()"
  Then I get 200 and a JSON list of open commits
  And each entry has change_id, commit_id, description, is_open

Scenario: invalid revset rejected
  Given a public repo "o/r"
  When I GET "/o/r/revsets?q=:::"
  Then I get 400 and the parser's error message

Scenario: private repo stays invisible
  Given a private repo "o/r" and I am not owner or collaborator
  When I GET "/o/r/revsets?q=open()"
  Then I get 404

Scenario: results capped
  Given a repo with more than 50 commits
  When I GET "/o/r/revsets?q=all()"
  Then the response has exactly 50 entries
```

## 19. Verification script (manual)

1. `cargo run` with a seeded repo.
2. `curl -s 'localhost:3000/o/r/revsets?q=open()' | jq .` — JSON commit list.
3. `curl -si 'localhost:3000/o/r/revsets?q=%%%' | head -1` — HTTP/1.1 400.
4. `curl -si 'localhost:3000/o/missing/revsets?q=open()' | head -1` — 404.

## 20. Traceability

`// story: e11s01` in the new route handler, the jj_meta.rs query helper,
and the `revset_parse` / `revset_query` / `revset_error` tests.
