# e09s01 — Read the colocated .meta operation log

## 1. Story

As a self-hosting admin, I browse each repository's newest jj operations
through jjhub, so I have an auditable record of who did what to which ref
and when — without shell access to the host.

## 2. Business value

Closes the "Operation-log audit" line in SCOPE_LATEST.yaml: "The server
exposes an auditable operation log per repository: who did what to which
ref, when, in jj's operation model." The colocated `.meta` repo already
records every operation — this story reads jj's own store instead of
inventing a parallel audit table that would drift.

## 3. Type

feat

## 4. Risk / context

risk: P0 (BCP 5, new read surface over the repo's audit data) — context:
domain + infra. jj-lib 0.41's `op_store` API is already a dependency;
read-only access to files jj itself reads.

## 5. Context

Zoom-out on the modules this touches:
- **jj_meta.rs purpose** — the only jj-lib access point (`Workspace::load`,
  `load_at_head`, `spawn_blocking` around blocking calls). **Callers** —
  repo create/push sync, kata sidecar, browse, trunk resolution.
  **Contracts** — async fns returning `Result<T, String>`, meta workspace
  at `{storage}/meta/{owner}/{name}/`.
- **routes.rs purpose** — every HTTP page handler (repo_page 398,
  mr_detail_page 648). **Callers** — lib.rs router. **Contracts** — axum
  handler signatures; tests assert HTML substrings.

Read path (Prior Art, adopted from planning-context v0.2.0): jj-lib's
`Operation` (op_store.rs:360) carries `view_id`, `parents`,
`metadata: OperationMetadata`; `OperationMetadata` (op_store.rs:417) has
`time: TimestampRange`, `description`, `hostname`, `username`,
`is_snapshot`, `workspace_name`. The `OpStore` trait (op_store.rs:464) —
`root_operation_id`, `read_operation`, `resolve_operation_id_prefix` —
reads `.meta` directly; `RepoLoader::load_operation` (repo.rs:808) wraps
it, `ReadonlyRepo::operation()` (repo.rs:301) and `Repo::op_store()`
(repo.rs:752) expose it, same `Workspace::load` + `load_at_head` pattern
as jj_meta.rs. Newest-first ordering follows the operation parent chain
from `root_operation_id`.

Discovery (jj-lib 0.41.0): `View` (op_store.rs:249) holds
`local_bookmarks`, `remote_views`, `git_refs`, `git_head` as `RefTarget`
— the "which ref" data e09s02 renders; `RefTarget::has_conflict()` exists
so even an operation view can be flagged.

## 6. Requirements

#### ADDED: Operation-log read helper
A jj_meta-style helper reads a repo's operations from its `.meta` store
via jj-lib (`OpStore`/`RepoLoader::load_operation`), newest-first from the
root operation, returning operation id, timestamp, username, hostname, and
description — run in `spawn_blocking` like every other jj-lib call.

#### ADDED: Operations list page
`GET /{owner}/{name}/operations` renders the newest operations (capped at
50) as rows: time, username@hostname, description, link to the operation
detail (e09s02), on the repo-detail chrome.

#### ADDED: Missing .meta degrades gracefully
A repo without a colocated `.meta` workspace (or one that fails to load)
renders an explicit "no operation log" empty state with 200 — never a 500
or a raw jj-lib error string.

#### ADDED: Repo authz on the log
The list page follows repo read rules: public repos readable by anyone,
private repos 404 for unauthorized viewers (same behavior as repo pages).

## 7. Actors

Self-hosting admin and any user who may read the repository.

## 8. Preconditions

e02s01 (bare + meta storage layout), e05s01 (colocated workspace),
e06s01 (site chrome), jj-lib 0.41 `op_store` in `crates/server`.

## 9. Postconditions

Every hosted repo exposes a per-repository, newest-first operation list
from jj's own log; missing log data is a visible state, not a failure.

## 10. Business rules

- jj's operation model is the contract — no new storage format for
  operations (planning-context constraint); read-only.
- List capped at 50 operations, newest first, server-side constant.
- `is_snapshot` operations are listed like any other — the log is raw
  audit data, not a curated feed.
- No write, no transaction, no op-store mutation (no `write_operation`).

## 11. Domain model touchpoints

**Repository** (scoped audit target), **Change/Revision** (operations
touch refs), server-side audit surface (SCOPE in_scope e09).

## 12. UI/UX notes

Server-rendered page on the repo-detail pattern: **Operations** entry in
the repo nav; rows show time, `username@hostname`, description (escaped —
descriptions are attacker-influenced), operation id as link. Missing-log
empty state names the cause plainly. No JS.

## 13. API contracts

`GET /{owner}/{name}/operations` — HTML page (200); unknown or
unauthorized private repo → 404; missing `.meta` → 200 empty state.

## 14. Data model

None — reads jj's `.meta` operation files; no new tables, no migrations.

## 15. Non-functional requirements

All jj-lib reads run in `spawn_blocking`; page renders 50 rows from one
op-chain walk; bounded subprocess/IO (no per-row disk scans beyond the
operation read).

## 16. Out of scope

Operation detail view with ref changes (e09s02), operation
`resolve_operation_id_prefix` search, filtering/pagination beyond the cap,
diff or snapshot rendering, cross-repo/global audit, auth-log or HTTP-log
audit, any write path.

## 17. Acceptance criteria

- [ ] `cargo test -p server oplog_read` — helper reads the fixture
      `.meta` store: operations newest-first with time, username,
      hostname, description populated from `OperationMetadata`.
      (e09s01t1)
- [ ] `cargo test -p server oplog_page` — list page renders newest-first
      rows capped at 50, escaped descriptions, private-repo 404 for
      unauthorized viewers. (e09s01t2)
- [ ] `cargo test -p server oplog_missing` — repo without `.meta` renders
      the "no operation log" empty state with 200, no jj-lib error text.
      (e09s01t3)
- [ ] `cargo test --workspace` green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: operations listed newest first
  Given a repo with three recorded jj operations
  When I GET "/o/r/operations"
  Then I get 200 and the newest operation is the first row
  And each row shows time, username@hostname, and description

Scenario: list is capped
  Given a repo with 60 recorded operations
  When I GET "/o/r/operations"
  Then the page shows exactly 50 rows

Scenario: missing operation log
  Given a repo without a colocated .meta workspace
  When I GET "/o/r/operations"
  Then I get 200 with the "no operation log" empty state
  And the body contains no raw jj-lib error text

Scenario: private repo stays invisible
  Given a private repo "o/r" and I am not owner or collaborator
  When I GET "/o/r/operations"
  Then I get 404
```

## 19. Verification script (manual)

1. `cargo run` with a seeded repo that has recorded operations.
2. Open `localhost:3000/o/r/operations` — newest first, identities and
   descriptions visible.
3. Remove/rename a repo's `.meta` — page shows the empty state, still 200.
4. Private repo from a logged-out session — 404.

## 20. Traceability

`// story: e09s01` in the op-log read helper, the operations handler, the
page markup, and the `oplog_read` / `oplog_page` / `oplog_missing` tests.
