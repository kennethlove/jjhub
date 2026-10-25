# e19s01 — Shared creation service: UI handler delegates

## 1. Story

Repo creation has exactly one code path: the web form, and later
both transports, all call one creation service, so the sequence
cannot drift between callers.

## 2. Business value

First slice of push-to-create: the create sequence lives inline in
`routes::create_repo` today. Extracting it is the seam e19s02 and
e19s03 need — without it, transport push-to-create would be a third
copy of validate/init/INSERT that rots against the UI form.

## 3. Type

refactor (behavior-preserving extraction)

## 4. Risk / context

risk: P1 (BCP 4, refactor of a live creation path) — context: pure
extraction; UI responses (409 duplicate, 422 invalid name, redirect)
must be byte-identical for existing callers and tests.

## 5. Context

Zoom-out on the modules this touches:
- **routes::create_repo** (routes.rs:264) — purpose: session+CSRF
  form handler that validates input, checks duplicates, initializes
  stores, inserts the row, and renders form errors. Callers: the
  POST /repos route. Contracts: response codes and redirect stay
  identical.
- **repo::init_bare** (repo.rs:6) — purpose: `git init --bare` for
  the storage root path. Callers: routes::create_repo today; the
  service after extraction.
- **jj_meta::init** (jj_meta.rs) — purpose: initialize jj op-store
  metadata beside the bare repo.
- **tokens** — untouched; no schema change in this story.

## 6. Requirements

#### MODIFIED: UI repo creation delegates to one service
**Before:** `routes::create_repo` runs validate -> dup-check ->
init_bare -> jj_meta::init -> INSERT tx inline (routes.rs:264).
**After:** the handler owns only form/session/CSRF/response-shape
concerns and calls the shared creation service for the sequence.

#### ADDED: Service owns failure cleanup and race re-lookup
The service removes initialized store directories on INSERT failure
and, on unique-constraint violation, re-looks-up the existing row
and returns it (the duplicate race proceeds as a duplicate, not a
crash). This behavior moves with the extraction, unchanged.

## 7. Actors

jj user (web form), transport code (later stories, same service).

## 8. Preconditions

Existing create-repo flow green (`cargo test -p server`); storage
root and DB reachable in dev.

## 9. Postconditions

POST /repos behaves exactly as before; `init_bare` has one caller
sequence — the service — used by UI now and transports next.

## 10. Business rules

- Validation stays `domain::repo_name_valid` + lowercase.
- Duplicate name -> 409, invalid name -> 422 (unchanged).
- Every store-init failure before INSERT leaves no orphan files.

## 11. Domain model touchpoints

**Repository** creation (owner, name, PRIVATE default comes from
the caller — UI keeps its visibility input).

## 12. UI/UX notes

None — form markup and responses unchanged.

## 13. API contracts

POST /repos unchanged (same codes, same redirect).

## 14. Data model

None — same INSERT, moved into the service.

## 15. Non-functional requirements

Extraction must not change concurrency behavior; the service is
async and pool-taking like the handler was.

## 16. Out of scope

Transport admission (e19s02), SSH parity (e19s03), any visibility
or 409/422 semantic change (explicitly out per SCOPE).

## 17. Acceptance criteria

- [ ] `cargo test -p server create_repo` — all existing UI create
      tests pass unchanged. (e19s01t1)
- [ ] `cargo test -p server creation_service` — cleanup-on-failure
      and unique-race re-lookup covered by new unit tests.
      (e19s01t2)
- [ ] `cargo test --workspace` green. (e19s01t4)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: UI create unchanged
  Given a signed-in user with a valid CSRF token
  When they POST /repos with a free name
  Then the repository row and stores exist and they redirect

Scenario: insert failure leaves no files
  Given the creation service fails after init_bare
  When the INSERT does not commit
  Then the bare and jj metadata directories are removed

Scenario: duplicate race re-looks-up
  Given two concurrent creates of the same name
  When one wins the unique constraint
  Then the loser re-looks-up and returns the existing row
```

## 19. Verification script (manual)

1. Start dev server, sign in, create a repo via the form — redirect
   works, page renders the new repo.
2. `cargo test -p server create_repo` — green.

## 20. Traceability

`// story: e19s01` in the creation service module, the
create_repo handler, and the `creation_service` tests.
