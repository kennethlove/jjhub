# e21s02 — Details screen preferences: name, theme, gravatar

## 1. Story

As a new user, the details screen also collects my display name and
theme preference and shows my account gravatar — so the profile is
mine from the first minute, not handle-only forever.

## 2. Business value

Completes the details screen's purpose: one screen, one completion
action, three preferences. Display name and theme are the two
identity dials every forge user expects; gravatar is derived (no
column, no sync job) with an honest link to gravatar.com to change
it.

## 3. Type

feat

## 4. Risk / context

risk: P2 (BCP 3, UI + two persisted fields) — context: columns
landed in e21s01; this story wires their effects and rendering.

## 5. Context

Zoom-out on the modules this touches:
- **details screen route/template (e21s01)** — gains the two
  inputs and the gravatar block; completion POST persists them.
- **session/render** — the theme value selects the rendering mode
  (`dark` | `light` | `system`); layout templates read it.
- **gravatar** — computed from the account email (md5 of the
  lowercased trimmed address per gravatar's spec) with a link to
  https://gravatar.com; derived at render time, never stored.

## 6. Requirements

#### ADDED: Display name persists with handle fallback
The details screen's display-name input saves to
users.display_name (NULL allowed). Anywhere the profile is shown,
NULL falls back to the handle — no "null" strings leak.

#### ADDED: Theme preference persists and applies
The theme select (dark | light | system, default system) saves to
users.theme and drives the rendered theme on every page; invalid
values fall back to system.

#### ADDED: Gravatar derived from email
The details screen shows the account gravatar (default
gravatar.com rating, identicon fallback) computed from the email,
with a visible link to gravatar.com to change it. Nothing is
stored; no webhook or sync.

## 7. Actors

Signed-in user (new signup or anyone reaching their profile).

## 8. Preconditions

e21s01 shipped (columns + screen exist).

## 9. Postconditions

Saving the screen persists both preferences; every later page
renders in the chosen theme and shows the display name (or handle
fallback).

## 10. Business rules

- Theme values are exactly the three tokens; anything else is
  treated as system.
- Display name is free text, trimmed, max length enforced, and is
  display-only (handles remain the identity for URLs and
  ownership checks).

## 11. Domain model touchpoints

**User** display_name, theme (read/write); gravatar is derived.

## 12. UI/UX notes

Inputs follow DESIGN.md tokens; theme preview applies on save;
audit gate runs before handoff (strict UX discipline).

## 13. API contracts

The details completion POST (e21s01) gains the two fields; the
profile/settings pages read them. No other route changes.

## 14. Data model

None new — e21s01's columns are used.

## 15. Non-functional requirements

Theme application must not flash (server-rendered attribute);
gravatar URL construction adds no network call at render time.

## 16. Out of scope

Avatar upload, email change, per-repo themes, profile pages beyond
existing settings.

## 17. Acceptance criteria

- [ ] `cargo test -p server display_name` — persists; NULL falls
      back to handle in every shown location. (e21s02t1)
- [ ] `cargo test -p server theme_preference` — three values
      persist and drive rendering; invalid falls back to system.
      (e21s02t2)
- [ ] `cargo test -p server gravatar` — URL derives from the
      lowercased email with the gravatar.com link present.
      (e21s02t3)
- [ ] `cargo test --workspace` green. (e21s02t4)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: display name with fallback
  Given a user whose display_name is NULL
  When their profile renders
  Then the handle appears, never an empty or null value

Scenario: theme applies on save
  Given the details screen with theme "dark"
  When the user completes the screen
  Then later pages render with the dark theme attribute

Scenario: gravatar is derived
  Given the account email "User@Example.com"
  When the details screen renders
  Then the gravatar URL uses the lowercased address hash
  And a link to gravatar.com is shown
```

## 19. Verification script (manual)

1. Open the details screen, set name and theme, complete it.
2. Reload any page — theme applied, name shown.
3. Clear the name in settings — handle shows instead.
4. Gravatar block links to gravatar.com.

## 20. Traceability

`// story: e21s02` in the details POST handler, the theme render
path, the gravatar helper, and the `display_name` /
`theme_preference` / `gravatar` tests.
