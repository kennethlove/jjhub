# e28s01 — protected heads configuration in the Operations panel

## 1. Story

As a repository owner I choose which refs the server protects
beyond the defaults: the Operations panel lists the current
protected set, lets me add a pattern such as `refs/heads/release/*`,
and lets me remove patterns I added. Trunk and `refs/tags/*`
stay protected and are not removable.

## 2. Business value

Immutability policy ships enforced but fixed (`receive_guard::
protected_refs` derives `refs/heads/{trunk}` + `refs/tags/*` and
the e10 doc says "Not user-configurable in 0.2.0"). This story
makes the policy configurable without touching the admission path:
the resolver gains user rows, `check_protected_refs` is unchanged.

## 3. Type

feat

## 4. Risk / context

risk: P2 — configuration touches a security surface, so add and
remove routes need the settings auth pattern (session +
CSRF) and the admission test must prove a user-added pattern is
actually refused on non-fast-forward and deletion. Placement
decision: the Operations panel already exists per repo (repo
tabs ship Browse/Merge requests/Search/Operations), so the form
lives there — no new page, no new tab. Storage: new migration
`protected_refs (repo_id → repositories cascade, pattern text,
created_at, unique(repo_id, pattern))`; `protected_refs()`
merges `refs/heads/{trunk}` + `refs/tags/*` + rows for the repo.
Defaults are never rows, so they are never removable.

## 5. Verification

`cargo test -p server --test protected_config` (new) +
`cargo test -p server --test receive_guard` (existing admission
suite stays green); CDP UAT of the panel at 1440 + 390;
`ux_audit` on the absolute app.css path.
