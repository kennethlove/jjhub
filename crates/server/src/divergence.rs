//! story: e16s01 — divergence detection over pushed revisions.
//!
//! A change has diverged when two or more of its pushed revisions are
//! heads of the change's graph and no head is an ancestor of another —
//! jj's model of one change-id carrying two independent heads (a
//! history rewrite across clones). The pushed bare graph is the source
//! of truth; detection is pairwise git ancestry, read-only.

use std::path::Path;

/// story: e16s01 — does this revision set diverge, and on which heads?
///
/// Takes the bare repo path plus the pushed commit ids of one change
/// and returns the diverging heads in input order, or empty when the
/// set is linear (every head contained in another), trivial (0-1
/// commits), or unverifiable (missing objects / ancestry errors fail
/// safe as not-divergent — same defensive posture as `is_ancestor` in
/// receive_guard, never a 500).
pub fn divergent_heads(git_dir: &Path, commits: &[String]) -> Vec<String> {
    // every id must resolve before any ancestry claim is trustworthy
    if commits
        .iter()
        .any(|id| !git_succeeds(git_dir, &["cat-file", "-e", id]))
    {
        return Vec::new();
    }
    // a head is a commit no other commit in the set contains
    let heads: Vec<String> = commits
        .iter()
        .filter(|candidate| {
            !commits.iter().any(|other| {
                other != *candidate
                    && git_succeeds(git_dir, &["merge-base", "--is-ancestor", candidate, other])
            })
        })
        .cloned()
        .collect();
    if heads.len() >= 2 {
        heads
    } else {
        Vec::new()
    }
}

/// story: e16s01 — argv-only git, env-neutralized like the existing
/// helpers; any spawn or non-zero exit counts as failure (fail safe).
fn git_succeeds(git_dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(git_dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
