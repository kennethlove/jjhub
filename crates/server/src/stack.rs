// story: e03s02
//! Stack derivation glue: open MRs + bare-repo commit parents -> the
//! bottom-first stack model the pulls page renders (invariant 3: derived,
//! never stored).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use domain::StackInput;
use sqlx::PgPool;

/// story: e03s02 — one open MR as the page model sees it.
#[derive(Debug, Clone)]
pub struct OpenMr {
    pub change_id: String,
    pub latest_commit: String,
}

/// story: e03s02 — open MRs with their newest revision commit.
pub async fn open_mrs(pool: &PgPool, repo_id: uuid::Uuid) -> Result<Vec<OpenMr>, sqlx::Error> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT mr.change_id, rev.commit_id FROM merge_requests mr \
         JOIN LATERAL ( \
             SELECT commit_id FROM revisions WHERE mr_id = mr.id \
             ORDER BY pushed_at DESC LIMIT 1 \
         ) rev ON true \
         WHERE mr.repo_id = $1 AND mr.state = 'open' \
         ORDER BY mr.change_id",
    )
    .bind(repo_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(change_id, latest_commit)| OpenMr {
            change_id,
            latest_commit,
        })
        .collect())
}

/// story: e03s02 — first-parent line of a commit, from the bare repo.
pub fn parents_of(bare: &Path, commit: &str) -> Vec<String> {
    let Ok(output) = std::process::Command::new("git")
        .arg("-C")
        .arg(bare)
        .args(["cat-file", "-p", commit])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.strip_prefix("parent ").map(|p| p.trim().to_string()))
        .collect()
}

/// story: e03s02 — is `commit` reachable from the trunk ref?
pub fn on_trunk(bare: &Path, commit: &str, trunk: &str) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(bare)
        .args(["merge-base", "--is-ancestor", commit, trunk])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// story: e03s02 — full stack model for the pulls page.
pub fn derive_page_model(bare: &Path, trunk: &str, mrs: &[OpenMr]) -> Vec<Vec<String>> {
    let parents: HashMap<String, Vec<String>> = mrs
        .iter()
        .map(|mr| {
            (
                mr.latest_commit.clone(),
                parents_of(bare, &mr.latest_commit),
            )
        })
        .collect();

    // only commits whose first parent is trunk-ancestor count as trunk for
    // the derivation; check once per distinct first parent
    let mut trunk_commits: HashSet<String> = HashSet::new();
    for first_parent in parents.values().filter_map(|p| p.first()) {
        if trunk_commits.contains(first_parent) || on_trunk(bare, first_parent, trunk) {
            trunk_commits.insert(first_parent.clone());
        }
    }

    let inputs: Vec<StackInput> = mrs
        .iter()
        .map(|mr| StackInput {
            change_id: mr.change_id.clone(),
            latest_commit: mr.latest_commit.clone(),
        })
        .collect();
    domain::derive_stacks(&inputs, parents, trunk_commits)
}
