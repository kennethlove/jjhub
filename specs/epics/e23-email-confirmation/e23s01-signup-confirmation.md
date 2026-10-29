---
id: e23s01
title: "e23s01 — Email confirmation on sign up (messagepit dev / resend prod)"
status: not_started
depends_on: []
---

# e23s01 — Email confirmation on sign up

## Intent

As a new account, I must confirm my email before the site works, so
outbound mail has a live recipient.

Delivery is provider-pluggable behind one `Mailer` trait:

- **dev** = [messagepit](https://github.com/coreydaley/messagepit)
  SendGrid v3 stub (`POST http://localhost:8100/v3/mail/send`,
  Bearer optional) — the app speaks the REAL SendGrid protocol locally.
- **prod** = resend (`POST https://api.resend.com/emails`, Bearer key).
- **fallback** = log provider (stdout) when unconfigured, so tests and
  bare `cargo run` never need a mail server.

## Story

**As** a new account,
**I want** a confirmation link in my inbox that I click once,
**So that** I can use the site.

### Gherkin

```gherkin
Scenario: Sign up means nothing until the click
  Given I POST /register with a fresh email
  When the account row is created
  Then I am redirected to /verify-sent with NO session cookie
  And an email_verifications row exists with a 24h expiry
  And the email carries /verify?token=<plaintext> (hash stored, never plaintext)

Scenario: The click is the gate
  Given I signed up but have not clicked
  When I GET /verify?token=<plaintext>
  Then the row is marked used and users.email_verified_at is set
  And I receive a session and land on /token-details (e21s01 handoff intact)

Scenario: Expired link explains the retry
  Given my link is older than 24h or already used
  When I GET /verify?token=<whatever>
  Then I land on /login with a plain-English message about the expired link

Scenario: Login while unverified gets the same treatment
  Given I signed up but have not clicked
  When I POST /login with the right password
  Then I get no session and a plain-English "confirm your email" message
  And the attempt sends a fresh confirmation email (resend on login)
```

## Acceptance

- t1 `cargo test -p server email_confirmation` — signup gate, click
  gate, expiry, login refusal, resend-on-login.
- t2 `cargo test -p server mailer` — provider payload shapes (resend +
  messagepit SendGrid v3) without network.
- t3 `cargo test --workspace` — no gate regressions.

## Design decisions

- Migration `0014_email_confirmation.sql`: `users.email_verified_at
  TIMESTAMPTZ NULL` + `email_verifications(id, user_id FK CASCADE,
  token_hash UNIQUE, expires_at, used_at NULL, created_at)`. Token
  stored as sha256 (same lesson as e22s02 lookup_hash; plaintext only
  in the link). New send deletes the user's previous unused rows.
- `email.rs`: `trait Mailer { async fn send(&self, to, subject, body)
  -> Result<(), MailError>; }` + `ResendMailer` + `MessagepitMailer`
  + `LogMailer`. AppState gains `mailer: Arc<dyn Mailer + Send + Sync>`
  built from env in `app()` (`JJHUB_EMAIL_PROVIDER` resend|messagepit|
  log [default], `RESEND_API_KEY`, `JJHUB_MESSAGEPIT_URL` default
  `http://127.0.0.1:8100`, `JJHUB_MAIL_FROM` default
  `jjhub@localhost`). Test constructors pass `LogMailer`.
- `register`: mint handoff token (unchanged, e21s01) then mint
  verification, send mail, redirect `/verify-sent` — NO session.
  Send failure after the insert → 502 plain message (known edge:
  account exists unverified; resend-on-login recovers).
- `POST /login`: after password OK, unverified → delete+resend
  verification, respond 422 login page message, no session.
- `GET /verify?token=`: sha256 lookup where used_at IS NULL, unexpired
  → tx marks used + verified → session → `/token-details`. Anything
  else → `/login` with expired-link message. GET is idempotent-safe by
  one-time use.

## Out of scope

- Changing signup token mint (e21s01 handoff stays), SMS, marketing
  mail, per-org sending domains, unsubscribe.
