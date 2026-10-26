# e21s03 — Regenerate rotate + demote repo-page form

## 1. Story

As a user, I rotate my token when I suspect it leaked: regenerate
revokes the old one, issues a new one, and shows the plaintext
once — and the repo-page minting form is demoted to an advanced
affordance so the account-level path is the default.

## 2. Business value

Closes the token-model epic: rotation is the answer to a leaked
credential, and demoting the repo-page form fixes the mis-signalling
that per-repo minting was the normal way to get a token. Route and
tests stay — one template change, no authz surgery.

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 3, credential lifecycle on settings) — context:
regenerate counts as token creation for the e20 policy row, so the
route wrap already exists.

## 5. Context

Zoom-out on the modules this touches:
- **settings token routes** (routes.rs:184 area) — purpose: mint
  tokens from settings. Contracts: the regenerate POST joins the
  same create/revoke helpers (token.rs:31 create, token.rs:59
  revoke).
- **repo-page token route (routes.rs create_repo_token)** —
  purpose: mint a repo-scoped token from the repo page. Contracts:
  route and tests unchanged; only its template placement moves
  behind a disclosure.
- **Tera templates** — settings gains the regenerate action; the
  repo page wraps the form in an explicit advanced disclosure.

## 6. Requirements

#### ADDED: Regenerate rotates the account token
A regenerate action on settings revokes the current account token
in the same operation that issues the replacement; the new
plaintext is shown exactly once (e15s02 discipline) and the old
token immediately fails authentication. Regenerate counts as token
creation for the e20 token-creation policy row.

#### MODIFIED: Repo-page token form demoted behind disclosure
**Before:** the repo page renders the token minting form inline as
a primary affordance.
**After:** the same form sits behind an explicit "Advanced"
disclosure; the route, its CSRF handling, and its tests are
unchanged, and per-repo minting remains possible.

## 7. Actors

Signed-in user (settings), repo visitor (repo page).

## 8. Preconditions

e21s01 shipped (auto-issue exists so rotation has a subject);
e20 token row in place for the rate-limit counting test.

## 9. Postconditions

A rotated token works immediately; the old one is dead; the repo
page no longer advertises minting.

## 10. Business rules

- Rotation is revoke+issue in one operation — no window where both
  or neither work.
- Plaintext shows once; regenerate again rotates again.
- Existing per-repo token consumers keep working (SCOPE
  out_of_scope: removing the route).

## 11. Domain model touchpoints

**Token** lifecycle (revoke_at set, new row); no schema change.

## 12. UI/UX notes

Settings: one regenerate control with copy-once confirmation.
Repo page: disclosure component per DESIGN.md; audit gate applies.

## 13. API contracts

New POST action on settings; repo-page route unchanged.

## 14. Data model

None.

## 15. Non-functional requirements

Rotation is two statements in one transaction; no plaintext ever
stored or logged.

## 16. Out of scope

Multiple named tokens per user, token editing, removing the
repo-page route, backfill.

## 17. Acceptance criteria

- [ ] `cargo test -p server token_regenerate` — old revoked, new
      shown once, old rejected at authentication. (e21s03t1)
- [ ] `cargo test -p server create_repo_token` — repo-page route
      and its tests still pass with the form behind the
      disclosure. (e21s03t2)
- [ ] `cargo test -p server rate_limit_token` — regenerate
      consumes the token-creation budget. (e21s03t3)
- [ ] `cargo test --workspace` green. (e21s03t4)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: rotation kills the old token
  Given a user with a working transport token
  When they regenerate on settings
  Then the new plaintext is shown once
  And the old token fails authentication immediately

Scenario: repo page no longer advertises minting
  Given a visitor on a repo page
  When the page renders
  Then the token form is inside a closed advanced disclosure

Scenario: regenerate is budgeted
  Given a token-creation budget of 1
  When a user regenerates twice in the window
  Then the second attempt gets 429
```

## 19. Verification script (manual)

1. Settings — regenerate; copy the new token once.
2. Old token push fails with 401; new token push works.
3. Repo page — form visible only after opening Advanced.

## 20. Traceability

`// story: e21s03` in the regenerate handler, the repo-page
template disclosure, and the `token_regenerate` /
`rate_limit_token` tests.
