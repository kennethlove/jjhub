# e03s04 — Abandon/reopen + collaborators

## 1. Story

As an owner, I abandon dead MRs and grant collaborators push/review rights; abandoned MRs reopen on re-push.

## 2. Business value

Completes the MR state machine (reopen-on-push done in e03s01; abandon triggers here) and enables the Collaborator concept from the glossary (push rights become owner OR collaborator).

## 3. Type

feat

## 4. Risk / context

risk: P1 (authz change) — context: domain + UI.

## 5. Context

Abandon: button on MR detail (owner) sets state='abandoned'; transport already treats bookmark deletion as abandon signal (spec'd in e05/e03 — wire: after receive-pack, deleted bookmark whose MR is open → abandoned). Reopen already lands via e03s01 push handling. Collaborators: `collaborators` table (repo_id, user_id, unique pair); add/remove UI on repo settings (owner only); `authorize()` in transport.rs and proxy authz upgrade: push/private-read now owner OR collaborator.

## 6. Requirements

#### ADDED: Abandon button
Owner POST sets state=abandoned; abandoned MR leaves the open list; invariant 1's partial index allows a new open MR for the same change-id only after abandon — while abandoned, push reopens THIS MR (single row transitions back to open, not a duplicate).

#### ADDED: Bookmark deletion abandons
Post-receive detecting a deleted ref matching an open MR's bookmark → abandoned.

#### ADDED: Collaborator management
Owner adds/removes collaborators by email on repo settings page; unique (repo, user).

#### ADDED: Collaborator rights in transport
authorize(): push requires owner OR collaborator; private clone requires owner OR collaborator; public clone stays open to all logged-in.

## 7. Actors

Owner, collaborator, stranger.

## 8. Preconditions

e03s01–e03s03 shipped (state machine exists).

## 9. Postconditions

State machine complete per tech-stack.md diagram; collaborator push works, stranger push still 403.

## 10. Business rules

- Only owner manages collaborators (non-owner → 403/404 on settings).
- Merged MRs cannot abandon or reopen (terminal — test).
- Collab removal revokes rights immediately (checked per-request).

## 11. Domain model touchpoints

**Collaborator** (new), **Merge Request.state**, transport **authorize()**.

## 12. UI/UX notes

Abandon button (danger styling, confirm); collaborator form + list on repo settings.

## 13. API contracts

POST /{owner}/{name}/merge-requests/{change_id}/abandon; POST/DELETE /{owner}/{name}/settings/collaborators.

## 14. Data model

collaborators(id, repo_id, user_id, created_at, unique(repo_id, user_id)).

## 15. Non-functional requirements

authorize() adds one indexed lookup; <1ms.

## 16. Out of scope

Team/org collaborators, per-repo roles beyond owner/collab, notifications.

## 17. Acceptance criteria

- [ ] Owner abandons → MR leaves open list; re-push reopens same row (state machine test).
- [ ] Deleted bookmark abandons its open MR (test).
- [ ] Collaborator push succeeds (403→200, e2e-ish test); stranger still 403.
- [ ] Private clone: collaborator allowed, stranger 404/401 per transport rules (test).
- [ ] Non-owner cannot manage collaborators (403).

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: collaborator push
  Given repo alice/demo with bob as collaborator
  When bob pushes with his token
  Then the push is accepted
```

## 19. Verification script (manual)

1. Add bob as collaborator; bob pushes — accepted; remove bob; push — 403.
2. Abandon MR, re-push change — MR shows open again.

## 20. Traceability

`// story: e03s04` in implementing code and tests.
