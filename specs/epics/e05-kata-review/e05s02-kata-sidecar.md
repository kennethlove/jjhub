# e05s02 — kata sidecar service in compose, pointed at repo store

## 1. Story

As a self-hoster, `docker compose up` starts a kata sidecar that sees every jjhub-hosted repo, so review UI is available without per-repo setup.

## 2. Business value

Delivers the review surface (patchset diffs, anchored comments) that e04 comment threads gave up to kata. This is the leverage the spike bought.

## 3. Type

feat

## 4. Risk / context

risk: P1 (external integration) — context: infra.

## 5. Context

kata runs as a sidecar container: its workspace root points at jjhub's storage layout (`{storage}/meta/{owner}/{name}` dirs from e05s01, colocated with bare repos). This story: pin kata (commit-pinned build stage in our Dockerfile or upstream image), add service to docker-compose, healthcheck, and prove the sidecar can open a repo created by jjhub (the load path kata itself uses: `Workspace::load`). Also include backfill: a small command/script that creates metadata dirs for repos that predate e05s01.

## 6. Requirements

#### ADDED: kata service in compose
`docker compose up` starts jjhub app + kata sidecar. Kata's workspace root = jjhub storage meta root.

#### ADDED: Kata pinned
Kata is pinned to an exact commit (vendored build stage or digest-pinned image). No floating tags.

#### ADDED: Backfill command
`jjhub backfill-meta` (or script) creates missing metadata dirs for existing repos, idempotent.

#### ADDED: Sidecar opens jjhub repos
Integration test: create repo via jjhub, push a commit, sidecar (in-process or container) loads the workspace and lists the change.

## 7. Actors

Self-hoster (operator), reviewer (uses sidecar UI).

## 8. Preconditions

e05s01 shipped. Kata reachable as buildable artifact (Apache-2.0 — license attribution in THIRD_PARTY file).

## 9. Postconditions

Sidecar UI lists changes of a freshly pushed repo with zero manual setup.

## 10. Business rules

- Sidecar is loopback/internal only in compose network; never published to host ports (auth bridge is e05s03's job; until then network isolation is the control).
- Backfill is safe to re-run; never touches the bare repo.

## 11. Domain model touchpoints

None new — consumes e05s01 storage layout.

## 12. UI/UX notes

Kata's own web UI; jjhub does not restyle it in this story.

## 13. API contracts

None new on jjhub side.

## 14. Data model

None.

## 15. Non-functional requirements

Sidecar cold start <10s. Pinned dependency must build reproducibly (lockfile committed).

## 16. Out of scope

Auth proxy (e05s03), MR page integration (e05s04), exposing sidecar publicly.

## 17. Acceptance criteria

- [ ] `docker compose up -d` healthy: app + kata both up.
- [ ] Create repo, push via jj, sidecar lists the change (automated test or script assertion).
- [ ] Backfill run on a pre-e05 repo produces loadable metadata.
- [ ] Kata pin recorded in Dockerfile/compose with exact commit/digest.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: sidecar sees pushed repo
  Given compose stack is up
  And repo alice/demo was created and pushed to
  Then the kata sidecar workspace loads alice/demo
  And the pushed change is listed
```

## 19. Verification script (manual)

1. `docker compose up -d`, both services healthy.
2. Create + push a repo (e2e script or manual).
3. Open sidecar UI (via `docker exec` port-forward until e05s03) — change visible.

## 20. Traceability

`// story: e05s02` in implementing code and tests.
