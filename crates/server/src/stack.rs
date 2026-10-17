// story: e03s02
//! Stack derivation glue: open MRs + bare-repo commit parents -> the
//! bottom-first stack model the pulls page renders (invariant 3: derived,
//! never stored).

use std::collections::HashMap;
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

/// story: e03s02 — full stack model for the pulls page.
pub fn derive_page_model(bare: &Path, mrs: &[OpenMr]) -> Vec<Vec<String>> {
    let parents: HashMap<String, Vec<String>> = mrs
        .iter()
        .map(|mr| {
            (
                mr.latest_commit.clone(),
                parents_of(bare, &mr.latest_commit),
            )
        })
        .collect();

    let inputs: Vec<StackInput> = mrs
        .iter()
        .map(|mr| StackInput {
            change_id: mr.change_id.clone(),
            latest_commit: mr.latest_commit.clone(),
        })
        .collect();
    domain::derive_stacks(&inputs, parents)
}

/// story: e03s02 — unified diff of a commit against its first parent (or
/// the empty tree for a root commit). Raw text; callers escape for HTML.
pub fn unified_diff(bare: &Path, commit: &str) -> String {
    const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
    let parent = parents_of(bare, commit)
        .into_iter()
        .next()
        .unwrap_or_else(|| EMPTY_TREE.to_string());
    let Ok(output) = std::process::Command::new("git")
        .arg("-C")
        .arg(bare)
        .args(["diff", &parent, commit, "--"])
        .output()
    else {
        return String::new();
    };
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// story: e03s02 — minimal HTML escaping for repo-controlled content.
pub fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// story: browse pin — resolve a ref to its commit id in the bare repo.
pub fn ref_commit(bare: &Path, ref_name: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(bare)
        .args(["rev-parse", "--verify", ref_name])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
