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

// story: e08s02 — conflict-aware reads: jj stores a conflicted path as a
// `.jjconflict*` directory set at the commit ROOT and records the term
// trees in `jj:trees` / `jj:conflict-labels` commit headers. All of this
// is per-path: a path identical in all terms must render normally.

/// story: e08s02 — guard against an absurdly-many-sides commit turning
/// one blob view into thousands of `git show` spawns.
const MAX_CONFLICT_TERMS: usize = 16;

/// story: e08s02 — how many bytes of one term get rendered (NFR §15 cap).
const SIDE_RENDER_CAP: usize = 1024 * 1024;

/// story: e08s02 — one conflict term (a side or a base), after render cap
/// and binary detection.
#[derive(Debug)]
pub enum Side {
    Text { text: String, truncated: bool },
    Binary { len: usize },
    Missing,
}

/// story: e08s02 — a path's conflict, in write order: sides[k] and
/// bases[k] are the k-th add/remove of the term list. Labels come from
/// the `jj:conflict-labels` header when present, else `side-k` / `base` /
/// `base-k`.
#[derive(Debug)]
pub struct Conflict {
    pub sides: Vec<Side>,
    pub bases: Vec<Side>,
    pub side_labels: Vec<String>,
    pub base_labels: Vec<String>,
}

/// story: e08s02 — parse a multi-line git commit header body (continuation
/// lines start with a space, per git header folding). jj headers may live
/// in the commit header block or in the message.
fn header_value<'a>(headers: &'a str, name: &str) -> Option<String> {
    let mut lines = headers.lines();
    let first = lines.find(|line| line.starts_with(&format!("{name} ")))?;
    let mut value = first.strip_prefix(&format!("{name} "))?.to_string();
    for line in lines {
        match line.strip_prefix(' ') {
            Some(cont) => {
                value.push('\n');
                value.push_str(cont);
            }
            None => break,
        }
    }
    Some(value)
}

/// story: e08s02 — classify a root-tree entry: (is_base, index) for
/// `.jjconflict-base-N` / `.jjconflict-side-N`, else None.
fn jjconflict_dir(name: &str) -> Option<(bool, usize)> {
    let (is_base, rest) = if let Some(rest) = name.strip_prefix(".jjconflict-base-") {
        (true, rest)
    } else {
        (false, name.strip_prefix(".jjconflict-side-")?)
    };
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((is_base, rest.parse().ok()?))
}

