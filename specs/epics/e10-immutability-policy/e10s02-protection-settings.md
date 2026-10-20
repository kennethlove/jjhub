# e10s02 — Protection rules visible in repo settings

## 1. Story

As a repository owner, the repo's settings surface lists exactly which
refs the server protects, so a rejection at the push gate is never a
surprise.

## 2. Business value

An invisible enforcement policy reads as a bug. Listing the protected set
where owners already look (next to visibility and trunk) makes e10s01's
refusals predictable and lets a pusher check the rules before pushing.

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 3, display-only over shipped enforcement) — context: UI
(server-rendered page, no JS).

## 5. Context

Zoom-out on the modules this touches:
- **repo_page / settings surface purpose** — the repo's detail and
  settings rendering (`routes.rs` `repo_page`, the `{visibility} — trunk:
  {trunk}` line, and `repo_settings`). **Callers** — lib.rs router.
  **Contracts** — axum handlers; tests assert HTML substrings.
- The settings surface gains a read-only **Protected refs** listing built
  from the e10s01 resolver — one source of truth, no second list. If e15's
  settings-template extraction ships first (release order places e15
  before e10), the block lands in the extracted template instead of the
  Rust string literal.
- Data comes from the repo row already loaded for the page (trunk) plus
  the resolver; no extra git subprocess.

## 6. Requirements

#### ADDED: Protected-refs listing in settings
The repo settings surface lists each resolved protected ref —
`refs/heads/{trunk}` and the `refs/tags/*` pattern — read-only, beside the
existing visibility/trunk information.

#### ADDED: Listing shares the enforcement source
The listing is produced by the same resolver the push gate uses (e10s01);
display and enforcement can never diverge.

## 7. Actors

Repo owner and collaborators viewing the settings surface.

## 8. Preconditions

e10s01 (resolver exists), repo settings surface exists (repo detail page
settings block; extracted template if e15 ships first).

## 9. Postconditions

A viewer sees the enforcement scope before pushing; trunk changes update
the listing automatically.

## 10. Business rules

- Read-only — 0.2.0 has no user configuration of the protected set.
- Listing obeys the same authz as the rest of the settings surface.

## 11. Domain model touchpoints

**Repository** (trunk → protected set).

## 12. UI/UX notes

Plain list in the existing settings block; tokens/states per DESIGN.md
when rendered inside the e12/e15 layout.

## 13. API contracts

None — server-rendered HTML only.

## 14. Data model

None.

## 15. Non-functional requirements

No additional git subprocess or query beyond the page's existing repo row.

## 16. Out of scope

Editing the protected set, per-ref toggles, enforcement logic (e10s01),
audit history (e10s03).

## 17. Acceptance criteria

- [ ] `cargo test -p server protect_page` — settings surface lists
      `refs/heads/{trunk}` and `refs/tags/*` for a seeded repo; a repo
      with a custom trunk lists its own branch. (e10s02t1)
- [ ] `cargo test --workspace` green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: settings lists the protected set
  Given a public repo "o/r" with default trunk
  When I open the repo settings surface
  Then refs/heads/trunk is listed as protected
  And refs/tags/* is listed as protected

Scenario: custom trunk is reflected
  Given a repo with trunk set to "main"
  When I open the repo settings surface
  Then refs/heads/main is listed as protected
```

## 19. Verification script (manual)

1. `cargo run` — open a repo's settings surface.
2. Confirm the protected list matches the repo's trunk and the tag pattern.

## 20. Traceability

`// story: e10s02` in the settings rendering block and the `protect_page`
test.
