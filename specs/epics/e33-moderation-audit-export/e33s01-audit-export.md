# e33s01 — filterable moderation log on /admin with CSV export

## 1. Story

As a public-node operator, the /admin surface shows every
moderation_actions row newest-first — who did what to whom and
when — filterable by action and target, downloadable as CSV for
an incident write-up or a dispute.

## 2. Business value

e29 writes an audit trail with no reader: a table nobody can
review is a write-only promise. This story gives the trail its
reader — incident review, "when did I do that", appeal
evidence — and the export makes it portable. SCOPE v0.6.0
in_scope e33.

## 3. Type

feat

## 4. Risk / context

risk: P1 — moderation rows can name victims and abusers, so
the whole surface stays operator-gated (404 for everyone else,
including the export endpoint) behind the same gate e29 built.
CSV values must be escaped (a reason containing `"` or a comma
is user-influenced input — no CSV injection). The export covers
the table, not a rolling window; monthly JSONL retention rolls
with the mail epic (v0.7.0).

## 5. Steps

1. RED cases in `crates/server/tests/moderation_log.rs`: after
   a suspend row exists, GET /admin as operator lists it with
   actor/target/action/time; `?action=suspend` and
   `?target=<handle>` filter it; GET /admin/moderation.csv
   returns text/csv with a header row and the same selection;
   non-operator gets 404 on both → verify: `cargo test -p
   server --test moderation_log` (expected RED: 404 — no
   endpoints)
2. GREEN: log query (newest-first, action/target filters),
   /admin section listing rows in a table on DESIGN.md tokens,
   CSV endpoint with proper content-type/disposition and
   quoted fields → verify: `cargo test -p server --
   test moderation_log`
3. Empty state: no rows yet renders an explicit empty state,
   not a blank panel → verify: same suite
4. Full gates → verify: `cargo fmt --all --check && cargo
   clippy --workspace --all-targets -- -D warnings && cargo
   test && cargo build`

## Verification Script

`cargo test -p server --test moderation_log` proves the list,
both filters, the CSV shape with header, the empty state, and
both 404s; ux_audit + CDP probe cover the populated panel at
1440 and 390.

## Out of scope

- Monthly JSONL rollup and log drains (v0.7.0), editing or
  deleting rows (the trail is append-only), non-operator views.

## Risks

- Putting the CSV route outside the operator gate would leak
  the whole abuse history — the suite asserts the 404 on the
  export endpoint itself, not just the page.
