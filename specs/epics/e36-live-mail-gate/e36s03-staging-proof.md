# Story: e36s03 — staging proof

## Story

As an operator about to trust a live relay, I want the staging-proof
procedure pinned by a test and one recorded staging send, so the
phase gate passes on evidence instead of intent.

## Business value

SCOPE_LATEST delta 3 and success criteria 1: the proof procedure
(the README section) must exist and be asserted, and a real staging
send must show its recorded outcome on `/admin`. Without this the
v0.8.0 gate stays open and no live SMTP default can ship later.

## Type

Verification / quality

## Risk context

The procedure text already shipped in e36s01; this story pins it so
it cannot rot silently, then performs the one step automated gates
cannot: a send through a real relay. The manual step needs relay
credentials from the operator — evidence is recorded only when that
run happens.

## Steps

1. `t1` — Write `crates/server/tests/staging_proof.rs` asserting the
   README carries the "Staging proof before you switch it on"
   section and that it names the SMTP env family
   (`JJHUB_EMAIL_PROVIDER=smtp`, `JJHUB_SMTP_*`), the `/admin`
   recent-deliveries readout, and the keep-log-mailer-until-ok rule.
2. `t2` — Staging send evidence (manual, `verify-script:` below):
   point the env at a real relay, run one signup + resend + notice,
   read `ok` outcomes on `/admin`, record the evidence in this file
   under Evidence and on the manual QA queue.
3. `t3` — Gates: fmt, clippy, tests, build.

## Verification Script

- Run: `cargo test -p server --test staging_proof`
- Expect: passing (t1).
- Check: full gate set still green (t3).

### verify-script: staging send (t2)

1. Set `JJHUB_EMAIL_PROVIDER=smtp` and `JJHUB_SMTP_*` to a staging
   relay; start the server (`cargo run`).
2. Sign up a fresh test account; open the verify page; resend.
3. Suspend the account to trigger a moderation notice.
4. Open `/admin` → **Recent deliveries**: all three sends show
   `ok` with no failure reason.
5. Record evidence here (outcome lines or screenshot) and check the
   item off the manual QA queue.

## Evidence

Operator staging run on the live relay (reported by the operator,
2026-11-07):

- Signup verification send — `ok`
- Resend to the real account — `ok`
- Resend to an `@example.com` address — refused by the relay
  (`server refused mail`), neutral retry shown, no enumeration —
  expected negative case, not a failure
- Suspend / unsuspend moderation notices — `ok`
- Verification links absolute after PR #113
  (`http(s)://host/verify/<token>`) — the bare-path link defect this
  procedure guards against is fixed
- Operator confirmed the emails arrived; the verification mail first
  landed in spam. Deliverability stays the operator DNS job
  (SPF/DKIM/DMARC, README § Mail delivery).

Manual QA queue: item confirmed off with the operator run above.

## Out of scope

- Choosing or enabling a live SMTP default (deferred phase gate).
- Queueing, backoff, suppression, drains, provider profiles.
- Changes to README procedure text (e36s01 shipped it).

## Risks

- Relay credentials unavailable → t2 stays pending; everything else
  can ship.
- Docs-assert test is brittle if the README heading is reworded —
  assert the heading plus anchors, not the full paragraph.
