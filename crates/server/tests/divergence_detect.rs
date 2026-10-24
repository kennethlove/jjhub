//! story: e16s01 — divergence detection over pushed revisions.
//!
//! Seeds a bare repo with jj-style commits (change-id headers) and
//! asserts the `divergent_heads` predicate: sibling heads diverge,
//! linear stacks and single revisions do not, missing objects fail
//! safe as not-divergent.

#[cfg(test)]
mod divergence_detect {
    use server::divergence::divergent_heads;
    use std::io::Write;
    use std::path::Path;

    fn git_in(cwd: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Build a jj-style commit object (with a `change-id` header) on
    /// top of `parent` with the staged tree of `dir`.
    fn mkcommit(dir: &Path, parent: &str, change_id: &str, msg: &str) -> String {
        let tree = git_in(dir, &["write-tree"]);
        let parent_line = if parent.chars().all(|c| c == '0') {
            String::new()
        } else {
            format!("parent {parent}\n")
        };
        let object = format!(
            "tree {tree}\n{parent_line}author A <a@e.com> 1700000000 +0000\ncommitter A <a@e.com> 1700000000 +0000\nchange-id {change_id}\n\n{msg}\n"
        );
        let mut child = std::process::Command::new("git")
            .args(["hash-object", "-t", "commit", "-w", "--stdin"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .current_dir(dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(object.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "mkcommit: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Seed a bare repo with a trunk commit T and two siblings S1, S2
    /// sharing one change-id, plus a child C of S1 (linear extension)
    /// and a merge M of S1 and S2. Returns (t, s1, s2, c, m).
    fn seed_diverged(bare: &Path) -> (String, String, String, String, String) {
        let seed = bare.join("../seed");
        std::fs::create_dir_all(&seed).unwrap();
        git_in(&seed, &["init", "-b", "trunk"]);
        std::fs::write(seed.join("base.txt"), "base\n").unwrap();
        git_in(&seed, &["add", "."]);
        let t = mkcommit(
            &seed,
            "0000000000000000000000000000000000000000",
            "0000trunk0000",
            "base",
        );
        // sibling one of the diverging pair
        std::fs::write(seed.join("s1.txt"), "one\n").unwrap();
        git_in(&seed, &["add", "."]);
        let s1 = mkcommit(&seed, &t, "kkkkktaaaaa", "sibling one");
        // sibling two: same parent, same change-id, different content
        git_in(&seed, &["reset", "--mixed", &t]);
        std::fs::write(seed.join("s2.txt"), "two\n").unwrap();
        git_in(&seed, &["add", "."]);
        let s2 = mkcommit(&seed, &t, "kkkkktaaaaa", "sibling two");
        // child of s1 (linear extension of one sibling)
        git_in(&seed, &["reset", "--mixed", &s1]);
        std::fs::write(seed.join("c.txt"), "three\n").unwrap();
        git_in(&seed, &["add", "."]);
        let c = mkcommit(&seed, &s1, "nnnnnbbbbb", "child of sibling one");
        // merge of the two siblings
        let m = {
            let tree = git_in(&seed, &["write-tree"]);
            let object = format!(
                "tree {tree}\nparent {s1}\nparent {s2}\nauthor A <a@e.com> 1700000000 +0000\ncommitter A <a@e.com> 1700000000 +0000\nchange-id kkkkktaaaaa\n\nmerge siblings\n"
            );
            let mut child = std::process::Command::new("git")
                .args(["hash-object", "-t", "commit", "-w", "--stdin"])
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .current_dir(&seed)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(object.as_bytes())
                .unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(
                out.status.success(),
                "merge commit: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git_in(
            &seed,
            &[
                "push",
                bare.to_str().unwrap(),
                &format!("{t}:refs/heads/trunk"),
                &format!("{s1}:refs/heads/s1"),
                &format!("{s2}:refs/heads/s2"),
                &format!("{c}:refs/heads/c"),
                &format!("{m}:refs/heads/m"),
            ],
        );
        (t, s1, s2, c, m)
    }

    #[test]
    fn siblings_with_same_change_id_diverge() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("repo.git");
        std::fs::create_dir_all(&bare).unwrap();
        git_in(&bare, &["init", "--bare"]);
        let (_t, s1, s2, _c, _m) = seed_diverged(&bare);

        let heads = divergent_heads(&bare, &[s1.clone(), s2.clone()]);
        assert_eq!(heads, vec![s1, s2]);
    }

    #[test]
    fn linear_stack_is_not_divergent() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("repo.git");
        std::fs::create_dir_all(&bare).unwrap();
        git_in(&bare, &["init", "--bare"]);
        let (t, s1, _s2, c, _m) = seed_diverged(&bare);

        assert!(divergent_heads(&bare, &[t.clone(), s1.clone()]).is_empty());
        assert!(divergent_heads(&bare, &[s1, c]).is_empty());
    }

    #[test]
    fn merge_head_contains_both_siblings_so_set_is_not_divergent() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("repo.git");
        std::fs::create_dir_all(&bare).unwrap();
        git_in(&bare, &["init", "--bare"]);
        let (_t, s1, s2, _c, m) = seed_diverged(&bare);

        assert!(divergent_heads(&bare, &[s1, s2, m]).is_empty());
    }

    #[test]
    fn single_revision_is_not_divergent() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("repo.git");
        std::fs::create_dir_all(&bare).unwrap();
        git_in(&bare, &["init", "--bare"]);
        let (t, _s1, _s2, _c, _m) = seed_diverged(&bare);

        assert!(divergent_heads(&bare, &[t]).is_empty());
    }

    #[test]
    fn empty_revision_set_is_not_divergent() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("repo.git");
        std::fs::create_dir_all(&bare).unwrap();
        git_in(&bare, &["init", "--bare"]);

        assert!(divergent_heads(&bare, &[]).is_empty());
    }

    #[test]
    fn missing_objects_fail_safe_as_not_divergent() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("repo.git");
        std::fs::create_dir_all(&bare).unwrap();
        git_in(&bare, &["init", "--bare"]);
        let (_t, s1, _s2, _c, _m) = seed_diverged(&bare);

        let ghost = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string();
        assert!(divergent_heads(&bare, &[s1, ghost]).is_empty());
    }
}
