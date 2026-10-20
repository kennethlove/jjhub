# e14s01 — Inline kata embed on merge request pages

## 1. Story

As a reviewer, the merge request page shows the kata review view for the
MR's change inline, so review context stays on the MR page instead of
splitting across two navigations.

## 2. Business value

Delivers the SCOPE success criterion: "An MR page shows the kata review
UI inline with no navigation away from the MR." The scope states why: "the
sidecar is part of reviewing, not a destination." e05s04 shipped the
per-change link and deliberately deferred the embed ("iframe constraints
unexplored"); this story closes that gap. The v0.2.0 prior-art table
records the vehicle: "kata same-origin forwarder | extend | this repo
PR #25 (/r /api /assets); no CSP or frame headers exist yet — set on
embed."

## 3. Type

feat

## 4. Risk / context

risk: P1 (BCP 3, presentation over shipped proxy with one new security
header) — context: UI + infra (MR page markup, frame-policy header on the
embed route).

## 5. Context

Zoom-out on the modules this touches:
- **review_proxy.rs purpose** — same-origin forwarder to the kata
  sidecar: `/r/{owner}__{name}/…` SPA, `/api/…`, `/assets/…`
  (`spa_route`, `api_route`, `assets_route`), plus the `/review/…` proxy.
  **Callers** — lib.rs router registrations. **Contracts** —
  authenticate → authorize repo → strip trust headers → inject jjhub's →
  forward; responses stream back unchanged (no CSP/frame headers exist
  today, per prior art).
- **mr_detail_page purpose** — the MR detail page (`routes.rs`).
  **Callers** — lib.rs router. **Contracts** — axum handler; tests assert
  HTML substrings. It already renders the e05s04 Review link from the
  same-origin `/review/…` URL.
- The MR detail page gains an `<iframe>` whose `src` is the same-origin
  kata URL for the MR's change (the e05s04 link target — no new URL
  scheme). The existing Review link stays as the no-JS/no-frame fallback
  (state wiring lands in e14s02).
- The embed route (the `/r/…` SPA path the iframe loads) responds with
  `Content-Security-Policy: frame-ancestors 'self'` so only same-origin
  pages may frame it; third-party framing is refused. The header is set
  on the embed responses only — direct navigation is unaffected.
- Sequencing: e15s03 owns MR template extraction; this story lands after
  it and edits the extracted MR template (release order: e15 before e14).

## 6. Requirements

#### ADDED: Inline embed on the MR page
The MR detail page embeds the kata view for the MR's change in an iframe
served same-origin through the existing forwarder; the page contains no
upstream (sidecar) host anywhere.

#### ADDED: Frame policy on the embed route
The embed route answers with `Content-Security-Policy: frame-ancestors
'self'`; only same-origin documents may frame it. The e05s04 Review link
remains as fallback for no-frame contexts.

## 7. Actors

Reviewer viewing an MR detail page (repo-authorized; the proxy enforces
authz inside the frame exactly as it does for the link).

## 8. Preconditions

e05s03 (auth proxy with `/r` forwarder), e05s04 (per-change review link),
e15s03 (MR template extraction), e12s02 (stylesheet for the frame
container).

## 9. Postconditions

Opening an MR shows the change's kata view without leaving the page;
third-party origins cannot frame the embed.

## 10. Business rules

- Iframe src is always same-origin — the sidecar host never appears in
  jjhub HTML.
- Frame authz is the proxy's, unchanged: an unauthorized viewer gets the
  same refusal inside the frame as in the link.
- The e05s04 link is never removed by the embed.

## 11. Domain model touchpoints

**Merge Request** (change-id key — same key the e05s04 link uses).

## 12. UI/UX notes

Iframe container sized from DESIGN.md tokens; loading/fallback states and
reduced motion land in e14s02. No JS beyond the iframe element.

## 13. API contracts

One response header added on the embed route:
`Content-Security-Policy: frame-ancestors 'self'`. No new endpoints.

## 14. Data model

None.

## 15. Non-functional requirements

One extra same-origin request per MR view (the frame); the MR page itself
gains no proxy round-trips; header adds no per-request I/O.

## 16. Out of scope

Embed loading/fallback states and reduced motion (e14s02), auto-resizing
iframe messaging, comment-count badges from kata, bidirectional links
(kata → jjhub MR), styling of kata's own UI, MR template extraction
(e15s03), auth changes.

## 17. Acceptance criteria

- [ ] `cargo test -p server embed_iframe` — MR detail page renders an
      iframe whose src is the same-origin forwarder URL for the MR's
      change; the HTML contains no upstream host. (e14s01t1)
- [ ] `cargo test -p server embed_csp` — the embed route's response
      carries `frame-ancestors 'self'`; other jjhub routes are unchanged.
      (e14s01t2)
- [ ] `cargo test --workspace` green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: MR page embeds the change inline
  Given an open MR for change kpqr… in alice/demo
  When I GET the MR detail page
  Then the page contains an iframe with a same-origin src for that change
  And the response body contains no sidecar host

Scenario: third-party origins cannot frame the embed
  Given the kata embed route
  When I GET it
  Then the response has Content-Security-Policy: frame-ancestors 'self'
```

## 19. Verification script (manual)

1. Open an MR page — kata renders inside the page, URL unchanged.
2. `curl -sI` the iframe's src — `frame-ancestors 'self'` present.
3. View source — no upstream host in any attribute.

## 20. Traceability

`// story: e14s01` in the MR template's embed block, the embed-route
header, and the `embed_iframe` / `embed_csp` tests.
