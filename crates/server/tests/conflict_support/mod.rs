#![allow(dead_code)] // each test binary uses a subset of the helper API
// story: e08s01 — git-plumbing builder for jj's git conflict representation,
// so transport tests need no jj CLI binary. Mirrors jj-lib 0.41's git
// backend: two term trees merged under `.jjconflict-base-N` /
// `.jjconflict-side-N`, a `JJ-CONFLICT-README` blob, a `jj:trees` commit
// header naming the term root trees, and `refs/jj/keep/*` refs pinning
// every commit's objects.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// The fixture chain: a resolved parent commit and a commit carrying jj's
/// conflict representation. Both ids resolve through `refs/jj/keep/*`.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub parent: String,
    pub conflicted: String,
    /// change-id header of the conflicted commit (what `build` writes).
    pub change_id: String,
}

/// change-id written into the conflicted commit by [`build`]; the identity
/// tests seed their open merge request with this value.
pub const CONFLICT_CHANGE_ID: &str = "0e1f2d3c4b5a69788796a5b4c3d2e1f0";

const PARENT_CHANGE_ID: &str = "9a8b7c6d5e4f30211203f4e5d6c7b8a9";
const TIMESTAMP: &str = "1700000000 +0000";

/// jj's own conflict README blob (git_backend.rs, JJ-CONFLICT-README).
const CONFLICT_README: &str = r#"This commit was made by jj, https://jj-vcs.dev/.
The commit contains file conflicts, and therefore looks wrong when used with
plain Git or other tools that are unfamiliar with jj.

The .jjconflict-* directories represent the different inputs to the conflict.
For details, see
https://docs.jj-vcs.dev/latest/git-compatibility/#format-mapping-details

If you see this file in your working copy, it probably means that you used a
regular `git` command to check out a conflicted commit. Use `jj abandon` to
recover.
"#;

/// Initialize a bare repo at `dir` when it has no `HEAD` yet (repos created
/// by the server already carry one).
pub fn ensure_bare(dir: &Path) {
    if dir.join("HEAD").exists() {
        return;
    }
    git(
        dir,
        &["init", "--bare", "--initial-branch=trunk", "."],
    );
}

/// Build the fixture with the default pushing identity (alice) — the
/// transport tests' conflicted repo.
pub fn build(dir: &Path) -> Fixture {
    build_as(dir, "Alice", "alice@example.com", CONFLICT_CHANGE_ID)
}

/// Build the fixture with an explicit identity and conflicted-commit
/// change-id: the identity test pushes a foreign author's conflicted commit.
pub fn build_as(dir: &Path, name: &str, email: &str, change_id: &str) -> Fixture {
    ensure_bare(dir);
    let signature = format!("{name} <{email}> {TIMESTAMP}");

    // one file with three different contents = the conflict's three terms
    let base_blob = hash_object(dir, "blob", "base\n");
    let side0_blob = hash_object(dir, "blob", "side0\n");
    let side1_blob = hash_object(dir, "blob", "side1\n");
    let term_tree = |blob: &str| {
        mktree(dir, &[format!("100644 blob {blob}\tconflict.txt\n")])
    };
    let base_tree = term_tree(&base_blob);
    let side0_tree = term_tree(&side0_blob);
    let side1_tree = term_tree(&side1_blob);

    // jj's write_tree_conflict: base/side subtrees + README + the first
    // term's entries, sorted by filename
    let readme = hash_object(dir, "blob", CONFLICT_README);
    let conflict_tree = mktree(
        dir,
        &[
            format!("040000 tree {base_tree}\t.jjconflict-base-0\n"),
            format!("040000 tree {side0_tree}\t.jjconflict-side-0\n"),
            format!("040000 tree {side1_tree}\t.jjconflict-side-1\n"),
            format!("100644 blob {readme}\tJJ-CONFLICT-README\n"),
            format!("100644 blob {base_blob}\tconflict.txt\n"),
        ],
    );

    let parent_body = format!(
        "tree {base_tree}\n\
         author {signature}\n\
         committer {signature}\n\
         change-id {PARENT_CHANGE_ID}\n\
         \n\
         e08s01 parent commit\n"
    );
    let parent = hash_object(dir, "commit", &parent_body);

    let conflicted_body = format!(
        "tree {conflict_tree}\n\
         parent {parent}\n\
         author {signature}\n\
         committer {signature}\n\
         jj:trees {base_tree} {side0_tree} {side1_tree}\n\
         change-id {change_id}\n\
         \n\
         e08s01 conflicted commit\n"
    );
    let conflicted = hash_object(dir, "commit", &conflicted_body);

    git(dir, &["update-ref", "refs/heads/trunk", &conflicted]);
    for commit in [&parent, &conflicted] {
        let keep = format!("refs/jj/keep/{commit}");
        git(dir, &["update-ref", &keep, commit]);
    }

    Fixture {
        parent,
        conflicted,
        change_id: change_id.to_string(),
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    git_with_stdin(dir, args, "")
}

fn git_with_stdin(dir: &Path, args: &[&str], input: &str) -> String {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git spawn");
    if !input.is_empty() {
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(input.as_bytes())
            .expect("write git stdin");
    }
    drop(child.stdin.take()); // close the pipe so git never waits on stdin
    let output = child.wait_with_output().expect("git wait");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim_end().to_string()
}

fn hash_object(dir: &Path, kind: &str, content: &str) -> String {
    git_with_stdin(
        dir,
        &["hash-object", "-t", kind, "-w", "--stdin"],
        content,
    )
}

fn mktree(dir: &Path, entries: &[String]) -> String {
    let input: String = entries.concat();
    git_with_stdin(dir, &["mktree"], &input)
}
