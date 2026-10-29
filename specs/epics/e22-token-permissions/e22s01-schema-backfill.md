# e22s01 — Token permission columns + backfill migration

## 1. Story

As an operator, tokens carry their permissions on the row:
scopes, optional expiry, optional repo pin, optional CIDR
allowlist — with every existing token backfilled to full
read+write so nothing changes until enforcement lands.

## 2. Business value

Schema-first slice of token permissions: the storage contract
exists before the enforcement code, and the backfill guarantees
zero behavior change on day one. This is the seam e22s02
(authenticate/authorize) and e22s03 (forms) build on.

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 3, schema change on the credential table) — context:
additive columns with defaults; the tokens table is hot on every
transport request, so defaults must keep the old SELECT path valid.

## 5. Context

Zoom-out on the modules this touches:
- **tokens table** (migrations/0002_tokens.sql) — columns today:
  id, user_id, label, prefix, token_hash, created_at, revoked_at.
  Contracts: token.rs queries select by hash and check
  revoked_at; new columns are additive with NOT NULL defaults.
- **token module** (token.rs) — create (token.rs:31) gains the
  scope/permission parameters with read+write as the default;
  authenticate (token.rs:48) is untouched in this story.
- **users/repositories** — repo_id pins reference repositories;
  FK with ON DELETE behavior decided here (cascade the pin or
  reject — document in the migration).

## 6. Requirements

#### MODIFIED: tokens row carries permissions
**Before:** tokens have identity columns only; every token is
full-power forever.
**After:** tokens gains `scopes TEXT[] NOT NULL DEFAULT
'{read,write}'`, `expires_at TIMESTAMPTZ NULL`, `repo_id UUID NULL
REFERENCES repositories(id)`, `allowed_inets INET[] NULL` (NULL
means "no restriction").

#### ADDED: Migration backfills existing tokens
Every existing row satisfies the NOT NULL defaults (read+write
scopes), so current clones and pushes keep working with no
application change. The migration is additive and re-runnable
(idempotent under sqlx migrate).

## 7. Actors

Operator (migration), all token users (implicitly).

## 8. Preconditions

Migration chain through 0011 applied (or this story lands first —
the two are independent).

## 9. Postconditions

`tokens` has four new columns; every pre-existing row is
read+write with no expiry, no pin, no allowlist; `cargo test
--workspace` is green.

## 10. Business rules

- Default scope set is exactly {read, write} — no token is born
  more restricted than today.
- NULL allowed_inets means unrestricted (not "deny all").
- Scope names are lowercase literals; the DB does not interpret
  them (application validates membership).

## 11. Domain model touchpoints

**Token** gains permission attributes; **Repository** referenced
by the optional pin.

## 12. UI/UX notes

None in this story (forms land in e22s03).

## 13. API contracts

None — token::create's signature grows in e22s02/e22s03; this
story only guarantees the storage shape.

## 14. Data model

Migration 0012 as in the requirement above, including the
backfill UPDATE for pre-existing rows (defaults handle it, but the
backfill is explicit and tested).

## 15. Non-functional requirements

The tokens SELECT on the transport hot path stays a single-row
index lookup; the new columns add no join.

## 16. Out of scope

Enforcement (e22s02), forms (e22s03), per-org tokens, scope
hierarchies (SCOPE out_of_scope).

## 17. Acceptance criteria

- [ ] `cargo test -p server token_permissions_migration` — columns
      exist with the documented defaults and FK. (e22s01t1)
- [ ] `cargo test -p server token_scopes_backfill` — pre-existing
      rows are read+write, unrestricted. (e22s01t2)
- [ ] `cargo test --workspace` green. (e22s01t3)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: fresh token defaults
  Given a token created without explicit permissions
  Then its scopes are {read, write} with no expiry, pin,
    or allowlist

Scenario: pre-existing rows survive the migration
  Given tokens that existed before migration 0012
  When the migration applies
  Then every row has read+write scopes and NULL restrictions
```

## 19. Verification script (manual)

1. Apply migrations on a dev DB with existing tokens.
2. `\d tokens` shows the four new columns.
3. `SELECT scopes FROM tokens;` shows {read,write} everywhere.

## 20. Traceability

`// story: e22s01` in migration 0012 and the
`token_permissions_migration` / `token_scopes_backfill` tests.
