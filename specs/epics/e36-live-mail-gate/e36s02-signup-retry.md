# Story: e36s02 — signup/resend clear retry

## Story

As a person signing up while the send path is broken, I want a clear
message telling me to retry instead of a blank failure or a 500, and
I want to know no half-finished account is left behind.

## Business value

MAIL-LATEST.md's failure behavior: on the signup and resend paths a
dead relay produces a clear retry, never a 500; reset stays
response-neutral. The retry text is the difference between a
recoverable hiccup and a lost user, and the no-half-account promise
keeps the data model honest when the send dies after the row exists.

## Type

Feature / quality

## Risk context

The neutral reset contract (shipped in e34s02) must not be reopened
here; only the signup/resend paths gain the retry surface. Half of
the failure modes live between "row committed" and "send returned",
so the test has to fail the send for real.

## Steps

1. `t1` — Write `crates/server/tests/send_retry.rs` asserting:
   signup with a failing send (SMTP arm on a dead port) returns a
   clear retry on the page, never a 500, and leaves no user row for
   that address; resend with a failing send answers the same way and
   the page says to retry; reset's response stays byte-identical
   whether the send succeeds or fails.
2. `t2` — Implement the retry rendering on the signup and resend
   paths, keeping the send-failure branch honest to the mailer's
   returned outcome.
3. `t3` — Gates: fmt, clippy, tests, build.

## Verification Script

- Run: `cargo test -p server --test send_retry`
- Expect: passing (t2), failing for the recorded reason (t1).
- Check: `reset_mail` suite still green (neutral contract untouched).
- Done when: both paths show the retry, no 500, no half-account.

## Out of scope

- Reset-path messaging changes (response stays neutral).
- Queueing, retries with backoff, suppression, bounce handling.
- Any SMTP default; drains.

## Risks

- Ordering: the account row must not survive a failed send; the
  test pins the order, not the implementation.
- Duplicating retry logic between the two paths; prefer one shared
  render.
