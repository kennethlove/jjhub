# e19s03 — SSH push-to-create parity + e2e

## 1. Story

As a jj user on SSH, my first push to a fresh path under my own
handle creates the repo — the same decision, the same service, the
same PRIVATE default as smart HTTP.

## 2. Business value

Closes push-to-create: one admission helper makes the decision for
both transports (the transport::authorize principle, extended), so
HTTP and SSH cannot drift. The e2e proves the whole path on both.

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 4, transport integration on a live SSH path) —
context: ssh.rs start_service already shares transport::authorize
with HTTP; the missing-repo branch gains the same guarded create.

## 5. Context

Zoom-out on the modules this touches:
- **ssh::start_service** (ssh.rs) — purpose: russh channel exec for
  git pack programs over SSH. Callers: the SSH listener (e17).
  Contracts: token password auth, env discipline, no git spawn
  before admission.
- **admission helper (e19s02)** — the shared "create-or-404" seam;
  SSH calls it with its TransportUser instead of duplicating logic.
- **creation service (e19s01)** — same service, same cleanup.

## 6. Requirements

#### ADDED: SSH receive-pack on a missing owned path creates the repo
The SSH exec path calls the same admission helper: ReceivePack +
TransportUser + URL owner equals handle creates the PRIVATE repo
via the creation service, then spawns git-receive-pack.

#### ADDED: SSH reads and foreign paths unchanged
upload-pack on a missing repo and any push into another user's
namespace disconnect with the same not-found shape as today.

#### ADDED: e2e parity proof
An end-to-end script proves an SSH push to a fresh path creates the
repo and that an HTTP push to the same path's sibling behaves the
same — one decision, both transports.

## 7. Actors

jj/git user over SSH.

## 8. Preconditions

e19s02 shipped (helper exists); e17 SSH listener and e2e harness
green.

## 9. Postconditions

`jj git push` over `ssh://` to a fresh owned path creates the repo;
existing SSH clone/push e2e stays green.

## 10. Business rules

- Identical admission semantics on both transports: PRIVATE
  default, ownership gate, upload-pack 404, foreign namespace 404.
- No git process spawns before the row exists (SSH included).

## 11. Domain model touchpoints

**Repository** (new row), **Token/User** (ownership gate) — same
entities as e19s02, different transport.

## 12. UI/UX notes

None.

## 13. API contracts

No new HTTP routes; SSH surface unchanged apart from the create.

## 14. Data model

None new.

## 15. Non-functional requirements

SSH creation path inherits the same failure cleanup; the e2e must
not require manual steps beyond a running dev stack.

## 16. Out of scope

Rate-limiting the SSH create (e20s03), ssh:// URL snippets (already
shipped in e17s02), publickey auth.

## 17. Acceptance criteria

- [ ] `cargo test -p server ssh_push_to_create` — SSH admission
      creates on owned ReceivePack, 404s otherwise. (e19s03t1)
- [ ] `bash scripts/e2e-ssh-push.sh` — SSH push e2e stays green
      including the create case. (e19s03t2)
- [ ] `cargo test --workspace` green. (e19s03t3)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: SSH first push creates
  Given a token-holder "alice" and the SSH listener
  When alice pushes to "ssh://host:2202/alice/newproj.git"
  Then the repository row exists, PRIVATE
  And the pushed refs are visible

Scenario: SSH clone of nothing stays not-found
  Given the same listener
  When anyone runs upload-pack on a missing repo
  Then the session reports not-found and no row is created

Scenario: one decision, both transports
  Given alice pushes the same tree over HTTP and SSH
  Then both create identical PRIVATE repositories
```

## 19. Verification script (manual)

1. Start jjhub with SSH enabled.
2. `jj git remote add sshdemo ssh://localhost:2202/alice/demop.git`
3. `jj git push` with the token as password — push succeeds.
4. Repo page shows the PRIVATE repo created by the push.

## 20. Traceability

`// story: e19s03` in the SSH admission call site, the e2e script
cases, and the `ssh_push_to_create` tests.
