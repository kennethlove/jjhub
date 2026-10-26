# e20s03 — SSH over-budget disconnect + idle pruning

## 1. Story

As an operator, an over-budget SSH push is cut off cleanly: the
session disconnects with a "rate limited, retry later" message
before any git process spawns — and stale buckets do not grow
forever.

## 2. Business value

Closes rate limiting: the SSH transport cannot be used to dodge
the HTTP budgets, and the in-process bucket map stays bounded
(idle pruning) so the limiter itself cannot become the leak.

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 3, SSH admission path) — context: russh has no HTTP
status channel, so the mapping is a disconnect with a message
(per ssh_failure_shape in planning-context).

## 5. Context

Zoom-out on the modules this touches:
- **ssh::start_service** (ssh.rs) — the receive-pack exec path
  consumes the push budget per user+IP before spawning git;
  over budget disconnects with the message.
- **rate_limit.rs bucket map (e20s01)** — gains idle pruning: a
  bucket untouched for longer than the window is dropped, so key
  cardinality (many users/IPs over months) cannot grow unbounded.

## 6. Requirements

#### ADDED: SSH push over budget disconnects before git spawn
When the SSH receive-pack path is over its push budget, the
session disconnects with a clear "rate limited, retry later"
message; no git process is spawned and no pack data transfers.

#### ADDED: SSH reads are not throttled
upload-pack over SSH is never budget-checked, mirroring the HTTP
read exemption.

#### ADDED: Idle bucket pruning
Buckets idle for longer than JJHUB_RATE_LIMIT_WINDOW are pruned on
access, keeping the map bounded without a background task.

## 7. Actors

jj/git user over SSH, operator.

## 8. Preconditions

e20s01 and e20s02 shipped; e19 SSH parity landed so the push path
is uniform.

## 9. Postconditions

With a push budget of 1, the second SSH push disconnects with the
message; the existing SSH e2e stays green with the limiter off.

## 10. Business rules

- Same budget, same window as the HTTP push row — one policy row,
  two transports.
- Disconnect happens before admission work and before git spawn.

## 11. Domain model touchpoints

None.

## 12. UI/UX notes

None (terminal-facing message only).

## 13. API contracts

SSH failure shape: clean disconnect + message (no status codes).

## 14. Data model

None.

## 15. Non-functional requirements

Pruning is O(map) amortized on access; no timer task, no new
dependency.

## 16. Out of scope

Per-channel (vs per-session) accounting, SSH login throttling,
background pruning tasks.

## 17. Acceptance criteria

- [ ] `cargo test -p server rate_limit_ssh` — over-budget
      receive-pack disconnects with the message, no git spawn.
      (e20s03t1)
- [ ] `cargo test -p server rate_limit_prune` — idle buckets are
      dropped after the window. (e20s03t2)
- [ ] `cargo test --workspace` green. (e20s03t3)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: SSH push budget of one
  Given the limiter on with a push budget of 1
  When a user pushes twice over SSH
  Then the second session disconnects with the rate-limit
    message and no git process spawns

Scenario: SSH clone never throttles
  Given the same limiter
  When a user clones repeatedly over SSH
  Then every clone succeeds

Scenario: idle buckets are pruned
  Given a bucket consumed long ago
  When pruning runs on the next access
  Then the stale entry is gone and a fresh budget applies
```

## 19. Verification script (manual)

1. Run dev with SSH and `JJHUB_RATE_LIMIT=on
   JJHUB_RATE_PUSH_USER=1`.
2. Push twice over SSH — second disconnects with the message.
3. Clone repeatedly — never throttled.

## 20. Traceability

`// story: e20s03` in the SSH receive admission call site, the
pruning function, and the `rate_limit_ssh` / `rate_limit_prune`
tests.
