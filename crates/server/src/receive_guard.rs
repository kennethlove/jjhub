// story: e03s01
//! Pre-receive guard for invariant 2: a push may not rewrite an open MR's
//! change-id with a different author email.
//!
//! Runs as the bare repo's `pre-receive` hook via `server pre-receive`
//! (installed at repo creation). The hook process inherits the quarantine
//! env receive-pack sets up, so `git cat-file` inside it sees the pushed
//! objects before any ref moves.

use sqlx::PgPool;

/// story: e03s01 — identity fields parsed from a raw git commit object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitIdentity {
    pub change_id: String,
    /// story: e03s01 — jj preserves `author` across rewrites and records the
    /// rewriting identity as `committer`, so the guard checks BOTH against
    /// the MR owner. A foreign machine rewrite shows up in committer only.
    pub author_email: String,
    pub committer_email: String,
}

/// story: e03s01 — one ref update line from pre-receive stdin.
#[derive(Debug, Clone)]
pub struct PushedRef {
    pub old: String,
    pub new: String,
    pub ref_name: String,
}

impl PushedRef {
    pub fn is_create(&self) -> bool {
        self.old.chars().all(|c| c == '0')
    }
}

/// story: e03s01 — a pushed head commit with its parsed identity.
#[derive(Debug, Clone)]
pub struct PushedCommit {
    pub ref_name: String,
    pub old: String,
    pub new: String,
    pub change_id: Option<String>,
    pub author_email: String,
    pub committer_email: String,
}

/// story: e03s01 — parse the raw commit object: jj writes a `change-id`
/// header alongside git's own headers; the author email lives in the
/// `author ... <email> ...` header. Plain git commits (no change-id)
/// parse to None and are ignored by the guard.
pub fn parse_commit_identity(raw: &str) -> Option<CommitIdentity> {
    let mut change_id: Option<String> = None;
    let mut author_email: Option<String> = None;
    let mut committer_email: Option<String> = None;
    for line in raw.lines() {
        if line.is_empty() {
            break; // headers end at the first blank line
        }
        if let Some(value) = line.strip_prefix("change-id ") {
            change_id = Some(value.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("author ") {
            author_email = extract_email(rest);
        } else if let Some(rest) = line.strip_prefix("committer ") {
            committer_email = extract_email(rest);
        }
    }
    Some(CommitIdentity {
        change_id: change_id?,
        author_email: author_email?,
        committer_email: committer_email?,
    })
}

fn extract_email(author_line: &str) -> Option<String> {
    let open = author_line.find('<')?;
    let close = author_line[open..].find('>')? + open;
    Some(author_line[open + 1..close].to_string())
}

/// story: e03s01 — parse pre-receive stdin: one `<old> <new> <ref>` line
/// per update.
pub fn parse_ref_updates(stdin: &[u8]) -> Vec<PushedRef> {
    String::from_utf8_lossy(stdin)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let old = parts.next()?.to_string();
            let new = parts.next()?.to_string();
            let ref_name = parts.next()?.to_string();
            Some(PushedRef { old, new, ref_name })
        })
        .collect()
}

/// story: e10s03 — one refused ref update, attributed: which ref, which
/// heads, who pushed, and why it was refused. The guard rules produce
/// these; `record_rejections` persists one audit row per item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedUpdate {
    pub ref_name: String,
    pub old: String,
    pub new: String,
    /// Pushing identity: the author email parsed from the pushed head
    /// commit, or `"unknown"` when no commit could be parsed (e.g. a
    /// protected-ref deletion).
    pub identity: String,
    pub reason: String,
}

/// story: e03s01 — invariant 2 decision: one rejection message per pushed
/// commit that rewrites an OPEN MR's change-id with a foreign author email.
/// Merged and abandoned MRs never block (e03s03 terminal rule; reopen
/// belongs to the upsert, which re-arms the guard once the row is open).
pub async fn check_push(
    pool: &PgPool,
    repo_id: uuid::Uuid,
    pushed: Vec<PushedCommit>,
) -> Result<Vec<String>, sqlx::Error> {
    Ok(identity_rejections(pool, repo_id, &pushed)
        .await?
        .into_iter()
        .map(|rejected| rejected.reason)
        .collect())
}

