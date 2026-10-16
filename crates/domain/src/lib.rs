// story: e01s01
use std::fmt;

pub const MIN_PASSWORD_LEN: usize = 10;
pub const MAX_REPO_NAME_LEN: usize = 100;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Visibility {
    Public,
    Private,
}

impl Visibility {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Visibility::Public),
            "private" => Some(Visibility::Private),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Private => "private",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum VisibilityError {
    BadVisibility,
}

/// story: e02s01 — repo names: ^[a-z0-9][a-z0-9._-]{0,99}$
pub fn repo_name_valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_REPO_NAME_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
        && !name.starts_with(['.', '-'])
}

/// story: e02s01 — safe storage path: validated owner/name only, then join.
pub fn storage_path(root: &str, owner: &str, name: &str) -> std::path::PathBuf {
    debug_assert!(repo_name_valid(name));
    std::path::Path::new(root)
        .join(owner)
        .join(format!("{name}.git"))
}

/// story: e05s01 — jj workspace metadata path, colocated beside the bare
/// repo. The kata sidecar loads this dir via `Workspace::load`.
///
/// story: e05s02 — flattened to a single level (`{owner}__{name}`) under a
/// shared `.meta/` base. kata's `scan_dir` is depth-1, so a nested
/// `.meta/{owner}/{name}` layout would be invisible to the sidecar. The
/// `__` separator keeps owner and name recoverable and can't collide with
/// a valid repo name (repo names allow only a single `_`).
pub fn meta_path(root: &str, owner: &str, name: &str) -> std::path::PathBuf {
    debug_assert!(repo_name_valid(name));
    std::path::Path::new(root)
        .join(".meta")
        .join(format!("{owner}__{name}"))
}

#[derive(Debug, PartialEq, Eq)]
pub enum RegistrationError {
    InvalidEmail,
    PasswordTooShort,
}

impl fmt::Display for RegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistrationError::InvalidEmail => write!(f, "invalid email address"),
            RegistrationError::PasswordTooShort => {
                write!(f, "password must be at least {MIN_PASSWORD_LEN} characters")
            }
        }
    }
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub fn validate_registration(
    email: &str,
    password: &str,
) -> Result<(String, String), RegistrationError> {
    let email = normalize_email(email);
    if !is_valid_email(&email) {
        return Err(RegistrationError::InvalidEmail);
    }
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(RegistrationError::PasswordTooShort);
    }
    let handle = handle_from_email(&email);
    Ok((email, handle))
}

/// story: e02s01 — default handle = email local part, lowercased, sanitized.
pub fn handle_from_email(email: &str) -> String {
    let local = email.split('@').next().unwrap_or("user");
    let cleaned: String = local
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "u".to_string()
    } else if cleaned.starts_with(['.', '-']) {
        format!("u{}", cleaned.trim_matches(['.', '-']))
    } else {
        cleaned
    }
}

fn is_valid_email(email: &str) -> bool {
    let (local, domain) = match email.split_once('@') {
        Some(pair) => pair,
        None => return false,
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.contains(char::is_whitespace)
}

// story: e03s02 — one open MR's identity for stack derivation.
#[derive(Debug, Clone)]
pub struct StackInput {
    pub change_id: String,
    pub latest_commit: String,
}

/// story: e03s02 — invariant 3: stack structure comes from commit parents,
/// never stored. Returns stacks bottom-first; each stack is change-ids in
/// bottom-to-top order. A commit whose ancestry reaches `trunk` without
/// passing another MR is a stack bottom; an unexplainable MR is still
/// rendered (its own stack) — data is never dropped.
pub fn derive_stacks(
    mrs: &[StackInput],
    parents: std::collections::HashMap<String, Vec<String>>,
) -> Vec<Vec<String>> {
    use std::collections::HashMap;

    let by_commit: HashMap<&str, &str> = mrs
        .iter()
        .map(|mr| (mr.latest_commit.as_str(), mr.change_id.as_str()))
        .collect();

    // parent of a commit's first parent chain that is itself an MR commit
    fn linked_parent(
        commit: &str,
        parents: &HashMap<String, Vec<String>>,
        by_commit: &HashMap<&str, &str>,
    ) -> Option<String> {
        let first = parents.get(commit)?.first()?;
        if by_commit.contains_key(first.as_str()) {
            Some(first.clone())
        } else {
            None
        }
    }

    let mut bottoms: Vec<&StackInput> = Vec::new();
    let mut children: HashMap<String, Vec<&StackInput>> = HashMap::new();
    for mr in mrs {
        // an MR whose first parent is another MR's latest commit joins that
        // stack; everything else (trunk parent, unknown parent, no parent)
        // is a bottom — render, never drop
        match linked_parent(&mr.latest_commit, &parents, &by_commit) {
            Some(parent_commit) => {
                children.entry(parent_commit).or_default().push(mr);
            }
            None => bottoms.push(mr),
        }
    }

    // deterministic order: sort bottoms by change_id; children by change_id
    bottoms.sort_by(|a, b| a.change_id.cmp(&b.change_id));
    let mut stacks = Vec::new();
    for bottom in bottoms {
        let mut stack = vec![bottom.change_id.clone()];
        let mut frontier = vec![bottom.latest_commit.clone()];
        while let Some(commit) = frontier.pop() {
            let mut kids: Vec<&StackInput> = children
                .get(&commit)
                .map(|v| v.as_slice())
                .unwrap_or(&[])
                .to_vec();
            kids.sort_by(|a, b| a.change_id.cmp(&b.change_id));
            for kid in kids {
                stack.push(kid.change_id.clone());
                frontier.push(kid.latest_commit.clone());
            }
        }
        stacks.push(stack);
    }
    stacks
}
