// story: e08s03 — acceptance (e08s03t1): a conflicted trunk degrades to a
// defined state — an empty file-name list, no error string — so pages can
// render their existing empty state plus the conflict flag; a skipped trunk
// bookmark (conflicted target or missing ref, the same `as_normal()` None
// path) returns the same defined state; a clean trunk keeps returning its
// file names unchanged.
mod conflict_support;

mod conflict_meta {
    use super::conflict_support;
    use std::path::Path;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-c")
            .arg("safe.directory=*")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git ran");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn seed_clean_trunk(bare: &Path, root: &Path) {
        std::fs::create_dir_all(bare).unwrap();
        git(bare, &["init", "--bare", "--initial-branch=trunk", "."]);
        let seed = root.join("seed");
        git(root, &["init", "-b", "trunk", seed.to_str().unwrap()]);
        std::fs::write(seed.join("hello.txt"), "hello jjhub\n").unwrap();
        git(&seed, &["add", "."]);
        git(
            &seed,
            &[
                "-c",
                "user.email=a@e.com",
                "-c",
                "user.name=A",
                "commit",
                "-m",
                "init",
            ],
        );
        git(&seed, &["push", bare.to_str().unwrap(), "trunk"]);
    }

    // AC: the conflicted trunk root returns a DEFINED state — Ok with no
    // names — instead of Err("trunk root tree is a conflict"). The pages
    // pair this with the conflict flag from the shared commit helper.
    #[tokio::test]
    async fn conflicted_trunk_root_returns_defined_state() {
        let root = tempfile::tempdir().unwrap();
        let bare = root.path().join("demo.git");
        let meta = root.path().join("meta");
        std::fs::create_dir_all(&bare).unwrap();
        conflict_support::build(&bare);

        server::jj_meta::init(&meta, &bare).await.expect("init");

        let loaded = server::jj_meta::load_trunk_file_names(&meta).await;
        assert_eq!(
            loaded.expect("conflicted trunk is a defined state"),
            Vec::<String>::new(),
            "conflicted trunk yields the empty name list"
        );
    }

    // AC: a trunk bookmark that cannot be resolved as a single normal target
    // is SKIPPED — the same defined state, never an error string. The ref's
    // absence exercises the identical `as_normal()` None branch a conflicted
    // bookmark takes (the git backend writes one ref per bookmark).
    #[tokio::test]
    async fn trunk_bookmark_skip_returns_defined_state() {
        let root = tempfile::tempdir().unwrap();
        let bare = root.path().join("demo.git");
        let meta = root.path().join("meta");
        std::fs::create_dir_all(&bare).unwrap();
        conflict_support::build(&bare);
        git(&bare, &["update-ref", "-d", "refs/heads/trunk"]);

        server::jj_meta::init(&meta, &bare).await.expect("init");

        let loaded = server::jj_meta::load_trunk_file_names(&meta).await;
        assert_eq!(
            loaded.expect("skipped bookmark is a defined state"),
            Vec::<String>::new(),
            "a skipped trunk bookmark yields the empty name list"
        );
    }

    // AC: a clean trunk keeps returning its file names — regression guard.
    #[tokio::test]
    async fn clean_trunk_returns_file_names() {
        let root = tempfile::tempdir().unwrap();
        let bare = root.path().join("demo.git");
        let meta = root.path().join("meta");
        seed_clean_trunk(&bare, root.path());

        server::jj_meta::init(&meta, &bare).await.expect("init");

        let loaded = server::jj_meta::load_trunk_file_names(&meta)
            .await
            .expect("clean trunk loads");
        assert!(
            loaded.iter().any(|name| name == "hello.txt"),
            "clean trunk keeps its names: {loaded:?}"
        );
    }
}
