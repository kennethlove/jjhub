# Story: e40s01 — fork and pin

## Story

As a jjhub operator, I want the kata build to source from our own
fork so the review UI's layout can be changed at the source
instead of worked around at the proxy.

## Business value

User decision 2026-11-09: fork `martint/kata` (Apache-2.0) and
change the chrome directly. Recon showed why this is cheap here:
`Dockerfile.kata` already exposes `ARG KATA_REPO`, the pin
(`KATA_COMMIT` shell default) is the single source of truth
(`scripts/check-kata-pin.sh` + its CI test read only that line),
and our pin equals upstream `main` tip — the fork starts at zero
delta. This story only flips the source; no behavior changes.

## Type

Feature

## Risk context

The fork must be public — the build clones anonymously. The pin
invariant must survive: exactly one 40-hex `KATA_COMMIT` literal,
in `Dockerfile.kata` only. If the fork's `main` ever drifts from
the pin, builds still use the pin (checkout by SHA) — drift is
detected in s02 when the pin moves to fork-owned commits.

## Requirements delta

#### MODIFIED: kata build sources
**Before:** `Dockerfile.kata` clones `https://github.com/martint/kata.git`
(only) at the pinned SHA.
**After:** compose build passes `KATA_REPO=https://github.com/kennethlove/kata.git`
to the same Dockerfile; pin mechanism, script, and CI test unchanged;
the fork's `main` starts identical to upstream `9c7903d`.

## Steps

1. `t1` — Create the public fork `kennethlove/kata` and push
   upstream `main` tip to it → verify: `gh api repos/kennethlove/kata --jq .fork && git ls-remote https://github.com/kennethlove/kata.git main | rg '^9c7903dd458160a7b91eb1e43d2a35e11731ccf8'`
2. `t2` — Pass `KATA_REPO` build-arg in `docker-compose.yml`,
   rebuild the stack, confirm kata still serves at the same pin →
   verify: `bash scripts/check-kata-pin.sh && docker compose up -d --build && sleep 10 && curl -s -o /dev/null -w '%{http_code}\n' http://localhost:3000/healthz`

## Verification Script

1. Fork exists and is a fork: `gh api repos/kennethlove/kata --jq .fork`
2. Fork main == upstream pin: `git ls-remote https://github.com/kennethlove/kata.git main`
3. Stack rebuilds from the fork with unchanged behavior:
   `docker compose up -d --build` then open any `/r/...` review page
4. Done when: pin script passes and the review page is unchanged.

## Out of scope

- Any source change in the fork (that is e40s02).
- Changing the pin value (stays `9c7903d` until s02's patch).
- Upstream PRs to `martint/kata` (optional follow-up, deferred).

## Risks

- Fork private → anonymous clone fails at build; detect: build
  error on `git clone`, fix: keep fork public.
- GitHub fork without updated `main` (fork initialized empty) →
  detect: `ls-remote` mismatch in t1.

## Evidence

- t1 2026-11-10: `gh repo fork martint/kata` ->
  `{"default_branch":"main","fork":true,"full_name":"kennethlove/kata","private":false}`;
  `git ls-remote .../kata.git main` = `9c7903dd458160a7b91eb1e43d2a35e11731ccf8`
  (== upstream tip).
- t2 2026-11-10: `docker compose config` resolves
  `KATA_REPO: https://github.com/kennethlove/kata.git` in the kata
  build args (compose edit adds the arg; pin mechanism untouched);
  `bash scripts/check-kata-pin.sh` -> `OK: kata pin single-sourced ...`
  (PIN_RC=0); `docker compose build` BUILD_RC=0 with clone line
  `RUN git clone "https://github.com/kennethlove/kata.git" /src && git checkout <pin>`;
  healthz=200, review HTML=200. Note: pin moved later by e40s02t1
  (10b15bd) — same script stays green (single literal).
