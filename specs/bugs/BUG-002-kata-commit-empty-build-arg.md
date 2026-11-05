# BUG-2026-11-05T111326: Coolify deploy fails — kata build receives empty KATA_COMMIT

## Problem

Coolify deployment of `jjhub:trunk` fails at the kata image build:

```
#16 [kata source 2/4] RUN test -n "${KATA_COMMIT}" || { echo "KATA_COMMIT build-arg is required" >&2; exit 1; }
#16 0.448 KATA_COMMIT build-arg is required
target kata: failed to solve: ... exit code: 1
```

Expected: the kata sidecar builds at the exact commit pinned for the
release. Actual: the pin never reaches the build; the fail-closed guard
aborts the deploy.

`Security impact: NONE` — build-time configuration only; no exploit path.
The guard failing closed is correct behavior; the defect is that the pin
is absent from the build environment.

## Root Cause Analysis

Coolify invokes `docker compose build` with the build argument passed
bare (`--build-arg KATA_COMMIT`, no `=value`), so the value resolves from
Coolify's generated `build-time.env`. That file carries no pin for this
project (the variable was never configured on the platform; Coolify is
known to pass build-args that exist as empty values — coollabsio/coolify
#7506). The Dockerfile consumes the argument with no fallback at the use
site, so an empty value reaches the guard and the build aborts.

Reproduced the resolution matrix locally (docker 29.8.2, compose v5.5.0,
fixture `ARG FOO=dfault` + compose `args: FOO: ${FOO:-fromyamldefault}`):

| build-time.env | CLI flags | resolved value |
| --- | --- | --- |
| `FOO=` (empty) | bare `--build-arg FOO` | `[]` — **empty beats ARG default and yaml default** (production path) |
| missing `FOO` | bare `--build-arg FOO` | `[dfault]` — Dockerfile ARG default wins, yaml default ignored |
| missing `FOO` | no CLI flag | `[fromyamldefault]` — yaml default wins |

Contributing factors:

- The pin exists in exactly one place (the compose file's
  `${KATA_COMMIT:-…}` default), but Coolify's bare `--build-arg` CLI flag
  overrides compose's `args:` entry, so that default is unreachable on
  the platform.
- A Dockerfile `ARG KATA_COMMIT=<pin>` default alone would NOT fix the
  production path: an explicitly empty build-arg overrides ARG defaults
  (repro case A).
- CI extracts the pin from the compose file, so any second copy of the
  pin risks silent drift between what CI tests and what the deploy builds.

Risk level: Medium — every Coolify deploy fails until fixed; a naive
"add an ARG default" fix would look right and still fail.

Verified root cause (diagnose-root phase 4): empty/absent `KATA_COMMIT`
at the Dockerfile guard, with no fallback at the use site, under
Coolify's bare `--build-arg` invocation. Single cause; repro matrix +
Coolify issue #7506 + production log agree.

## TDD Fix Plan

Single source of truth: the pin becomes a shell default at the checkout
command in the Dockerfile — the only fallback that survives an explicitly
empty build-arg (repro case A). Consumers read the pin from there.

1. **RED**: Add `scripts/check-kata-pin.sh` asserting the invariant: the
   Dockerfile carries a `KATA_COMMIT:-<40-hex>` shell default, the
   compose file carries no second 40-hex KATA_COMMIT literal, and CI
   extracts the pin from the Dockerfile. Run it — fails today (no shell
   default; compose holds the literal).
   **verify**: `bash scripts/check-kata-pin.sh` (exit 1 → RED)

2. **GREEN**: Dockerfile.kata — drop the `test -n` guard (its job is
   taken by the fallback, which always yields the pin) and checkout with
   `"${KATA_COMMIT:-<pin>}"`; docker-compose.yml — keep the
   `KATA_COMMIT: ${KATA_COMMIT:-}` entry only as an override conduit,
   drop the literal; ci.yml — sed-extract the pin from Dockerfile.kata
   and key the kata rust-cache on `hashFiles('Dockerfile.kata')`.
   **verify**: `bash scripts/check-kata-pin.sh` (exit 0 → GREEN)

3. **Behavioral proof** (the deploy's actual claim): build the source
   stage with an explicitly empty build-arg and read back the checked-out
   commit.
   **verify**: `docker build --target source --build-arg KATA_COMMIT= -f Dockerfile.kata .`
   then `docker run --rm <image> git -C /src rev-parse HEAD` equals the
   pin.

**REFACTOR**: none expected — consumers already point at the pin's new
home.

## Acceptance Criteria

- [ ] `bash scripts/check-kata-pin.sh` passes
- [ ] Source stage builds with `--build-arg KATA_COMMIT=` (empty) and
      `/src` HEAD equals the pinned commit
- [ ] `KATA_COMMIT=<other>` override still reaches the build (compose
      entry retained)
- [ ] CI e2e job still checks out the pinned kata commit
- [ ] Preflight green (`fmt`, `clippy`, `test`, `build`)
- [ ] All new tests pass; existing tests still pass

## Resolution

<!-- filled in by validate-fix -->