/// story: e10s03 — the e03s01 identity rule with per-ref attribution for
/// the audit record.
pub async fn identity_rejections(
    pool: &PgPool,
    repo_id: uuid::Uuid,
    pushed: &[PushedCommit],
) -> Result<Vec<RejectedUpdate>, sqlx::Error> {
    let open_mrs: Vec<(String, String)> = sqlx::query_as(
        "SELECT mr.change_id, u.email FROM merge_requests mr \
         JOIN users u ON u.id = mr.owner_id \
         WHERE mr.repo_id = $1 AND mr.state = 'open'",
    )
    .bind(repo_id)
    .fetch_all(pool)
    .await?;

    let owners: std::collections::HashMap<&str, &str> = open_mrs
        .iter()
        .map(|(change_id, email)| (change_id.as_str(), email.as_str()))
        .collect();

    Ok(pushed
        .iter()
        .filter_map(|commit| {
            let change_id = commit.change_id.as_deref()?;
            let owner = owners.get(change_id)?;
            let foreign =
                *owner != commit.author_email.as_str() || *owner != commit.committer_email.as_str();
            foreign.then(|| {
                let reason = format!(
                    "change {change_id} belongs to an open merge request owned by {owner}; \
                     rewriting it requires the owner's identity (author {}, committer {})",
                    commit.author_email, commit.committer_email
                );
                RejectedUpdate {
                    ref_name: commit.ref_name.clone(),
                    old: commit.old.clone(),
                    new: commit.new.clone(),
                    identity: commit.author_email.clone(),
                    reason,
                }
            })
        })
        .collect())
}

/// story: e10s01 — the protected set for one repo, derived from
/// `repositories.trunk` plus `refs/tags/*`. Not user-configurable in 0.2.0.
/// e10s02 reuses this resolver for display.
pub async fn protected_refs(
    pool: &PgPool,
    repo_id: uuid::Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    let trunk: String = sqlx::query_scalar("SELECT trunk FROM repositories WHERE id = $1")
        .bind(repo_id)
        .fetch_one(pool)
        .await?;
    Ok(vec![
        format!("refs/heads/{trunk}"),
        "refs/tags/*".to_string(),
    ])
}

/// story: e10s01 — match a ref name against the protected set: an exact
/// match (trunk branch) or a `*`-suffix prefix match (`refs/tags/*`).
pub fn ref_is_protected(ref_name: &str, protected: &[String]) -> bool {
    protected
        .iter()
        .any(|pattern| match pattern.strip_suffix('*') {
            Some(prefix) => ref_name.starts_with(prefix),
            None => ref_name == pattern,
        })
}

/// story: e10s01 — push-admission rule: refuse any update that
/// rewrites a protected ref (non-fast-forward advance or deletion) before
/// any ref moves. Creation (old = zeros) and fast-forward advances pass;
/// unprotected refs are never examined. Returns human-readable reasons;
/// empty = allow. `git_dir` is the bare repo (hook cwd) where ancestry is
/// checked with argv-only `git merge-base --is-ancestor <old> <new>`.
pub async fn check_protected_refs(
    pool: &PgPool,
    repo_id: uuid::Uuid,
    updates: &[PushedRef],
    git_dir: &std::path::Path,
) -> Result<Vec<String>, sqlx::Error> {
    Ok(protected_ref_rejections(
        pool,
        repo_id,
        updates,
        git_dir,
        // story: e10s03 — the string-only wrapper has no identity context;
        // the hook uses admit_push, which supplies the parsed identities
        &std::collections::HashMap::new(),
    )
    .await?
    .into_iter()
    .map(|rejected| rejected.reason)
    .collect())
}

/// story: e10s03 — the e10s01 protected-ref rule with per-ref attribution
/// for the audit record. `identities` maps a ref name to the pushing
/// identity parsed from its new head commit; misses record `"unknown"`.
pub async fn protected_ref_rejections(
    pool: &PgPool,
    repo_id: uuid::Uuid,
    updates: &[PushedRef],
    git_dir: &std::path::Path,
    identities: &std::collections::HashMap<String, String>,
) -> Result<Vec<RejectedUpdate>, sqlx::Error> {
    let protected = protected_refs(pool, repo_id).await?;
    let mut rejections = Vec::new();
    for update in updates {
        if !ref_is_protected(&update.ref_name, &protected) {
            continue;
        }
        let reason = if update.new.chars().all(|c| c == '0') {
            format!(
                "refusing to delete protected ref {}: its history is immutable",
                update.ref_name
            )
        } else if !update.is_create() && !is_ancestor(git_dir, &update.old, &update.new) {
            format!(
                "refusing non-fast-forward update of protected ref {}: new head does not contain the recorded head",
                update.ref_name
            )
        } else {
            continue;
        };
        rejections.push(RejectedUpdate {
            ref_name: update.ref_name.clone(),
            old: update.old.clone(),
            new: update.new.clone(),
            identity: identities
                .get(&update.ref_name)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string()),
            reason,
        });
    }
    Ok(rejections)
}

