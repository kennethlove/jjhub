# Story: e38s01 — product page

## Story

As a self-hoster evaluating jjhub, I want the logged-out home page to
tell me what the product is, what already ships, and how to run it
myself, so I can decide whether to deploy it — without being pushed
toward an account.

## Business value

VISION_LATEST names solo self-hosters as the MVP target, but the
current landing (e23s01) pitches the jj workflow to developers who
already use jj. An operator evaluating the forge needs the operator
story first: self-hosted, what ships (mail that delivers, moderation,
rate limits, delivery readout), one-command deploy — the shipped
facts from CHANGELOG v0.8.0 and README. The copy is drafted from
those facts and reviewed by the user before any code (session
2026-11-07).

## Type

Feature

## Risk context

`home.html` is one template with two branches: the logged-in
dashboard (must not regress) and the logged-out landing (the
rewrite target). The page is built from DESIGN.md tokens only —
ux_audit strict is the blocking gate — and must hold at 390px with
no horizontal scroll. Existing suites that assert current landing
content must be updated in the same change, never loosened away.

## Requirements delta

- **Before:** logged-out `/` renders the e23s01 landing — hero
  "A forge built around Jujutsu", why rows, stack band,
  self-hosting, status/roadmap, footer — with whatever auth
  affordances the shared nav carries.
- **After:** logged-out `/` renders the self-hoster product page
  (copy below); exactly one auth affordance (footer /login,
  e38s02); no signup/register/push CTA anywhere; the logged-in
  dashboard branch is unchanged.

## Copy draft (reviewed input)

- **Hero:** "Run your own forge, built around Jujutsu." — jjhub is
  self-hosted repository hosting for jj (and git), merge reviews
  anchored to change-ids, surviving every history rewrite.
  CTAs: `Run it yourself` (in-page anchor to deploy) ·
  `View on GitHub`.
- **What ships today (v0.8.0 facts):** change-anchored merge
  reviews (MR follows the change, edit-in-place revisions, stacks
  from the graph); mail that delivers (HTML + plain-text pair,
  per-send readout on /admin, clear retry); moderation built in
  (suspend/unsuspend, takedown/restore, reason quoted, audit CSV);
  rate limits and SSH transport; git smart HTTP alongside jj.
- **Deploy:** one `docker compose` up — jjhub + Postgres;
  configuration via env (SMTP, rate limits, operator handle);
  README §Staging proof walks the first live send.
- **Footer:** quiet `Log in` link only.

## Steps

1. `t1` — RED: new `crates/server/tests/product_home.rs`.
   Asserts a logged-out `GET /` contains the approved-copy
   markers (hero headline, shipped-features headings, deploy
   pointer) and that a session-carrying `GET /` still returns the
   dashboard. Fails against today's landing — that is the RED.
2. `t2` — GREEN: rewrite the logged-out branch of
   `crates/server/templates/home.html` from the copy draft above —
   tokens/DESIGN.md only, no off-system values, no account CTAs
   (e38s02 covers the auth assertions), dashboard branch
   untouched. Update any existing suites that assert the old
   landing wording to the new markers.
3. `t3` — Gates: `cargo fmt --all -- --check`, `cargo clippy
   --workspace --all-targets -- -D warnings`, full `cargo test`,
   `cargo build`; ux_audit strict on `app.css` (e38s03 records it).

## Verification Script

- Run: `cargo test -p server --test product_home`
- Expect: failing at t1 (old landing lacks the markers), passing
  after t2; every other suite stays green.
- Check: `git diff` touches only the logged-out branch of
  home.html, the new test file, and suites asserting old landing
  text.
- Done when: a logged-out visitor sees the self-host story and the
  full gate set is green.

## Out of scope

- Any change to `/login`, `/register`, the shared nav on other
  pages, or the logged-in dashboard (e38s02 owns the auth
  surface rules for this page).
- A separate `/product` route or static HTML (rejected: same Tera
  app, replace `/`).
- Copy beyond the draft — no invented claims (review gate).

## Risks

- Shared template: a careless edit regresses the dashboard branch —
  the logged-in assertion in t1 guards it.
- Existing landing tests assert old wording; updating them in the
  same commit keeps the suite honest instead of deleting coverage.
