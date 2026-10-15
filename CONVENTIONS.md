# Conventions

All planning documents for this project live in `specs/` at the project root. Every skill writes its output there.

## Conventional Commits & Semantic Versioning

All changes to this repository MUST follow [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/). Versioning MUST follow [Semantic Versioning 2.0.0](https://semver.org/).

### Commit Message Format

`<type>(<scope>): <description>` (space after colon is MANDATORY). No emoji. No AI attribution footers (`Co-authored-by:* NEVER).

### Types & Version Bumps

- `feat`: Minor (x.Y.z) — new feature
- `fix`: Patch (x.y.Z) — bug fix
- `perf`: Patch (x.y.Z) — performance improvement
- `docs`, `chore`, `style`, `refactor`, `test`: no bump (unless breaking)
- `BREAKING CHANGE:` (or `!` after type): Major (X.y.z)

## Jujutsu & Git Operations

This repo is a colocated jj + git repo. Remote `origin` is GitHub.

- All work happens in a jj workspace on a `feature/*` or `fix/*` bookmark. NEVER edit in the primary checkout or on `trunk` directly.
- No direct work on `main` or `trunk`. Every task MUST start via `kickoff-branch` (jj workspace add).
- Prefer jj commands (`jj new`, `jj describe`, `jj bookmark`) for day-to-day work. Colocated git HEAD is detached — do not interpret it as branch state.
- Never merge a pull request on your own. The human merges.
- Never open a PR before formatting, linting, and tests all pass.
- Never create GitHub issues from automated workflows — use local Markdown files in `specs/` instead.
- Never call GitHub REST API directly (curl, fetch, etc.) — use `gh`.

## Agent Workflow Mandates

**AGENTS MUST NEVER BYPASS THE BIGPOWERS WORKFLOW.**

- **No Direct Coding:** Route all feature work through bigpowers skills. DO NOT write code directly in response to "build this feature".
- **Required Skills:** `survey-context` for context, `plan-work` for tasks, `develop-tdd`/`execute-plan` for implementation, `investigate-bug` for bug reports.
- **Verification Mandate:** Every story ends with a step-by-step manual verification script for the user. Wait for user confirmation (UAT) before declaring done.
- **Traceability Mandate:** Every story has at least one `story: eNNsNN` tag in implementing code or test files.
- **Task Tracking:** File an issue in the project tracker before non-trivial work; close on completion.
- **Ask, Don't Guess:** If a requirement or behavior is unclear, ask the user. NEVER guess.

## Always Green / Shift Left

Solo developer owns the whole codebase. **Always Green** means Preflight and CI are green before any forward work — not "green enough for this task".

**Shift Left (1-10-100):** Defects cost ~1× to fix in development, 10× in integration, 100× in production. Fix a red gate now.

**Preflight** — full local verification stack. See AGENTS.md Commands table.

Preflight MUST pass before kickoff, develop, or verify phases advance.

**CI green** — when a PR exists, `gh pr checks` MUST show passing before merge. Merge is human-owned; agents stop at green + PR open.

## Discovered Defects

Any **reproducible gate failure** encountered during unrelated work is a discovered defect.

**fix-or-log ladder (mandatory):**

1. **quick-fix** — trivial, data-only, or single-file fixes within guardrails.
2. **fix-bug** — needs investigation (`specs/bugs/BUG-*.md` + TDD).
3. **Log** — only when reproduction is blocked after good-faith attempt; write a BUG spec and stop forward work until triaged.

Discovered fixes ship in the **same PR** as the original work but in **separate commits**. Never narrate a failure and continue.

**Hard block:** Red Preflight or red CI blocks `kickoff-branch`, `develop-tdd`, and `verify-work` forward progress until fix-or-log produces green.

### Banned dismissive phrases

Agents MUST NOT use these phrases (or close paraphrases) to ignore reproducible failures:

| Banned phrase                      | Required behavior instead                                                    |
| ---------------------------------- | ---------------------------------------------------------------------------- |
| Pre-existing / pre-existing issues | Run fix-or-log; prove with a passing repro after revert if truly unrelated   |
| unrelated to this session          | Session boundaries do not waive green gates                                  |
| not introduced by my changes       | Bisect or fix anyway; solo-default owns the whole tree                       |
| out of scope (ignoring a red gate) | Invoke quick-fix or fix-bug; scope-minimization never overrides Always Green |

## Tests

- TDD where possible: red → green → refactor.
- Maintain high test coverage. New functions and bug fixes include tests; regressions get regression tests.
- F.I.R.S.T: fast, independent, repeatable, self-validating, thorough.
- Use `#[tokio::test]` for async; no real network in tests.

## Formatting & Lint

- Language default formatter: `cargo fmt`. No style debates beyond that.
- `cargo clippy -- -D warnings` — warnings are errors.

## Logging

- Structured `tracing` JSON for debugging and observability.
- Plain text only for user-facing CLI output.

## Defensive Code

Implement ONLY these categories:

- **Rate limit** — priority. Throttle inbound requests per user/IP on the axum server.
- **Graceful degradation** — priority. Pages render with missing or slow repository data. A placeholder shows, not a 500.

## Structure

- Cargo workspace: `crates/` — one crate per bounded context (`server`, `domain`, `repo-store`, etc.).
- Axum handlers thin; domain logic in `domain` crate; repository storage concerns confined to `repo-store`.
- Templates in Tera (`.html`), Tailwind for styling.
