# specs/requirements/MAIL-LATEST.md — outbound mail requirements (draft v1)

Status: drafted 2026-11-04 from operator decisions in session (notice mail
yes, JSONL rollups, protocol-generic providers). Pending: slice into an epic.

## Purpose

Outbound mail covers the account lifecycle and moderation transparency.
Rule: **no secret actions** — every operator action that touches a user
generates an email to that user.

## Triggers

| Mail | When | Behavior |
| --- | --- | --- |
| Address verification | signup while `email_verified_at IS NULL` | resend endpoint with 60s cooldown; login stays gated (e23 behavior unchanged) |
| Password reset | user request | single-use token, 1h expiry, always-neutral response (no enumeration) |
| Suspension notice | operator suspends | immediate: what happened, operator's reason verbatim, appeal path (reply-to operator) |
| Takedown notice | operator takes a repo down | repo name, that it is reversible, who to ask |
| Unsuspend / restore notice | operator reverses an action | so the user knows they are restored without testing it |

## Transport and provider selection

Decision: **abstract over the protocol, not the provider list.**

1. **SMTP is the primary path** — one `SmtpMailer` (see Prior Art) covers
   every major provider and any self-hosted relay. jjhub maintains no
   provider list; the list lives in providers' own docs. Selection is
   explicit only: `JJHUB_EMAIL_PROVIDER=smtp` plus
   `JJHUB_SMTP_HOST/PORT/USER/PASSWORD/FROM`. No implicit choice from a
   key being present — a personal preference must never become a hardcode;
   it lives in deploy config.
2. **HTTP API providers are a deferred escape hatch** — if a provider
   without SMTP appears, add one declarative profile (endpoint, auth
   header, field map) instead of a coded arm. Not built until a real
   API-only need exists (YAGNI).
3. **Existing arms stay**: `resend` (production sugar) and `messagepit`
   (dev stub) remain explicit `JJHUB_EMAIL_PROVIDER` values;
   `LogMailer` is the safe default when the var is unset.

Unset/unknown provider = `LogMailer`: mail never leaves the box, no error,
signup/reset flows still succeed in-page.

## Prior art (research-first, 2026-11-04)

- **Choice: `lettre` 0.11.23** (MIT, MSRV 1.85, toolchain 1.99.1) —
  de-facto standard Rust SMTP client; prior art: axum/htmx production
  pattern at hda.daz.is.
- Features: `tokio1`, `tokio1-rustls`, `aws-lc-rs`, `pool`, `builder`.
  WHY rustls: repo already runs rustls (reqwest); native-tls would pull
  OpenSSL onto every self-hosted box. (Old `rustls-tls` alias deprecated
  in lettre 0.11.14.)
- `AsyncSmtpTransport` with connection pooling; `Tls::Required` for remote
  servers, `Tls::None` only for a trusted local relay, never default.
- Rejected: `mail` crate (stale), hand-rolled SMTP, SES API (SigV4 — SES
  goes through the SMTP arm).
- New dependency: justify at use site with a `WHY:` comment.

## Templates

Tera, HTML + plain-text pair per mail, one context per trigger. Tokens in
the link path (`/verify/<token>`), never the query string —
`email::verify_link` builds path form from the request origin.
Plain language; suspension/takedown mails quote the operator's reason
verbatim.

## Tokens and security

- Reuse `token::issue_email` / `token::verify`: single-use, expiring,
  stored hashed. No new token type.
- Verification TTL 24h (existing), reset TTL 1h.
- Send/resend endpoints rate-limited per the e20 pattern.
- Bodies carry only: what happened, why, the link, the recourse.

## Audit and retention

- One record per send (template, recipient, result, time).
- **Monthly JSONL rollup**, append-only:
  `logs/moderation_log_YYYY-MM.jsonl` and `logs/mail_log_YYYY-MM.jsonl`.
- One JSON object per line, timestamp-first:
  `{"ts":"2026-11-03T14:22:01Z","action":"suspend","actor":"alice","target":"bob","reason":"..."}`.
- Rotation by write date — no cron; a month-boundary burst lands in the
  correct file.
- `moderation_actions` (DB) stays the working copy for `/admin` and e33
  CSV; table keeps the current month, files are the durable unbounded
  history.
- Drain-ready by construction: a future `JJHUB_LOG_DRAIN=<url>` can ship
  the same lines to a log service with no second write path.
- Single write point for mail records: the `Mailer::send` call site.

## Failure behavior

- Provider unreachable or rejecting: fail the user-visible action with a
  clear retry message where the flow is the signup/resend path; password
  reset stays response-neutral either way. Never a 500 from a mail error.
- Disabled provider (default): degrade to in-page link (dev) or neutral
  message.

## Non-goals

No newsletters, digests, notification preference UI, inbound mail, or
third-party provider APIs beyond the existing Resend/SendGrid shapes.
Provider APIs are an external dependency only via research-first.

## Operator obligations

SPF/DKIM/DMARC on the sending domain are the operator's DNS job —
documented in README deploy section, not enforced by code.
