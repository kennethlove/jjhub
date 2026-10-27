# e24s01 — htmx auth cards: negotiation, push URLs, restyle

## 1. Story

As a visitor I move between log in, register, and the reset-request
form without full page reloads, while every card URL stays
shareable: opening the URL cold serves the same full page.

## 2. Business value

The approved auth design ships (spec: specs/designs/auth.html).
The same route answers browsers and htmx, so there is one handler
per flow, no duplicated endpoints, and links remain linkable — the
property the user called out: "how else can one user link another
to that page?"

## 3. Type

feat

## 4. Risk / context

risk: P3 (presentation + content negotiation only) — context:
`routes.rs` (HX-Request branch), `templates/{login,register}.html`
shells, new `templates/{login,register}_card.html`, `render.rs`
include list, `base.html` htmx script, `app.css` auth section.
Contract tests `extract_login`, `auth_ux`, `site_shell` pinned.

## 5. Verification

`cargo test --workspace`; CDP UAT at 1440 + 390 (zero overflow,
swap flips h1, pushed URL cold-loads full page); `ux_audit` on
app.css.