/// story: e10s01 — ancestry inside `git_dir` via argv-only git, same
/// subprocess pattern (and quarantine visibility) as the hook's
/// `git cat-file`. Fails closed: an error resolving either oid counts as
/// "not an ancestor" and rejects.
fn is_ancestor(git_dir: &std::path::Path, old: &str, new: &str) -> bool {
    std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", old, new])
        .current_dir(git_dir)
        // never leak git's own diagnostics into the hook's stderr — the
        // pusher only sees our reasons
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// story: e03s01, e10s01 — identity extraction, moved out of the
/// hook body so the whole admission path is testable: for each branch
/// create/advance, read the new head's identity from the (quarantined)
/// object store of `git_dir` via argv-only `git cat-file`.
pub fn pushed_commits(updates: &[PushedRef], git_dir: &std::path::Path) -> Vec<PushedCommit> {
    let mut pushed = Vec::new();
    for update in updates {
        // identity rule covers branch updates only; tags and deletes are
        // the protected-ref rule's job
        if !update.ref_name.starts_with("refs/heads/") || update.new.chars().all(|c| c == '0') {
            continue;
        }
        let Ok(output) = std::process::Command::new("git")
            .args(["cat-file", "commit", &update.new])
            .current_dir(git_dir)
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let Some(identity) = parse_commit_identity(&String::from_utf8_lossy(&output.stdout)) else {
            continue;
        };
        pushed.push(PushedCommit {
            ref_name: update.ref_name.clone(),
            old: update.old.clone(),
            new: update.new.clone(),
            change_id: Some(identity.change_id),
            author_email: identity.author_email,
            committer_email: identity.committer_email,
        });
    }
    pushed
}

/// story: e10s01, e10s03 — push admission: both guard rules over one push
/// — protected-ref rewrites (e10s01) and foreign-identity rewrites of
/// open-MR changes (e03s01) — attributed per refused ref update so the
/// hook can persist each one to the audit table. Empty = allow. The hook
/// exits 1 on any rejection before any ref moves.
pub async fn admit_push(
    pool: &PgPool,
    repo_id: uuid::Uuid,
    updates: &[PushedRef],
    git_dir: &std::path::Path,
) -> Result<Vec<RejectedUpdate>, sqlx::Error> {
    let pushed = pushed_commits(updates, git_dir);
    let identities: std::collections::HashMap<String, String> = pushed
        .iter()
        .map(|commit| (commit.ref_name.clone(), commit.author_email.clone()))
        .collect();
    let mut rejections =
        protected_ref_rejections(pool, repo_id, updates, git_dir, &identities).await?;
    if !pushed.is_empty() {
        rejections.extend(identity_rejections(pool, repo_id, &pushed).await?);
    }
    Ok(rejections)
}

/// story: e10s03 — persist one audit row per refused ref update, in one
/// statement per rejection. Errors propagate to the hook, which logs them
/// and still refuses the push: a failed insert never accepts anything.
pub async fn record_rejections(
    pool: &PgPool,
    repo_id: uuid::Uuid,
    rejections: &[RejectedUpdate],
) -> Result<u64, sqlx::Error> {
    let mut recorded = 0u64;
    for rejection in rejections {
        let rows = sqlx::query(
            "INSERT INTO push_rejections (repo_id, ref_name, old_sha, new_sha, identity, reason) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(repo_id)
        .bind(&rejection.ref_name)
        .bind(&rejection.old)
        .bind(&rejection.new)
        .bind(&rejection.identity)
        .bind(&rejection.reason)
        .execute(pool)
        .await?
        .rows_affected();
        recorded += rows;
    }
    Ok(recorded)
}

/// story: e03s01 — parse the pkt-line command list of a receive-pack POST:
/// `<4-hex-len><old> <new> <ref>\n` frames, terminated by flush-pkt.
pub fn parse_push_commands(pkt_bytes: &[u8]) -> Vec<PushedRef> {
    let mut updates = Vec::new();
    let mut offset = 0;
    while offset + 4 <= pkt_bytes.len() {
        let len_bytes = &pkt_bytes[offset..offset + 4];
        let Ok(len_text) = std::str::from_utf8(len_bytes) else {
            break;
        };
        let Ok(len) = usize::from_str_radix(len_text, 16) else {
            break; // flush-pkt (0000) or malformed
        };
        if len < 4 || offset + len > pkt_bytes.len() {
            break;
        }
        let payload = &pkt_bytes[offset + 4..offset + len];
        let line = String::from_utf8_lossy(payload);
        if let Some((old, rest)) = line.trim_end().split_once(' ') {
            if let Some((new, ref_name)) = rest.split_once(' ') {
                updates.push(PushedRef {
                    old: old.to_string(),
                    new: new.to_string(),
                    ref_name: ref_name.to_string(),
                });
            }
        }
        offset += len;
    }
    updates
}
