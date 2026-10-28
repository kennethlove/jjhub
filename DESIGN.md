---
version: alpha
name: jjhub Forge
description: Site-wide design tokens for jjhub — the hosting forge for Jujutsu repositories.
colors:
  primary: "#16211C"
  secondary: "#4A574F"
  tertiary: "#106845"
  tertiary-hover: "#0D5739"
  tertiary-pressed: "#0A4730"
  neutral: "#F4F6F4"
  surface: "#FBFCFB"
  surface-sunken: "#ECEFEC"
  surface-inverse: "#1D2A24"
  border: "#DCE2DD"
  on-accent: "#F7FAF8"
  on-inverse: "#E7ECE8"
  error: "#A32218"
  error-surface: "#FBEDEC"
typography:
  headline-display:
    fontFamily: IBM Plex Sans
    fontSize: 49px
    fontWeight: 600
    lineHeight: 1.1
    letterSpacing: -0.02em
  headline-lg:
    fontFamily: IBM Plex Sans
    fontSize: 39px
    fontWeight: 600
    lineHeight: 1.15
    letterSpacing: -0.015em
  headline-md:
    fontFamily: IBM Plex Sans
    fontSize: 31px
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: -0.01em
  headline-sm:
    fontFamily: IBM Plex Sans
    fontSize: 25px
    fontWeight: 600
    lineHeight: 1.3
    letterSpacing: -0.01em
  body-lg:
    fontFamily: IBM Plex Sans
    fontSize: 20px
    fontWeight: 400
    lineHeight: 1.6
  body-md:
    fontFamily: IBM Plex Sans
    fontSize: 16px
    fontWeight: 400
    lineHeight: 1.6
  body-sm:
    fontFamily: IBM Plex Sans
    fontSize: 14px
    fontWeight: 400
    lineHeight: 1.5
  label-md:
    fontFamily: IBM Plex Sans
    fontSize: 14px
    fontWeight: 500
    lineHeight: 1.4
  label-sm:
    fontFamily: IBM Plex Sans
    fontSize: 12px
    fontWeight: 500
    lineHeight: 1.3
    letterSpacing: 0.02em
  data-md:
    fontFamily: IBM Plex Mono
    fontSize: 14px
    fontWeight: 400
    lineHeight: 1.5
    fontFeature: '"tnum" 1'
  data-sm:
    fontFamily: IBM Plex Mono
    fontSize: 12px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"tnum" 1'
rounded:
  none: 0px
  sm: 4px
  md: 8px
  lg: 12px
  full: 9999px
spacing:
  half: 4px
  xs: 8px
  sm: 16px
  md: 24px
  lg: 32px
  xl: 48px
  2xl: 64px
components:
  button-primary:
    backgroundColor: "{colors.tertiary}"
    textColor: "{colors.on-accent}"
    typography: "{typography.label-md}"
    rounded: "{rounded.md}"
    height: 40px
    padding: 12px
  button-primary-hover:
    backgroundColor: "{colors.tertiary-hover}"
    textColor: "{colors.on-accent}"
    typography: "{typography.label-md}"
    rounded: "{rounded.md}"
    height: 40px
    padding: 12px
  button-primary-active:
    backgroundColor: "{colors.tertiary-pressed}"
    textColor: "{colors.on-accent}"
    typography: "{typography.label-md}"
    rounded: "{rounded.md}"
    height: 40px
    padding: 12px
  button-primary-disabled:
    backgroundColor: "{colors.surface-sunken}"
    textColor: "{colors.secondary}"
    typography: "{typography.label-md}"
    rounded: "{rounded.md}"
    height: 40px
    padding: 12px
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.primary}"
    typography: "{typography.label-md}"
    rounded: "{rounded.md}"
    height: 40px
    padding: 12px
  button-secondary-hover:
    backgroundColor: "{colors.surface-sunken}"
    textColor: "{colors.primary}"
    typography: "{typography.label-md}"
    rounded: "{rounded.md}"
    height: 40px
    padding: 12px
  button-secondary-disabled:
    backgroundColor: "{colors.surface-sunken}"
    textColor: "{colors.secondary}"
    typography: "{typography.label-md}"
    rounded: "{rounded.md}"
    height: 40px
    padding: 12px
  input:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.primary}"
    typography: "{typography.body-md}"
    rounded: "{rounded.md}"
    height: 40px
    padding: 12px
  input-disabled:
    backgroundColor: "{colors.surface-sunken}"
    textColor: "{colors.secondary}"
    typography: "{typography.body-md}"
    rounded: "{rounded.md}"
    height: 40px
    padding: 12px
  input-error:
    backgroundColor: "{colors.error-surface}"
    textColor: "{colors.primary}"
    typography: "{typography.body-md}"
    rounded: "{rounded.md}"
    height: 40px
    padding: 12px
  link:
    backgroundColor: "{colors.neutral}"
    textColor: "{colors.tertiary-hover}"
    typography: "{typography.body-md}"
    rounded: none
  link-hover:
    backgroundColor: "{colors.neutral}"
    textColor: "{colors.tertiary-pressed}"
    typography: "{typography.body-md}"
    rounded: none
  rule:
    backgroundColor: "{colors.border}"
    textColor: "{colors.secondary}"
    typography: "{typography.label-sm}"
    rounded: none
    height: 1px
  band-inverse:
    backgroundColor: "{colors.surface-inverse}"
    textColor: "{colors.on-inverse}"
    typography: "{typography.body-md}"
    rounded: none
  badge:
    backgroundColor: "{colors.surface-sunken}"
    textColor: "{colors.secondary}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.full}"
    padding: 12px
  badge-accent:
    backgroundColor: "{colors.tertiary}"
    textColor: "{colors.on-accent}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.full}"
    padding: 12px
  badge-error:
    backgroundColor: "{colors.error-surface}"
    textColor: "{colors.error}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.full}"
    padding: 12px
