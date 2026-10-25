# e19s02 — Smart-HTTP push-to-create admission

## 1. Story

As a jj user, I add a remote for a repo I do not have yet and my
first push creates it — PRIVATE, under my own handle — so the
natural workflow never dead-ends at a 404 or a web form.

## 2. Business value

Second slice of push-to-create: the smart-HTTP receive path learns
to create on admission. Every create-on-push forge (Gitea, Codeberg,
GitLab, soft-serve) works this way; jjhub is the outlier. Ownership-
gated and PRIVATE by default means no squatting surface.

## 3. Type

feat

## 4. Risk / context

risk: P0 (BCP 5, auth/security boundary on the transport path) —
context: `transport::smart_http` (transport.rs:66) returns 404 on a
missing row today; that branch gains a guarded create, nothing else
moves.

## 5. Context

Zoom-out on the modules this touches:
- **transport::smart_http** (transport.rs:66) — purpose: smart-HTTP
  git endpoint; resolves (owner, name), authorizes, spawns
  http-backend. Callers: the /git/* routes. Contracts: 404 must
  still be returned for upload-pack on missing repos and for
  foreign namespaces; authorize still runs before any subprocess.
- **token_user / authorize** (transport.rs) — purpose: map the
  Authorization header to a TransportUser and make one admission
  decision. Contracts: unchanged signatures; create admission reads
  the same TransportUser.
- **creation service (e19s01)** — purpose: validate, dup-check,
  init_bare, jj_meta init, INSERT tx, cleanup, race re-lookup.
  The admission branch calls it; no second sequence exists.
- **receive_guard::admit_push** (receive_guard.rs:329) — the row
  must exist before http-backend spawns so the pre-receive hook and
  post-receive bookkeeping see the repo.

## 6. Requirements

#### ADDED: ReceivePack on a missing owned path creates the repo
When the repo row is missing, op is ReceivePack, a TransportUser is
attached, and the URL owner segment equals that user's handle, the
admission calls the creation service, inserts the row (PRIVATE),
and proceeds to http-backend. Unique-race re-lookup proceeds.

#### ADDED: upload-pack on a missing repo stays 404
Clone/fetch of a nonexistent repo is not a create; the missing-row
branch falls through to 404 for UploadPack.

#### ADDED: another user's namespace stays 404
When the URL owner segment differs from the token user's handle
(or no token is presented), the response is 404 — indistinguishable
from a nonexistent repo (no probing, no squatting).

## 7. Actors

jj/git user over smart HTTP.

## 8. Preconditions

e19s01 shipped (creation service); transport tokens exist (e17).

## 9. Postconditions

`jj git remote add` + `jj git push` on a fresh path under one's own
handle creates the repo and lands the refs; reads and foreign paths
behave exactly as before.

## 10. Business rules

- Created repos are PRIVATE (e10s02 settings UI flips later).
- Owner segment is lowercased and validated via
  `domain::repo_name_valid` before any lookup or create.
- One admission decision: create-or-authorize happens in the shared
  transport path, never in a transport-specific branch.

## 11. Domain model touchpoints

**Repository** (new row, PRIVATE), **Token/User** (ownership gate).

## 12. UI/UX notes

None — transport behavior only.

## 13. API contracts

/git/* smart-HTTP surface unchanged except the guarded create;
404 semantics preserved everywhere else.

## 14. Data model

None new — reuses repositories via the creation service.

## 15. Non-functional requirements

No git subprocess spawns before the row exists; creation counts
against the e20 create budget once that epic wraps the route.

## 16. Out of scope

SSH parity (e19s03), UI form changes, visibility flags in the push
URL, repo-count quotas (SCOPE out_of_scope).

## 17. Acceptance criteria

- [ ] `cargo test -p server push_to_create_admission` — create on
      owned ReceivePack; 404 for upload-pack and foreign owner.
      (e19s02t1)
- [ ] `cargo test -p server smart_http_create` — row exists and is
      PRIVATE before http-backend runs; pushed refs land.
      (e19s02t2)
- [ ] `bash scripts/e2e-push-to-create.sh` — real jj/git push to a
      fresh owned path creates the repo end-to-end. (e19s02t3)
- [ ] `cargo test --workspace` green. (e19s02t4)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: first push creates the owned repo
  Given a token-holder "alice"
  When alice pushes to "alice/newproj.git" which does not exist
  Then the repository row exists, PRIVATE
  And the pushed refs are visible

Scenario: clone of nothing is not a create
  Given the same server
  When anyone fetches "alice/nothing.git" (no repo)
  Then the response is 404 and no row is created

Scenario: foreign namespace stays closed
  Given alice's token
  When alice pushes to "bob/squat.git" (no repo)
  Then the response is 404 and no row is created
```

## 19. Verification script (manual)

1. `jj git remote add demo http://localhost:3000/alice/demo.git`
2. `jj git push` with alice's token — push succeeds.
3. Open the repo page — PRIVATE badge, refs listed.
4. Fetch `alice/other.git` — 404.

## 20. Traceability

`// story: e19s02` in the smart_http admission branch and the
`push_to_create_admission` / `smart_http_create` tests +
`scripts/e2e-push-to-create.sh`.
