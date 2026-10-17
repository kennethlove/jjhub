// story: e07s01 — read-only git plumbing for the native file browser.
// argv-only, no shell; refs and paths validated before they reach git.

use std::path::Path;

/// story: e07s01 — what a blob lookup found.
#[derive(Debug)]
pub enum Blob {
    Text(String),
    Binary { len: usize },
}

/// story: e07s01 — reject anything that could escape the repo: traversal,
/// absolute paths, NUL, leading dashes (argv injection).
pub fn validate_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Ok(());
    }
    if path.starts_with('/') || path.starts_with('-') {
        return Err("path must be relative".into());
    }
    if path.contains('\0') || path.contains('\\') {
        return Err("invalid path".into());
    }
    for part in path.split('/') {
        if part == ".." || part == "." {
            return Err("path traversal is not allowed".into());
        }
    }
    Ok(())
}

/// story: e07s01 — ref must resolve to a commit; anything else is 404.
fn resolve_commit(bare: &Path, git_ref: &str) -> Option<String> {
    if git_ref.starts_with('-') || git_ref.contains('\0') || git_ref.contains(char::is_whitespace) {
        return None;
    }
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(bare)
        .args(["rev-parse", "--verify", &format!("{git_ref}^{{commit}}")])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// story: e07s01 — non-recursive listing at ref+path. Each entry is
/// (name, is_dir). None = ref does not resolve.
pub fn list_tree(bare: &Path, git_ref: &str, path: &str) -> Option<Vec<(String, bool)>> {
    validate_path(path).ok()?;
    resolve_commit(bare, git_ref)?;
    let spec = if path.is_empty() {
        format!("{git_ref}:")
    } else {
        format!("{git_ref}:{path}")
    };
    // full format + -z: the type column is the only reliable dir marker
    // (-z omits the trailing slash git puts on tree names)
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(bare)
        .args(["ls-tree", "-z", &spec])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .ok()?;
    if !output.status.success() {
        // valid commit but missing tree (empty dir): empty listing
        return Some(Vec::new());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let entries = stdout
        .split('\0')
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            // "<mode> <type> <sha>\t<name>"
            let mut parts = entry.splitn(3, ' ');
            parts.next()?; // mode
            let kind = parts.next()?;
            let name_with_sha = parts.next()?;
            let name = name_with_sha.split_once('\t')?.1;
            (kind == "tree" || kind == "blob").then(|| (name.to_string(), kind == "tree"))
        })
        .collect();
    Some(entries)
}

/// story: e07s01 — file contents at ref+path; binary detection by NUL byte
/// in the first 8000 bytes (git's own heuristic).
pub fn read_blob(bare: &Path, git_ref: &str, path: &str) -> Result<Blob, String> {
    validate_path(path)?;
    if path.is_empty() {
        return Err("not a file".into());
    }
    if resolve_commit(bare, git_ref).is_none() {
        return Err("unknown revision".into());
    }
    let spec = format!("{git_ref}:{path}");
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(bare)
        .args(["show", &spec])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .map_err(|e| format!("spawn git: {e}"))?;
    if !output.status.success() {
        return Err("no such file at revision".into());
    }
    if output.stdout[..output.stdout.len().min(8000)].contains(&0) {
        return Ok(Blob::Binary {
            len: output.stdout.len(),
        });
    }
    Ok(Blob::Text(
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}
