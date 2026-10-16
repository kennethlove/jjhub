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

/// story: e03s01 — invariant 2 decision: one rejection message per pushed
/// commit that rewrites an OPEN MR's change-id with a foreign author email.
/// Merged and abandoned MRs never block (e03s03 terminal rule; reopen
/// belongs to the upsert, which re-arms the guard once the row is open).
pub async fn check_push(
    pool: &PgPool,
    repo_id: uuid::Uuid,
    pushed: Vec<PushedCommit>,
) -> Result<Vec<String>, sqlx::Error> {
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
                format!(
                    "change {change_id} belongs to an open merge request owned by {owner}; \
                     rewriting it requires the owner's identity (author {}, committer {})",
                    commit.author_email, commit.committer_email
                )
            })
        })
        .collect())
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
