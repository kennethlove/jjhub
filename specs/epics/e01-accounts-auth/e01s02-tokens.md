# e01s02 — Personal access tokens for git transport

## 1. Story

As a self-hoster, I create and revoke personal access tokens so git/jj transport can authenticate without my password.

## 2. Business value

HTTP smart git auth (e02) depends on tokens. Passwords never travel on the wire.

## 3. Type

feat

## 4. Risk / context

risk: P0 (auth boundary, BCP 3) — context: infra

## 5. Context

Tokens page on /settings: generate (shown once, full value), list (prefix only), revoke. Token auth extractor validates Authorization header for git transport routes (used fully in e02).

## 6. Requirements

#### ADDED: Token generation
Logged-in user generates a token with a label. Full token value shown once at creation; only prefix stored/displayed after.

#### ADDED: Token revocation
User revokes own token; revoked token stops authenticating immediately.

#### ADDED: Transport auth extractor
Git transport routes accept HTTP Basic with username "x-access-token" and token as password; extractor resolves token → user or rejection.

## 7. Actors

User (token owner).

## 8. Preconditions

e01s01 shipped (sessions exist).

## 9. Postconditions

Active token authenticates transport extractor; revoked token does not.

## 10. Business rules

- Tokens stored hashed (argon2 or SHA-256; prefix stored plaintext).
- Token value = random 40+ chars, unique.
- Revocation is hard delete or revoked flag — both acceptable.

## 11. Domain model touchpoints

**Token** (new): id, user_id, label, prefix, hash, created_at, last_used_at (nullable).

## 12. UI/UX notes

Settings page section; copy-once banner with warning.

## 13. API contracts

Extractor contract only; no JSON API in this story.

## 14. Data model

tokens(id uuid pk, user_id fk users, label text, prefix text, token_hash text, created_at, revoked_at timestamptz null).

## 15. Non-functional requirements

Token plaintext never logged or stored.

## 16. Out of scope

Scopes/permissions per token, token expiry, OAuth apps.

## 17. Acceptance criteria

- [ ] Generate shows full token once; reload shows prefix only.
- [ ] Extractor resolves valid token to owning user.
- [ ] Revoked token rejected by extractor.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: token shown once
  Given I am logged in
  When I generate a token labeled "laptop"
  Then the full token is displayed exactly once
  And the tokens list shows only the prefix

Scenario: revoked token rejected
  Given I have an active token
  When I revoke it
  Then the transport extractor rejects it
```

## 19. Verification script (manual)

1. Log in, /settings → generate token "laptop".
2. Copy token; reload page → only prefix visible.
3. `curl -u "x-access-token:<token>" localhost:3000/...` (e02 route; until then extractor unit test).
4. Revoke → same curl fails.

## 20. Traceability

`// story: e01s02` in implementing code and tests.