---

# jjhub Design System

## Overview

Direction: **the commit ledger, not a SaaS dashboard** · type IBM Plex Sans +
IBM Plex Mono · palette green-black ink `#16211C` on cool paper `#F4F6F4` with
one accent, status green `#106845` · signature: hairline-ruled data rows with
12-character mono revision IDs set in tabular numerals.

jjhub is a hosting forge for Jujutsu repositories. The look should feel like a
version-control workbench: ruled, dense, exact, and calm. Hierarchy comes from
scale, weight, and hairline rules — not shadows or decoration. The audience is
developers reading diffs, revisions, and merge requests, so the interface
prioritizes scanability and honest data over marketing polish.

The page background is cool tinted paper (`neutral`), never pure white. Text
and dark bands use green-black ink (`primary`), never pure black. One accent
(`tertiary`, status green) drives every interactive affordance; a clean
working copy is green, so green means "act here."

## Colors

One accent plus tinted neutrals. Every neutral is tinted with the same cool
green hue that inks the page; no pure `#ffffff` or `#000000` appears anywhere.

- **Primary (#16211C):** Green-black ink for headlines, body text, and the
  inverse band. Roughly 15:1 on paper.
- **Secondary (#4A574F):** Muted slate-green for metadata, captions, and
  disabled text. Roughly 7:1 on paper.
- **Tertiary (#106845):** The single accent — status green. Exclusive driver
  of links, primary buttons, focus rings, and accent badges. Interaction
  states use darkened shades of the same accent (`tertiary-hover` for resting
  links, which must clear AA on paper, `tertiary-pressed` for hover and
  press); no second hue is ever introduced.
- **Neutral (#F4F6F4):** Cool paper page background.
- **Surface (#FBFCFB):** Raised card and input background, one step above
  paper. Surface-sunken (#ECEFEC) sits below paper for wells, disabled
  controls, and hover rows. Surface-inverse (#1D2A24) is the dark band color.
- **Border (#DCE2DD):** Hairline separators and input outlines.
- **Error (#A32218) / error-surface (#FBEDEC):** Validation text and its tinted
  container. Used only where a failure exists; never decorative.
- **on-accent (#F7FAF8) / on-inverse (#E7ECE8):** Text colors that sit on the
  accent and on the inverse band.

## Typography

**IBM Plex Sans** carries display and body; **IBM Plex Mono** carries every
revision ID, hash, branch name, and numeric datum. The pairing reads
"engineering document": personality comes from weight contrast (600 display
against 400 body) and hairline rules, not decoration.

The scale is modular: base 16px, ratio 1.25, rounded to whole pixels —
12, 14, 16, 20, 25, 31, 39, 49. No one-off font sizes are permitted; every
size a page uses must map to a `typography` token.

- **Headlines:** `headline-display` (49px) through `headline-sm` (25px),
  weight 600, tight leading, negative tracking.
- **Body:** `body-lg` (20px) leads, `body-md` (16px) reads, `body-sm` (14px)
  supports — all 400 weight, 1.5–1.6 leading.
- **Labels:** `label-md` / `label-sm` at 500 weight for buttons, badges, and
  field labels. Sentence case, never tracked-out all caps.
- **Data:** `data-md` / `data-sm` in IBM Plex Mono with `tnum` so revision
  columns align.

## Layout

An 8px spacing grid governs the whole site: `half` (4px) is the single
micro-step, then `xs` 8, `sm` 16, `md` 24, `lg` 32, `xl` 48, `2xl` 64. Every
margin, padding, and gap snaps to these tokens; no magic pixel values.

Content lives in a fixed max-width column (72rem) with `lg` page margins.
Tables and list rows are hairline-ruled (`border`) with equal row heights and
one datum per column; numeric and mono columns use `data-*` typography with
tabular numerals. Related items group by whitespace and background shift
before a border, and a border before a shadow.

## Elevation & Depth

Depth is mostly tonal: paper → surface → inverse band. When an element must
lift (menus, dialogs), use exactly four named elevation levels — `sm`, `md`,
`lg`, `xl`. No ad-hoc shadows: every box-shadow maps to one level.

- **sm:** `0 1px 2px rgba(22, 33, 28, 0.06)` — resting cards.
- **md:** `0 2px 6px rgba(22, 33, 28, 0.08)` — hovered cards, dropdowns.
- **lg:** `0 6px 16px rgba(22, 33, 28, 0.10)` — popovers, side panels.
- **xl:** `0 12px 32px rgba(22, 33, 28, 0.14)` — modals only.

Order is strict: a modal is nearer than a card, a dropdown nearer than the
page, and that order never changes between components.

## Shapes

Corners are slightly softened, never bubbly: `rounded-none` for tables and
full-bleed bands, `rounded-sm` (4px) for badges of text, `rounded-md` (8px)
for buttons, inputs, and cards, `rounded-lg` (12px) for dialogs, and
`rounded-full` for pills and avatars. One radius per component family; never
mix sharp and soft corners in the same view.

## Components

### State contract

Every interactive element documents all five states, and any component that
renders one of them ships the token values in `components` above:

- **Default** — the resting token values.
- **Hover** — background shifts one step (surface → surface-sunken, accent →
  tertiary-hover); cursor pointer on clickables.
- **Focus-visible** — 2px `tertiary` outline, 2px offset, on `:focus-visible`
  only; never remove focus indication.
- **Active (pressed)** — accent → tertiary-pressed, 1px translate or shadow
  drop to reinforce the press.
- **Disabled** — surface-sunken background with secondary text, no pointer
  events, `aria-disabled` set; never communicated by color alone.

Status states where relevant:

- **Error** — `error` text on `error-surface`, with a message tied to the
  field via `aria-describedby`; never color alone, always an icon or word.
- **Empty** — an empty list renders one line of `secondary` text naming what
  is missing plus the primary action that creates it; never a blank canvas.
- **Loading** — a `secondary` spinner or skeleton rows in `surface-sunken`;
  buttons in flight keep their width and show a disabled-style busy state.

### Motion

Motion only confirms an action (a row expanding, a toast appearing): 120–180ms
ease-out, transform and opacity only. Every transition or animation in the
codebase ships a `prefers-reduced-motion: reduce` fallback that removes the
motion. No autoplaying, no parallax, no ambient animation.

### Signature details

Hairline-ruled data rows with 12-character mono revision IDs in tabular
numerals are the signature. Buttons are 40px tall with `rounded-md`; inputs
match. Badges are pills (`rounded-full`) and carry state color only when the
state is real (open, merged, conflict, error).

## Do's and Don'ts

- Do pull every color, size, radius, and shadow from a named token.
- Don't hardcode hex values, magic pixels, or invent a fifth shadow.
- Do use the accent (`tertiary`) for exactly the primary action per view.
- Don't introduce a second accent, a gradient, or a glow.
- Do separate content with whitespace, then background shift, then a border —
  a border is the last resort, not the first.
- Do ship `:focus-visible`, `:disabled`, and a `prefers-reduced-motion`
  fallback with every interactive component.
- Don't use pure `#ffffff` or `#000000` for page backgrounds or text.
- Don't use one-off font sizes; add a `typography` token instead.
