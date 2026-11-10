# Story: e40s03 — chrome injection

## Story

As a user browsing `/r/...` in a jjhub session, I want the site's
own header above the kata review UI — wordmark, Repos, Settings,
my handle, Log out — so the review page feels like one site.

## Business value

The e39 seam already proved the proxy is the right place: HTML
responses are transformed, API/assets stay byte-identical,
session-less requests redirect to `/login`. This story extends
that single buffer transform to prepend jjhub's rendered nav
before kata's `#app`, plus link the fork-chrome stylesheet that
declares `--jj-chrome-h`. The fork (s02) makes the offset math
honor it; nothing else in kata changes at runtime.

## Type

Feature

## Risk context

Session-only surface: the nav carries a CSRF token for the
existing logout form — no new endpoint, no new secret.
Byte-identical guards must survive (API/JSON and asset bodies).
The nav must not appear on non-HTML or unauthenticated responses.

## Requirements delta

#### ADDED: site chrome on proxied HTML
**Before:** `/r/*` text/html = kata shell (506-byte body with
`<div id="app">`) + injected theme link (e39); API/JSON and
assets byte-identical; no session → 303 `/login`.
**After:** the same text/html additionally carries
`<link rel="stylesheet" href="/assets/kata-chrome.css">` in head
and the rendered `nav.html` site-header markup immediately
before `<div id="app">` (fallback: before `</body>`), with
`--jj-chrome-h` pinned by that stylesheet; API/JSON and
`/assets/*` pass-through byte-identical; no session still 303
`/login`. Scope = `/r/*` only; `/api/`, `/assets/`, `/review/*`
untouched.

## Steps

1. `t1` — RED: extend `crates/server/tests/origin_spa.rs` mock
   body to contain `<div id="app"></div>`; assert session HTML
   contains `class="site-header"`, `--jj-chrome-h`, and a logout
   `_csrf`; byte-identical API/asset guards unchanged; no-session
   still 303 → verify: `DATABASE_URL=postgres://postgres:jjhub@localhost:5432/jjhub_test cargo test -p server --test origin_spa`
   (fails on the new assertion only).
2. `t2` — GREEN: `spa_route` single-buffer text/html transform
   keeps the e39 theme link, adds the `kata-chrome.css` link,
   prepends `render::render("nav.html")` with `user_handle` from
   the session cookie and `csrf = session::csrf_token(secret,
   cookie)` before `<div id="app">` (case-insensitive, fallback
   `</body>`); register `kata_chrome_css` route (OnceLock + etag,
   ahead of `/assets/{*rest}`) serving
   `crates/server/assets/kata-chrome.css` = `:root { --jj-chrome-h: <single pinned height> }` +
   the `.site-header` block extracted from `app.css` (WHY:
   linking whole `app.css` would bleed element selectors onto
   kata) → verify: same suite green.
3. `t3` — Stack acceptance: rebuild, logged-in `/r/...` HTML
   shows nav markup + both stylesheet links; session-less and
   API probes unchanged → verify: `curl -s http://localhost:3000/r/capture__accept6js/browse -b /tmp/capture.cookies | rg -c 'site-header|--jj-chrome-h|kata-chrome.css'`

## Verification Script

1. RED: `origin_spa` fails on `site-header` assertion, passes after GREEN.
2. Byte-identical guards (API JSON, asset bytes) still pass in the same suite.
3. Live: `curl -s http://localhost:3000/r/... -b <cookie> | rg 'site-header'`
4. Done when: suite green and live HTML shows the nav once.

## Out of scope

- Non-`/r/` routes (kata `/api/` JSON, `/assets/`, `/review/*`).
- Changing nav.html's markup for the dashboard (same template reused as-is).
- Style refactors of `.site-header` beyond extraction.

## Risks

- Nav markup differs from server-rendered context → detect:
  logout form 400s / missing handle; fix: pass the same context
  fields `base.html` passes (`user_handle`, `csrf`, unset
  `nav_section`).
- Double-injection if the transform runs twice → detect: test
  asserts one occurrence.

## Evidence

- t1 RED 2026-11-10: new proxied_html_carries_site_chrome in
  crates/server/tests/origin_spa.rs (mock HTML with div#app; asserts the
  e39 theme link survives, /assets/kata-chrome.css injected, exactly one
  site-header, nav precedes div#app, one logout _csrf; byte-identical
  API/asset + anon 303 guards re-proven by the same suite).
  Run: RED_RC=101, 7 passed; 1 failed, failure text: 'chrome stylesheet
  link injected: <!doctype html>...kata-theme.css...<body><div id=app>'.
- t2 GREEN: review_proxy.rs — inject_theme_link replaced by
  inject_chrome(state, headers, response): ONE buffer pass, two
  insertions (close-head+7 -> theme+chrome LINKS; div#app
  case-insensitive with </body> fallback -> routes::nav_html output),
  applied back-to-front via Vec::splice; find_ascii_case_insensitive
  helper; site_nav = session cookie -> verify -> handle query +
  csrf_token -> routes::nav_html (same seam as server pages);
  session_cookie helper extracted, session_user delegates (one parse).
  routes.rs += kata_chrome_css (OnceLock + etag from kata_theme_css);
  lib.rs route registered ahead of /assets/{*rest}. New
  assets/kata-chrome.css (2420 B): pin --jj-chrome-h: 48px (s02 slot)
  + extracted header block with token aliases resolving against kata's
  runtime vars (dark flip free) + focus-visible/disabled states +
  reduced-width rules. One fix mid-run: E0599 Vec<u8> has no
  insert_str -> splice(at..at, bytes). Run: GREEN_RC=0, 8 passed; 0 failed.
- t3 2026-11-10: docker compose -p jjhub build app BUILD_RC=0 (first
  attempt RC=1 was 'unsupported --progress value minimal' — flag error;
  the stale image behind it produced misleading 0-counts). healthz=200;
  anon /r/capture__accept6js/browse -> 303; logged-in HTML:
  site-header=1, kata-chrome.css=1, kata-theme.css=1, _csrf=1,
  nav-before-div#app True; /assets/kata-chrome.css -> 200 2420 B;
  /api/repos/... no session -> 303.

