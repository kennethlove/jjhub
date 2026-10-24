# e18s01 — Compose on Coolify: env-gated ports, healthcheck, docs

## 1. Story

As a self-hoster, I register the compose stack on Coolify and it
comes up healthy without editing files: no fixed host port collides
with the platform proxy, the container healthcheck gates readiness,
and SSH host keys survive container replacement.

## 2. Business value

The compose file is the install. A hard-bound `3000:3000` and host
keys written to the container layer are the two things that break a
first Coolify deploy (port collision with the proxy or another app)
or break clients later (known_hosts mismatch on every redeploy).

## 3. Type

feat

## 4. Risk / context

risk: P3 (BCP 4 — compose + one Dockerfile line + docs; no Rust
change) — context: the local self-host flow (e02s01, plain
`docker compose up -d` from a filled .env) must keep working.

## 5. Context

Zoom-out on the modules this touches:
- **compose ports** — the app service publishes `3000:3000`
  unconditionally today. Ports become env-gated list entries
  `- "${JJHUB_BIND_HTTP:-}"` and `- "${JJHUB_BIND_SSH:-}"`: unset
  (Coolify) compose warns `no port specified: <empty>` and publishes
  nothing; `.env` sets `3000:3000` locally. `.env` is gitignored, so
  a platform clone never sees the local defaults. Verified empirically
  (docker compose config, 2026-10-24): empty entry — config valid,
  no `ports:` in the rendered output.
- **healthcheck** — the runtime image (debian:bookworm-slim) has no
  curl; the healthcheck probes `/healthz` (routes.rs, exists), so the
  apt line gains curl. The compose healthcheck mirrors the kata
  service's CMD-SHELL pattern; Coolify gates deployment health on it.
- **host keys** — `JJHUB_SSH_HOST_KEY_DIR` defaults to
  `./data/host-keys`, outside the volume, so keys regenerate per
  container. Compose sets it to `/var/lib/jjhub/host-keys` inside the
  repo-data volume: fingerprints stable across redeploys.
- **README** — documents the Coolify path: two secrets, no bound
  ports, optional port mappings (HTTP to 3000, SSH to 2202).

## 6. Requirements

#### ADDED: Host ports bind only when the operator asks
The compose stack publishes app HTTP and SSH ports only through
`JJHUB_BIND_HTTP` / `JJHUB_BIND_SSH`. Unset means zero published
ports. The local `.env` (via `.env.example` defaults) keeps
`docker compose up -d` serving http://localhost:3000.

#### ADDED: App healthcheck gates readiness
The app service healthchecks `/healthz` on the configured
`JJHUB_PORT`; an unhealthy app shows in `docker compose ps` and in
the platform's deployment status.

#### ADDED: SSH host keys persist on the volume
Compose sets `JJHUB_SSH_HOST_KEY_DIR=/var/lib/jjhub/host-keys`
(repo-data volume), so regeneration is never per-container.

#### ADDED: Coolify deployment documented
README section: required envs, unset bind vars, optional port
mappings, healthcheck meaning.

## 7. Actors

Platform operator (human), Coolify (orchestrator), `docker compose`
(local self-hoster).

## 8. Preconditions

`.env` exists with `POSTGRES_PASSWORD` and `SESSION_SECRET` (local)
or both are supplied by the platform (Coolify); `.env` is gitignored.

## 9. Postconditions

Local: stack healthy, http://localhost:3000 answers. Coolify: same
stack, no host ports bound by compose, platform maps what it needs,
app container healthy.

## 10. Business rules

- Compose never hard-codes a host port; only `JJHUB_BIND_*` publishes.
- A platform clone (no `.env`) must render a valid config with zero
  published ports.
- Healthcheck must exercise the real `/healthz` route, not a TCP poke.

## 11. Domain model touchpoints

None — deployment configuration only; no storage, auth, or review
semantics change.

## 12. UI/UX notes

No product UI; README prose only (simple-english discipline).

## 13. API contracts

None new — env vars compose already interpolates; `/healthz` exists.

## 14. Data model

None.

## 15. Non-functional requirements

Local flow unchanged: `docker compose up -d` from a filled `.env`
serves http://localhost:3000 (e02s01 preserved). Image grows by one
package (curl) for an honest HTTP healthcheck.

## 16. Out of scope

Kubernetes/Helm manifests; TLS or proxy configuration (the
platform's); CI deploy jobs; kata service changes; changing default
listen ports.

## 17. Acceptance criteria

- [ ] t1 `docker compose config`: zero published ports with
  `JJHUB_BIND_*` unset; exactly `3000:3000` when
  `JJHUB_BIND_HTTP=3000:3000`.
- [ ] t2 `docker compose up -d` → app healthy in
  `docker compose ps`; host-key dir resolves under `/var/lib/jjhub`.
- [ ] t3 README carries the Coolify section; `.env.example` carries
  both `JJHUB_BIND_*` vars.
- [ ] t4 Preflight green (`fmt`, `clippy -D warnings`, workspace
  tests, build).

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: platform clone binds nothing
  Given no JJHUB_BIND_HTTP or JJHUB_BIND_SSH is set
  When docker compose config renders the stack
  Then no service publishes a host port

Scenario: local .env keeps the e02s01 flow
  Given .env sets JJHUB_BIND_HTTP=3000:3000
  When docker compose config renders the stack
  Then the app publishes host port 3000

Scenario: healthcheck answers
  Given the stack is up
  When the app container healthcheck runs
  Then curl of /healthz inside the container succeeds
```

## 19. Verification script (manual)

```bash
env -u JJHUB_BIND_HTTP -u JJHUB_BIND_SSH docker compose config \
  | grep -c 'published'          # expect 0 (no ports: section)
JJHUB_BIND_HTTP=3000:3000 docker compose config | grep -q '3000'
docker compose up -d
docker compose ps                # app healthy
docker compose down
```

## 20. Traceability

| Requirement | Files |
| --- | --- |
| env-gated ports | docker-compose.yml, .env.example |
| healthcheck | docker-compose.yml, Dockerfile (curl) |
| host keys on volume | docker-compose.yml |
| documentation | README.md, .env.example |
