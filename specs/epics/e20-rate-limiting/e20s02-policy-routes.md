# e20s02 — Policy table routes: push, create, token (HTTP 429)

## 1. Story

As an operator, the expensive HTTP actions each have a budget:
git push, repo creation, and token creation return 429 with
Retry-After when the budget is spent — per user and per IP.

## 2. Business value

The policy table becomes real on HTTP: the three abuse-prone write
paths (subprocess-spawning push, store-initializing create,
credential-minting token routes) get budgets. Rows are data — the
middleware from e20s01 is wrapped, not rewritten.

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 4, wraps the hot push path) — context: only wrapped
when the limiter exists; push-to-create admission (e19) shares the
create budget, so e19 must land first (beads: nn9.2 depends on
nn9.1).

## 5. Context

Zoom-out on the modules this touches:
- **transport::smart_http** (transport.rs:66) — the push (ReceivePack)
  path gains the push policy row; upload-pack (read) is NOT wrapped
  (SCOPE: no blanket throttling of reads).
- **routes::create_repo** (routes.rs:264) and the e19 push-to-create
  admission — both consume the create budget: one action, one row.
- **routes::create_repo_token** (routes.rs:184) and
  POST /settings/tokens — token creation rows (per-user only, per
  the policy table).
- **policy table** (rate_limit.rs) — data: action, dimensions,
  default budget, burst, env knobs (JJHUB_RATE_PUSH_USER/IP,
  JJHUB_RATE_CREATE_USER/IP, JJHUB_RATE_TOKEN_USER).

## 6. Requirements

#### ADDED: Push row (smart HTTP), per-user and per-IP
ReceivePack over smart HTTP consumes the push budget
(JJHUB_RATE_PUSH_USER default 30/hour burst 10;
JJHUB_RATE_PUSH_IP likewise). Over budget: 429 + Retry-After,
before any git subprocess spawns.

#### ADDED: Create row, per-user and per-IP
POST /repos and push-to-create admission both consume the create
budget (JJHUB_RATE_CREATE_USER/IP default 10/hour burst 3).

#### ADDED: Token-creation row, per-user
POST /settings/tokens and the repo-page token route consume the
token budget (JJHUB_RATE_TOKEN_USER default 10/hour burst 5).

## 7. Actors

jj user (push), signed-in user (create, token mint).

## 8. Preconditions

e20s01 shipped (middleware + env); e19 shipped (push-to-create
route exists to wrap).

## 9. Postconditions

With a budget of 1, the second push/create/token action in the
window returns 429 + Retry-After; reads and unwrapped routes never
429.

## 10. Business rules

- Reads (upload-pack) are never throttled — the policy table is
  explicit; no global layer.
- User dimension uses the session/transport user; IP dimension is
  the socket peer. Both must pass.
- Push-to-create charges BOTH push and create budgets (it is both
  actions) — decide in code comments; document in README env table.

## 11. Domain model touchpoints

None — session/transport-level defense.

## 12. UI/UX notes

429 renders as a plain error response; no template work.

## 13. API contracts

429 + Retry-After on the wrapped routes only.

## 14. Data model

None.

## 15. Non-functional requirements

Bucket checks are in-memory; no DB read per request; unwrapped
routes pay nothing.

## 16. Out of scope

SSH push row (e20s03), throttling reads, per-route fine-tuning
beyond the table (SCOPE out_of_scope).

## 17. Acceptance criteria

- [ ] `cargo test -p server rate_limit_policy` — table rows parse
      with documented defaults and env overrides. (e20s02t1)
- [ ] `cargo test -p server rate_limit_routes` — push/create/token
      429 past budget; reads and other routes unaffected.
      (e20s02t2)
- [ ] `cargo test -p server rate_limit_push_create` —
      push-to-create charges the create budget. (e20s02t3)
- [ ] `cargo test --workspace` green. (e20s02t4)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: push budget of one
  Given the limiter on with JJHUB_RATE_PUSH_USER=1
  When a user pushes twice over smart HTTP
  Then the second push gets 429 and spawns no git process

Scenario: reads never throttle
  Given the same limiter
  When a user fetches 50 times
  Then every fetch is served

Scenario: create and push-to-create share one budget
  Given JJHUB_RATE_CREATE_USER=2
  When a user creates a repo via the form and via push
  Then the third creation attempt gets 429
```

## 19. Verification script (manual)

1. Run dev with `JJHUB_RATE_LIMIT=on JJHUB_RATE_PUSH_USER=1`.
2. Push twice over smart HTTP — second returns 429.
3. Fetch repeatedly — never 429.

## 20. Traceability

`// story: e20s02` in the policy table, the wrapped routes, and the
`rate_limit_policy` / `rate_limit_routes` / `rate_limit_push_create`
tests.
