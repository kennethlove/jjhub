# Story: e39s02 — gate pass

## Story

As a maintainer, I want the injected theme proven against the
design gates before handoff — ux_audit strict on the theme
stylesheet, captures showing jjhub's palette and type on the real
review page, and a green preflight — so a proxy transform can
never ship blind.

## Business value

The theme is user-visible CSS written against a third-party DOM:
contrast, token coverage, and mobile overflow are exactly where
injection work regresses. Evidence in the story is the gate —
unrecorded checks are not checks (same rule as e38s03).

## Type

Verification

## Risk context

`kata-theme.css` shares the repo's token discipline but is audited
on its own; a fail may also implicate an existing token pair —
fix and re-audit, never waive. Captures must run against the live
proxied page (real bundle, real injection), not a local mock.

## Requirements delta

- **Before:** no gate evidence exists for the injected theme
  (work not yet built).
- **After:** ux_audit strict passes on `kata-theme.css`, desktop
  and 390px captures show the jjhub palette/type with no
  horizontal scroll, preflight is green — all recorded here as
  Evidence.

## Steps

1. `t1` — ux_audit strict on `crates/server/assets/kata-theme.css`;
   paste the full result into Evidence.
2. `t2` — Capture the themed `/r/` page at 1440px and 390px;
   confirm palette/type render and no horizontal scroll; paste
   both results into Evidence.
3. `t3` — Full preflight:
   `cargo fmt --all -- --check && cargo clippy --workspace
   --all-targets -- -D warnings && cargo test --workspace &&
   cargo build`; paste the suite count into Evidence.

## Verification Script

- Run: `verify-script: ux_audit (strict) on kata-theme.css;
  captures at 1440/390 of the proxied review page; preflight
  from t3`
- Expect: audit PASS, captures show themed chrome with no
  horizontal scroll, preflight exit 0 — outputs pasted into
  Evidence.
- Done when: Evidence holds all three results.

## Out of scope

- Restyling beyond the token layer (probe records the ceiling).
- Audits of pages other than the themed review view.

## Risks

- A themed-but-unreadable capture (low contrast on kata's
  surfaces) is a fail — fix in the theme file, re-capture, never
  hand off on "close enough."

## Evidence

### e39s02t1 — ux_audit strict PASS (2026-09-27)

`ux_audit` on `crates/server/assets/kata-theme.css` with 28
computed pairs (light + dark arms, pairs resolved from the
stylesheet's own color-mix values by script):

```
✅ UX AUDIT PASSED

✓ Contrast (APCA)  28/28 pairs pass (body min Lc 75)
   light: body 101.52 · muted 80.67 · faint 76.38 · link 87.22/79.18
          add 77.24 · remove 76.94 · warn/attention 76.38 · error 75.64
          success 77.24 · question 78.59/79.12 · chips 77.24/79.18
   dark:  body -91.30 · muted -80.37 · faint -77.46 · link -77.15/-75.68
          add -75.68 · remove -76.83 · warn/attention -77.11 · error -76.74
          success/question -75.68 · chips -75.68
   WCAG sidecar: all pairs ≥ 5.68:1 (most ≥ 7:1)
✓ Tokens (no hardcoded hex outside :root, no ad-hoc shadows)
✓ States (:focus-visible / :disabled / reduced-motion present)
✓ Slop tells (no glassmorphism, glow, default-card, gray borders)
```

Iterations: 4 balance rounds. Light remove bg raised 8%→5% tint;
dark arm rebalanced (text mixes 15–20% tertiary/error over
on-inverse, tinted bgs 10–14%) until every pair cleared |Lc| 75.


### e39s02t2 — captures at 1440 and 390 PASS (2026-09-27)

Authenticated capture harness: headless renderers carry no cookies,
so a local proxy (`127.0.0.1:4173`) forwards to the stack on
`:3000` with the logged-in session cookie injected. Rendered
`/r/probe__probe/browse` through `keystone_render` at both
viewports (stack rebuilt first so `spa_route` injects the theme).

```
1440px: scrollWidth 1440 == innerWidth 1440  (no horizontal scroll)
  390px: scrollWidth  390 == innerWidth  390  (no horizontal scroll)
  finalUrl carries ?commit=8ec330c1… — kata SPA mounted, not shell
  screenshots: captures/screenshot-1440.png, captures/screenshot-390.png
```

Visual check (both captures): jjhub palette renders — green
tertiary accents (Create review button, Reviews link, id chips),
tinted neutral panels (`--bg-panel`), token borders; no broken
chrome, no cut-off text at 390; header and sidebar collapse
cleanly on mobile. Note: keystone's DOM/computed JSON came back
empty for this SPA render; screenshots + viewport metrics carry
the evidence.

### e39s02t3 — full preflight PASS (2026-09-27)

```
cargo fmt --all -- --check        clean (one wrap fixed in origin_spa.rs)
cargo clippy --workspace --all-targets -- -D warnings
                                   No issues found
cargo test --workspace            460 passed (132 suites, 176.92s)
cargo build                       Finished dev profile
```

All three e39s02 gates green; story complete.
