# e23s01 — Visitor landing: the approved design on the front door

## 1. Story

As a visitor, I see the approved jjhub design when I arrive logged
out: a hero claim, a ruled merge-request ledger, four why rows, the
dark stack band, two self-hosting steps, an honest status badge with
the roadmap, and a footer. Logging in still takes one click.

## 2. Business value

The front door currently says one sentence. The landing explains the
jj-specific model (change-anchored reviews, stacks) and the
self-hosting path, so a visitor can decide without reading the
README.

## 3. Type

feat

## 4. Risk / context

risk: P3 (BCP 3, presentation only) — context: `home.html` visitor
branch, `nav.html` (shared chrome, restyled not re-linked),
`base.html` (font link), `assets/app.css` (landing components). The
logged-in dashboard branch of `home` is untouched.

## 5. Context

Zoom-out on the modules this touches:
- **`home` route (routes.rs)** — purpose: landing for visitors,
  dashboard for users. Contracts: unchanged context keys; template
  renders both branches.
- **`nav.html`** — purpose: shared site chrome. Contracts: keeps the
  literal `<nav>` element and every existing href (site_shell test).
- **`assets/app.css`** — purpose: the DESIGN.md token system.
  Contracts: hex only in `:root`, named tokens everywhere, state
  contract on interactive elements, reduced-motion fallback.

## 6. Requirements

#### ADDED: Visitor landing sections
Logged-out `GET /` renders, in order: header (in nav), hero claim
"A forge built around Jujutsu." with MR ledger, why rows
(change-anchored MRs, edit-in-place review, stacks, git
compatibility), inverse stack band, self-hosting steps with compose
and secrets, early-development badge + roadmap, inverse footer.

#### MODIFIED: Visitor chrome
Nav keeps `Log in` and gains `Get started` -> `/register`; landing
anchors resolve from any page (`/#why` etc.).

## 7. Acceptance

- Logged-out `GET /` contains hero claim, ledger change-ids,
  "Merge the bottom" band heading, "Early development" badge,
  "Revset search" roadmap row, "Apache License 2.0" footer.
- Existing assertions hold: `/register`, `/login` present, no
  create form for visitors; logged-in dashboard unchanged.
- `ux_audit` passes on the complete `app.css`.
- Preflight green.

## 8. Verification script (manual UAT)

1. `cargo test -p server landing_page` passes.
2. `cargo run`, open `http://localhost:PORT/` logged out: hero +
   ledger render, sections in order, focus ring visible on buttons,
   no horizontal scroll at 390px.
3. Click `Get started` -> registration form.
4. Log in, reload `/`: dashboard form + repos, no landing sections.
