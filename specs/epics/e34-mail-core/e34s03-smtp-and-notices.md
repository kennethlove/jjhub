# e34s03 — SMTP arm and moderation notices

## 1. Story

As a user whose account was suspended or whose repository was
taken down, I get an email immediately: what happened, the
operator's reason verbatim, and how to appeal; as an operator I
choose SMTP (or keep the default in-box arm) with explicit env
config — and a mail problem never fails the moderation action
itself.

## 2. Business value

The no-secret-actions promise of v0.7.0: every operator action
that touches a user now reaches that user, in plain language,
with the reason quoted as written — SCOPE v0.7.0 in_scope e34
(4 moderation notices verbatim, SMTP primary, no provider list).

## 3. Type

feat

## 4. Risk / context

risk: P1 — notices quote operator input verbatim into mail
bodies (render, never concatenate) and go to the affected user
only; the SMTP arm is the first outbound network path of its
kind here, so it is selected only by explicit
`JJHUB_EMAIL_PROVIDER=smtp` plus `JJHUB_SMTP_*` (unset still =
LogMailer, no implicit choice from a key being present).
`lettre` is a new dependency: WHY comment at the use site
(MAIL-LATEST prior art — de-facto standard, rustls, repo already
runs rustls). Best-effort send: an unreachable relay logs and
swallows; the moderation action's 303 stands.

## 5. Steps

1. RED cases in `crates/server/tests/mail_notices.rs`: each
   moderation action (suspend, unsuspend, takedown, restore)
   records exactly one delivery to the affected user with the
   action's template and the reason verbatim in the body;
   `JJHUB_EMAIL_PROVIDER=smtp` with `JJHUB_SMTP_HOST` selected
   as the arm (config parse, no network connect in test); an
   unreachable relay (smtp pointed at a dead port) still
   returns the action's normal 303 and its moderation_actions
   row; unset provider keeps every notice in-box → verify:
   `cargo test -p server --test mail_notices` (expected RED:
   no notice hooks, no SmtpMailer arm)
2. GREEN: `lettre`-backed SmtpMailer arm behind explicit env
   selection (WHY comment), notice hooks at the four
   moderation write sites, best-effort error swallowing →
   verify: `cargo test -p server --test mail_notices`
3. README deploy section: SMTP env table + SPF/DKIM/DMARC as
   the operator's DNS job (documented, not enforced) → verify:
   `cargo test -p server --test mail_notices`
4. Full gates → verify: `cargo fmt --all --check && cargo
   clippy --workspace --all-targets -- -D warnings && cargo
   test && cargo build`

## Verification Script

`cargo test -p server --test mail_notices` proves one delivery
per action with the reason verbatim, explicit-only SMTP
selection, and that mail failure never blocks an operator;
full preflight proves no regression.

## Out of scope

- HTTP-API provider profiles (deferred escape hatch), notices
  for registration flips (node-wide, not user-targeted),
  in-app notification centre, mail templates beyond the four
  notices + verification/reset.

## Risks

- Verbatim quoting must be HTML-escaped in the rendered body —
  the suite asserts an escaped operator reason, not raw markup.
