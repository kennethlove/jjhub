# e05s04 — MR pages link/embed kata review UI per change-id

## 1. Story

As a reviewer, the MR page for a change leads straight into its kata review view, so review context is one click away.

## 2. Business value

Completes the review-layer handoff: jjhub owns lifecycle/stack/merge; kata owns diff/comment UX. Without the link, users must find repos in the sidecar by hand.

## 3. Type

feat

## 4. Risk / context

risk: P2 — context: domain + infra glue.

## 5. Context

e03's MR detail page gains a "Review" section linking to the kata view for the MR's change-id (via `/review/{owner}/{name}/…` proxy from e05s03). Embed decision from spike: link-first (iframe embed optional follow-up; spike noted iframe constraints unexplored). Stack pages show per-MR links so a whole stack is reviewable in order.

## 6. Requirements

#### ADDED: Review link on MR detail
MR detail page shows a Review link targeting the kata view for the MR's change-id at its latest revision.

#### ADDED: Stack review links
MR list (stack view) shows review links per MR.

#### ADDED: Link only, no embed
Iframes are out of scope here (spike: constraints unexplored — separate spike if wanted).

## 7. Actors

Reviewer.

## 8. Preconditions

e05s03 shipped (proxy with auth). e03 MR pages exist.

## 9. Postconditions

Every open MR has exactly one working Review link.

## 10. Business rules

- Link present only when the change exists in the repo's jj metadata (gracefully absent for pre-backfill repos — no dead links).
- Links respect repo privacy through the proxy (no client-side leakage of private repo names beyond what MR page already shows).

## 11. Domain model touchpoints

**Merge Request** (change-id key), **Revision** (latest for link target).

## 12. UI/UX notes

Plain link ("Review in kata →") on Tera page; no styling of kata UI.

## 13. API contracts

None.

## 14. Data model

None.

## 15. Non-functional requirements

Link generation adds no external calls (pure template data from MR row).

## 16. Out of scope

Iframe embed, comment count badges from kata, bidirectional links (kata → jjhub MR).

## 17. Acceptance criteria

- [ ] MR detail page shows Review link; clicking it (as owner) lands on kata's view of that change.
- [ ] Stack view shows links for each MR.
- [ ] MR whose change lacks metadata has no link (no 404-link).

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: review link present
  Given open MR for change kpqr… in alice/demo
  When I open the MR detail page
  Then a Review link points to /review/alice/demo/…kpqr…
```

## 19. Verification script (manual)

1. Push stack, open MR page, click Review — kata shows the change.
2. Stack page — links per MR, ordered.

## 20. Traceability

`// story: e05s04` in implementing code and tests.
