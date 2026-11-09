# Story: e38s03 — gate pass

## Story

As a maintainer, I want the product page proven against the
design gates before handoff — ux_audit strict, a 390px viewport
with no horizontal scroll, and a green preflight — so a
marketing page can never be the change that breaks the system.

## Business value

UX discipline is active at strict level in this repo: ux_audit is
the blocking authority for any screen, and a failed audit keeps the
work in draft. The product page is the first page most visitors
see; contrast, token coverage, interaction states, and mobile
overflow are the cheapest regressions to prevent and the most
visible when missed.

## Type

Verification

## Risk context

The audit runs against `crates/server/assets/app.css` (the page
styles live there with everything else); a 390px check needs a real
render, not a stylesheet read. Evidence belongs in this story's
Evidence section — unrecorded gates are not gates.

## Requirements delta

- **Before:** no gate evidence exists for the product page (work
  not yet built).
- **After:** ux_audit strict passes, the 390px capture shows no
  horizontal scroll, preflight is green — all three recorded
  here as Evidence.

## Steps

1. `t1` — ux_audit strict on `crates/server/assets/app.css`;
   paste the full result into Evidence. Any fail keeps the page
   in draft until fixed and re-audited.
2. `t2` — Render the logged-out `/` at 390px
   (keystone_render or headless capture); confirm no horizontal
   scroll, no cut-off text; paste the capture result into
   Evidence.
3. `t3` — Full preflight:
   `cargo fmt --all -- --check && cargo clippy --workspace
   --all-targets -- -D warnings && cargo test --workspace &&
   cargo build`; paste the suite count into Evidence.

## Verification Script

- Run: `verify-script: ux_audit (strict) on app.css; 390px
  capture of /; preflight command from t3`
- Expect: audit PASS, no horizontal scroll at 390px, preflight
  exit 0 — with all three outputs pasted into this story's
  Evidence section.
- Done when: Evidence section holds the three results and no
  handoff proceeds without them.

## Out of scope

- Visual redesign beyond the approved copy and token system.
- Audits of pages other than the product page.

## Risks

- The stylesheet is shared: an audit fail may implicate an
  existing rule — fix in the same draft loop, re-run the audit,
  never waive it.

## Evidence

### t1 — ux_audit strict on `crates/server/assets/app.css` (2026-11-09)

Full result: **UX AUDIT PASSED** — Contrast (APCA) all 12 auto-extracted pairs pass
(Lc -93.46 to +87.91, every pair |Lc| >= 75; WCAG sidecar 10.05:1 to 14.79:1,
min 4.5), Tokens pass, States pass, Slop tells pass. No edits were needed:
the product page uses only existing `.hero/.section/.ruled-list/.step-row/.btn`
rules, all token-driven.

### t1 — 390px + 1440px render of logged-out `/` (2026-11-09)

`keystone_render url=http://localhost:3000/ viewports [1440, 390]` against the
rebuilt stack (jjhub-app-1, health 200):

- 1440: scrollWidth 1440 == innerWidth 1440 — no horizontal scroll.
- 390: scrollWidth 390 == innerWidth 390 — no horizontal scroll.
- Screenshots: `captures/screenshot-1440.png`, `captures/screenshot-390.png`.
- 1440 screenshot check: nav (wordmark + What ships + Deploy, no auth CTAs),
  hero headline + both CTAs, what-ships ruled rows — hierarchy clean, no
  cut-off text. 390 check: wordmark + hero fully visible, no horizontal
  overflow. `.site-nav` text links hidden <=640 is pre-existing app.css
  (`.site-nav .nav-link { display: none; }`), not a regression; the footer
  `/login` stacks into the small-screen footer.

### t2 — full preflight (2026-11-09, final tree after `cargo fmt`)

```
FMT=0 CLIPPY=0 TEST=0 BUILD=0
134 test result: ok suites, 0 FAILED
clippy: No issues found
```

(`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace` with `DATABASE_URL=...jjhub_test`, `cargo build` —
explicit exit codes captured per run.)
