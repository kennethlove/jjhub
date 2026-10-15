# jjhub — Tech Stack & Domain Language

Single-context repo. Axum web service, Rust workspace, Postgres, Tera/htmx.

## Language

### Core review model

**Change**:
The client-side jj unit of work, identified by its `change-id` commit header. Exists in the author's local repo before push. _Avoid_: branch, patch, PR
_Aliases seen in the wild_: jj change

**Merge Request (MR)**:
Server-side entity keyed 1:1 to (repo, change-id). Lifecycle: `open → merged | abandoned`; reopen is legal on re-push. Created automatically by push (one per change-with-diff); repo setting can make pushes draft. _Avoid_: pull request, PR

**Revision**:
One pushed snapshot of a Change. Every push creates a new Revision; all Revisions are retained for edit-in-place review. A push whose change-id matches an open MR must carry the MR owner's commit author email; mismatch = push rejected. _Avoid_: commit version, iteration

**Stack**:
An ordered chain of Changes where each depends on its predecessor, derived from commit parents — not from MR metadata. Bottom MR targets Trunk; descendant MRs target their parent MR. Merging cascades server-side (squash bottom, rebase the rest onto new Trunk). _Avoid_: PR chain, dependency list

**Trunk**:
The repo's default line of immutable history. Repo setting; default bookmark name `trunk`. Owner or Collaborators can change it. MRs target trunk (bottom of Stack) by name, so rename is a plain update. _Avoid_: master, default branch (in domain prose)

### Hosting model

**Repository (Repo)**:
Server-hosted collection of jj/git objects (bare git repo) plus its MR/collaborator metadata. Public or private. _Avoid_: project, workspace

**Collaborator**:
A User with push/review rights on a Repo beyond its owner. _Avoid_: member, contributor

**Token**:
HTTP auth credential for git transport operations (push/pull). Scoped to a User. _Avoid_: API key, password (for transport)

## Relationships

- A **User** owns many **Repos** and can be **Collaborator** on others
- A **Repo** has zero or more **Merge Requests**
- A **Merge Request** anchors to exactly one (repo, change-id)
- A **Merge Request** has one or more **Revisions** (pushes)
- A **Repo**'s **Stack** structure is derived from commit parents at push time
- A **Repo** has a merge setting: squash (default) or merge commit
- An open **Merge Request** is abandoned by its Abandon button or by deleting its bookmark

## Invariants

These MUST always hold. Violations are bugs, not edge cases.

1. At most ONE open MR per (repo, change-id).
2. Every MR Revision's commit author email equals the MR owner's account email.
3. Stack parentage is always derived from commit parents; it is never stored as authority.
4. The trunk bookmark never receives a non-fast-forward rewrite from jjhub. Merge is squash or merge-commit, forward only.
5. All Revisions are immutable once pushed; the server never rewrites pushed objects.
6. Private repo: MR pages and git transport both require authentication. Public repo: read access is open.
7. Merging an MR never merges descendants before it. Cascade order equals stack order.

## MR state machine

```
(none) --push--> open --squash/merge-commit--> merged
                    |                            |
                    | abandon button /           | re-push of same
                    | bookmark delete            | change-id
                    v                            v
                abandoned --------------------> open (reopen)
```

No other transitions. `merged → abandoned` is illegal. An abandoned MR reopens on re-push.

## Example dialogue

> **Dev:** "Reviewer commented on revision 2, but the author rebased and re-pushed — is the comment dead?"
> **Domain expert:** "No. The comment pins to (MR, Revision, file, context). Revision 3 is a new Revision; the old diff is still viewable, and the thread survives."

## Flagged ambiguities

- "MR = change" was ambiguous: the MR is NOT the jj Change itself (client-side). Resolved: MR is a server-side entity keyed on (repo, change-id).
