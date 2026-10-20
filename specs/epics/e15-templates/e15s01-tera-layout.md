# e15s01 — Tera renderer and base layout

## 1. Story

As a contributor who changes jjhub page markup:
**From** login HTML living as a `format!` string literal inside `routes.rs`
(`login_page`, line 136);
**To** login HTML living in a Tera template that extends one shared base
layout, rendered by a Tera instance owned by the server;
**So** handlers pass context only and every later epic (e12s02 styling,
e13 landing) edits templates instead of Rust strings.

## 2. Business value

First half of epic e15's SCOPE constraint — "Page HTML lives in Tera
templates; routes.rs holds handlers only". Creates the renderer and the
base layout that every later extraction story reuses; e12s02 explicitly
preconditions on this story ("e15s01 — base layout template exists"),
because the design system must style templates, not Rust literals.

## 3. Type

feat

## 4. Risk / context

risk: P0 (BCP 5, shared rendering path) — context: infra (new renderer
wiring) plus one page extraction; no domain or API behavior change.

## 5. Context

Zoom-out on the module this touches:
- **routes.rs purpose** — every HTTP page handler (31 fns); pages are
  `format!` HTML string literals — 11 page bodies start with
  `<!doctype html>`. **Callers** — the router registrations in lib.rs.
  **Contracts** — axum handler signatures, response status/content-type,
  and existing HTML-substring tests (`crates/server/tests/login.rs`,
  `home_page.rs`, `site_shell.rs`, …).
- **Tera is already a dependency** — `tera = "1"` at workspace
  `Cargo.toml:10`, `tera.workspace = true` in `crates/server/Cargo.toml`;
  no Tera usage in `src/` yet and no `templates/` directory exists.
- **Loading decision** — templates are embedded with `include_str!` and
  registered on a `Tera` instance at startup (one registration function
  in a new module). Rationale, mirroring e12s02's `include_str!`
  stylesheet choice: the file ships inside the binary, no filesystem
  path or cwd dependency at runtime, and no new package is needed —
  `tera` [OK] (existing workspace dependency), slopcheck clean.
- New files: `crates/server/templates/base.html` (doctype, `<head>`,
  title block, body block, nav slot) and `crates/server/templates/login.html`
  extending base.

## 6. Requirements

#### ADDED: Tera renderer with base layout
The server owns a `Tera` instance preloaded with `base.html`; a test
renders the base layout and gets a full document (doctype, title block,
body block).

#### MODIFIED: Login page served from a template
**Before:** `login_page(error)` (routes.rs:136) builds the whole login
document with an inline `r#"<!doctype html>…"#` literal.
**After:** `login_form` renders `login.html` extending `base.html` with
context (`error`, form values); the response keeps the existing status,
content type, and HTML-substring contracts so current tests pass
unchanged.

#### ADDED: Renderer and extraction tests
Test names `tera_layout` and `extract_login` cover base-layout render
and the extracted login page.

## 7. Actors

Visitors hitting `GET /login`; contributors and coding agents that add
or change pages.

## 8. Preconditions

`tera = "1"` declared (already true); baseline suite green.

## 9. Postconditions

`templates/` exists with base layout and login; `login_form` holds
handler logic only; the routes.rs doctype-literal count drops from 11
to 10; `cargo test --workspace` green.

## 10. Business rules

- Routes, methods, status codes, and existing HTML-substring test
  contracts do not change — extraction moves markup, not behavior.
- Page HTML lives in templates; routes.rs holds handlers only (SCOPE
  constraint) — no new `<!doctype html>` literal may be added to
  routes.rs (guards the e15s03 doctype gate).

## 11. Domain model touchpoints

None — presentation layer only.

## 12. UI/UX notes

Extraction is markup-neutral: output must match pre-extraction markup.
Per SCOPE constraint, UI work runs the keystone render gates and the ux
audit before handoff; no styling changes happen in this story (e12s02
styles the templates afterwards).

## 13. API contracts

`GET /login` unchanged (status codes, content type, form fields).

## 14. Data model

None.

## 15. Non-functional requirements

Templates compiled once at startup; no per-request file reads; render
adds no dependency beyond the already-declared `tera`.

## 16. Out of scope

All other pages (register, home, settings, repo, tree, blob, MR,
review), stylesheet/tokens (e12s02), htmx/JS, error-handling changes,
tasks.yaml status flips (stay `todo` until develop-tdd), epic.yaml.

## 17. Acceptance criteria

- [x] `cargo test -p server tera_layout` green — base layout renders
      with doctype, title block, and body block (e15s01t1).
- [x] `cargo test -p server extract_login` green — `GET /login` served
      from `login.html`; existing login assertions unchanged
      (e15s01t2).
- [x] `cargo test --workspace` green after renderer wiring
      (e15s01t3).
- [x] `grep -c doctype crates/server/src/routes.rs` reads 10 (was 11).
- [ ] keystone render gate: `/login` rendered at 390 / 768 / 1440
      matches the pre-extraction capture — no layout drift.
- [ ] ux audit: no new findings (extraction adds no CSS; gate re-runs
      with its full input once e12s02 lands).

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: base layout renders through the Tera renderer
  Given the server builds with the Tera renderer registered
  When a test renders the base layout template
  Then the output contains a doctype, a title block, and a body block

Scenario: login page is served from its template
  Given login.html extends the base layout
  When I GET /login
  Then the response body matches the pre-extraction markup contracts
  And the existing login tests pass without assertion changes
```

## 19. Verification script (manual)

1. `cargo test -p server tera_layout` — green.
2. `cargo test -p server extract_login` — green.
3. `cargo test --workspace` — green.
4. `grep -c doctype crates/server/src/routes.rs` — prints 10.
5. `cargo run`, open `http://localhost:3000/login` — page identical to
   before the extraction.
6. keystone render gate on `/login` at 390 / 768 / 1440 — inspect
   screenshots against the pre-extraction captures.
7. ux audit — no new findings against the stylesheet baseline.

## 20. Traceability

`// story: e15s01` in the renderer module and the `tera_layout` /
`extract_login` tests.
