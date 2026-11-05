# e35s01 — monthly JSONL rollups at the single write points

## 1. Story

As an operator, every mail send and every moderation action
lands as one timestamp-first JSON object per line in a
month-named file, written at the one place each event already
passes through — so history outlives the table, a month
boundary needs no cron, and a future log drain can attach
without a second write path.

## 2. Business value

`moderation_actions` is the working copy; the durable,
unbounded history is files — this gives v0.7.0 its retention
half and the drain-ready construction MAIL-LATEST requires —
SCOPE v0.7.0 in_scope e35 (monthly JSONL, single write points,
rotation by write date).

## 3. Type

feat

## 4. Risk / context

risk: P2 — two rules hold the design: one event = one line at
one call site (Mailer::send for mail, the moderation INSERT
sites for actions), and rotation is the event's write date, not
a scheduler. Lines must never carry a secret: fingerprints and
hashes only (the 1695cf1 rule), which the writer enforces by
taking structured fields, not free-form bodies.

## 5. Steps

1. RED cases in `crates/server/tests/jsonl_retention.rs`: a
   recorded send appends exactly one line to
   `mail_log_YYYY-MM.jsonl` under `JJHUB_LOG_DIR` (tempdir);
   a moderation action appends one line to
   `moderation_log_YYYY-MM.jsonl`; each line parses as JSON
   with `ts` first and the event's own keys; two events in the
   same month share a file while a different month lands in a
   different file (rotation by write date, no timer); with
   `JJHUB_LOG_DRAIN` unset only the file is written → verify:
   `cargo test -p server --test jsonl_retention` (expected
   RED: no writer, no hooks)
2. GREEN: one jsonl writer (append, structured fields, ts
   first) hooked at the Mailer::send call site and the four
   moderation INSERT sites, `JJHUB_LOG_DIR` config with an
   explicit default → verify: `cargo test -p server --test
   jsonl_retention`
3. Full gates → verify: `cargo fmt --all --check && cargo
   clippy --workspace --all-targets -- -D warnings && cargo
   test && cargo build`

## Verification Script

`cargo test -p server --test jsonl_retention` proves one line
per event at one call site, month-named files from the write
date, and parseable timestamp-first objects; full preflight
proves no regression.

## Out of scope

- The drain itself (`JJHUB_LOG_DRAIN` ships later — the single
  write point is the readiness), log-service integrations, any
  admin UI over the files, pruning old months (retention
  config arrives with real drain demand).

## Risks

- Writing a line at a second place "for convenience" would
  double-count history; the suite asserts exactly one line per
  event so the single-write-point rule stays enforced.
