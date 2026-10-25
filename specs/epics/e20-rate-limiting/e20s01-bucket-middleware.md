# e20s01 — Token bucket core, AppState Option, login row

## 1. Story

As an operator, expensive actions have a budget: an in-process
token bucket, opt-in via env, present on AppState as an Option so
tests never see it — and login POST gets its first policy row.

## 2. Business value

Tracer-bullet slice for rate limiting: the bucket, the middleware,
the env wiring, and the one unauthenticated state-changing route.
From here, adding policy rows is data plus one wrap (the why of
hand-rolling instead of governor/tower-http).

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 4, new middleware on the auth path) — context: limiter
is absent (None) unless the production constructor reads env; the
80+ existing tests must not notice it.

## 5. Context

Zoom-out on the modules this touches:
- **AppState** (lib.rs) — purpose: shared server state (pool,
  storage_root, config). Callers: every handler via State.
  Contracts: test constructors keep building AppState without a
  limiter; the new field is `Option<RateLimiter>`.
- **routes::login** (routes.rs:147) — purpose: POST /login session
  creation. Callers: the login form. Contracts: success/error
  responses unchanged when the limiter is off or under budget.
- **Config / main.rs** — purpose: env parsing and boot wiring;
  main.rs builds the limiter from env when JJHUB_RATE_LIMIT is on.
- **New module rate_limit.rs** — purpose: token bucket keyed
  (action, dimension, key), windowed refill, idle pruning hook,
  and the axum middleware via from_fn_with_state. No new crate.

## 6. Requirements

#### ADDED: In-process token bucket, keyed and windowed
A hand-rolled token bucket keyed (action, dimension, key) refills
over JJHUB_RATE_LIMIT_WINDOW seconds; consuming past the budget
fails with the retry-after duration.

#### ADDED: AppState carries Option<RateLimiter>
Test constructors leave it None (structurally absent); the
production constructor builds it from env. Middleware wraps only
explicitly named routes — never a global layer.

#### ADDED: Login POST per-IP budget
login POST checks the per-IP bucket (JJHUB_RATE_LOGIN_IP, default
20/hour, burst 5); over budget returns 429 with Retry-After. Under
budget nothing changes.

## 7. Actors

Operator (env config), anonymous client (login), jj user.

## 8. Preconditions

JJHUB_RATE_LIMIT unset in dev/test = limiter off; existing suite
green.

## 9. Postconditions

With the limiter on and a budget of 1, the second login POST from
one IP returns 429 + Retry-After; `cargo test --workspace` is
untouched by construction.

## 10. Business rules

- Master switch: JJHUB_RATE_LIMIT off disables all rows.
- Per-IP catches anonymous floods; per-user is added in e20s02.
- 429 body is plain text; Retry-After is whole seconds.

## 11. Domain model touchpoints

None — transport/session-level defense; no entity changes.

## 12. UI/UX notes

429 renders as a plain error page for browser flows; no template
work in this story beyond the status response.

## 13. API contracts

New failure mode on wrapped routes: HTTP 429 + Retry-After header.

## 14. Data model

None — buckets live in process memory.

## 15. Non-functional requirements

Bucket map entries are pruned when idle (hook landed here, used in
e20s03); middleware does no I/O — no DB hit on the hot path.

## 16. Out of scope

Push/create/token policy rows (e20s02), SSH disconnect shape
(e20s03), distributed counters, login lockout/CAPTCHA (SCOPE
out_of_scope).

## 17. Acceptance criteria

- [ ] `cargo test -p server rate_limit_config` — env parsing, off
      by default, window/burst defaults per the policy table.
      (e20s01t1)
- [ ] `cargo test -p server rate_limit_middleware` — None bypasses;
      wrapped route 429s past budget with Retry-After.
      (e20s01t2)
- [ ] `cargo test -p server rate_limit_login` — login POST per-IP
      row behaves per the policy table. (e20s01t3)
- [ ] `cargo test --workspace` green with limiter absent from test
      constructors. (e20s01t4)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: tests never see the limiter
  Given a test-constructed AppState
  Then the limiter field is None and routes behave as before

Scenario: login budget of one
  Given JJHUB_RATE_LIMIT on with JJHUB_RATE_LOGIN_IP=1
  When one IP POSTs /login twice
  Then the second response is 429 with Retry-After

Scenario: master switch off
  Given JJHUB_RATE_LIMIT unset
  When any wrapped route is hit repeatedly
  Then no response is 429
```

## 19. Verification script (manual)

1. Run dev with `JJHUB_RATE_LIMIT=on JJHUB_RATE_LOGIN_IP=2`.
2. POST /login three times quickly from one client.
3. Third response is 429 with a Retry-After header.

## 20. Traceability

`// story: e20s01` in rate_limit.rs, the AppState wiring, and the
`rate_limit_config` / `rate_limit_middleware` / `rate_limit_login`
tests.
