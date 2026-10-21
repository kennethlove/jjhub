# e12s01 — DESIGN.md token system

## 1. Story

As a contributor styling any jjhub page, I have a repo-root DESIGN.md that
defines the exact tokens, so every UI change pulls color, type, spacing,
radius, elevation, and states from one source instead of inventing values.

## 2. Business value

Pages today are unstyled inline HTML — no `.css` file exists anywhere in
the repo. This story creates the token set that e12s02 applies and that
e13/e14 consume; without it each later epic would invent its own styles.

## 3. Type

feat

## 4. Risk / context

risk: P3 — context: design documentation and a lint gate; no runtime
behavior changes.

## 5. Context

New root file `DESIGN.md` following the Google design.md standard: YAML
frontmatter tokens (colors, typography, spacing, rounded, elevation) plus
rationale sections (Overview, Colors, Typography, Layout, Elevation,
Shapes, Components, Do's & Don'ts). Contents: one accent color plus
tinted neutrals, modular type scale, 8px spacing grid, named elevation
levels, and a state contract (hover, focus-visible, active, disabled,
error/empty/loading, prefers-reduced-motion for any motion). Lint gate:
`npx @google/design.md lint DESIGN.md` — CLI v0.4.0 verified available
(`npm view @google/design.md version` → `0.4.0`) from this machine.

## 6. Requirements

#### ADDED: DESIGN.md at repo root
Frontmatter tokens plus rationale sections; one accent, tinted
neutrals, no pure #fff/#000 page backgrounds.

#### ADDED: State contract
Every interactive element documents hover, focus-visible, active, and
disabled states; error/empty/loading where relevant; any transition or
animation ships a prefers-reduced-motion fallback.

#### ADDED: Lint gate
`npx @google/design.md lint DESIGN.md` exits 0 on the committed file.

## 7. Actors

Contributors and coding agents that style pages.

## 8. Preconditions

None — greenfield documentation.

## 9. Postconditions

A named token exists for every color, size, and shadow a page may use;
the lint command proves the file valid.

## 10. Business rules

- One accent color; tinted neutrals only.
- Elevation: named levels only (sm, md, lg, xl); no ad-hoc shadows.
- Type: modular scale only; no one-off font sizes.

## 11. Domain model touchpoints

None — presentation layer only.

## 12. UI/UX notes

The file also fixes the direction (type voice, palette mood,
signature element) that e12s02 migration follows.

## 13. API contracts

None.

## 14. Data model

None.

## 15. Non-functional requirements

Lint gate deterministic; file reviewable in one sitting.

## 16. Out of scope

Applying tokens to pages (e12s02), asset serving, kata UI, per-page
redesign, dark-mode toggle.

## 17. Acceptance criteria

- [ ] `DESIGN.md` exists at repo root with required sections.
- [ ] `npx @google/design.md lint DESIGN.md` exits 0.
- [ ] State contract and reduced-motion fallback are documented.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: lint gate passes on the committed file
  Given DESIGN.md exists at the repo root
  When I run the design.md lint CLI
  Then it exits 0 with no errors
```

## 19. Verification script (manual)

1. Open `DESIGN.md` — frontmatter tokens and rationale sections present.
2. Run `npx @google/design.md lint DESIGN.md` — exit 0.

## 20. Traceability

`// story: e12s01` in the preflight/audit documentation added by e12s03.
