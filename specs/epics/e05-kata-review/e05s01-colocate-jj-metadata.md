# e05s01 — Colocate jj metadata beside bare repo at creation

## 1. Story

As a self-hoster, every repository jjhub creates carries a jj metadata directory next to its bare git store, so the kata sidecar (e05s02) can open repos unmodified.

## 2. Business value

Unblocks the entire kata review layer (spike: `specs/archive/spikes/SPIKE-kata-review-integration.md`). Cheap now, retrofitting later means touching every existing repo.

## 3. Type

feat

## 4. Risk / context

risk: P1 — context: infra. Depends on jj-lib as a library dependency; repo creation flow already exists (e02s01).

## 5. Context

Repo creation currently runs `git init --bare` under the storage root. This story adds jj metadata initialization beside it: a per-repo store directory (layout agreed in spike: `{storage}/meta/{owner}/{name}/`) initialized via jj-lib's `GitBackend::init_external`-compatible layout, so `Workspace::load`-based tools (kata) can open the pair. The git transport continues to serve the bare repo; jj metadata is invisible to git clients.

## 6. Requirements

#### ADDED: Metadata dir created with repo
Creating a repository also creates `{storage}/meta/{owner}/{name}/` with jj metadata pointing at the bare repo. Failure to init metadata fails repo creation (no half-created repos).

#### ADDED: Metadata is never served
Git transport routes serve only the bare repo path. Metadata dirs are unreachable via `/git/...` URLs.

#### ADDED: Layout matches kata expectations
The metadata dir layout must be loadable by jj-lib 0.41 `Workspace::load` with default store factories — verified by a test that opens the created pair and reads HEAD's tree.

## 7. Actors

User (creates repos), kata sidecar (future reader).

## 8. Preconditions

e02s01 shipped (repo creation + bare init exist). jj-lib pinned at 0.41.

## 9. Postconditions

Every new repo has a loadable jj workspace; invariant: no repo row without both bare store and metadata dir.

## 10. Business rules

- Metadata path: `{storage}/meta/{owner}/{name}/`, mirrors repo name validation.
- Creation is transactional with the DB insert: bare + metadata + row, or nothing.
- Existing repos (pre-e05) handled by backfill in e05s02 — NOT here.

## 11. Domain model touchpoints

**Repository** gains a derived storage layout (no schema change): bare at `{storage}/repos/{owner}/{name}.git`, metadata at `{storage}/meta/{owner}/{name}/`.

## 12. UI/UX notes

None — creation flow unchanged from the user's view.

## 13. API contracts

None new.

## 14. Data model

No DB changes. Storage layout only.

## 15. Non-functional requirements

Metadata init adds <50ms to repo creation. jj-lib dependency must not pull git2 (gix-only, per kata's stack).

## 16. Out of scope

Backfill for existing repos (e05s02), kata deployment (e05s02), auth proxy (e05s03), MR page links (e05s04).

## 17. Acceptance criteria

- [ ] Creating a repo produces bare store + metadata dir.
- [ ] jj-lib `Workspace::load` opens the created pair and resolves the trunk bookmark's tree (test).
- [ ] `GET /git/{owner}/{name}.git/info/refs` still works; metadata dir not reachable over transport.
- [ ] Failure in metadata init rolls back bare dir + DB row (test with fault injection point).
- [ ] Preflight green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: repo creation colocates metadata
  Given I am logged in
  When I create repo "demo"
  Then {storage}/repos/alice/demo.git exists
  And {storage}/meta/alice/demo/ exists

Scenario: kata-side load works
  Given repo "demo" with one pushed commit
  When the test opens the pair via jj-lib Workspace::load
  Then the trunk commit's tree contains the pushed file

Scenario: metadata not served over transport
  When a client requests /git/alice/demo-meta.git/info/refs
  Then the response is 404
```

## 19. Verification script (manual)

1. Start server, create `alice/demo` via UI.
2. `ls $JJHUB_STORAGE_ROOT/repos/alice/demo.git` and `ls $JJHUB_STORAGE_ROOT/meta/alice/demo` — both exist.
3. `jj log -R $JJHUB_STORAGE_ROOT/meta/alice/demo` after pushing a commit — shows the change.

## 20. Traceability

`// story: e05s01` in implementing code and tests.
