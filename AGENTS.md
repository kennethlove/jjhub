# jjhub — AI Agents

Read CONVENTIONS.md before any GitHub or git operation.

<!-- BEGIN bigpowers:project -->

## Project

Hosting service for Jujutsu repositories similar to GitHub, GitLab, and Forgejo.

Stack: Rust / axum / htmx / Tera / Tailwind (Cargo workspace)

## Commands

| Action    | Command                                                                                  |
| --------- | ---------------------------------------------------------------------------------------- |
| Run       | `cargo run`                                                                              |
| Test      | `cargo test`                                                                             |
| Build     | `cargo build`                                                                            |
| Lint      | `cargo clippy -- -D warnings`                                                            |
| Format    | `cargo fmt --all -- --check`                                                             |
| Preflight | `cargo fmt --all -- --check && cargo clippy -- -D warnings && cargo test && cargo build` |
| CI        | `gh pr checks` (when a PR is open)                                                       |

## Architecture

Axum web service serving Tera/htmx pages over a domain model: users have profiles and repositories. Repos are public or private, carry collaborators, issues, and merge requests.

## Conventions

- Follow standard Rust conventions (rustfmt defaults, clippy, idiomatic naming).
- TDD where possible; maintain high test coverage.
- Conventional Commits without emoji.
- Use beads for task tracking.

## Never

- Never dismiss reproducible gate failures as pre-existing or out of scope.
- Never proceed on red Preflight or red CI — invoke quick-fix or fix-bug first.
- Never merge a pull request on your own.
- Never open a PR before formatting, linting, and tests all pass.
- Never mention beads in documentation, comments, or PR bodies.
- Never guess. If you do not know, ask.

## Agent Rules

- **Workflow Mandate:** You MUST use the bigpowers skills (e.g. `plan-work`, `develop-tdd`, `orchestrate-project`) to perform tasks. DO NOT write code directly in response to a user prompt like "build this feature".
- **Always Green:** Preflight and CI must be green before forward work. Reproducible gate failures require **fix-or-log** (quick-fix → fix-bug) per CONVENTIONS § Discovered Defects.
- Read specs/ before writing code.
- All planning and specifications MUST be written to `specs/` (`product/SCOPE_LATEST.yaml`, `release-plan.yaml`, `epics/`) before any code is generated.
- Write the minimum code that solves the stated problem. Nothing extra.
- Run tests after every change. Show evidence before declaring done.
- One clarifying question beats a wrong assumption baked into 200 lines.

<!-- END bigpowers:project -->

<!-- BEGIN bigpowers:learned-preferences -->

## Learned User Preferences

(none yet)

## Workspace Facts

(none yet)
<!-- END bigpowers:learned-preferences -->
