# e26s01 — repo landing shell per the approved design

## 1. Story

As a visitor I land on a repository page that matches
specs/designs/repo.html: eyebrow wordmark, repo title, status pill,
visibility note, description, clone command with Copy, section
tabs, trunk chip, and a split grid with an About rail and a Files
panel with an empty state.

## 2. Business value

The visitor-facing repository page carries the product's look on
every repo visit; hierarchy moves from a bare h1 to the designed
header + split grid.

## 3. Type

feat

## 4. Risk / context

risk: P3 (presentation) — context: `templates/repo.html` rewrite,
`assets/app.css` gains the repo.css block, `routes.rs::repo_page`
context unchanged except where tests demand new keys. Pinned
by design: eyebrow 'jj / {owner}', h1 repo name, status pill by
visibility, visibility-def line, 'jj git clone' + Copy, tabs
(active Browse), span.repo-trunk, split-grid 18rem/1fr, About
panel, Files panel with visually-hidden h2 + file tree or
onboarding empty state.

## 5. Verification

`cargo test --workspace`; CDP UAT at 1440 + 390 (zero overflow,
Copy works, tab active state); `ux_audit` (absolute path).
