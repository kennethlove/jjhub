# Changelog

All notable changes to jjhub. Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.1.0] - 2026-10-19

First public cut: the v0.1.0 MVP per `specs/release-plan.yaml` — accounts, repo
hosting, merge requests, kata review layer, site shell, file browser, and review
threads, all green under CI.

### Added

- User accounts: registration, login sessions, transport tokens (#1)
- Repository hosting: bare storage, Git smart HTTP transport, push authorization (#2)
- Change-anchored merge requests: MR identity is `(repo, change-id)`, stable across rebase and history rewrite (ADR-0001)
- Stack derivation from commit parentage, with MR list/detail/diff pages (#11)
- Cascade merge: squash or merge-commit at the bottom of a stack with server-side rebase of the rest (#12)
- MR state machine: abandon, reopen, collaborators with per-repo permissions (#13)
- kata review layer: colocated jj metadata at repo creation (#6), pinned sidecar in compose (#5), session/token auth proxy, review links from MR pages (#15)
- Site shell: home page, navigation, repo creation form, clone/origin snippet (#20)
- Native file browser: tree and blob views at any revision (#29)
- Revision-scoped comment threads, CI status rendering, auth UX, API token snippet (#31)
- Browse-files link on the repo page (#22)
- CSRF protection on session-authenticated form endpoints (#16)

### Changed

- kata SPA, API, and assets served at the jjhub origin instead of the sidecar's (#25)
- Preflight and Lint commands match CI exactly: `cargo clippy --workspace --all-targets -- -D warnings` (#32)
- CI runs the full e2e suite (#34)
- Restrict issue tracking to this repository's own tracker (docs)

### Fixed

- Repo page lists open merge requests (#21)
- Browse link pins the trunk head commit, not a moving ref (#26)
- kata links target the loopback sidecar directly (#23)
- Server creates the `.meta` root at boot for the sidecar scanner (#19)
- Compose stack builds and kata startup unblocked (#18)
- Preflight and e2e gates restored after MVP shipments (#33)
- CI debuginfo disabled after runner disk/linker exhaustion on e07 branch

[0.1.0]: https://github.com/kennethlove/jjhub/releases/tag/v0.1.0