/// story: e08s02 — read the commit root tree's conflict directory trees as
/// term sides. None = no conflict at this path.
pub fn read_conflict(bare: &Path, git_ref: &str, path: &str) -> Option<Conflict> {
    validate_path(git_ref).ok()?;
    if path.is_empty() {
        return None;
    }
    let commit = resolve_commit(bare, git_ref)?;

    // jj headers live in the commit object, not the tree
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(bare)
        .args(["cat-file", "commit", &commit])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    // jj headers may sit in the commit header block or (as the test
    // fixtures do) in the message; scan the whole object
    let headers = raw.as_ref();
    let trees_value = header_value(headers, "jj:trees")?;
    // a jj-conflicted commit records one tree per term, add first: an odd
    // count of 40-hex values. clean commits have no such header at all.
    let terms: Vec<&str> = trees_value.split_whitespace().collect();
    if terms.len() < 3 || terms.len() % 2 == 0 || terms.len() > MAX_CONFLICT_TERMS {
        return None;
    }
    if !terms.iter().all(|t| {
        t.len() == 40 && t.bytes().all(|b| b.is_ascii_hexdigit() || b.is_ascii_uppercase())
    }) {
        return None;
    }

    // labels: interleaved like trees, even = side, odd = base
    let mut side_labels = Vec::new();
    let mut base_labels = Vec::new();
    if let Some(labels) = header_value(headers, "jj:conflict-labels") {
        for (index, label) in labels.split_whitespace().enumerate() {
            if index >= terms.len() {
                break;
            }
            if index % 2 == 0 {
                side_labels.push(label.to_string());
            } else {
                base_labels.push(label.to_string());
            }
        }
    }
    while side_labels.len() * 2 < terms.len() {
        side_labels.push(format!("side-{}", side_labels.len()));
    }
    if base_labels.is_empty() {
        base_labels.push("base".into());
    }
    while base_labels.len() * 2 + 1 < terms.len() {
        base_labels.push(format!("base-{}", base_labels.len()));
    }

    // actual term trees: the root tree's .jjconflict-{base,side}-N dirs
    // (read from the tree, not the header — labels can misorder, names cannot)
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(bare)
        .args(["ls-tree", "-z", &format!("{commit}:")])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut side_trees: Vec<(usize, String)> = Vec::new();
    let mut base_trees: Vec<(usize, String)> = Vec::new();
    let raw = String::from_utf8_lossy(&output.stdout);
    for entry in raw.split('\0').filter(|s| !s.is_empty()) {
        // "<mode> <type> <sha>\t<name>"
        let mut parts = entry.splitn(3, ' ');
        parts.next()?; // mode
        let kind = parts.next()?;
        if kind != "tree" {
            continue;
        }
        let name_with_sha = parts.next()?;
        let (sha, name) = name_with_sha.split_once('\t')?;
        if let Some((is_base, index)) = jjconflict_dir(name) {
            if index >= MAX_CONFLICT_TERMS {
                return None;
            }
            let bucket = if is_base {
                &mut base_trees
            } else {
                &mut side_trees
            };
            if let Some(slot) = bucket.iter_mut().find(|(i, _)| *i == index) {
                slot.1 = sha.to_string();
            } else {
                bucket.push((index, sha.to_string()));
            }
        }
    }
    if side_trees.is_empty() {
        return None;
    }
    side_trees.sort_by_key(|(i, _)| *i);
    base_trees.sort_by_key(|(i, _)| *i);
    let side_count = side_trees.len().max(1);
    let base_count = base_trees.len();

    // per-path rule: identical content in every present term = not a
    // conflict (jj's README and placeholder files land in the root tree
    // beside the conflict dirs)
    let mut term_bytes: Vec<Option<Vec<u8>>> = Vec::new();
    for (_, sha) in side_trees.iter().chain(base_trees.iter()) {
        term_bytes.push(term_file_bytes(bare, sha, path));
    }
    match term_bytes.first() {
        Some(Some(first)) if term_bytes.iter().all(|t| t.as_deref() == Some(first.as_slice())) => {
            return None;
        }
        None => return None,
        _ => {}
    }
    let render = |bytes: Option<Vec<u8>>| match bytes {
        None => Side::Missing,
        Some(bytes) if bytes[..bytes.len().min(8000)].contains(&0) => Side::Binary {
            len: bytes.len(),
        },
        Some(bytes) => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let truncated = text.len() > SIDE_RENDER_CAP;
            let display = if truncated {
                let mut cut = SIDE_RENDER_CAP;
                while cut > 0 && !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                format!("{}\n(truncated — first 1 MiB shown)", &text[..cut])
            } else {
                text
            };
            Side::Text {
                text: display,
                truncated,
            }
        }
    };
    let mut sides = Vec::new();
    for bytes in term_bytes.iter().take(side_count).cloned() {
        sides.push(render(bytes));
    }
    let mut bases = Vec::new();
    for bytes in term_bytes
        .iter()
        .skip(side_count)
        .take(base_count)
        .cloned()
    {
        bases.push(render(bytes));
    }
    Some(Conflict {
        sides,
        bases,
        side_labels,
        base_labels,
    })
}

/// story: e08s02 — one term's file contents from its term tree sha;
/// None = the path is absent from that term.
fn term_file_bytes(bare: &Path, tree_sha: &str, path: &str) -> Option<Vec<u8>> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(bare)
        .args(["show", &format!("{tree_sha}:{path}")])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}
