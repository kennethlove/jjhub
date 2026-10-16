// story: e02s01
use std::path::{Path, PathBuf};
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
        // story: e02s02 — authz lives in our handler; http-backend's own
        // REMOTE_USER gate is bypassed so receive-pack is reachable.
        let configured = Command::new("git")
            .args(["-C"])
            .arg(path)
            .args(["config", "http.receivepack", "true"])
            .output()
            .map_err(|e| format!("spawn git config: {e}"))?;
        if !configured.status.success() {
            return Err(format!(
                "git config http.receivepack failed: {}",
                String::from_utf8_lossy(&configured.stderr)
            ));
        }
        install_pre_receive_hook(path)?;
        Ok(())
    } else {
        Err(format!(
            "git init --bare failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// story: e03s01 — invariant 2 lives in the bare repo's pre-receive hook.
/// The hook re-enters this same binary (`server pre-receive`) with the
/// database URL baked in: http-backend's CGI env does not carry
/// DATABASE_URL, and the hook must be able to refuse the push on its own.
fn install_pre_receive_hook(path: &Path) -> Result<(), String> {
    fn sh_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
    let database_url = std::env::var("DATABASE_URL")
        .map_err(|e| format!("DATABASE_URL required to install hook: {e}"))?;
    let storage_root =
        std::env::var("JJHUB_STORAGE_ROOT").unwrap_or_else(|_| "./data/repos".into());
    // exec the exact binary that created this repo: PATH is unreliable in
    // CGI/hook contexts and absent in test harnesses.
    let bin = std::env::var("JJHUB_BIN").unwrap_or_else(|_| {
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "server".to_string())
    });
    let script = format!(
        "#!/bin/sh\n\
         # story: e03s01 — invariant 2: refuse rewrites of open changes by foreign authors.\n\
         export DATABASE_URL={}\n\
         export JJHUB_STORAGE_ROOT={}\n\
         exec {} pre-receive\n",
        sh_quote(&database_url),
        sh_quote(&storage_root),
        sh_quote(&bin),
    );
    let hooks = path.join("hooks");
    std::fs::create_dir_all(&hooks).map_err(|e| format!("create hooks dir: {e}"))?;
    let hook = hooks.join("pre-receive");
    std::fs::write(&hook, script).map_err(|e| format!("write hook: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod hook: {e}"))?;
    }
    Ok(())
}
