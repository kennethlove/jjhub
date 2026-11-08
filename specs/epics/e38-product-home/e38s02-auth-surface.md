# Story: e38s02 — auth surface

## Story

As a visitor, I want the product page to hold nothing account-related
— no signup, no push — with one quiet footer link to `/login` for
the operator who already runs an instance, so the page reads as a
product pitch, not a funnel.

## Business value

User directive (session 2026-11-07): the product page must have
nothing related to accounts; footer `/login` only. A self-hoster
evaluating the software is not a signup lead — every CTA that asks
them to register undermines the "run it yourself" story.

## Type

Feature (constraint enforcement)

## Risk context

`home.html` shares `nav.html` with logged-in surfaces; account
links that are correct elsewhere (dashboard, settings) must not
appear on this page. The rule is assertion-shaped: absence of
`/register` hrefs and signup/push CTA text, presence of exactly one
auth affordance — the footer link to `/login`. Absence assertions
must search the full rendered HTML, not a section.

## Requirements delta

- **Before:** the logged-out landing may carry account CTAs
  (shared nav links, any signup-style buttons).
- **After:** the product page carries exactly one auth
  affordance — a footer link to `/login` — and zero
  signup/register/push/account CTAs anywhere in the rendered
  page.

## Steps

1. `t1` — RED: extend `product_home.rs` with the auth-surface
   assertions: rendered logged-out HTML contains
   `href="/login"`, contains no `href="/register"`, and no
   signup/push CTA wording; the footer link is the only auth
   affordance counted. Fails against the current shared nav —
   that is the RED.
2. `t2` — GREEN: the logged-out branch renders without account
   CTAs — nav auth links suppressed on this page, footer carries
   the single `/login` link. Other pages' nav behavior untouched.

## Verification Script

- Run: `cargo test -p server --test product_home`
- Expect: failing at t1, passing after t2; suites exercising
  logged-in nav stay green.
- Check: grep of the rendered logged-out page shows one auth
  href (`/login`) and zero `/register` hrefs.
- Done when: the product page holds exactly one quiet way in and
  nothing that asks the visitor to join.

## Out of scope

- Removing login/register from other pages (dashboard, settings
  keep their nav).
- Disabling or relocating the auth routes themselves.

## Risks

- Nav is shared: suppressing links only in the logged-out product
  branch could leak into other rendered pages if done by
  global state — the assertion suite plus logged-in nav checks
  catch it.
