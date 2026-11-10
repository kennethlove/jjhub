# Story: e40s02 — chrome slot

## Story

As a jjhub operator, I want the kata layout to account for the
site chrome above it, so the proxy can inject jjhub's nav without
fighting kata's sticky header and viewport math.

## Business value

Kata's chrome geometry lives in four known places (recon
2026-11-09): `header.app` sticky `top:0` (`web/src/app.css:141`),
panel heights `calc(100vh - var(--app-header-h))`
(`ReviewViewer.svelte:4050`, `BrowseViewer.svelte:685`), one more
calc in `ReviewViewer.svelte:4191`, and overlay tops
`calc(var(--app-header-h) + 16px)`. Everything else already reads
the `--app-header-h` variable, so the whole slot is one new
variable: every outer-top offset gains `var(--jj-chrome-h, 0px)`.
Default `0` keeps the page pixel-identical until s03 injects the
nav — this story ships the knob, not the nav.

## Type

Feature

## Risk context

Layout-only, zero behavioral change at default (heuristic P3 for
the patch itself; tracked P2 because it changes what ships in the
product bundle). The pin moves from upstream `9c7903d` to the
fork's own commit — the single-literal pin invariant must hold
before and after. Auto-hide logic (`--app-header-h` / `header-hidden`,
`scrollAnchor.ts`) is untouched: `--jj-chrome-h` is independent of
kata's own header state.

## Requirements delta

#### ADDED: kata layout offset variable
**Before:** kata assumes it owns the full viewport — `header.app`
sticks at `top:0`, panels fill `100vh - var(--app-header-h)`.
**After:** every outer-top offset in the fork adds
`var(--jj-chrome-h, 0px)`; with the variable unset the rendered
page is unchanged; with the variable set (s03) the whole page
sits below the injected site header.

#### MODIFIED: kata build sources
**Before:** pin `9c7903d` = upstream `martint/kata` main tip.
**After:** pin = fork-owned commit on `kennethlove/kata` main
containing the slot patch; pin stays one 40-hex literal.

## Steps

1. `t1` — Patch the fork's `main` (or patch branch): `header.app`
   sticky `top: var(--jj-chrome-h, 0px)`; `BrowseViewer`/`ReviewViewer`
   height calcs and overlay tops gain `+ var(--jj-chrome-h, 0px)`
   (or `calc(100vh - var(--app-header-h) - var(--jj-chrome-h, 0px))`).
   Commit and push to the fork → verify: `git ls-remote https://github.com/kennethlove/kata.git main`
   returns the new SHA and it differs from `9c7903d`.
2. `t2` — Set `KATA_COMMIT` to the fork SHA in
   `docker-compose.yml` (single literal; pin script + CI unchanged)
   → verify: `bash scripts/check-kata-pin.sh` (exit 0, no second literal).
3. `t3` — Rebuild and confirm default-0 behavior is unchanged
   (kata serves, existing origin_spa suite green) → verify:
   `docker compose up -d --build && sleep 10 && curl -s -o /dev/null -w '%{http_code}\n' http://localhost:3000/healthz`
   and `DATABASE_URL=postgres://postgres:jjhub@localhost:5432/jjhub_test cargo test -p server --test origin_spa`.

## Verification Script

1. Fork main moved and is a fork: `gh api repos/kennethlove/kata --jq .default_branch` plus `git ls-remote https://github.com/kennethlove/kata.git main`
2. Pin script passes: `bash scripts/check-kata-pin.sh`
3. Stack serves at the new pin with `--jj-chrome-h` unset and
   behavior unchanged: `curl` a `/r/...` page + origin_spa suite
4. Done when: suite green and pin script green.

## Out of scope

- Injecting nav or the variable (that is e40s03).
- kata's own header auto-hide behavior (upstream, untouched).
- `/review/*` proxy (still the 404 stub).

## Risks

- Patch misses a fixed-position overlay → detect: s03 captures
  show kata overlay under the site header; fix: add offset there.
- Pin moves but fork `main` rewritten later → detect: pin script
  / CI test still guards the single literal; builds are SHA-pinned.

## Evidence

- t1 2026-11-10: fork patch on branch `feat/e40-chrome-slot` (local
  commit direct to main was correctly blocked by the guard — branch+PR
  route), `feat(web): add --jj-chrome-h offset for an outer site chrome`
  (7960f1d), PR kennethlove/kata#1 merged -> fork main
  `10b15bd75752a190b71db9881b5eb38628a23c53`. Patch = 10 sites / 4
  files (count-asserted python replace): app.css header.app top,
  ReviewViewer x7 (overlay tops, heights, inset), BrowseViewer height,
  FileDiff top — all `var(--jj-chrome-h, 0px)`. Pin bumped in
  Dockerfile.kata single literal (spec said compose; the pin's one home
  is Dockerfile.kata per scripts/check-kata-pin.sh) ->
  `OK: kata pin single-sourced in Dockerfile.kata (10b15bd7...)`;
  `! ls-remote | rg 9c7903d` passes (fork moved off upstream).
- t2 2026-11-10: build BUILD_RC=0, log shows
  `git clone ".../kennethlove/kata.git" && git checkout 10b15bd75752a190b71db9881b5eb38628a23c53`;
  container force-recreated, healthz=200, review HTML=200;
  `cargo test -p server --test origin_spa` SUITE_RC=0,
  `7 passed; 0 failed` — default-0 behavior unchanged.
