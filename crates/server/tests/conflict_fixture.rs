// story: e08s01 — acceptance (e08s01t1): the fixture repo carries jj's git
// conflict form — `jj:trees` header parses, `.jjconflict-base-0` and
// `.jjconflict-side-0` trees are present, and `refs/jj/keep/*` resolves.
mod conflict_support;

mod conflict_fixture {
    use super::conflict_support;
    use std::path::Path;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git spawn");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn fixture() -> (tempfile::TempDir, conflict_support::Fixture) {
        let dir = tempfile::tempdir().expect("fixture dir");
        let fixture = conflict_support::build(dir.path());
        (dir, fixture)
    }

    #[test]
    fn conflicted_commit_parses_jj_trees_header() {
        let (dir, fixture) = fixture();
        let raw = git(dir.path(), &["cat-file", "commit", &fixture.conflicted]);
        let trees = raw
            .lines()
            .find_map(|line| line.strip_prefix("jj:trees "))
            .expect("jj:trees commit header");
        let hexes: Vec<&str> = trees.split_whitespace().collect();
        assert_eq!(hexes.len(), 3, "one base term and two side terms: {trees}");
        for hex in &hexes {
            assert_eq!(git(dir.path(), &["cat-file", "-t", hex]).trim(), "tree");
            let entries = git(dir.path(), &["ls-tree", hex]);
            assert!(
                entries.contains("conflict.txt"),
                "term tree {hex} keeps the conflicted file:\n{entries}"
            );
        }
        let parent = git(dir.path(), &["cat-file", "commit", &fixture.parent]);
        assert!(
            !parent.contains("jj:trees "),
            "parent commit is resolved, not conflicted"
        );
    }

    #[test]
    fn conflict_trees_and_readme_present() {
        let (dir, fixture) = fixture();
        let listing = git(dir.path(), &["ls-tree", &fixture.conflicted]);
        for expected in [
            ".jjconflict-base-0",
            ".jjconflict-side-0",
            ".jjconflict-side-1",
            "JJ-CONFLICT-README",
            "conflict.txt",
        ] {
            assert!(
                listing.contains(expected),
                "missing {expected} entry:\n{listing}"
            );
        }
        let base_line = listing
            .lines()
            .find(|line| line.contains(".jjconflict-base-0"))
            .unwrap();
        assert!(
            base_line.starts_with("040000 tree"),
            "base entry is a subtree: {base_line}"
        );
        let term = |name: &str| {
            git(
                dir.path(),
                &[
                    "show",
                    &format!("{}:{name}/conflict.txt", fixture.conflicted),
                ],
            )
        };
        assert_eq!(term(".jjconflict-base-0"), "base\n");
        assert_eq!(term(".jjconflict-side-0"), "side0\n");
        assert_eq!(term(".jjconflict-side-1"), "side1\n");
        let plain = git(
            dir.path(),
            &["show", &format!("{}:conflict.txt", fixture.conflicted)],
        );
        assert_eq!(plain, "base\n", "plain-git view is the first term");
        let readme = git(
            dir.path(),
            &[
                "show",
                &format!("{}:JJ-CONFLICT-README", fixture.conflicted),
            ],
        );
        assert!(
            readme.contains(".jjconflict-* directories represent the different inputs"),
            "jj's conflict README: {readme}"
        );
    }

    #[test]
    fn keep_refs_pin_parent_and_conflicted_commits() {
        let (dir, fixture) = fixture();
        for commit in [&fixture.parent, &fixture.conflicted] {
            let ref_name = format!("refs/jj/keep/{commit}");
            let resolved = git(dir.path(), &["rev-parse", "--verify", &ref_name]);
            assert_eq!(resolved.trim(), commit, "{ref_name} pins {commit}");
        }
        let out = Command::new("git")
            .args(["fsck"])
            .current_dir(dir.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git fsck spawn");
        assert!(
            out.status.success(),
            "fixture fsck: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
