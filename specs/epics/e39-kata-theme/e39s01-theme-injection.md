# Story: e39s01 — theme injection

## Story

As a visitor following a review link, I want the kata review UI to
wear jjhub's design system — same palette, type, and chrome — so I
never feel like I left the product.

## Business value

Review links already land at jjhub's origin (`/r/...` through the
auth proxy), but the pinned Svelte bundle renders its own chrome:
foreign fonts, palette, header. Option 1 (user decision,
2026-11-07) was chosen first because the theme is wanted no matter
what — if the iframe shell (option 3) lands later, a themed inner
view still helps. The seam already exists: `spa_route` forwards
every document request, so one transform + one stylesheet restyles
the app with zero fork and zero Svelte in jjhub. The commit pin
freezes the DOM, so selector overrides are re-checked only when the
pin moves.

## Type

Feature

## Risk context

The transform must touch `text/html` responses only: `/api/...`
JSON and `/assets/...` bytes pass through untouched (asserted
byte-identical), or the SPA breaks. The theme stylesheet follows
the `app.css` precedent — its own static route registered ahead of
the `/assets/{*rest}` forwarder. Selector overrides are only as
stable as the pin (`Dockerfile.kata`, enforced by
`scripts/check-kata-pin.sh`); a bump re-runs the probe (t1). The
stylesheet is user-visible CSS: ux_audit strict gates it (e39s02).

## Requirements delta

- **Before:** proxied HTML responses pass through unmodified; the
  kata bundle's own chrome is the only styling a visitor sees.
- **After:** proxied `text/html` responses carry an injected
  `<link>` to `/assets/kata-theme.css`; that stylesheet is served
  by jjhub, built from DESIGN.md tokens plus the probe's selector
  overrides; API and asset pass-through stay byte-identical.

## Steps

1. `t1` — Probe: bring the stack up, fetch the pinned bundle's
   HTML and CSS through the proxy seam, inventory its theming
   surface (CSS custom properties, palette, font stack, key
   chrome selectors) into this story's Evidence. The inventory is
   the input to the stylesheet.
2. `t2` — RED: extend `crates/server/tests/origin_spa.rs` —
   proxied HTML contains the injected theme link; `/api/` JSON and
   `/assets/` bytes stay identical to upstream. Fails today (no
   injection) — that is the RED.
3. `t3` — GREEN: `spa_route` injects the link into `text/html`
   responses only; add the `kata-theme.css` route (static, ahead
   of the assets forwarder) serving the stylesheet; write the
   stylesheet from DESIGN.md tokens + the probe overrides.

## Verification Script

- Run: `cargo test -p server --test origin_spa`
- Expect: failing at t2 (no injection exists), passing after t3;
  proxy suites (review_proxy, origin_spa, mr_review_link) green.
- Check: injection appears only on HTML content types — a test
  asserts byte-identical API/asset pass-through.
- Done when: the review page wears jjhub's tokens and every
  non-HTML response is untouched.

## Out of scope

- Header/nav injection (option 2) and the iframe shell
  option 3) — escalations decided after this lands.
- Forking kata or changing its source; no upstream PRs (user
  decision).
- Restyling kata's inner views beyond what token + selector
  overrides reach — the probe records the ceiling.

## Risks

- Selector overrides are a maintenance tail against upstream
  DOM drift — mitigated by the pin; a bump re-runs t1's probe.
- Injecting into the wrong content type would break the SPA —
  guarded by byte-identical assertions in t2.

## Evidence

### t1 — theming-surface inventory (probe 2026-11-07)

Stack up via `docker compose up -d`; probe user `probe@example.com`,
public repo `probe/probe`; fetched `GET /r/probe__probe/browse` and
`GET /assets/index-DRn4R2rA.css` (94,460 bytes) through jjhub's
proxy seam. Pin: `9c7903dd458160a7b91eb1e43d2a35e11731ccf8`.

**Pinned bundle HTML** (verbatim, the injection target):

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <link rel="icon" type="image/svg+xml" href="/favicon.svg" />
    <title>Kata</title>
    <script type="module" crossorigin src="/assets/index-BAJGxJY3.js"></script>
    <link rel="stylesheet" crossorigin href="/assets/index-DRn4R2rA.css">
  </head>
  <body>
    <div id="app"></div>
  </body>
</html>
```

**Theming surface — the seam is custom properties, not selectors.**
Almost all chrome reads `var(--*)`; palette lives in exactly two
blocks:

1. `:root { … }` — light palette + base type:
   `--bg:#fff --bg-panel:#f6f8fa --bg-elevated:#eef0f3
   --border:#d0d7de --border-muted:#eef0f3 --text:#1f2328
   --text-muted:#57606a --text-faint:#8c959f --link:#0969da
   --link-bg:#ddf4ff --on-accent:#fff --app-header-h:48px`,
   plus diff/status tints (`--add-* #e6ffec/#ccffd8/#abf2bc/#1a7f37`,
   `--remove-* #ffebe9/#ffd7d5/#fdb8c0/#82071e`, `--warn-*`,
   `--attention-*`, `--error-*`, `--success-*`, `--question-*`),
   id chips (`--change-id-* #8250df/#f3e8ff`, `--commit-id-*
   #0969da/#ddf4ff`), `--selection-*`. Base type:
   `font-family:ui-sans-serif,system-ui,sans-serif; font-size:14px;
   line-height:1.45`.
2. `@media (prefers-color-scheme: dark) { :root { … } }` — dark
   palette: `--bg:#0d1117 --bg-panel:#161b22 --bg-elevated:#21262d
   --border:#30363d --text:#e6edf3 --text-muted:#7d8590
   --link:#58a6ff …` (same variable names, GitHub-dark values).

A third `:root { --app-header-h:44px; font-size:13px }` sits inside
a `@media (width<=640px)` — mobile density, keep.

**Font stack:** body sans `ui-sans-serif,system-ui,sans-serif`;
code/mono hardcoded per component as
`ui-monospace,SFMono-Regular,Menlo,monospace` (12–13px) and
`var(--mono,monospace)`. No webfont — the bundle ships no @font-face.

**Key chrome selectors (all var-driven):**

- `header.app` — sticky, `background:var(--bg-panel)`,
  `border-bottom:1px solid var(--border)`, `.header-row.primary`
  height 48px, h1 16px/600.
- `button{background:var(--bg-panel);border:1px solid var(--border);
  border-radius:6px;padding:6px 12px;color:var(--text)}`,
  `button:disabled{opacity:.5}`, `button.primary:hover{filter:brightness(1.08)}`.
- `.file-header` / `.viewer-header` / `.detail-header` — all
  `var(--bg-elevated|bg-panel)` + `1px solid var(--border)`.
- `input` — `border:1px solid var(--border);background:var(--bg)`.
- `body{color:var(--text);background:var(--bg);margin:0}`.

**Ceiling / constraints recorded:**

- Svelte scope hashes (`body.svelte-9wdxx7`, `.file-header:where(...)`)
  make per-class overrides pin-fragile — the stable seam is the
  `:root` variable block, so the theme stylesheet restyles by
  overriding variables, not by chasing scoped selectors.
- Injection must land **after** kata's own stylesheet link (same
  `:root` specificity, later sheet wins).
- No `@font-face` in the bundle: adopting IBM Plex means jjhub's own
  font loading; the theme stylesheet declares the family and lets a
  missing webfont fall back to system sans (font loading is out of
  scope for e39s01 — selector/variable overrides only).
