# Changelog

All notable changes to jjhub. Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.9.0] - 2026-11-09

The v0.9.0 build: the public face and the privacy hole, in one cut.
The logged-out home now says what jjhub is and how to run it,
private repositories answer 404 to anyone outside the repo while
owners flip visibility from the repo page, the review surface wears
the jjhub theme and navigation over kata, a push can create the
repo in one command over HTTPS, and the app finally logs every
request and transport decision. Shipped as PRs #118–#124.

### Added

- **Kata theme on the review surface** (#118): the proxy injects
  `/assets/kata-theme.css` — a token mirror of the site palette over
  kata's `:root` variables — after kata's own sheet, so the review
  surface reads light or dark like the rest of jjhub
- **Kata chrome** (#124): the logged-in site nav renders above the
  kata iframe with `/assets/kata-chrome.css` scoped to the site-nav
  block, and the kata pin moved to the fork commit that carries the
  chrome-height slot
- **Product home** (#121): the logged-out `/` is the self-hoster
  page — what ships today, one `docker compose up`, login only, and
  no register path
- **Visibility flip from the repo page** (#123): owners switch a
  repository public or private where they already read its metadata
- **Structured application logs** (#120): one INFO line per request
  with method, path, status and duration; token, transport,
  rate-limit and authorize decisions log as events;
  `JJHUB_LOG_FORMAT=json` turns them into machine lines

### Fixed

- **Private repositories were readable on ungated web routes** (#122):
  repo, tree, blob, revset, operations and merge-request pages answer
  404 without visibility or collaborator standing — only the smart
  HTTP transport gate had been checking
- **Push-to-create over HTTPS** (#119): a missing repository answers
  the 401 challenge to anonymous receive instead of 404, so real git
  clients send credentials and land the first push

## [0.8.0] - 2026-11-07

The v0.8.0 build: mail you can read in either format and trust from
the inbox. Every message ships a plain-text arm and an HTML arm
rendered from one context, verification links are absolute and
clickable, `/admin` shows the outcome of every send, and a failed
signup says so and rolls back. Shipped as PRs #99–#116.

### Added

- **Delivery readout** (#101): `/admin` gains Recent deliveries —
  every send with its provider, duration, and recorded outcome from
  the mailer's single write point, so a staging run can prove which
  messages actually went out
- **Clear retry on failed signup and resend** (#102): a signup whose
  confirmation never sends rolls the account back and answers with
  the retry instead of a dead form; a failed resend says the last
  send didn't go through instead of redirecting like it worked
- **HTML + plain-text pair per mail** (#116): every mail renders both
  arms from one Tera context (`mail/{verify,reset,moderation}.{txt,
  html}`); Resend carries an `html` field, SendGrid a second content
  part, SMTP a `multipart/alternative`, and the moderation reason
  stays verbatim in the text arm while the HTML arm escapes it

### Changed

- A resend or login-triggered confirmation now lands on the sent page
  with a visible neutral confirmation for every address — the flow
  never reveals whether the address exists (#113)
- Closed registration sends the signup to `/login` with the reason
  instead of a dead-end 403 on the form (#113)

### Fixed

- **Absolute verification links** (#113): verification mails carried
  a bare `/verify/<token>` path that mail clients turned into a dead
  link; links now build from the request origin, honoring
  `X-Forwarded-Proto` behind a proxy
- **SMTP TLS picked by port** (#109): 465 speaks SMTPS, every other
  port STARTTLS
- Compose forwards the mail env vars under the names the code reads
  (#110)

## [0.7.0] - 2026-11-05

The v0.7.0 build: jjhub sends mail. Verification and reset links
arrive as path-form one-time tokens from one mailer with a neutral
resend, moderation notices carry every operator action, an explicit
SMTP arm sits behind env opt-in, and the whole history rolls up into
month-named JSONL files that never hold a secret. Shipped as PRs
#94–#97.

### Added

- **Mailer default and neutral resend** (#94): every provider is
  selected through one `from_env()` with `LogMailer` as the safe
  default for unset and unknown values, and an unauthenticated resend
  endpoint answers identically for known and unknown addresses with a
  60-second cooldown keyed by a hashed address — mail failure never
  surfaces a 500
- **Password reset through the mailer** (#95): requesting a reset
  always returns the same neutral response and delivers the link
  through the mailer instead of the server log; the token is
  single-use, expires in an hour, and after rotation the old password
  fails while the new one passes
- **Moderation notices** (#96): suspend, unsuspend, takedown, and
  restore each email the affected user once, after the audit row
  stands, with the operator's reason rendered as escaped text; an
  unreachable relay never fails the action or its row
- **SMTP arm behind explicit env** (#96): `JJHUB_EMAIL_PROVIDER=smtp`
  plus `JJHUB_SMTP_*` selects a lettre (rustls) relay — proven without
  a mail server — and the README gains a Mail delivery deploy section
  with the env table and the SPF/DKIM/DMARC note
- **Monthly JSONL rollups** (#97): every send appends one
  timestamp-first line to `mail_log_YYYY-MM.jsonl` and every moderation
  row one to `moderation_log_YYYY-MM.jsonl` under `JJHUB_LOG_DIR`,
  month named from the write date; only structured fields are written,
  so tokens never reach the log, and no drain code ships — the file is
  the contract

### Changed

- **Verification and reset links are path-form** (#94, #95):
  `/verify/<token>` and `/reset/<token>` replace the query-string
  forms (old query links continue to work), and reset links no longer
  appear in the server log

## [0.6.0] - 2026-11-05

The v0.6.0 build: moderation follow-through is complete. Repos come
down and come back, CAPTCHA guards signup, registration can close,
and the audit trail is readable. Shipped as PRs #82–#91.

### Added

- **Repo takedown with restore** (#89): an operator marks a repository
  taken down from the `/admin` console — every browse route 404s for
  everyone except the owner and operators, rows, refs, and history
  are untouched, each flip lands in `moderation_actions`, and restore
  brings the repo back with its own audit row
- **Transport refusal for taken-down repos** (#91): smart HTTP
  answers 404 and SSH refuses before any git process spawns for
  stranger fetch and push on a taken-down repo — the decision lives
  in the shared `authorize` chain ahead of the scope gate, the owner
  passes on argument alone, fresh rows are never taken down, and
  restore reopens every path
- **CAPTCHA on signup** (#88): when `JJHUB_CAPTCHA_*` is configured,
  the register form renders the Turnstile widget and a signup without
  a solved token creates no account; a rejected, stale, or
  unreachable verifier fails closed to the retry message;
  unconfigured nodes sign up exactly as before, with no widget or
  provider script in the page
- **Registration toggle** (#85): one `site_settings` row flips new
  signups closed from the `/admin` Registration panel — refusal is
  403, live sessions survive the flip, reopen reopens, and every
  transition is an audit row
- **Moderation log with CSV export** (#86): `/admin` lists
  `moderation_actions` newest-first with action and target filters
  and an explicit empty state; `/admin/moderation.csv` serves the
  same selection RFC4180-quoted with a filename; both are
  operator-only, and the timestamp column folds away below 768px

### Changed

- **SMTP relay configuration** (#87): deployment gained the SMTP
  relay environment family — the transport arm the v0.7.0 mail
  epic will use
- README and the landing roadmap now match shipped capabilities;
  the cycle's planning docs and capsules landed with the code

### Fixed

- The deploy kata pin survives an empty Coolify build-arg (#90)

## [0.5.0] - 2026-11-03

The v0.5.0 build: a public node can refuse an abuser. Operator
moderation and protected-ref configuration ship, and preflight CI
shards four ways under nextest. Shipped as PRs #75–#81.

### Added

- **Protected-refs configuration** (#77): the Operations panel lists
  the enforced defaults (`refs/heads/<trunk>` and `refs/tags/*`) and
  takes user-added patterns, merged into `receive_guard`'s admission
  list — defaults are never rows and never removable, patterns are
  charset-checked at the schema and at the form, and every add/remove
  is CSRF-protected with an admission test proving a stored pattern
  refuses non-fast-forwards and deletions
- **Operator moderation** (#80): an operator designated by
  `JJHUB_OPERATOR=<handle>` or `users.is_operator` gets a site-level
  `/admin` console to suspend and unsuspend accounts — live sessions
  die at the session middleware, login answers with the exact
  bad-password response, smart HTTP returns 401, SSH rejects before
  any git process spawns, and every action lands in
  `moderation_actions` with actor, target, and time; operators can
  never be suspended, and unsuspend restores the same account and
  its live sessions

### Changed

- **Preflight CI** (#78, #79): tests run under cargo-nextest and
  preflight shards four ways with a test-only cheap Argon2 cost —
  warm preflight dropped from 5m14s to about 2m per shard with the
  same checks
- README and the landing roadmap now match shipped capabilities
  (#75); the cycle's spec and planning docs landed with it (#76,
  #81)

## [0.4.0] - 2026-11-02

The v0.4.0 build: bounded credentials, throttled abuse paths, verified
signups, and the approved UI on every page. Eleven epics and surfaces
shipped as PRs #63–#73.

### Added

- **Rate limiting** (#63): per-IP login budget backed by a policy table,
a shared push budget across HTTP and SSH, and SSH disconnect on
exhaustion — the unthrottled push path that spawned git subprocesses
without bound is closed
- **Auth flows** (#64): htmx card swaps that push real URLs (`/login`,
`/register`, `/reset` are shareable and cold-load correctly), plus
password reset through expiring single-use tokens
- **Token model** (#65): auto-issued signup token, one-shot regenerate
that rotates the value in place, and details-once screen preferences
- **Preferences design** (#66): the approved settings surface — profile
with display name and theme, change password, a token ledger with
scoped revoke, and two-step account deletion that ends the session
- **Token permissions** (#67): scopes, expiry, repository pinning, and IP
allowlist on top of the token model
- **Repo landing** (#68): the approved repository shell — header with
status pill and copyable clone snippet, section tabs, About rail, file
tree with empty state
- **Email confirmation** (#69): signup verifies by email before first
login; mailer facade, verify link, and a gated login that points at the
sent page

### Changed

- Browse, search, operations, and merge-request pages moved into the
shared frame — site nav, centered wrap, footer (#71)
- `app.css` revalidates with a strong ETag and `no-cache` instead of
Cloudflare's default 4-hour `max-age`, so a deploy's styles can no
longer outlive it (#72)
- The repository page and the logged-in dashboard were reframed to match
every other page (#70, #73)

## [0.3.0] - 2026-10-24

The v0.3.0 build: visible divergence, built-in SSH transport, and a
Coolify-ready deployment. Three epics shipped as PRs #54–#57, plus
bookkeeping (#58, #59).

### Added

- **Divergence visible** (#54): pairwise same-change non-ancestor heads are
detected per commit (`server::divergence::divergent_heads`), surfaced as a
divergence notice on merge-request detail (both heads linked), a mark on MR
list items, and a flag on the repository page — one shared `divergence-flag`
class that fails safe to not-divergent
- **SSH transport** (#55, #56): russh-based listener with generated host keys
on disk, transport-token auth over password (publickey declined), read-only
clone over `ssh://`, and receive-pack push admission through the same
authorize chain as smart HTTP — pushes flow through `admit_push` and
post-receive, refused pushes land in `push_rejections`, and repository pages
show the `ssh://` clone line when the listener is enabled
- **Coolify-ready deployment** (#57): host ports move out of
`docker-compose.yml` into tracked `docker-compose.ports.yaml`, loaded only
via `COMPOSE_FILE` in `.env` — a clone with only `POSTGRES_PASSWORD` and
`SESSION_SECRET` binds no host ports; containers healthcheck `GET /healthz`;
SSH host keys persist on the `repo-data` volume across redeploys

### Changed

- CI e2e runs `e2e-ssh-push` alongside the existing suites (#56)

### Fixed

- The e16 divergence code never reached trunk: #53's squash shipped only
bookkeeping; the wave commits were restored from dangling history and
verified as #54

## [0.2.0] - 2026-10-23

The v0.2.0 build: a design system, a server-side rendering pass, a push
immutability policy, revset search, and conflict-tolerant hosting. Eleven
stories across eight epics, plus the LICENSE, shipped as stacked PRs #38–#52.

### Added

- **Design system** (#39, #45, #48): repo-root `DESIGN.md` as the single source
  of design tokens (color, type, spacing, radius, elevation); tokenized `app.css`
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

[0.3.0]: https://github.com/kennethlove/jjhub/releases/tag/v0.3.0
[0.2.0]: https://github.com/kennethlove/jjhub/releases/tag/v0.2.0
[0.1.0]: https://github.com/kennethlove/jjhub/releases/tag/v0.1.0
