# e02s02 — Push over HTTP + private repo enforcement

## 1. Story

As a self-hoster, I know push always needs my token and private repos stay private, so my code does not leak.

## 2. Business value

Closes invariant 6 (private repo: transport requires authentication) and makes push authorization real: only owner/collaborators may push. Collaborator management itself lands in e03s04; for now owner-only push.

## 3. Type

feat

## 4. Risk / context

risk: P0 (auth boundary) — context: infra

## 5. Context

e02s01 served transport with token resolution but no authorization. This story: every push requires a valid token whose user is the repo owner (collaborators later, e03s04). Upload-pack (clone/fetch) requires token iff repo is private. Visibility toggle appears on repo settings page (owner only).

## 6. Requirements

#### ADDED: Push requires authorization
`git-receive-pack` and push-side `info/refs?service=git-receive-pack` require valid token; token user must be repo owner. Otherwise 401 (no/invalid token) or 403 (valid token, wrong user).

#### ADDED: Private clone requires token
`git-upload-pack` and `info/refs?service=git-upload-pack` for a private repo require valid token. Public repo clone stays open.

#### ADDED: Visibility toggle
Repo settings page (owner only) flips public/private. Non-owner gets 404 on the settings page.

## 7. Actors

Owner, visitor, anonymous git client.

## 8. Preconditions

e02s01 shipped.

## 9. Postconditions

Invariant 6 enforced by tests. Anonymous push impossible.

## 10. Business rules

- 401 = missing/invalid credentials; 403 = valid user without rights.
- Owner check: repositories.owner_id == token user id.

## 11. Domain model touchpoints

**Repository.visibility**, **Collaborator** (rights reserved for e03s04).

## 12. UI/UX notes

Repo settings page: visibility radio + save.

## 13. API contracts

Git smart HTTP as e02s01; auth semantics per §10.

## 14. Data model

No new tables; repositories.visibility already exists.

## 15. Non-functional requirements

Auth decisions happen before spawning git subprocess.

## 16. Out of scope

Collaborator management (e03s04), org-level permissions, deploy keys.

## 17. Acceptance criteria

- [ ] Push without token → 401.
- [ ] Push with non-owner token → 403.
- [ ] Private clone without token → 401; with owner token → 200.
- [ ] Public clone without token → 200.
- [ ] Owner flips visibility via settings page; anonymous clone of now-private repo → 401.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: anonymous push rejected
  Given public repo alice/demo
  When a client pushes without credentials
  Then the response is 401

Scenario: stranger cannot push
  Given public repo alice/demo and bob's valid token
  When bob pushes to alice/demo
  Then the response is 403
```

## 19. Verification script (manual)

1. Start server; create `alice/demo` (public), `bob` account with token.
2. Clone as anonymous — works. Push as anonymous — fails with 401.
3. Push with bob's token — 403. Flip `alice/demo` to private — anonymous clone now 401.
4. Push with alice's token — works.

## 20. Traceability

`// story: e02s02` in implementing code and tests.
