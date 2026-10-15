// story: e05s01
use std::path::Path;

fn git(args: &[&str], cwd: Option<&Path>) {
    let mut cmd = std::process::Command::new("git");
    cmd.args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

mod jj_metadata_init {
    use super::*;

    #[tokio::test]
    async fn init_colocates_workspace_with_bare_repo() {
        let root = tempfile::tempdir().unwrap();
        let bare = root.path().join("repos/alice/demo.git");
        let meta = root.path().join("meta/alice/demo");
        std::fs::create_dir_all(bare.parent().unwrap()).unwrap();
        git(&["init", "--bare", bare.to_str().unwrap()], None);

        server::jj_meta::init(&meta, &bare)
            .await
            .expect("jj_meta::init");

        assert!(
            meta.join(".jj/repo").exists(),
            "jj workspace metadata created at {meta:?}"
        );
    }

    #[tokio::test]
    async fn init_rejects_missing_bare_repo() {
        let root = tempfile::tempdir().unwrap();
        let result =
            server::jj_meta::init(&root.path().join("meta/x"), &root.path().join("nope.git")).await;
        assert!(result.is_err(), "missing bare repo must error");
    }
}

mod workspace_load_proof {
    use super::*;

    // story: e05s01 — kata-side proof: Workspace::load opens the pair jjhub
    // created and resolves the trunk bookmark's tree (unknown #1 follow-through).
    #[tokio::test]
    async fn workspace_load_reads_trunk_tree() {
        let root = tempfile::tempdir().unwrap();
        let bare = root.path().join("repos/alice/demo.git");
        let meta = root.path().join("meta/alice/demo");
        std::fs::create_dir_all(bare.parent().unwrap()).unwrap();
        git(&["init", "--bare", bare.to_str().unwrap()], None);

        // seed one commit on trunk through a scratch clone
        let seed = root.path().join("seed");
        git(&["init", "-b", "trunk", seed.to_str().unwrap()], None);
        std::fs::write(seed.join("hello.txt"), "hello jjhub\n").unwrap();
        git(&["add", "."], Some(&seed));
        git(
            &[
                "-c",
                "user.email=a@e.com",
                "-c",
                "user.name=A",
                "commit",
                "-m",
                "init",
            ],
            Some(&seed),
        );
        git(&["push", bare.to_str().unwrap(), "trunk"], Some(&seed));

        server::jj_meta::init(&meta, &bare)
            .await
            .expect("jj_meta::init");

        // the load path kata uses
        let loaded = server::jj_meta::load_trunk_file_names(&meta)
            .await
            .expect("workspace load");
        assert!(
            loaded.iter().any(|name| name == "hello.txt"),
            "trunk tree contains pushed file: {loaded:?}"
        );
    }
}
