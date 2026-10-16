// story: e03s03
//! Merge executor: squash or merge-commit the bottom stack MR into trunk,
//! forward-only, in one compare-and-swap ref update (invariants 4 and 7).
//!
//! Descendant rebasing is e03s03t2; this module owns the bottom merge and
//! the ref CAS everything else builds on.

use std::path::Path;

use sqlx::PgPool;

#[derive(Debug, PartialEq, Eq)]
pub enum MergeError {
    NotFound,
    NotOwner,
    NotBottom,
    Internal(String),
}

impl std::fmt::Display for MergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeError::NotFound => write!(f, "merge request not found"),
            MergeError::NotOwner => write!(f, "only the merge request owner can merge"),
            MergeError::NotBottom => write!(f, "only the bottom of a stack can merge first"),
            MergeError::Internal(msg) => write!(f, "merge failed: {msg}"),
        }
    }
}

/// story: e03s03 — what one merge did.
#[derive(Debug)]
pub struct MergeSummary {
    pub new_trunk_commit: String,
}

fn git_output(bare: &Path, args: &[&str], stdin: Option<&str>) -> Result<String, MergeError> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("git")
        .arg("-C")
        .arg(bare)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| MergeError::Internal(format!("spawn git {args:?}: {e}")))?;
    if let Some(input) = stdin {
        child
            .stdin
            .as_mut()
            .expect("piped stdin")
            .write_all(input.as_bytes())
            .map_err(|e| MergeError::Internal(format!("git stdin: {e}")))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| MergeError::Internal(format!("wait git {args:?}: {e}")))?;
    if !output.status.success() {
        return Err(MergeError::Internal(format!(
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// story: e03s03 — invariant 4: compare-and-swap on the ref. `expected_old`
/// must match the current value or nothing moves. Used by the cascade too.
pub fn apply_ref_update(
    bare: &Path,
    ref_name: &str,
    expected_old: &str,
    new_commit: &str,
) -> Result<(), MergeError> {
    let command = format!("update {ref_name} {new_commit} {expected_old}\n");
    git_output(bare, &["update-ref", "--stdin"], Some(&command))?;
    Ok(())
}

fn identity_line(bare: &Path, commit: &str, header: &str) -> Result<String, MergeError> {
    let raw = git_output(bare, &["cat-file", "commit", commit], None)?;
    raw.lines()
        .find(|line| line.starts_with(&format!("{header} ")))
        .map(|line| line.to_string())
        .ok_or_else(|| MergeError::Internal(format!("no {header} on {commit}")))
}

fn commit_message(bare: &Path, commit: &str) -> Result<String, MergeError> {
    let raw = git_output(bare, &["cat-file", "commit", commit], None)?;
    Ok(raw
        .split_once("\n\n")
        .map(|(_, msg)| msg.to_string())
        .unwrap_or_default())
}

/// story: e03s03 — squash (or merge-commit) the bottom change into trunk.
pub async fn merge_bottom(
    pool: &PgPool,
    storage_root: &str,
    repo_id: uuid::Uuid,
    change_id: &str,
    actor: uuid::Uuid,
) -> Result<MergeSummary, MergeError> {
    let repo: Option<(String, String)> =
        sqlx::query_as("SELECT r.trunk, r.merge_setting FROM repositories r WHERE r.id = $1")
            .bind(repo_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| MergeError::Internal(format!("repo query: {e}")))?;
    let Some((trunk, merge_setting)) = repo else {
        return Err(MergeError::NotFound);
    };

    let mr: Option<(uuid::Uuid, String, String)> = sqlx::query_as(
        "SELECT mr.id, mr.owner_id::text, rev.commit_id FROM merge_requests mr \
         JOIN LATERAL (SELECT commit_id FROM revisions WHERE mr_id = mr.id \
                        ORDER BY pushed_at DESC LIMIT 1) rev ON true \
         WHERE mr.repo_id = $1 AND mr.change_id = $2 AND mr.state = 'open'",
    )
    .bind(repo_id)
    .bind(change_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| MergeError::Internal(format!("mr query: {e}")))?;
    let Some((mr_id, owner_id, change_commit)) = mr else {
        return Err(MergeError::NotFound);
    };
    if owner_id != actor.to_string() {
        return Err(MergeError::NotOwner);
    }

    let owner_handle: String = sqlx::query_scalar(
        "SELECT u.handle FROM repositories r JOIN users u ON u.id = r.owner_id WHERE r.id = $1",
    )
    .bind(repo_id)
    .fetch_one(pool)
    .await
    .map_err(|e| MergeError::Internal(format!("owner query: {e}")))?;
    let repo_name: String = sqlx::query_scalar("SELECT name FROM repositories WHERE id = $1")
        .bind(repo_id)
        .fetch_one(pool)
        .await
        .map_err(|e| MergeError::Internal(format!("repo name query: {e}")))?;
    let bare = domain::storage_path(storage_root, &owner_handle, &repo_name);

    let trunk_ref = format!("refs/heads/{trunk}");
    let trunk_head = git_output(&bare, &["rev-parse", &trunk_ref], None)
        .map_err(|e| MergeError::Internal(format!("trunk head: {e}")))?;

    // bottom check: the change's first parent must be reachable from trunk
    let parents = crate::stack::parents_of(&bare, &change_commit);
    let first_parent = parents
        .first()
        .ok_or_else(|| MergeError::Internal(format!("no parents on {change_commit}")))?;
    let on_trunk = std::process::Command::new("git")
        .arg("-C")
        .arg(&bare)
        .args(["merge-base", "--is-ancestor", first_parent, &trunk_head])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !on_trunk {
        return Err(MergeError::NotBottom);
    }

    let tree = git_output(
        &bare,
        &["rev-parse", &format!("{change_commit}^{{tree}}")],
        None,
    )
    .map_err(|e| MergeError::Internal(format!("tree: {e}")))?;
    let author = identity_line(&bare, &change_commit, "author")
        .map_err(|e| MergeError::Internal(format!("author: {e}")))?;
    let message = commit_message(&bare, &change_commit)
        .map_err(|e| MergeError::Internal(format!("message: {e}")))?;

    let actor_email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(actor)
        .fetch_one(pool)
        .await
        .map_err(|e| MergeError::Internal(format!("actor query: {e}")))?;
    let committer = format!(
        "committer jjhub <{actor_email}> {} +0000",
        chrono::Utc::now().timestamp()
    );

    let object = match merge_setting.as_str() {
        "merge_commit" => format!(
            "tree {tree}\nparent {trunk_head}\nparent {change_commit}\n{author}\n{committer}\n\n{message}\n"
        ),
        // squash (default): one parent, the change's content on trunk
        _ => format!(
            "tree {tree}\nparent {trunk_head}\n{author}\n{committer}\n\n{message}\n"
        ),
    };
    let new_trunk_commit = git_output(
        &bare,
        &["hash-object", "-t", "commit", "-w", "--stdin"],
        Some(&object),
    )
    .map_err(|e| MergeError::Internal(format!("write merge commit: {e}")))?;

    // invariant 4: forward-only — refuse if trunk moved while we worked
    apply_ref_update(&bare, &trunk_ref, &trunk_head, &new_trunk_commit)?;

    sqlx::query("UPDATE merge_requests SET state = 'merged', merge_commit_sha = $2 WHERE id = $1")
        .bind(mr_id)
        .bind(&new_trunk_commit)
        .execute(pool)
        .await
        .map_err(|e| MergeError::Internal(format!("mark merged: {e}")))?;

    Ok(MergeSummary { new_trunk_commit })
}
