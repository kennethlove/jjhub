# Story: e37s01 — mail pair

## Story

As a mail recipient, I want every mail to arrive as an HTML arm and a
plain-text arm rendered from one context, so the HTML mail reads like
the product and the text arm survives clients that strip HTML.

## Business value

MAIL-LATEST §Templates asks for "Tera, HTML + plain-text pair per
mail, one context per trigger"; §Bodies fixes the content contract
(what happened, why, link, recourse). Today every mail is a hand-built
plain string: readable, but no HTML arm for clients that render it,
and two future copies of the same copy if HTML is bolted on per call
site. One context rendered into both arms keeps the wording — and the
neutral, no-enumeration rules — identical by construction.

## Type

Feature

## Risk context

The pair rides on `Mailer::send`, a cross-module interface: 5 source
impls (Log, Resend, SendGrid, Logged wrapper, SMTP) and 6 test
RecordingMailers across the suite, plus 5 call sites in `routes.rs`
(signup verify, resend, login auto-resend, reset, moderation notice).
A signature change ripples everywhere mechanically; the copy does
not — the text arm must stay the complete fallback (what happened,
why, link, recourse), the HTML arm must be self-contained (mail
clients block external stylesheets), the verify link must stay
path-form and absolute in both arms, and unknown-address wording
must stay identical in both (no enumeration leak through HTML).

## Steps

1. `t1` — RED: write `crates/server/tests/mail_html_text.rs`
   asserting the contract: the mail pair renderer produces a text arm
   and an HTML arm from one context for `verify`, `reset`, and
   `moderation`; both arms carry the absolute path-form link (verify,
   reset) or the verbatim operator reason (moderation); the HTML arm
   is self-contained (inline styles, no external stylesheet link);
   the text arm is non-empty and carries what happened/why/link;
   a signup through `AppState.mailer` records an HTML arm next to
   the text arm with identical link and identical unknown-address
   wording (neutrality holds in both arms). Suite fails to compile
   against today's `send(&self, to, subject, body)` — that is the
   RED.
2. `t2` — GREEN: widen `Mailer::send` to carry both arms
   (`send(&self, to, subject, text, html)`), add the Tera mail
   templates (`templates/mail/{verify,reset,moderation}.{txt,html}`)
   rendered from one context per trigger through a `mail_pair`
   helper beside the existing web render, convert the 5 call sites
   to build the context once, and carry both arms through every
   impl: Resend `html` field, SendGrid an html part, SMTP
   multipart/alternative, Log/Recording record the text arm (and the
   HTML arm where the recorder holds it).
3. `t3` — Gates: `cargo fmt --all -- --check`, `cargo clippy
   --workspace --all-targets -- -D warnings`, full `cargo test`,
   `cargo build`, plus the ux_audit pass on any stylesheet touched
   (none expected — mail HTML is inline, not app.css).

## Verification Script

- Run: `cargo test -p server --test mail_html_text`
- Expect: failing at t1 (compile RED against the old signature),
  passing after t2; the pre-existing mail suites (mail_core,
  reset_mail, mail_notices, email_confirmation, send_retry,
  delivery_readout, auth_reset) stay green after the signature
  change.
- Check: one context per trigger — grep shows each call site builds
  a single Tera context rendered into both arms, no per-arm copy.
- Done when: every mail type ships both arms with neutral wording
  intact and the full gate set is green.

## Out of scope

- Visual design beyond readable structured HTML (no images, logos,
  or a mail design system).
- An explicit `template` field in the audit record (the subject
  line still identifies the mail; add when the readout needs it).
- HTML for any web page (already Tera-served) and attachments of
  any kind.

## Risks

- The signature change is wide (12 impls, 5 call sites); a missed
  impl surfaces as a compile error, a missed call site as a stale
  one-arm mail — the new suite catches the contract either way.
- lettre's multipart construction is new code on the SMTP arm; a
  malformed multipart must fail loudly (MailError), never silently
  drop the HTML arm.
