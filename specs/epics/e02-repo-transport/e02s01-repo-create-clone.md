# e02s01 — Create repo, bare storage, repo page, clone over HTTP (public happy path)

## 1. Story

As a self-hoster, I create a repository on jjhub and clone/push it over HTTP with `jj git`, so the hosting actually hosts.

## 2. Business value

Tracer bullet: proves storage + transport + accounts together. Everything later (MRs, review) hangs off repos existing and being reachable.

## 3. Type

feat

## 4. Risk / context

risk: P0 (external integration: git smart HTTP protocol) — context: infra

## 5. Context

Logged-in user creates a repo (name, visibility). Server creates a bare git repo on disk under a configured storage root and a `repositories` row. Git smart HTTP (info/refs, upload-pack, receive-pack) is served via `git http-backend` CGI mounted at `/git/{owner}/{name}.git`. Public repos clone without auth; push always requires a token (story e02s02 enforces private-clone).

## 6. Requirements

#### ADDED: Repository creation
Logged-in user POSTs name + visibility. Name unique per owner, `[a-z0-9._-]+`. Bare repo initialized under storage root. Redirect to repo page.

#### ADDED: Repo landing page
Public page showing owner/name, visibility, clone URL.

#### ADDED: Git smart HTTP transport
`GET/POST /git/{owner}/{name}.git/info/refs`, `/git-upload-pack`, `/git-receive-pack` served by `git http-backend`. `jj git push` and `jj git clone` work end-to-end against a public repo with no jj-side config beyond remote URL + token.

#### ADDED: Transport wiring for token auth
Push requests carry Basic `x-access-token`; token must resolve to a user (extractor from e01s02). Push authorization rules land in e02s02.

## 7. Actors

Visitor (reads public repos), User (creates repos, pushes).

## 8. Preconditions

Postgres reachable; `git` binary on PATH (http-backend); storage root configured (`JJHUB_STORAGE_ROOT`, default `./data/repos`).

## 9. Postconditions

Repo row + bare repo on disk exist; clone over HTTP returns real git objects.

## 10. Business rules

- Repo name: `^[a-z0-9][a-z0-9._-]{0,99}$`, unique per owner.
- Storage path: `{storage_root}/{owner}/{name}.git`.
- Unknown repo → 404. No directory traversal (owner/name validated before path join).

## 11. Domain model touchpoints

**Repository** (tech-stack.md): id, owner_id, name, visibility, trunk (default `trunk`), merge_setting (default squash), created_at.

## 12. UI/UX notes

Tera templates start here: base layout, repo page. Tailwind via CDN for now.

## 13. API contracts

Git smart HTTP (standard). Web: `POST /repos`, `GET /{owner}/{name}`.

## 14. Data model

repositories(id uuid pk, owner_id fk users, name citext, visibility text check in ('public','private'), trunk text default 'trunk', merge_setting text default 'squash', created_at, unique(owner_id, name)).

## 15. Non-functional requirements

Owner/name validated before filesystem join — no traversal. Transport subprocess runs with no shell interpolation of user input (env + argv only).

## 16. Out of scope

Private-clone enforcement (e02s02), MRs (e03), repo deletion, renames, avatars/descriptions.

## 17. Acceptance criteria

- [ ] Logged-in user creates repo; lands on repo page showing clone URL.
- [ ] Duplicate name for same owner → 409.
- [ ] `jj git clone http://localhost:3000/alice/demo.git` retrieves commits (public, no auth).
- [ ] `jj git push` to the clone works with token auth.
- [ ] `GET /git/nobody/nope.git/info/refs` → 404.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: create repo
  Given I am logged in as alice
  When I create repo "demo" as public
  Then I am redirected to /alice/demo
  And a bare repository exists at the storage path

Scenario: clone public repo over smart HTTP
  Given public repo alice/demo exists with one commit pushed
  When a client clones /git/alice/demo.git
  Then the clone contains the commit

Scenario: unknown repo is 404
  When a client requests /git/nobody/nope.git/info/refs
  Then the response is 404
```

## 19. Verification script (manual)

1. `JJHUB_STORAGE_ROOT=/tmp/jjhub-repos DATABASE_URL=... ./target/debug/server`
2. Register + login in browser, create repo `alice/demo` (public).
3. `jj git clone http://localhost:3000/alice/demo.git /tmp/demo` — empty clone OK.
4. `cd /tmp/demo; echo hi > f; jj describe -m init; jj bookmark create trunk; jj git push` with token in URL.
5. Fresh clone again — file present.

## 20. Traceability

`// story: e02s01` in implementing code and tests.
