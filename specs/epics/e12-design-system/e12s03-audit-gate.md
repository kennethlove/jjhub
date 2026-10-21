# e12s03 — Audit gate documented in conventions

## 1. Story

As a contributor, CONVENTIONS.md tells me the design audit step exists
and where the token source lives, so style changes go through the same
gate as formatting and tests.

## 2. Business value

A gate nobody can find is not a gate; this makes the audit discoverable
in the doc every agent reads first.

## 3. Type

feat

## 4. Risk / context

risk: P3 — context: documentation only.

## 5. Context

`CONVENTIONS.md` gains a short design-audit section: DESIGN.md is the
token source; run the stylesheet audit before declaring UI work done.
Preflight command list itself stays as-is (the audit runs on UI
changes, not every cargo preflight).

## 6. Requirements

#### ADDED: Design audit section in CONVENTIONS.md
Names DESIGN.md as the token source and the audit as the UI-change gate.

## 7. Actors

Contributors and coding agents.

## 8. Preconditions

e12s01 (DESIGN.md exists), e12s02 (stylesheet exists to audit).

## 9. Postconditions

The design gate is one grep away from any agent starting UI work.

## 10. Business rules

None beyond doc accuracy.

## 11. Domain model touchpoints

None.

## 12. UI/UX notes

None.

## 13. API contracts

None.

## 14. Data model

None.

## 15. Non-functional requirements

Section ≤ 10 lines.

## 16. Out of scope

Editing AGENTS.md (managed block), changing the cargo Preflight chain.

## 17. Acceptance criteria

- [ ] CONVENTIONS.md references DESIGN.md and the audit step.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: gate is discoverable
  Given CONVENTIONS.md documents the design audit
  When an agent searches the conventions for DESIGN.md
  Then the audit step is found
```

## 19. Verification script (manual)

1. Open CONVENTIONS.md — design-audit section present and accurate.

## 20. Traceability

`// story: e12s03` in the CONVENTIONS.md section.
