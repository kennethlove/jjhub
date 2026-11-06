# Story: e36s01 — delivery readout

## Story

As a self-hosting operator, I want every send's outcome recorded in
the mail log and shown on the admin page, so I can see whether mail
works before I configure a live relay.

## Business value

The v0.7.0 phase gate says "no live send until confirmation links and
a delivery readout pass in staging." This story makes the readout
real: each `mail_log_YYYY-MM.jsonl` line carries the send's outcome
(ok or failed, with a structured reason) recorded at the single write
point, and `/admin` shows the most recent deliveries so the operator
reads the fact off the screen, not out of a file.

## Type

Feature / quality

## Risk context

The gate's whole point is that mail health is a fact, not a hope.
A readout that only ever says "ok" (because failures are swallowed
elsewhere) would pass tests and fail the operator. Failure must be
injected for real and recorded where the send is written.

## Steps

1. `t1` — Write `crates/server/tests/delivery_readout.rs` asserting the contract:
   a failed send (SMTP arm pointed at a dead port) still answers
   the page without a 500 and writes a mail_log line whose outcome
   is the failure with a structured reason; a successful send
   (LogMailer) writes an ok line; both lines carry `ts`, `to`,
   `subject`, and `outcome`; `/admin` (operator) lists the recent
   deliveries readout.
2. `t2` — Implement: record the outcome at the single write point
   (the mailer's send, where e35 already writes the line), then add
   the operator readout surface on `/admin`.
3. `t3` — README staging-proof instructions (point the SMTP env at a
   relay, send one verification, read the outcome on `/admin`) plus
   the full gate set (fmt, clippy, tests, build).

## Verification Script

- Run: `cargo test -p server --test delivery_readout`
- Expect: passing (t2), failing for the recorded reason (t1).
- Check: injected failure shows up in the log line and on `/admin`.
- Done when: the phase-gate "delivery readout" condition has a
  test-backed implementation and the staging instructions exist.

## Out of scope

- Queueing, retries, bounce/complaint handling, suppression lists.
- Any live SMTP default (operator configures after the gate).
- Log-service drains; HTTP-API provider profiles.

## Risks

- Outcomes must be written at the one write point, not
  reconstructed per surface; otherwise the readout and the log can
  disagree.
- Failure injection must not need a real mail server.
