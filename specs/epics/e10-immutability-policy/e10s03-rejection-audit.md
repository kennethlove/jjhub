# e10s03 — Audit trail for rejected pushes

## 1. Story

As a repository owner, every push the server refuses is recorded with the
pushing identity, the ref, and the reason, so refusals are answerable
("who tried what, and why did it fail?") instead of vanishing into hook
stderr.

## 2. Business value

Completes the trust-boundary story: e10s01 refuses the write, this story
makes the refusal auditable. SCOPE v0.2.0 requires an auditable,
per-repository record with actor and time for ref operations; a rejected
push is exactly the operation most worth recording and currently the only
one that leaves no trace.

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 3, audit write on the guard path) — context: infra +
domain (pre-receive hook, one new table).

## 5. Context

Zoom-out on the modules this touches:
- **receive_guard.rs purpose** — push refusal decisions. **Callers** —
  `main.rs::run_pre_receive`. **Contracts** — `check_push` returns
  rejection reasons; the hook exits 1. This story adds recording, not new
  refusal logic: when any guard rule (e10s01 protected refs, e03s01
  open-MR identity) produces reasons, each refused ref update is inserted
  into a new `push_rejections` table before the hook exits.
- New migration `0009_push_rejections.sql`: `push_rejections(id, repo_id
  REFERENCES repositories, ref_name TEXT, old_sha TEXT, new_sha TEXT,
  identity TEXT, reason TEXT, created_at TIMESTAMPTZ DEFAULT now())`.
- Identity: the author/committer email already parsed from the pushed
  commit (`parse_commit_identity`); for updates with no parseable commit
  (e.g. deletion of a protected ref), `identity` records the ref name's
  pusher as unknown and still records ref + reason.
- Recording failure must never turn a refusal into an acceptance: on
  insert error the hook still exits 1 and logs the insert error to stderr.

## 6. Requirements

#### ADDED: Rejection recorded with identity and ref
Any guard rejection writes one row per refused ref update — repo, ref
name, old and new head, pushing identity, reason, timestamp — before the
hook exits non-zero.

#### ADDED: Both guard rules record
Protected-ref rewrites (e10s01) and open-MR identity violations (e03s01)
both produce audit rows; accepted pushes write nothing.

## 7. Actors

Enforcement: the pre-receive hook. Readers: repo owners (querying the
table directly in 0.2.0; UI is out of scope).

## 8. Preconditions

e10s01 (protected-ref rejections exist), e03s01 (identity rejections
exist), migration pipeline runs on server start.

## 9. Postconditions

Every refused push leaves a durable, per-repo, per-ref record with actor
and time.

## 10. Business rules

- One row per refused ref update, not per push attempt batch.
- Never accept a push because audit insertion failed.
- Accepted pushes stay audit-free (no hot-path writes).

## 11. Domain model touchpoints

**Repository** (scoped audit target), ref updates, pushing **User**
(identity).

## 12. UI/UX notes

None — storage only; no page or API in this story.

## 13. API contracts

None.

## 14. Data model

New table `push_rejections` via migration `0009_push_rejections.sql`
(schema in §5).

## 15. Non-functional requirements

One insert per refused update; hook exit code is 1 regardless of insert
success; timestamps are server-side.

## 16. Out of scope

Audit-view page or API, pagination, retention/pruning, op-log integration
(e09 remains jj-operation reading), metrics/alerting.

## 17. Acceptance criteria

- [ ] `cargo test -p server protect_audit` — a rejected push inserts a row
      with repo, ref, identity, reason, and timestamp; an accepted push
      inserts none; insert failure still refuses the push. (e10s03t1)
- [ ] `cargo test --workspace` green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: rejected push is recorded
  Given a public repo "o/r" with trunk at commit A
  When I push a rewrite of refs/heads/trunk
  Then the push is refused
  And a push_rejections row exists naming the ref, the identity, and the reason

Scenario: accepted push writes no audit row
  Given a public repo "o/r"
  When I push a new change branch successfully
  Then no push_rejections row exists for that ref
```

## 19. Verification script (manual)

1. Force-push a rewrite of trunk — refused.
2. `psql` — `SELECT ref_name, identity, reason, created_at FROM
   push_rejections ORDER BY created_at DESC LIMIT 1;` shows the refusal.

## 20. Traceability

`// story: e10s03` in the hook's recording step, the migration, and the
`protect_audit` test.
