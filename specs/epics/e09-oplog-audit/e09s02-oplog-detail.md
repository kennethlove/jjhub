# e09s02 — Operation detail view with ref changes

## 1. Story

As a self-hosting admin, I open one operation in the audit log and see
exactly which refs moved and which commits were added or removed, so "who
did what to which ref" is answerable per operation, not inferred.

## 2. Business value

Delivers the per-operation precision of the SCOPE_LATEST e09 acceptance —
"an auditable, per-repository sequence of ref operations with actor and
time" — by rendering the operation's `View`: the ref deltas are already
stored in `.meta`, this story surfaces them.

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 3, presentation over shipped read path) — context: UI
(server-rendered pages over jj-lib `View` data).

## 5. Context

Zoom-out on the modules this touches:
- **routes.rs purpose** — every HTTP page handler (`repo_page` 398,
  repo chrome reused). **Callers** — lib.rs router. **Contracts** — axum
  handler signatures; tests assert HTML substrings.
- **e09s01 op-log helper purpose** — reads operations from the `.meta`
  store. **Callers** — operations list page. **Contracts** — operation id
  + `OperationMetadata` fields. This story adds a `read_view` step on the
  same path — one read implementation, no second jj-lib wiring.

Data source (Prior Art, adopted): `Operation::view_id` (op_store.rs:360)
names the `View` (op_store.rs:249) — `local_bookmarks`,
`remote_views`, `git_refs: BTreeMap<GitRefNameBuf, RefTarget>`,
`git_head`, `view.head_ids`. Each `RefTarget` reports `as_normal()` for
the prior/target commit ids, `added_ids()`/`removed_ids()` for the delta,
and `has_conflict()` so a ref that landed conflicted is flagged. `OpStore::read_view`
(op_store.rs:464) loads it through the same `RepoLoader`/`op_store`
access e09s01 established.

## 6. Requirements

#### ADDED: Operation detail page
`GET /{owner}/{name}/operations/{operation_id}` renders the operation:
timestamp, username@hostname, description, parent operations (linked),
and its view's head ids — on the repo-detail chrome, linked from every
list row.

#### ADDED: Ref-change table
The detail lists every ref that changed in the operation's view
(local bookmarks, remote views, git refs, git head) with prior → new
commit ids from `added_ids()`/`removed_ids()`; unchanged refs are omitted.
Conflicted ref targets are flagged with the e08 conflict flag.

#### ADDED: Unknown operation is 404
An operation id that does not resolve in the repo's `.meta` store → 404;
missing `.meta` → same "no operation log" empty state as e09s01, no 500.

#### ADDED: Repo authz on the detail
Same read rules as the list page: public readable, private 404 for
unauthorized viewers.

## 7. Actors

Self-hosting admin and any user who may read the repository.

## 8. Preconditions

e09s01 (read helper + list page), e06s01 (site chrome), e08s03
(conflict-flag helper).

## 9. Postconditions

Each operation in the log opens to its ref deltas with actor and time —
the audit record is complete per operation, server-side only.

## 10. Business rules

- One `read_view` per detail render; no per-ref subprocess.
- Ref names and descriptions are HTML-escaped (attacker-influenced data).
- Conflict flag reuses the e08s03 helper — one definition.
- Read-only: no operation replay, no `write_operation`, no transaction.

## 11. Domain model touchpoints

**Repository** (audit scope), **Revision**/**Change** (commit ids in
deltas), refs (bookmarks/remote views as stored by jj).

## 12. UI/UX notes

Server-rendered page on the repo-detail pattern: header block (time,
actor, description), parents as links, then a ref-change table — ref name
left, prior → new commit ids (short ids, links to browse pages when the
commit exists), conflicted rows flagged. Empty ref-change view says "no ref
changes" plainly. No JS.

## 13. API contracts

`GET /{owner}/{name}/operations/{operation_id}` — HTML page (200);
unknown id → 404; missing `.meta` → 200 empty state; unauthorized private
repo → 404.

## 14. Data model

None — reads the `View` jj already stores.

## 15. Non-functional requirements

Single `read_view` + single `read_operation` per render; table scales to
the ref count of one view (bounded by the repo's ref set); escape-then-
render on every value.

## 16. Out of scope

Graph/visualization of the operation DAG, operation search by id prefix,
snapshot/diff rendering of view contents, pagination of ref rows,
cross-repo audit views, any write path.

## 17. Acceptance criteria

- [x] `cargo test -p server oplog_detail` — detail page shows timestamp,
      actor, description, linked parents, and the ref-change table with
      prior → new ids from `added_ids()`/`removed_ids()`; conflicted
      targets flagged; unknown operation id → 404; missing `.meta` →
      empty state. (e09s02t1) — 5/5 pass, run 2026-10-22, evidence: handler `operation_detail_page` + `ref_changes()`/`load_operation_detail` in jj_meta.rs + `templates/operation_detail.html` + `tests/oplog_detail.rs`
- [x] `cargo test --workspace` green. (214/0 after git-config fix 62b88453)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: detail shows ref deltas
  Given a repo whose newest operation moved a bookmark
  When I GET "/o/r/operations/<op-id>"
  Then the page shows actor, time, and description
  And the ref table shows the bookmark with prior and new commit ids

Scenario: no ref changes
  Given an operation whose view changed no refs
  When I GET its detail page
  Then the table area shows "no ref changes"

Scenario: unknown operation id
  Given a public repo "o/r"
  When I GET "/o/r/operations/deadbeefnotanid"
  Then I get 404

Scenario: conflicted ref target flagged
  Given an operation whose view holds a conflicted ref target
  When I GET its detail page
  Then the ref row carries the conflict flag
```

## 19. Verification script (manual)

1. Open the operations list; click the newest row — detail renders with
   actor, time, description, ref table.
2. Follow a parent link — chain walks back.
3. Request a bogus operation id — 404.
4. Ref moved by a push — prior → new ids match `git log`.

## 20. Traceability

`// story: e09s02` in the detail handler, the ref-change render helper,
the list→detail links, and the `oplog_detail` test.
