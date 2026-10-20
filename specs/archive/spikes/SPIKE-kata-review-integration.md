# Spike: kata review-layer integration

## Question

Can the review model of [kata](https://github.com/martint/kata) (Apache-2.0) front jjhub-hosted bare repos, replacing from-scratch e03 review UI + e04 comment threads? Decided by four unknowns; unknown #1 was the go/no-go.

## Result

Answered. **GO** for sidecar federation (option A); option B viable with one small constructor.

## Findings

1. **jj-lib reads bare git repos without any `.jj/` workspace.** `GitBackend::init_external(store_path, git_repo_path)` (jj-lib 0.41) wraps a bare repo, keeping jj's extra metadata (`extra/`, `git_target`) in a *separate* store directory. Commit, tree, and file reads all work end-to-end.
2. **kata's `open_repo` hard-requires a workspace.** It calls `jj_lib::workspace::Workspace::load` (libjj.rs:81), which needs `.jj/repo` on disk. jjhub's hosted repos are bare git only.
3. **Two integration shapes fall out:**
   - **A (sidecar, recommended):** jjhub runs `git init --bare` AND a colocated `jj git init` metadata dir per repo (or `init_external`-style store dir beside it). kata opens repos unmodified via its existing workspace path. jjhub MR pages link/embed kata's rust-embed web UI; jjhub proxy injects auth (kata is loopback-trust by default).
   - **B (embed kata-service crate):** add one constructor to kata-jj (`open_bare` using `RepoLoader` over `GitBackend::init_external`). `JjBackend` trait is kata's single I/O surface, so everything above it (anchor resolution, patchset diff, log graph) works unchanged.
4. **Refresh/concurrency:** kata's `RepoLoader` re-stats op-heads before each load and only reopens when they change; jj imports refs from the git backend at load time. Concurrent force-push from jjhub network clients is safe; reads may be stale by one load, never corrupt. Unknown #2 resolved.
5. **Auth bridge:** kata trusts loopback by default; jjhub's sidecar proxy authenticates (session/token) and forwards with a shared header token. Header-auth hook exists in kata's server config surface. Unknown #3 resolved — no kata changes needed for A.
6. **UI cost:** kata ships a prebuilt Svelte 5 bundle (rust-embed). Option A: zero UI stack in jjhub (htmx pages link out). Option B: maintain both Svelte bundle and htmx. Unknown #4 resolved — A wins decisively.

## Evidence

Spike binary (deleted after this doc, per spike policy) in `/tmp/jjlib-spike` against a bare repo with one commit:

```
OPEN OK: jj-lib wrapped bare git repo, no .jj/ workspace needed
  commit 272f2df328342d6fe9ef80d02c741747f018ec5e: "spike: first commit on a bare repo"
  entries in root tree:
    greeting.txt: "hello from jjhub spike\n"
```

Source evidence: `GitBackend::init_external` at jj-lib-0.41.0/src/git_backend.rs:258; `Workspace::load` requirement at kata crates/kata-jj/src/libjj.rs:81.

## Implications for the plan

- **e03 (merge-requests) loses its UI/diff-rendering stories** — review surface becomes kata sidecar pages. e03 keeps: MR lifecycle, stack derivation, cascade merge (domain logic, no UI beyond list/link pages).
- **e04 (review-ci) keeps CI statuses; comment threads move to kata.**
- New epic **e05-kata-review** (created): colocate jj metadata at repo creation, deploy kata sidecar, auth bridge, MR page integration.
- Repo creation flow gains one step: jj metadata init next to the bare repo (cheap; no per-push cost).

## What was NOT explored

- kata's patchset refresh under *sustained* concurrent pushes (single-force-push behavior reasoned from source, not load-tested).
- kata web bundle's exact embedding constraints (iframe vs reverse-proxy path rewriting) — spike #2 if e05s03 needs it.
- Upstreaming `open_bare` to kata (worth proposing; not required for A).

## Recommendation

**Proceed with option A (sidecar federation).** Build e05 in order: metadata colocated at repo creation → kata sidecar in docker-compose → auth proxy → MR page links. Keep option B's `open_bare` patch in the back pocket if sidecar ops prove annoying. plan-work for e05 should treat kata as a pinned external dependency (commit-pin + own Dockerfile stage).
