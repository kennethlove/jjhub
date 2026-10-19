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

Planned post-MVP: conflict-tolerant hosting, operation-log audit, server-side immutability policy, revset search.

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

## License

Apache License 2.0.
