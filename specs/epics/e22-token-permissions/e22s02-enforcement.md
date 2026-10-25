# e22s02 — Enforcement: authenticate rejects, authorize scopes

## 1. Story

As an operator, a token's permissions are enforced: expired,
foreign-repo, and off-list tokens are rejected at authentication;
read scope is required for clone/fetch and write for push — one
admission decision shared by HTTP and SSH.

## 2. Business value

The payload slice of token permissions: a leaked read-only token
is a bounded problem, an expired one stops itself, a pinned one
dies outside its repo, and an allowlisted one dies off-box —
identically on both transports, no new dependency (Postgres
`inet <<=` does the CIDR math).

## 3. Type

feat

## 4. Risk / context

risk: P0 (BCP 5, auth boundary on the credential path) — context:
token::authenticate (token.rs:48) and transport admission
(authorize) change behavior for restricted tokens only; full-scope
tokens keep working (backfill guarantee from e22s01).

## 5. Context

Zoom-out on the modules this touches:
- **token::authenticate** (token.rs:48) — purpose: map plaintext
  to user id. Callers: transport::require_token (HTTP),
  ssh.rs password auth. Contracts: it now also loads restrictions
  and rejects expired / foreign-repo / wrong-IP tokens in the same
  SQL (one round trip, `allowed_inets @> inet $client_ip` using
  Postgres' `inet <<=` semantics — no Rust CIDR crate).
- **transport authorize path** (transport.rs:348
  push_or_read_allowed / authorize) — purpose: one admission
  decision. Callers: smart_http and ssh.rs. Contracts: upload-pack
  maps to read scope, receive-pack to write; both transports call
  the same helper, so scope enforcement is written once.
- **receive_guard::admit_push** — unchanged; scope rejection
  happens before admission.

## 6. Requirements

#### MODIFIED: authenticate rejects restricted tokens
**Before:** authenticate matches hash + revoked_at and returns the
user id for any live token.
**After:** it also loads scopes/expires_at/repo_id/allowed_inets
and rejects (None) when the token is expired, the requested repo
is not its pin, or the client IP is outside the allowlist
(NULL allowlist = unrestricted).

#### MODIFIED: authorize maps operations to scopes
**Before:** any authenticated token may push or fetch per the
existing visibility/ownership rules.
**After:** inside the same shared admission decision, UploadPack
requires the read scope and ReceivePack the write scope; a token
missing the scope is refused on both transports with the same
denial shape as today's forbidden responses.

## 7. Actors

jj/git user over HTTP or SSH; token holder.

## 8. Preconditions

e22s01 shipped (columns + backfill); e19 admission helper in place
so both transports share one decision.

## 9. Postconditions

Full-scope tokens behave exactly as before; a read-only token
clones but its push is refused; expired/pinned/off-list tokens are
rejected at authentication on both transports.

## 10. Business rules

- Restrictions are evaluated at authentication (one SQL, no CIDR
  crate); scopes at authorize.
- Scope denial and identity denial are indistinguishable to the
  client (no existence leak).
- SSH maps the same decisions onto its disconnect/error shape.

## 11. Domain model touchpoints

**Token** restrictions (read), **Repository** pin target.

## 12. UI/UX notes

None — transport behavior only (forms land in e22s03).

## 13. API contracts

New denial causes on /git/* (401/403 per existing shapes); SSH
error messages stay generic.

## 14. Data model

None new — e22s01's columns are consumed.

## 15. Non-functional requirements

Authentication stays one SQL statement (load + restrictions in a
single row read); no extra round trip per request on the hot path.

## 16. Out of scope

Scope selection UI (e22s03), write-scope granularity beyond
read/write, IP ranges per repo, audit logging of denials.

## 17. Acceptance criteria

- [ ] `cargo test -p server token_restrictions` — expired,
      foreign-repo, and wrong-IP tokens rejected at
      authentication; NULL allowlist unrestricted. (e22s02t1)
- [ ] `cargo test -p server token_scope_authorize` — upload-pack
      needs read, receive-pack needs write; both transports share
      the decision. (e22s02t2)
- [ ] `bash scripts/e2e-token-scopes.sh` — read-only clone works,
      its push is refused; pinned and off-list cases refuse on
      HTTP and SSH. (e22s02t3)
- [ ] `cargo test --workspace` green. (e22s02t4)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: read-only token cannot push
  Given a token with scopes {read}
  When the holder pushes over HTTP and SSH
  Then both attempts are refused and the clone still works

Scenario: expired token dies at authentication
  Given a token with expires_at in the past
  When any transport presents it
  Then authentication fails before any git spawn

Scenario: pinned and off-list tokens stay home
  Given a token pinned to repo A with an IP allowlist
  When it is used on repo B or from an off-list IP
  Then the request is refused on both transports
```

## 19. Verification script (manual)

1. Create a read-only token in settings (or via SQL for the test).
2. Clone with it — works; push — refused, on HTTP and SSH.
3. Set expires_at in the past — clone now fails with auth error.

## 20. Traceability

`// story: e22s02` in token::authenticate, the shared authorize
helper, and the `token_restrictions` / `token_scope_authorize`
tests + `scripts/e2e-token-scopes.sh`.
