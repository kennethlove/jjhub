# jjhub

Forge-style hosting for [Jujutsu](https://jj-vcs.dev) repositories — like GitHub, GitLab, and Forgejo, but built around jj's model instead of Git's.

**Status: early development. Use at your own risk.**

## Why

Git forges fight the jj workflow: review identity dies on force-push, stacks are bolted-on metadata, and history rewrites break every PR. jjhub instead anchors reviews to jj's `change-id`, which survives rebase, squash, and rewrite.

- **Change-anchored merge requests** — MR = (repo, change-id), stable across history edits (see [ADR-0001](specs/adr/0001-change-id-anchored-mr.md))
- **Edit-in-place review** — every push is a new Revision; prior revisions and comments stay readable
- **Stacks from the graph** — parentage derived from commit parents; merging the bottom MR squashes and server-rebases the rest
- **Git compatibility** — jj's git backend means plain `git clone` works too

## Roadmap

Shipped since the MVP cut: revset search, operation-log audit, conflict-aware
browsing, protected-ref enforcement on push, protected-ref configuration in
the Operations panel, email confirmation and password reset, rate limiting
(off by default), personal access tokens (mint, regenerate-rotate, revoke),
token permissions (scopes, expiry, repo pin, allowlist), account preferences
(profile, password, account deletion), operator moderation — suspend and
unsuspend abusers across the web UI, smart HTTP, and SSH from a site-level
`/admin` console — moderation follow-through (repo takedown with restore
refused everywhere but the owner and operator, CAPTCHA on signup,
registration that closes from the same console, and a filterable
moderation log with CSV export), and mail operations (path-form
verification and password-reset links plus a neutral resend from one
mailer, moderation notices, an env-selected SMTP arm, and month-named
JSONL rollups that keep secrets out of every line).

Next up (v0.8.0, to be scoped): live mail — proving delivery against
the phase gate with a staging send and a delivery readout before any
SMTP default could ship.

## Development

Stack: Rust, axum, htmx, Tera, Tailwind, sqlx + Postgres.

```bash
# tests need Postgres (e.g. docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=jjhub -e POSTGRES_DB=jjhub_test postgres:17-alpine)
DATABASE_URL=postgres://postgres:jjhub@localhost:5432/jjhub_test cargo test --workspace

cargo run   # http://localhost:3000
```

Preflight (must be green before any forward work):

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings \
  && cargo test --workspace && cargo build
```

## Self-hosting

```bash
cp .env.example .env   # set POSTGRES_PASSWORD and SESSION_SECRET
docker compose up -d   # http://localhost:3000
```

## Deploy on Coolify

Register the repository as a compose stack. Set only two secrets:

```bash
POSTGRES_PASSWORD=
SESSION_SECRET=      # openssl rand -hex 32
```

A clone ships no `.env`, so `COMPOSE_FILE` is never set and the optional
port file `docker-compose.ports.yaml` does not load: the stack binds no
host port. Map ports in the platform instead.

Local self-hosting is unchanged — `cp .env.example .env` gives you
`COMPOSE_FILE=docker-compose.yml:docker-compose.ports.yaml` plus
`JJHUB_BIND_HTTP=3000:3000`, so `docker compose up -d` serves
http://localhost:3000 as before.

SSH publish opt-in: set `JJHUB_BIND_SSH=2202:2202` in `.env` and
uncomment the SSH line in `docker-compose.ports.yaml` (SSH clones).

The app container healthchecks `GET /healthz` with curl every 30 seconds.
`docker compose ps` and the platform show the app healthy only while the
route answers.

SSH host keys live on the `repo-data` volume at
`/var/lib/jjhub/host-keys`, so clone fingerprints stay stable across
redeploys.

## Mail delivery

jjhub's default mailer writes to the server log: verification and reset
links appear in `docker compose logs`. Set these to relay through an SMTP
server instead:

```bash
JJHUB_EMAIL_PROVIDER=smtp   # unset keeps the in-box log mailer
JJHUB_SMTP_HOST=
JJHUB_SMTP_PORT=587
JJHUB_SMTP_USERNAME=
JJHUB_SMTP_PASSWORD=
JJHUB_MAIL_FROM=jjhub@your.domain
```

The SMTP arm exists only when `JJHUB_EMAIL_PROVIDER=smtp`; unset or an
unknown value keeps the log mailer. An unreachable relay never fails the
action that triggered a send — the moderation row stands and the failure
is logged.

SPF, DKIM, and DMARC are DNS records in your zone. jjhub cannot manage
them for you; add them for the `JJHUB_MAIL_FROM` domain using the values
your mail provider publishes (start DMARC with `p=none`).

### Staging proof before you switch it on

The live-mail gate (v0.8.0): prove delivery on a staging relay before
production traffic depends on it.

1. Point `JJHUB_EMAIL_PROVIDER=smtp` and `JJHUB_SMTP_*` at a staging
   relay — a real account on any provider that speaks SMTP.
2. Sign up one test account, then resend from the verify page.
3. Open `/admin` → **Recent deliveries**: every send shows its outcome
   (`ok`, or `failed` with the reason). The same lines are in
   `mail_log_YYYY-MM.jsonl` under `JJHUB_LOG_DIR`.
4. Suspend a test account to send a moderation notice and confirm its
   outcome there too.

A dead relay never fails the page that triggered the send; the outcome
line is the fact you are proving. Keep production on the in-box log
mailer until this reads `ok`.

## Rate limiting

Token buckets, in memory, off by default. Set `JJHUB_RATE_LIMIT=on`
to enable; then any row may be overridden:

| Knob | Default (per-window:burst) |
| --- | --- |
| `JJHUB_RATE_LIMIT_WINDOW` (seconds) | `3600` |
| `JJHUB_RATE_LOGIN_IP` | `20/3600:5` |
| `JJHUB_RATE_PUSH_USER` / `JJHUB_RATE_PUSH_IP` | `30/3600:10` |
| `JJHUB_RATE_CREATE_USER` / `JJHUB_RATE_CREATE_IP` | `10/3600:3` |
| `JJHUB_RATE_TOKEN_USER` | `10/3600:5` |

Format: `N[/window-secs[:burst]]`. Over budget answers `429` with
`Retry-After`. Reads (fetch/clone) are never throttled. Push-to-create
charges both the push and the create budget; a form create and a
push-to-create share one create budget per user and per IP. Malformed
values fail loudly at boot.

## License

Apache License 2.0.
