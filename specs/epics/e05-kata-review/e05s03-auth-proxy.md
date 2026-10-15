# e05s03 — Auth proxy: jjhub session/token to kata header-auth

## 1. Story

As a self-hoster, only authenticated jjhub users reach the review sidecar, and the sidecar trusts jjhub instead of the network.

## 2. Business value

Closes the auth gap the spike identified: kata is loopback-trust by default; jjhub must be the gate. Without this, private repo review leaks.

## 3. Type

feat

## 4. Risk / context

risk: P0 (auth boundary) — context: infra.

## 5. Context

jjhub proxies sidecar traffic under `/review/{owner}/{name}/...`: session (web) or token (API) auth runs first, then jjhub forwards to kata with a shared header token kata is configured to trust. Kata stays unpublished on the compose network. Repo-level authorization reuses invariant 6 semantics: public read open to logged-in users, private repo requires owner (collaborators land with e03s04 rights).

## 6. Requirements

#### ADDED: Proxy route
`/review/**` on jjhub proxies to the sidecar after auth. Unauthenticated → login redirect (web).

#### ADDED: Sidecar header trust
Kata configured to accept jjhub's shared secret header; direct sidecar requests without it rejected (network isolation + header).

#### ADDED: Repo authorization on proxy
Private repo: non-owner authenticated user gets 404 on `/review/{owner}/{name}/**` (no existence leak).

#### ADDED: Secret config
Shared secret from env (`KATA_TRUST_SECRET`), required at boot like SESSION_SECRET will be (fail-fast deferred per earlier decision — but compose must provide it).

## 7. Actors

Reviewer (logged in), attacker (unauthenticated or wrong-user).

## 8. Preconditions

e05s02 shipped (sidecar reachable internally).

## 9. Postconditions

No path to review data without jjhub auth; invariant 6 extends to `/review/**`.

## 10. Business rules

- 401 → login redirect for browsers; 401 JSON for token clients.
- Wrong user on private repo → 404 (match repo page behavior).
- Proxy strips client-supplied trust header before forwarding (header spoofing impossible).

## 11. Domain model touchpoints

**Repository.visibility**, **User** session/token (e01).

## 12. UI/UX notes

Transparent — kata UI under `/review/` path prefix; relative assets must work (verify in task; fall back to subdomain-style routing note if kata's bundle assumes root path).

## 13. API contracts

`/review/{owner}/{name}/{**path}` — reverse proxy, no new jjhub JSON API.

## 14. Data model

None.

## 15. Non-functional requirements

Proxy adds <5ms p50. Streaming responses (SSE if kata uses it) must not be buffered.

## 16. Out of scope

Per-repo kata access control beyond owner check, rate limiting on proxy (hardening epic), TLS termination.

## 17. Acceptance criteria

- [ ] Unauthenticated `/review/alice/demo/` → login redirect.
- [ ] Logged-in non-owner on private repo → 404.
- [ ] Logged-in owner → sidecar content rendered.
- [ ] Client-sent trust header is stripped (test: request with forged header from unauthenticated client still rejected).
- [ ] Preflight green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: spoofed trust header rejected
  Given an unauthenticated client
  When they GET /review/alice/demo/ with header X-Kata-Trust: <secret>
  Then the response is a login redirect

Scenario: private repo invisible to strangers
  Given private repo alice/secret and logged-in bob
  When bob GETs /review/alice/secret/
  Then the response is 404
```

## 19. Verification script (manual)

1. Stack up; `curl -i localhost:3000/review/x/y/` unauthenticated → 303 login.
2. Log in as owner; browse review page — kata UI renders.
3. Second user on private repo — 404.

## 20. Traceability

`// story: e05s03` in implementing code and tests.
