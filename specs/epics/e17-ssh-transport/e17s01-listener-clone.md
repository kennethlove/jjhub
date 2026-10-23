# e17s01 — SSH listener, host keys, token auth, clone

## 1. Story

As a self-hoster, jjhub answers on an SSH port so `jj git clone
ssh://…` works with the transport token I already have — HTTP is no
longer the only way to reach my repositories.

## 2. Business value

First half of the scope's SSH choice: get a real SSH endpoint running
with the product's own credential (the transport token issued at
account creation) and prove read-only access end-to-end. Clone is the
trust test — if pack delivery works over the listener, push is a
second command on the same rails.

## 3. Type

feat

## 4. Risk / context

risk: P2 (BCP 4, new network listener + one new dependency, russh) —
context: new module beside transport.rs; auth reuses the token table;
no schema change.

## 5. Context

Zoom-out on the modules this touches:
- **russh 0.64.1** (new dependency, workspace Cargo.toml) — async SSH
  server on tokio, same runtime as axum; its keys module loads host
  keys from disk and generates them at first boot.
- **Config** — SSH bind address/port from environment (default
  2202), host-key directory from environment; boot wiring next to
  wherever the HTTP listener starts. Config-gating: listener starts
  when configured/always — see requirement.
- **Authentication** — password auth presents the account's transport
  token; look it up against the existing token table (the same table
  the HTTP push auth reads), map to the account, reject everything
  else (publickey auth declined — tokens only).
- **Git execution** — `git-upload-pack --strict <bare-path>` via
  stdio plumbing, env discipline copied from the HTTP path
  (GIT_CONFIG_GLOBAL/SYSTEM neutralization); bare-path resolution and
  repo-exists/visibility checks reuse the HTTP transport's helpers so
  SSH cannot fetch what HTTP would refuse (public read, owner-only
  private read).
- **Existing patterns** — transport.rs (stdio exec, pkt-line),
  receive_guard (admission, called in e17s02 not here).

## 6. Requirements

#### ADDED: Listener with first-boot host keys
jjhub starts an SSH listener on the configured port; if host keys are
absent it generates and persists them before accepting connections.

#### ADDED: Token password authentication
`ssh -p <port> <user>@host` with the account's transport token as
password authenticates; any other password, publickey attempt, or
unknown user is rejected without leaking whether the repo/user exists.

#### ADDED: Clone over SSH
An authenticated client clones a public repository via
`ssh://` — the pack delivered is equivalent to the HTTP smart
transport's (same upload-pack, same admission of refs).

## 7. Actors

Operators (boot/config), jj users (clone).

## 8. Preconditions

e16 shipped (no ordering dependency — e16 was chosen first in the
release plan); transport tokens exist (e01); upload-pack binaries on
host (already true for HTTP).

## 9. Postconditions

`jj git clone ssh://host:2202/owner/repo.git` succeeds with the
transport token; the HTTP path is untouched and still green.

## 10. Business rules

- SSH authentication admits exactly what the HTTP token auth admits —
  one credential model, one token table.
- Private repos are unreadable over SSH without their owner's token.
- Host keys are generated once and reused; regenerating is an
  operator action (delete files), never automatic.

## 11. Domain model touchpoints

**User** (transport token), **Repository** (bare path, visibility),
read path only.

## 12. UI/UX notes

None in this story (URL snippet lands in e17s02).

## 13. API contracts

SSH protocol surface: SSH-2.0 on the configured port, password auth,
exec channel running git pack programs. No new HTTP routes.

## 14. Data model

None — host keys are files; auth reads the existing tokens table.

## 15. Non-functional requirements

Listener lifecycle owned by the axum/tokio shutdown story; boot fails
loudly on bind conflict (misconfiguration is visible, not silent);
existing test suite keeps passing with the listener disabled in
tests.

## 16. Out of scope

Push over SSH (e17s02), ssh:// URL snippet (e17s02), publickey auth,
agent forwarding, SCP/SFTP/shell, port 22, SSH-only config UI.

## 17. Acceptance criteria

- [ ] `cargo test -p server ssh_auth` — token password accepted,
      wrong password rejected, unknown user rejected, publickey
      declined. (e17s01t1)
- [ ] `cargo test -p server ssh_host_keys` — first boot generates
      persistable keys into the configured dir; second boot reuses
      them (byte-identical fingerprint). (e17s01t2)
- [ ] `bash scripts/e2e-ssh-clone.sh` — end-to-end: listener up,
      `git clone ssh://` of a public repo with the token succeeds,
      contents equal the HTTP clone's. (e17s01t3)
- [ ] `cargo test --workspace` green. (e17s01t4)

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: clone with the transport token
  Given jjhub is running with SSH on port 2202
  And an account whose transport token is "tok…"
  When I clone "ssh://jjhub.test:2202/o/r.git" as password "tok…"
  Then the clone succeeds and matches the repository contents

Scenario: wrong password never reaches git
  Given the same listener
  When I clone with password "wrong"
  Then authentication fails and no pack data transfers

Scenario: host keys persist across boots
  Given first boot with an empty host-key directory
  When the service starts
  Then keys are generated
  And a restart reuses the same fingerprint
```

## 19. Verification script (manual)

1. Start jjhub with an empty key dir — logs show listener on 2202 and
   keys generated.
2. `git clone ssh://localhost:2202/owner/repo.git` — password prompt
   accepts the token; tree matches the HTTP clone.

## 20. Traceability

`// story: e17s01` in the SSH listener module, auth handler, key
bootstrapping, and the `ssh_auth` / `ssh_host_keys` tests +
`scripts/e2e-ssh-clone.sh`.
