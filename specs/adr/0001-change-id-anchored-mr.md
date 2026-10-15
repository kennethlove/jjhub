# Merge Requests anchored to change-id, not branch tip

MR identity is (repo, change-id) — the `change-id` commit header jj writes since v0.30 — not a branch name or commit hash. Pushes carry new Revisions; MRs survive rebase, squash, and force-push, which is the core jj-native review model.

Alternatives rejected: branch-tip identity (GitHub model — loses review state on every rewrite, the exact problem jjhub exists to solve); commit-hash identity (same, worse). Trade-off accepted: change-ids are forgeable, so pushes to an open MR enforce author-email matching (invariant 2, tech-stack.md).
