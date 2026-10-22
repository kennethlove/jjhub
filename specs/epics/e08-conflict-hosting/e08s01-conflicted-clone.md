# e08s01 — Clone and serve repos containing conflicted commits

## 1. Story

As a jj user, I push a commit that carries an unresolved conflict and later
clone my repository through jjhub, because a jj-first forge hosts conflicted
commits instead of rejecting the workflow it claims to support.

## 2. Business value

Closes the "Conflict-tolerant hosting" line in SCOPE_LATEST.yaml: "Repos
whose commits carry unresolved jj conflicts are stored and served like any
other; conflict state stops being a rejection case for push, fetch, and
pages." This story builds the storage and transport half — every later
story (browse, trunk, MR views) needs a conflicted repo to exist first.

## 3. Type

feat

## 4. Risk / context

risk: P0 (BCP 5, push/clone is the site's core loop) — context: domain +
infra. jj-lib 0.41 already writes the git conflict representation; this
story reads and serves it over existing smart-http plumbing.

## 5. Context

Zoom-out on the modules this touches:
- **transport.rs purpose** — smart-http git transport
  (`smart_http` transport.rs:66, `run_cgi` transport.rs:247).
  **Callers** — lib.rs routes `/r/{*rest}`. **Contracts** — Git protocol
  over CGI; `authorize()` runs before any subprocess; post-receive hooks
  (`sync_refs`, `abandon_deleted_refs`, `upsert_mrs_after_push`).
- **receive_guard.rs purpose** — push admission checks (identity on open
  MRs). **Callers** — receive-pack path in transport.rs. **Contracts** —
  `parse_push_commands(pkt_bytes)` (receive_guard.rs:139) and
  `check_push(pool, repo_id, pushed)` (receive_guard.rs:100) return
  reject strings.
- **scripts/e2e-clone.sh purpose** — end-to-end clone proof (register →
  create → token → clone, PORT 3900, DB postgres://postgres:jjhub@localhost:5432/jjhub_test).

Prior Art (SCOPE_LATEST.yaml table, adopted): jj represents conflicts in
git as `.jjconflict-base-N` / `.jjconflict-side-N` subtrees plus a
`jj:trees` commit header (`JJ_TREES_COMMIT_HEADER`, git_backend.rs:104) and
a README entry, with objects kept alive by `refs/jj/keep/*`
(`NO_GC_REF_NAMESPACE`, git_backend.rs:100). So admission logic mirrors
`receive_guard` style: a push whose commits carry conflicts is accepted
(only conflicted *bookmarks* are skipped later — jj_meta.rs:116 already
does this), and upload-pack must advertise the `refs/jj/keep/*` refs so a
clone fetches every object a conflicted commit needs.

Discovery (jj-lib 0.41.0, registry): conflict trees are written by
`write_tree_conflict` (git_backend.rs:1519) and the plain-git view of a
conflicted commit is the first side minus `.jjconflict*` entries — a clone
that only saw that view would read corrupt content, which is exactly why
the GC refs must be served.

## 6. Requirements

#### ADDED: Conflict fixture
A test fixture builds a bare repo containing a commit in jj's git conflict
form (`.jjconflict-base-N` / `.jjconflict-side-N` trees, `jj:trees` header
naming the merge of root trees, `refs/jj/keep/*` ref pinning the objects)
without requiring a jj CLI binary.

#### ADDED: Transport serves conflicted repos
`upload-pack` over smart-http serves the fixture repo unchanged —
advertised refs include `refs/jj/keep/*`, and a clone receives every object
reachable from the conflicted commit.

#### ADDED: Push admission unchanged for conflicts
A receive-pack push whose new commits contain jj conflicts is accepted;
`receive_guard` rejections still apply only to its existing identity rules.
Conflict state alone is never a rejection case.

#### ADDED: e2e clone proof
The end-to-end clone script clones a conflicted fixture repo through the
running server and `git fsck` passes on the clone.

## 7. Actors

Repo owner (pusher) and anyone who may clone the repository.

## 8. Preconditions

e02s01 (bare storage + smart-http), e05s01 (colocated meta workspace),
e05s02 (`sync_refs` site). jj-lib 0.41 in `crates/server`.

## 9. Postconditions

A repo containing conflicted commits clones through jjhub with intact
objects; pushes carrying conflicts land like any other push.

## 10. Business rules

- Conflict representation is jj's own (git + `.jjconflict*` + `jj:trees`);
  no new storage format (planning-context constraint).
- `refs/jj/keep/*` refs are advertised and fetchable — GC must never prune
  objects a conflicted commit needs.
- Conflicted *bookmarks* remain skipped at trunk resolution
  (jj_meta.rs:116) — serving a conflict is not the same as publishing a
  conflicted ref.
- Existing authz (private repos, tokens) applies to conflicted repos with
  no exceptions.

## 11. Domain model touchpoints

**Repository** (storage + transport), **Revision** (conflicted commit is
an immutable revision — invariant 5 holds), read-only git object access.

## 12. UI/UX notes

None — transport and fixture only; pages land in e08s02/e08s03.

## 13. API contracts

No new HTTP surface. Existing smart-http endpoints (`/r/{owner}/{name}/git-upload-pack`,
`/r/{owner}/{name}/git-receive-pack`) gain no behavior change beyond
serving conflict refs.

## 14. Data model

None — git objects and refs only; no new tables, no migrations.

## 15. Non-functional requirements

Clone of the fixture is a single protocol exchange with no server-side
post-processing of conflict objects; argv-only subprocess calls (no shell
interpolation) stay intact.

## 16. Out of scope

Conflict rendering in pages (e08s02), trunk/MR conflict state (e08s03),
conflict *resolution* or merge commits, jj transaction replay, per-side
diff UI.

## 17. Acceptance criteria

- [x] `cargo test -p server conflict_fixture` — fixture repo exists with a
      jj-conflicted commit: `jj:trees` header parses, `.jjconflict-base-0`
      and `.jjconflict-side-0` trees present, `refs/jj/keep/*` ref resolves.
      (e08s01t1) — 3 passed.
- [x] `cargo test -p server conflict_refs` — smart-http upload-pack over
      the fixture advertises `refs/jj/keep/*`; receive-pack accepts a push
      containing a conflicted commit; existing identity rejections still
      fire. (e08s01t2) — 4 passed.
- [x] `bash scripts/e2e-clone.sh` — e2e clone of the conflicted fixture
      through the running server; clone passes `git fsck`. (e08s01t3) — rc=0.
- [x] `cargo test --workspace` green. — preflight PASS at the integration tip.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: fixture carries jj's git conflict form
  Given the conflict fixture is built
  Then the conflicted commit has a "jj:trees" header
  And its tree contains ".jjconflict-base-0" and ".jjconflict-side-0"
  And "refs/jj/keep" pins the conflict objects

Scenario: clone fetches every conflict object
  Given a public repo built from the fixture
  When I clone it over smart-http
  Then the clone passes "git fsck"
  And the conflicted commit is present with its header

Scenario: conflicted push is not rejected
  Given a public repo on the server
  When I push a commit carrying an unresolved conflict
  Then receive-pack accepts the push
  And the existing identity rejection still fires for the identity case
```

## 19. Verification script (manual)

1. Build the fixture and run `cargo test -p server conflict_fixture`.
2. `git ls-remote http://localhost:3000/r/o/r` — `refs/jj/keep/*` listed.
3. Clone — `git fsck` clean, `git log` shows the conflicted commit.
4. Push a conflicted commit — accepted (exit 0).

## 20. Traceability

`// story: e08s01` in the fixture helper, the transport/ref-advertisement
changes, and the `conflict_fixture` / `conflict_refs` tests plus the e2e
clone script path.
