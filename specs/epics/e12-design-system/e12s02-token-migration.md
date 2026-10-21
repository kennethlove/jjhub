# e12s02 — Site pages migrated to design tokens

## 1. Story

As a visitor, jjhub pages have consistent, readable styling with visible
keyboard focus states — one server-served stylesheet built from DESIGN.md
tokens instead of browser defaults.

## 2. Business value

Turns the design system into visible product quality: consistent
identity, readable contrast, keyboard accessibility. First real styling
layer the site has ever had.

## 3. Type

feat

## 4. Risk / context

risk: P0 (BCP 5, shared rendering module) — context: UI + one new route.

## 5. Context

Zoom-out on the modules this touches:
- **routes.rs purpose** — every HTTP page handler; pages are inline HTML
  string literals (26 pub fns). **Callers** — lib.rs router (59 route
  registrations). **Contracts** — axum handler signatures; tests assert
  HTML substrings.
- **No stylesheet exists** — repo-wide search found no `*.css`, no
  asset-serving route, no Tailwind usage. AGENTS.md's stack line names
  Tailwind/Tera, but rendering today is plain format! HTML; this story
  introduces the styling layer with plain CSS from tokens (no build
  step, no new dependency).
- New file `crates/server/assets/app.css` embedded via `include_str!`
  and served at `GET /assets/app.css` (chose include_str over ServeDir:
  one route, no service wiring, file ships inside the binary).
- Sequencing note (change-request, mid-planning): e15 extracts page
  HTML into Tera templates before this story runs — migrate the
  templates instead of the Rust string literals; the route and
  stylesheet steps are unchanged.

## 6. Requirements

#### MODIFIED: Site pages are styled
**Before:** Browser-default rendering; two stray class attributes; no
CSS file anywhere in the repo.
**After:** Every page links `/assets/app.css`; color, type, and spacing
come from DESIGN.md tokens; interactive elements expose hover,
focus-visible, active, and disabled states.

#### ADDED: Stylesheet route
`GET /assets/app.css` serves the embedded stylesheet with no auth or
CSRF change to existing routes.

#### ADDED: Reduced-motion fallback
Any transition or animation in the stylesheet ships a
`prefers-reduced-motion` block.

## 7. Actors

Any visitor (public pages) and authenticated users (settings/MR pages).

## 8. Preconditions

e12s01 (DESIGN.md tokens exist) and e15s01 (base layout template
exists) — this story styles the Tera templates, not Rust string
literals.

## 9. Postconditions

Site-wide consistent type, color, spacing; keyboard users see focus
rings; audit gate passes on the stylesheet.

## 10. Business rules

- Tokens only — no hardcoded hex or ad-hoc shadows outside the token
  definitions block.
- Class additions must keep existing HTML-substring tests green; where a
  test breaks, update the assertion, never drop the coverage.

## 11. Domain model touchpoints

None — presentation layer; no domain logic changes.

## 12. UI/UX notes

Migrate in test-green groups: stylesheet route → token base layer →
shell/nav + auth pages → repo/MR/tree pages → states → audit.

## 13. API contracts

One new GET route returning `text/css`.

## 14. Data model

None.

## 15. Non-functional requirements

Stylesheet served from memory (no per-request file read); first paint
needs one extra static request; no render-time JS.

## 16. Out of scope

JavaScript/htmx, layout and copy redesign, dark-mode toggle, kata iframe
styling (e14), per-page custom CSS.

## 17. Acceptance criteria

- [ ] `GET /assets/app.css` returns the stylesheet (test).
- [ ] Shell, auth, repo, MR, and browse pages link the stylesheet
      (tests).
- [ ] `:focus-visible`, `:disabled`, `prefers-reduced-motion` present.
- [ ] UX audit gate reports zero contrast/token/state failures.
- [ ] `cargo test --workspace` green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: stylesheet is served
  Given the server is running
  When I GET /assets/app.css
  Then the response is 200 with content-type text/css

Scenario: shell pages link the stylesheet
  Given the home page renders
  Then the HTML contains a stylesheet link to /assets/app.css
  And a keyboard focus style exists for links and buttons
```

## 19. Verification script (manual)

1. `cargo run` — open the home page: type and spacing match DESIGN.md.
2. Tab through the login form — focus ring visible on every control.
3. Run the UX audit on `crates/server/assets/app.css` — zero failures.
4. keystone render of migrated pages at 390/768/1440 — inspect the
   screenshots against the direction brief.

## 20. Traceability

`// story: e12s02` in the stylesheet route handler and migration tests.
