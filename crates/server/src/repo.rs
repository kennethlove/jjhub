// story: e02s01
use std::path::PathBuf;
use std::process::Command;

/// story: e02s01 — initialize a bare git repository at a validated storage path.
pub fn init_bare(path: &PathBuf) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("create storage dir: {e}"))?;
    let output = Command::new("git")
        .args(["init", "--bare", "--initial-branch=trunk"])
        .arg(path)
        .output()
        .map_err(|e| format!("spawn git: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git init --bare failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}
