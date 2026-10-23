# Changelog

All notable changes to jjhub. Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.2.0] - 2026-10-23

The v0.2.0 build: a design system, a server-side rendering pass, a push
immutability policy, revset search, and conflict-tolerant hosting. Eleven
stories across eight epics, plus the LICENSE, shipped as stacked PRs #38–#52.

### Added

- **Design system** (#39, #45, #48): repo-root `DESIGN.md` as the single source
  of design tokens (colour, type, spacing, radius, elevation); tokenised `app.css`
  replacing ad-hoc values; an audit gate that blocks UI handoff until token
  coverage, APCA contrast, interaction states, and reduced-motion fallback pass
- **Server-rendered Tera templates** (#39, #41, #42): page HTML moved out of Rust
  into templates — shell, auth, and browse pages first, then MR, review, and CI
  pages (#15's structure now lives in templates, not string literals)
- **Immutability policy for protected refs** (#45, #46, #48): `refs/heads/trunk`
  and `refs/tags/*` reject non-fast-forward updates server-side; refused pushes
  are audited into `push_rejections`; repo settings surface the protected set
- **Revset search** (#40, #43): server-side jj query syntax via the `jj_lib`
  parser, a repository-scoped search UI, and a repo-nav entry
- **Conflict-tolerant hosting** (#46, #47, #52): repositories whose trunk or
  commits are conflicted are served and displayed instead of erroring — the
  landing page, MR pages, and file browser flag conflicted state, and the clone
  path serves refs that a conflicted `.meta` tree would otherwise hide
- **Operation-log audit trail** (#46, #48): the colocated `.meta` operation log
  is read into a repository-scoped operations view (list + detail)
- **Landing-page file tree** (#47, #49): the repository landing page lists the
  trunk tree with `tree`/`blob` links and a first-commit empty state
- **Inline kata embed** (#47, #48): merge-request pages embed the kata review
  session (loading state, noscript fallback, reduced-motion) instead of linking
  out to the sidecar
- Apache-2.0 LICENSE (#36)

### Changed

- CI runs the full preflight (`cargo fmt --all -- --check`, `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo test`, `cargo build`) and the
  whole e2e suite
- Merge requests, review threads, and the file browser are Tera-rendered rather
  than assembled in Rust handlers

### Fixed

- e13s01's landing-tree code was orphaned by a chain rebuild and never reached
  trunk through #47; its tests and templates are restored in #49
- Protected-refs tests no longer depend on the developer's global git config

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

[0.2.0]: https://github.com/kennethlove/jjhub/releases/tag/v0.2.0
[0.1.0]: https://github.com/kennethlove/jjhub/releases/tag/v0.1.0
