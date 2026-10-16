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
    /// story: e03s03 — trunk after the cascade (equals new_trunk_commit
    /// when there were no descendants).
    pub final_trunk_commit: String,
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

    // invariant 7: rebase descendants in stack order, each on the head the
    // previous one produced. A failure stops the cascade; what already
    // rebased stands (trunk is append-only history, never partially written).
    let final_head = cascade_descendants(
        pool,
        &bare,
        &trunk_ref,
        repo_id,
        &change_commit,
        new_trunk_commit.clone(),
    )
    .await;

    Ok(MergeSummary {
        new_trunk_commit,
        final_trunk_commit: final_head,
    })
}

/// story: e03s03 — rebase every open descendant MR onto `head`, one CAS per
/// step, appending a rebased revision. Change-id header preserved.
async fn cascade_descendants(
    pool: &PgPool,
    bare: &Path,
    trunk_ref: &str,
    repo_id: uuid::Uuid,
    merged_commit: &str,
    head: String,
) -> String {
    let mrs = match crate::stack::open_mrs(pool, repo_id).await {
        Ok(mrs) => mrs,
        Err(err) => {
            tracing::warn!(%err, "cascade: open MRs query failed");
            return head;
        }
    };
    let parents: std::collections::HashMap<String, Vec<String>> = mrs
        .iter()
        .map(|mr| {
            (
                mr.latest_commit.clone(),
                crate::stack::parents_of(bare, &mr.latest_commit),
            )
        })
        .collect();

    // descendants of the merged commit, in stack order (BFS)
    let mut ordered = Vec::new();
    let mut frontier = vec![merged_commit.to_string()];
    while let Some(commit) = frontier.pop() {
        let mut kids: Vec<&crate::stack::OpenMr> = mrs
            .iter()
            .filter(|mr| {
                parents
                    .get(&mr.latest_commit)
                    .and_then(|p| p.first())
                    .is_some_and(|first| *first == commit)
            })
            .collect();
        kids.sort_by(|a, b| a.change_id.cmp(&b.change_id));
        for kid in kids {
            ordered.push(kid.clone());
            frontier.push(kid.latest_commit.clone());
        }
    }

    let mut current_head = head;
    for descendant in ordered {
        match rebase_one(pool, bare, trunk_ref, repo_id, &descendant, &current_head).await {
            Ok(new_commit) => current_head = new_commit,
            Err(err) => {
                tracing::warn!(
                    change_id = %descendant.change_id,
                    %err,
                    "cascade stopped: descendant not rebased"
                );
                break;
            }
        }
    }
    current_head
}

async fn rebase_one(
    pool: &PgPool,
    bare: &Path,
    trunk_ref: &str,
    repo_id: uuid::Uuid,
    descendant: &crate::stack::OpenMr,
    new_parent: &str,
) -> Result<String, MergeError> {
    let raw = git_output(
        bare,
        &["cat-file", "commit", &descendant.latest_commit],
        None,
    )?;
    let headers_end = raw
        .find("\n\n")
        .ok_or_else(|| MergeError::Internal("malformed commit".into()))?;
    let headers = &raw[..headers_end];
    let message = &raw[headers_end + 2..];

    let header = |name: &str| -> Result<String, MergeError> {
        headers
            .lines()
            .find(|l| l.starts_with(&format!("{name} ")))
            .map(|l| l.to_string())
            .ok_or_else(|| MergeError::Internal(format!("no {name} header")))
    };
    let tree = header("tree")?;
    let author = header("author")?;
    let committer = header("committer")?;
    // preserve jj's change-id header; plain-git commits have none and are
    // not MR material — skip them
    let change_id = header("change-id")?;

    let object =
        format!("{tree}\nparent {new_parent}\n{author}\n{committer}\n{change_id}\n\n{message}");
    let rebased = git_output(
        bare,
        &["hash-object", "-t", "commit", "-w", "--stdin"],
        Some(&object),
    )?;
    apply_ref_update(bare, trunk_ref, new_parent, &rebased)?;

    let mr_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT id FROM merge_requests WHERE repo_id = $1 AND change_id = $2 AND state = 'open'",
    )
    .bind(repo_id)
    .bind(&descendant.change_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    if let Some(mr_id) = mr_id {
        sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2)")
            .bind(mr_id)
            .bind(&rebased)
            .execute(pool)
            .await
            .map_err(|e| MergeError::Internal(format!("append revision: {e}")))?;
    }
    Ok(rebased)
}
