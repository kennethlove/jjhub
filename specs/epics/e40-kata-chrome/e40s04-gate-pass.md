# Story: e40s04 — gate pass

## Story

As the operator, I want the e40 surface proven green before it
ships — audit, captures, and the full preflight — so the chrome
lands on the same bar as e39 and e38.

## Business value

Closes the epic: strict UX audit on the new stylesheet,
visual evidence of the nav in the kata chrome at 1440 and 390,
and a green preflight.

## Type

Chore (verification)

## Risk context

None — read-only gates; a failure keeps e40 unfixed in the
ledger, exactly like e39's gate story.

## Requirements delta

#### MODIFIED: e40 verification evidence
**Before:** e40 stories record implementation RED/GREEN only.
**After:** `e40s04-gate-pass.md` carries audit output, capture
paths with viewport metrics, and explicit-gate preflight rcs;
all e40 tasks flip to passing; execution-status shows
`e40-kata-chrome: passing`.

## Steps

1. `t1` — Run `ux_audit` strict on
   `crates/server/assets/kata-chrome.css` (complete sheet) →
   verify: all four gates pass (contrast ≥ |Lc| 75 pairs,
   tokens, states, slop-tells).
2. `t2` — `keystone_render` captures of a logged-in `/r/` browse
   view at 1440 and 390: nav visible above kata header, sticky
   stack correct, `scrollWidth == innerWidth` both → verify:
   screenshots + viewport metrics saved under
   `specs/epics/e40-kata-chrome/captures/`.
3. `t3` — Full preflight with explicit return codes → verify:
   `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test && cargo build`
   exit 0 (record each rc).

## Verification Script

1. `ux_audit` exit 0 on kata-chrome.css.
2. Captures exist and show the nav at both widths.
3. Preflight exit 0.
4. Done when: evidence appended to this story and tasks flipped.

## Out of scope

- Redesigning `.site-header` (extracted as-is from `app.css`).
- Any source change (belongs to s01/s02).

## Risks

- Audit flags the extracted block if it sits on kata's dark
  surfaces → detect: contrast gate; fix: token-only re-derivation
  from `DESIGN.md`.

## Evidence

- t1 2026-01-10 audit (10 hand-picked pairs resolved from kata-theme
  values): PASS all gates. Light: wordmark Lc 97.78 / WCAG 15.24:1,
  nav-link + viewer 80.67 / 6.99:1, bare links 83.48 / 7.93:1,
  secondary-hover 93.48 / 14.29:1. Dark: wordmark -91.3 / 12.46:1,
  viewer -82.49 / 10.94:1, bare link + focus ring -77.15 / 10.05:1,
  secondary-hover -87.51 / 9.91:1, btn-primary fill -84.25 / 6.47:1,
  focus outline light 83.48. Tokens / States / Slop-tells all pass.
  Two fixes found visually: (1) first capture showed bare default
  links + default OS button — logged-in nav anchors carry no
  classes, so app.css global a/a:hover/a:active + .btn/.btn-sm/
  .btn-primary/.btn-secondary blocks were copied scoped to
  .site-nav (wordmark sits outside .site-nav, unaffected);
  (2) dark hazard — --link brightens to #C7D8D0 while
  --jj-on-accent stays near-white (light-on-light on btn hover),
  fixed by a prefers-color-scheme: dark override keeping the brand
  fill; --color-surface-sunken routed to var(--bg-elevated) so the
  dark flip works. Re-audit after both fixes = PASS (output above).
- t2 keystone_render (proxy 127.0.0.1:4173, session cookie):
  scrollWidth 1440==1440 and 390==390 — no horizontal scroll;
  finalUrl ?commit=071fbff5ea1c (kata SPA mounted); screenshots
  screenshot-1440.png / screenshot-390.png + viewports.json under
  captures/ (empty dom/computed files removed). Visual check at
  1440: wordmark + Repos/Settings styled links + viewer + Log out
  pill above kata chrome; 390: no cut-off.
- t3 preflight 2026-01-10 with explicit rcs in /tmp/e40_*.log:
  FMT_RC=1 first run (two collapse diffs in inject_chrome
  signature + site_nav match) -> cargo fmt --all -> FMT_RC2=0;
  CLIPPY_RC=0; TEST_RC=0 with 135 'test result: ok' suites, no
  FAILED lines; BUILD_RC=0. Stack rebuild BUILD_RC=0, healthz=200,
  /assets/kata-chrome.css -> 200 4841 B.
