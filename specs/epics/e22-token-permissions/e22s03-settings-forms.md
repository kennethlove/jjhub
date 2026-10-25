# e22s03 — Settings selectors and repo-page form pin

## 1. Story

As a user, I choose my token's permissions where I mint it: the
settings create and regenerate forms select scopes, expiry, and
repo pin; the demoted repo-page form pre-pins its own repo — what
a per-repo token should mean.

## 2. Business value

Closes token permissions: enforcement (e22s02) is useless if users
cannot mint restricted tokens. Defaults stay read+write everywhere
(signup, regenerate, unadorned creates), so the safety floor never
drops.

## 3. Type

feat

## 4. Risk / context

risk: P2 (BCP 3, forms + minting parameters) — context: settings
token routes and the demoted repo-page route (e21s03) gain
parameters; the storage and enforcement layers already exist.

## 5. Context

Zoom-out on the modules this touches:
- **settings token create route** (routes.rs:184 area) — purpose:
  mint a token from settings. Contracts: defaults to read+write
  when the form carries no selection; validated scope membership
  (only read/write accepted).
- **regenerate route (e21s03)** — the replacement token inherits
  (or re-selects) permissions; default remains read+write.
- **repo-page token route (routes.rs create_repo_token)** — the
  demoted form always sets repo_id to its own repo; scopes default
  read+write.
- **signup auto-issue (e21s01)** — mints read+write explicitly
  once e22s01's column exists.

## 6. Requirements

#### MODIFIED: Settings create form selects permissions
**Before:** settings mints a full-power token with no choices.
**After:** the create form offers scope checkboxes (read, write —
at least one required), an optional expiry date, and an optional
repo pin; submitting without a selection keeps read+write.

#### MODIFIED: Regenerate preserves or re-selects permissions
**Before:** regenerate had no permission concept.
**After:** the regenerate confirmation shows the current token's
permissions and lets the user adjust them before rotating; no
selection means read+write.

#### MODIFIED: Repo-page form pre-pins its repo
**Before:** the repo-page form minted a token bound by nothing.
**After:** the token it mints is pinned to that repo (repo_id set
server-side; the pin is not a user-editable field on that form).

#### ADDED: Signup and regenerate default to read+write
Newly auto-issued signup tokens and any unadorned regenerate
carry {read, write} — the SCOPE-stated default.

## 7. Actors

Signed-in user (settings), repo visitor (demoted repo form).

## 8. Preconditions

e22s01 (columns) and e22s02 (enforcement) shipped; e21s01's
signup auto-issue mints tokens.

## 9. Postconditions

A user can mint a read-only, expiring, pinned, or allowlisted
token from settings (allowlist field included) and watch each
restriction take effect per e22s02's enforcement.

## 10. Business rules

- At least one scope must be selected; empty selection falls back
  to read+write, never to no-scope.
- The repo-page pin is server-assigned, never client-controlled.
- Expiry accepts future dates only; past dates are a validation
  error (422), consistent with existing form error shapes.

## 11. Domain model touchpoints

**Token** minting parameters; **Repository** pin target.

## 12. UI/UX notes

Settings form gains checkboxes/date/pin select; repo-page form
unchanged visually (already behind the e21s03 disclosure). UI audit
gate applies to the settings template.

## 13. API contracts

Settings token POST gains form fields (backward-compatible:
absent fields mean defaults). Repo-page POST unchanged in shape.

## 14. Data model

None — writes the columns from e22s01.

## 15. Non-functional requirements

Minting stays one INSERT with parameters; no extra queries for
defaults.

## 16. Out of scope

Editing existing tokens, per-organization scopes, allowlist
validation beyond Postgres inet parsing errors.

## 17. Acceptance criteria

- [ ] `cargo test -p server token_form_permissions` — settings
      create with/without selections; empty falls back to
      read+write; past expiry is 422. (e22s03t1)
- [ ] `cargo test -p server token_defaults` — signup and
      regenerate mint read+write; repo-page mint is pinned to its
      repo. (e22s03t2)
- [ ] `cargo test --workspace` green. (e22s03t3)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: mint a read-only expiring token
  Given a user on the settings token form
  When they select read only and an expiry next month
  Then the minted token clones, cannot push, and dies after
    the expiry date

Scenario: empty selection keeps the floor
  Given the same form submitted with no scope checked
  Then the token is minted read+write

Scenario: repo-page mint is pinned
  Given a visitor on repo A's demoted token form
  When they mint
  Then the token works only on repo A
```

## 19. Verification script (manual)

1. Settings — mint a read-only token; clone works, push refused.
2. Mint with a past expiry — form shows a validation error.
3. Repo page — mint behind Advanced; the token only works there.

## 20. Traceability

`// story: e22s03` in the settings token handlers, the repo-page
mint path, and the `token_form_permissions` / `token_defaults`
tests.
