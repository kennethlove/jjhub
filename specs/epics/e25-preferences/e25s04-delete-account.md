# e25s04 — Delete account

## 1. Story

As a signed-in user I reach the Danger zone last, read exactly
what goes (account, N owned repositories, tokens, handle), pass
"No, cancel" and continue, then confirm with my password. Success
ends my session at the visitor home page. A wrong password keeps
the account and the step-2 state.

## 2. Business value

The designed two-step destructive flow ships. FKs already cascade
from `users` (repositories, tokens, MRs, comments, collaborators —
migrations 0002–0008), so the handler is one verified `DELETE`.

## 3. Type

feat

## 4. Risk / context

risk: P1 (irreversible data loss) — context:
`routes.rs::delete_account` (password gate via
`password::verify_password`, ownership-scoped `DELETE FROM users
WHERE id`, session cookie cleared like logout). Repo working trees
on disk are left in place: unreachable once the row is gone
(all lookups are DB-backed). Disk reaping is a separate follow-up,
recorded in the story notes, not this route.

## 5. Verification

`cargo test --workspace` (delete tests: wrong password 401/422,
success 303 + user gone + tokens gone + old session rejected);
CDP walk of the two :target confirm states.
