# e07s01 — Tree and blob pages at any ref

## 1. Story

As a reviewer, I can browse the repository's files as they exist at a revision — a GitHub-style tree and file view served by jjhub itself, because kata's browser is change-centric (log + per-commit diffs) and cannot show a tree at a snapshot.

## 2. Business value

Closes the browsing gap the user hit: "browse shows history, not files at the current revision". Uses plumbing jjhub already has (bare-repo git access, HTML escaping).

## 3. Type

feat

## 4. Risk / context

risk: P1 — context: UI + git plumbing (read-only subprocess calls, argv-only).

## 5. Context

Two new pages on the repo-detail pattern:
- `GET /{owner}/{name}/tree` — tree at the repo's trunk (redirects/lists).
- `GET /{owner}/{name}/tree/{ref}/{*path}` — directory listing at `ref` (default `trunk`): entries from `git ls-tree` (non-recursive), directories linked deeper, files linked to blob pages, breadcrumb navigation.
- `GET /{owner}/{name}/blob/{ref}/{*path}` — file contents at `ref` via `git show ref:path`, line-numbered, fully HTML-escaped, binary detected and summarized.
`{ref}` is a git ref name or commit id, validated against the bare repo (`rev-parse --verify`); unknown refs/paths → 404. Repo page gains a **Files** link next to Browse files.

## 6. Requirements

#### ADDED: Tree page
Directory listing at ref+path; dirs `name/` link to subtree; files link to blob; breadcrumb shows path with links to each ancestor; empty dirs and empty repos render friendly empty states.

#### ADDED: Blob page
File contents at ref with line numbers; content escaped (XSS); binary files show size + "binary" note instead of content.

#### ADDED: Ref validation
`ref` resolved with `rev-parse --verify <ref>^{commit}`; failure → 404. Path traversal impossible (paths validated: no `..`, no leading `/`).

#### ADDED: Files link on repo page
Links to `/tree` (trunk view) when the repo has commits; absent otherwise.

## 7. Actors

Any visitor on public repos (matches repo-page visibility behavior today).

## 8. Preconditions

e02s01 (bare storage), e06s01 (repo page).

## 9. Postconditions

A pushed repo's full file tree is browsable at any revision without jj or kata.

## 10. Business rules

- Read-only: no git command writes; argv-only (no shell interpolation of refs/paths).
- Escaping via the existing html_escape helper.

## 11. Domain model touchpoints

**Repository**, read-only git object access (same as `stack::` glue).

## 12. UI/UX notes

Server-rendered inline HTML + existing Tailwind-ish classes; no JS.

## 13. API contracts

HTML pages only.

## 14. Data model

None.

## 15. Non-functional requirements

Tree listing of 1000 entries < 300ms (single `ls-tree` call). Blob caps at 1 MiB rendered (larger: truncated note).

## 16. Out of scope

Raw-file endpoint, blame, syntax highlighting, search, download, symlink following, submodules.

## 17. Acceptance criteria

- [ ] Tree at trunk lists seeded nested structure with working links (test).
- [ ] Blob shows escaped content with line numbers (XSS test: payload renders as text).
- [ ] Binary file shows note, not content (test).
- [ ] Unknown ref and `..` path → 404 (tests).
- [ ] Repo page shows Files link when commits exist.
- [ ] Preflight green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: browse nested tree
  Given a repo with docs/guide.md pushed
  When I GET /alice/demo/tree
  Then "docs/" links to /alice/demo/tree/trunk/docs
  And "README.md" links to the blob page
```

## 19. Verification script (manual)

1. Push a repo with nested files.
2. Open `/tree` — navigate into dirs, open a file — line numbers, escaped content.
3. Binary file — summary only.

## 20. Traceability

`// story: e07s01` in implementing code and tests.
