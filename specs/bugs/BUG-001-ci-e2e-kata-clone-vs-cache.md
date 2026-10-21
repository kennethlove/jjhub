---
bug_id: BUG-001
status: fixed
severity: high
scope: ci
title: e2e job fails — kata clone collides with rust-cache-restored workspace dir
---

# BUG-001: e2e kata clone collides with cache-restored workspace

## Symptom

PR #35 (docs-only) failed the `e2e` job in 51s while `preflight` passed.
Failing step: "Build kata sidecar at the compose-pinned commit".

    fatal: destination path '/home/runner/work/_temp/kata' already exists and is not an empty directory.
    ##[error]Process completed with exit code 128.

## RCA (4-phase)

1. **Reproduce** — CI run 39334703694, job 118341141346, e2e job, 51s fail;
   preflight job same run: pass (2m51s). Docs-only diff, no Rust touched.
2. **Isolate** — failure at `git clone ... "$RUNNER_TEMP/kata"` inside the
   build step; nothing in the diff touches `docker-compose.yml` or the
   workflow.
3. **Hypothesize** — the kata `Swatinem/rust-cache@v2` step
   (`workspaces: ${{ runner.temp }}/kata`) restores its cache *before* the
   clone step. On a hit the restore creates the workspace directory
   non-empty, and `git clone` refuses a non-empty destination.
4. **Verify** — log ordering proves it:

       22:24:58 Cache hit for: v0-rust-kata-...-da39a3ee
       22:25:08 Restored from cache key "...full match: true"
       22:25:10 fatal: destination path '.../kata' already exists ...

   PR #34 passed because the kata cache slot was empty on first run (no
   restore, dir absent, clone OK) and the job-end save populated it. Every
   subsequent run with a full cache hit fails — deterministic, not flaky.

## Fix approach

Reorder the e2e job: clone the pinned kata commit *first*, then let the
kata cache restore extract `target/` over the checkout, then build.
Restore-into-existing-dir merges cleanly; clone-into-restored-dir cannot.

## Verify steps

1. `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"` parses.
2. Structural: clone step appears before the kata `rust-cache` step in `ci.yml`.
3. PR #35 `e2e` check green after push (the real assertion).
