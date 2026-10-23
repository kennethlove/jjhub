# e14s02 — Embed states, fallback, reduced motion

## 1. Story

As a reviewer, the inline kata embed has a loading state while the frame
fetches, a working fallback link when it cannot render, and respects
reduced-motion preferences — so the embed degrades honestly instead of
showing a blank box.

## 2. Business value

Hardens the e14s01 inline embed into a complete user-facing state. The
sidecar can be slow or unreachable; without explicit states the reviewer
sees a silent blank frame — the same dead-end class e05s04's metadata
guard was written to prevent. Reduced-motion compliance is the repo-wide
UX discipline (e12 token system, `prefers-reduced-motion` fallback
required for anything that moves).

## 3. Type

feat

## 4. Risk / context

risk: P2 (BCP 2, presentation-only) — context: UI (MR template markup +
stylesheet; no server I/O beyond e14s01).

## 5. Context

Zoom-out on the modules this touches:
- **MR template** (`routes.rs::mr_detail_page`, extracted by e15s03)
  purpose — render one merge request's detail page. **Callers** — lib.rs
  router. **Contracts** — axum handler; tests assert HTML substrings. It
  embeds the kata iframe from e14s01; this story adds the state layer
  around that element.
- State design, all server-rendered markup, no JS orchestration:
  - **Loading**: a server-rendered placeholder inside the frame container
    (title/aria text so screen readers announce the pending embed);
    `<noscript>` keeps the e05s04 Review link visible as the universal
    fallback.
  - **Fallback**: the existing e05s04 per-change link sits beside the
    frame container and is always present; when the frame cannot load
    (blocked, offline sidecar), the reviewer still has a working
    navigation path — same URL, no new scheme.
  - **Reduced motion**: the container's reveal animation (loading →
    loaded fade/transition, from e12's token system) ships a
    `prefers-reduced-motion: reduce` rule that disables it; the embed
    appears instantly.
- Styling comes from the e12 stylesheet; no ad-hoc values.

## 6. Requirements

#### ADDED: Loading state and fallback link
The embed container renders a loading placeholder (announced, not
silent); the e05s04 Review link remains beside it as a visible fallback;
`<noscript>` exposes the link for frame-less contexts.

#### ADDED: Reduced-motion compliance
Any transition or animation on the embed container is disabled under
`prefers-reduced-motion: reduce`; the embed renders statically.

## 7. Actors

Reviewer on an MR detail page, including users with reduced-motion
preferences and users with frames disabled.

## 8. Preconditions

e14s01 (inline iframe renders on the MR page), e05s04 (per-change link
exists), e12s02 (stylesheet + token system).

## 9. Postconditions

Every reviewer — slow sidecar, blocked frame, or reduced motion — gets an
honest, usable MR page.

## 10. Business rules

- The fallback link is never conditional: it ships on every MR page.
- Placeholder copy uses jjhub's words (§ Design writing), no apology
  framing.
- No new colors, sizes, or spacing outside the e12 tokens.

## 11. Domain model touchpoints

None beyond e14s01's MR change-id key.

## 12. UI/UX notes

Placeholder and fallback inside the e12 token system; focus-visible on
the fallback link; reduced-motion rule targets the embed container only.

## 13. API contracts

None.

## 14. Data model

None.

## 15. Non-functional requirements

Zero extra server requests; zero JavaScript beyond the iframe element.

## 16. Out of scope

Retry logic, iframe auto-resize messaging, comment-count badges, error
telemetry, embed for non-MR pages, per-user preference storage.

## 17. Acceptance criteria

- [x] `cargo test -p server embed_states` — MR detail page renders the
      loading placeholder inside the embed container, keeps the e05s04
      Review link visible beside the container, and includes the
      `<noscript>` fallback link. (e14s02t1)
- [x] `verify-script: confirm prefers-reduced-motion disables embed animation`
      — the e12 stylesheet contains the embed transition rule and a
      `prefers-reduced-motion: reduce` rule that disables it.
      (e14s02t2)
- [ ] `cargo test --workspace` green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: embed shows loading state with fallback
  Given an open MR for change kpqr… in alice/demo
  When I GET the MR detail page
  Then the embed container shows a loading placeholder
  And the kata Review link is visible beside the container
  And a no-JS client still gets the Review link

Scenario: reduced motion disables the embed animation
  Given a user with prefers-reduced-motion: reduce
  When the MR page loads
  Then the embed container renders without transition or animation
```

## 19. Verification script (manual)

1. Open an MR page — placeholder appears, then the kata view; Review link
   always visible beside it.
2. Block the sidecar (or inspect the stylesheet) — link remains the only
   path and works.
3. DevTools → emulate `prefers-reduced-motion: reduce` — embed appears
   with no animation.

## 20. Traceability

`// story: e14s02` in the embed state markup and the reduced-motion
stylesheet rule.
