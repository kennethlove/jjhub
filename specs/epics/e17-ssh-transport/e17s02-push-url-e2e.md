# e17s02 — SSH push through admission + ssh:// URL + e2e

## 1. Story

As a jj user, I push to `ssh://…` and it behaves exactly like my HTTP
push — same admission rules, same audited rejections — and the repo
page shows me the SSH URL so I never have to construct it.

## 2. Business value

Completes SSH transport: writes (the half that matters for jj's
workflow) plus discoverability. Crucially the push goes through the
existing receive path — protected refs, identity rules, and rejection
auditing mean SSH is a second door into one building, not a side
entrance around the guards.

## 3. Type

feat

## 4. Risk / context

risk: P2 (BCP 6, write path + UI snippet + e2e) — context: receive
exec wired to the same admission chain as HTTP; Tera snippet.

## 5. Context

Zoom-out on the modules this touches:
- **receive_guard (e10) + transport receive path** — `admit_push`
  and `record_rejections` are transport-agnostic; e17s02 runs
  `git-receive-pack` over the SSH exec channel with the SAME post-
  receive hook wiring the HTTP path uses, so one code path decides
  acceptance for both transports.
- **Post-receive effects** — MR upsert, operation-log side effects
  (e09), bookmark/branch bookkeeping all fire identically; SSH pushes
  are indistinguishable from HTTP pushes in the DB.
- **Repo page snippet (Tera)** — the existing clone URL block gains
  an `ssh://` line beside the HTTP line (same shape e16s03's surface
  touches nearby — coordinate templates, different sections).
- **e2e** — new `scripts/e2e-ssh-push.sh` joins the e2e suite pattern
  (clone, foreign-author rewrite refused by identity rule, re-push)
  with SSH on port 2202 and the psql shim; CI e2e job gains the step.
- **Ref advertisement** — receive-pack advertises refs exactly as the
  HTTP push parser expects; the pkt-line machinery in transport.rs is
  reused where practical.

## 6. Requirements

#### ADDED: Push over SSH is admitted like HTTP
An authenticated client pushes a change stack over SSH; protected-ref
and identity rules apply unchanged, and a refused push is recorded in
`push_rejections` exactly as an HTTP refusal is.

#### ADDED: ssh:// URL on the repo page
The repo page's clone block renders the SSH URL (host/port from the
configured listener) beside the HTTP URL.

#### ADDED: e2e push suite over SSH
An e2e script proves clone → push → refused rewrite → re-push over
SSH, asserting DB state (MR revisions) identical in shape to the HTTP
suite's.

## 7. Actors

jj users (push), reviewers (MRs appear from SSH pushes).

## 8. Preconditions

e17s01 (listener, auth, clone), e10 (admission chain), e03 (MR
upsert from pushes).

## 9. Postconditions

A jj user can live entirely on the SSH URL — push, see MR revisions,
get refused when policy says so; repo page documents that URL.

## 10. Business rules

- One admission chain: no SSH-specific bypass, exemption, or second
  implementation of receive_guard checks.
- Rejections audited (push_rejections row) regardless of transport.
- The ssh:// URL renders only when the SSH listener is enabled
  (config-gated display matches config-gated listener).

## 11. Domain model touchpoints

**MergeRequest**/**Revision** (created by pushes), **PushRejection**
(e10s03 rows) — all via existing paths.

## 12. UI/UX notes

One extra line in the clone block; copy/monospace treatment already
exists for the HTTP line; ux audit before handoff.

## 13. API contracts

None new — SSH exec surfaces (receive-pack), HTTP unchanged.

## 14. Data model

None.

## 15. Non-functional requirements

SSH push throughput equals HTTP (same binaries); listener shutdown
drains with the server; e2e step adds no CI dependency beyond the
listener (port 2202 test-harness config).

## 16. Out of scope

Publickey auth, SSH-only rate limiting distinct from HTTP's,
multi-port listeners, ssh:// in MR/review surfaces, client key
management.

## 17. Acceptance criteria

- [ ] `cargo test -p server ssh_push_admission` — push over the SSH
      exec path routes through admit_push: protected-ref violation
      refused + recorded, identity rule holds. (e17s02t1)
- [ ] `cargo test -p server ssh_url_snippet` — enabled listener
      renders the ssh:// line; disabled renders none. (e17s02t2)
- [ ] `bash scripts/e2e-ssh-push.sh` — full SSH suite green (clone,
      push, refusal, re-push; MR revisions present). (e17s02t3)
- [ ] `cargo test --workspace` green; ux audit passes. (e17s02t4)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: push a stack over SSH
  Given a jj user with a transport token
  When I add the ssh:// remote and push my change stack
  Then the push succeeds
  And the MR shows my pushed revisions

Scenario: protected-ref refusal is audited over SSH
  Given the same remote
  When I push a non-fast-forward update to trunk
  Then the push is refused
  And a push_rejections row records the attempt

Scenario: URL snippet follows config
  Given SSH disabled in config
  When I GET the repository page
  Then no ssh:// URL renders
```

## 19. Verification script (manual)

1. Repo page with SSH on: ssh:// line present and correct (port from
   config); with SSH off: absent.
2. `jj git push <ssh remote>` a stack — MR revisions appear; force a
   trunk rewrite — refusal logged in push_rejections.

## 20. Traceability

`// story: e17s02` in the SSH receive wiring, the clone-block
template, `ssh_push_admission` / `ssh_url_snippet` tests, and
`scripts/e2e-ssh-push.sh`.
