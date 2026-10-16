// story: e03s03
mod merge_execute {
    use server::merge::{merge_bottom, MergeError};

    fn git_in(cwd: &std::path::Path, args: &[&str]) -> String {
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

    /// Seed trunk with commit T and a stacked change commit C (child of T).
    /// Returns (trunk_commit, change_commit).
    async fn seed(bare: &std::path::Path) -> (String, String) {
        let seed = bare.join("../seed");
        std::fs::create_dir_all(&seed).unwrap();
        git_in(&seed, &["init", "-b", "trunk"]);
        std::fs::write(seed.join("base.txt"), "base\n").unwrap();
        git_in(&seed, &["add", "."]);
        git_in(
            &seed,
            &[
                "-c",
                "user.email=a@e.com",
                "-c",
                "user.name=A",
                "commit",
                "-m",
                "base",
            ],
        );
        let trunk_commit = git_in(&seed, &["rev-parse", "HEAD"]);
        std::fs::write(seed.join("feature.txt"), "feature\n").unwrap();
        git_in(&seed, &["add", "."]);
        git_in(
            &seed,
            &[
                "-c",
                "user.email=a@e.com",
                "-c",
                "user.name=A",
                "commit",
                "-m",
                "feat: the change",
            ],
        );
        let change_commit = git_in(&seed, &["rev-parse", "HEAD"]);
        // trunk stays at T; the change object arrives via a scratch ref
        git_in(&seed, &["push", bare.to_str().unwrap(), "trunk"]);
        git_in(
            &seed,
            &["push", bare.to_str().unwrap(), "HEAD:refs/heads/work"],
        );
        git_in(
            bare,
            &[
                "update-ref",
                "refs/heads/trunk",
                &trunk_commit,
                &change_commit,
            ],
        );
        (trunk_commit, change_commit)
    }

    pub(crate) async fn seed_repo(
        pool: &sqlx::PgPool,
        merge_setting: &str,
    ) -> (uuid::Uuid, uuid::Uuid) {
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let repo_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO repositories (owner_id, name, merge_setting) VALUES ($1, 'demo', $2) RETURNING id",
        )
        .bind(user_id)
        .bind(merge_setting)
        .fetch_one(pool)
        .await
        .unwrap();
        (repo_id, user_id)
    }

    pub(crate) async fn open_mr(
        pool: &sqlx::PgPool,
        repo_id: uuid::Uuid,
        owner: uuid::Uuid,
        change: &str,
        commit: &str,
    ) -> uuid::Uuid {
        let mr_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(repo_id)
        .bind(owner)
        .bind(change)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2)")
            .bind(mr_id)
            .bind(commit)
            .execute(pool)
            .await
            .unwrap();
        mr_id
    }

    async fn setup(
        pool: &sqlx::PgPool,
        root: &std::path::Path,
        merge_setting: &str,
    ) -> (uuid::Uuid, uuid::Uuid, String, String) {
        let bare = domain::storage_path(root.to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let (trunk_commit, change_commit) = seed(&bare).await;
        let (repo_id, alice) = seed_repo(pool, merge_setting).await;
        (repo_id, alice, trunk_commit, change_commit)
    }

    pub(crate) fn git_cat(bare: &std::path::Path, rev: &str) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(bare)
            .args(["cat-file", "commit", rev])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn squash_merge_advances_trunk_and_merges_mr(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (repo_id, alice, _trunk, change) = setup(&pool, root.path(), "squash").await;
        open_mr(&pool, repo_id, alice, "aaaa", &change).await;

        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        let summary = merge_bottom(&pool, root.path().to_str().unwrap(), repo_id, "aaaa", alice)
            .await
            .expect("merge");

        let raw = git_cat(&bare, &summary.new_trunk_commit);
        assert!(
            raw.lines().filter(|l| l.starts_with("parent ")).count() == 1,
            "squash has one parent: {raw}"
        );
        assert!(raw.contains("feat: the change"), "message preserved: {raw}");
        assert!(
            raw.contains("author A <a@e.com>"),
            "author preserved: {raw}"
        );
        let tree = git_in(
            &bare,
            &[
                "rev-parse",
                &format!("{}^{{tree}}", summary.new_trunk_commit),
            ],
        );
        let change_tree = git_in(&bare, &["rev-parse", &format!("{change}^{{tree}}")]);
        assert_eq!(tree, change_tree, "squash tree equals the change tree");

        let state: String =
            sqlx::query_scalar("SELECT state FROM merge_requests WHERE change_id = 'aaaa'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, "merged");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn merge_commit_setting_keeps_two_parents(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (repo_id, alice, _trunk, change) = setup(&pool, root.path(), "merge_commit").await;
        open_mr(&pool, repo_id, alice, "aaaa", &change).await;

        let summary = merge_bottom(&pool, root.path().to_str().unwrap(), repo_id, "aaaa", alice)
            .await
            .expect("merge");
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        let raw = git_cat(&bare, &summary.new_trunk_commit);
        assert_eq!(
            raw.lines().filter(|l| l.starts_with("parent ")).count(),
            2,
            "merge commit keeps both parents: {raw}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn non_owner_cannot_merge(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (repo_id, alice, _trunk, change) = setup(&pool, root.path(), "squash").await;
        // MR owned by a different user
        let bob: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('bob@example.com', 'x', 'bob') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        open_mr(&pool, repo_id, bob, "aaaa", &change).await;

        // alice (not the MR owner) tries to merge bob's change
        let err = merge_bottom(&pool, root.path().to_str().unwrap(), repo_id, "aaaa", alice)
            .await
            .unwrap_err();
        assert!(matches!(err, MergeError::NotOwner), "got {err:?}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn stale_trunk_update_is_refused(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (repo_id, alice, trunk_commit, change) = setup(&pool, root.path(), "squash").await;
        open_mr(&pool, repo_id, alice, "aaaa", &change).await;
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");

        // invariant 4: the ref update carries the expected old value; feed a
        // stale one the way a concurrent trunk move would
        let new_commit = change.clone();
        let err = server::merge::apply_ref_update(
            &bare,
            "refs/heads/trunk",
            "0000000000000000000000000000000000000001",
            &new_commit,
        );
        assert!(err.is_err(), "stale expected-old must be refused");

        // sanity: correct old value works
        let ok =
            server::merge::apply_ref_update(&bare, "refs/heads/trunk", &trunk_commit, &new_commit);
        assert!(ok.is_ok(), "current expected-old applies: {ok:?}");
    }
}

// story: e03s03 — t2: cascade rebase of descendants.
mod cascade {
    use super::merge_execute::*;
    use server::merge::merge_bottom;

    fn git_in(cwd: &std::path::Path, args: &[&str]) -> String {
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

    /// Build a jj-style commit object (with a `change-id` header, like jj
    /// writes) on top of `parent` with the staged tree of `dir`.
    fn mkcommit(dir: &std::path::Path, parent: &str, change_id: &str, msg: &str) -> String {
        use std::io::Write;
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

    /// Seed trunk at T with a two-change stack C1(aaa) -> C2(bbb).
    async fn seed_stack(bare: &std::path::Path) -> (String, String, String) {
        let seed = bare.join("../seed-stack");
        std::fs::create_dir_all(&seed).unwrap();
        git_in(&seed, &["init", "-b", "trunk"]);
        std::fs::write(seed.join("base.txt"), "base\n").unwrap();
        git_in(&seed, &["add", "."]);
        let trunk_commit = mkcommit(
            &seed,
            "0000000000000000000000000000000000000000",
            "0000trunk0000",
            "base",
        );
        std::fs::write(seed.join("one.txt"), "one\n").unwrap();
        git_in(&seed, &["add", "."]);
        let c1 = mkcommit(&seed, &trunk_commit, "kkkkktaaaaa", "change one");
        std::fs::write(seed.join("two.txt"), "two\n").unwrap();
        git_in(&seed, &["add", "."]);
        let c2 = mkcommit(&seed, &c1, "mmmmmtttttt", "change two");
        git_in(
            &seed,
            &[
                "push",
                bare.to_str().unwrap(),
                &format!("{c2}:refs/heads/work"),
            ],
        );
        git_in(bare, &["update-ref", "refs/heads/trunk", &trunk_commit]);
        (trunk_commit, c1, c2)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn merging_bottom_rebases_descendants(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let (_trunk, c1, c2) = seed_stack(&bare).await;

        let (repo_id, alice) = seed_repo(&pool, "squash").await;
        open_mr(&pool, repo_id, alice, "aaaa", &c1).await;
        open_mr(&pool, repo_id, alice, "bbbb", &c2).await;

        let summary = merge_bottom(&pool, root.path().to_str().unwrap(), repo_id, "aaaa", alice)
            .await
            .expect("merge");

        // bbb stayed open and gained a rebased revision on new trunk
        let bbb: (String,) =
            sqlx::query_as("SELECT state FROM merge_requests WHERE change_id = 'bbbb'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(bbb.0, "open", "descendant stays open");

        let new_rev: Option<String> = sqlx::query_scalar(
            "SELECT rev.commit_id FROM revisions rev \
             JOIN merge_requests mr ON mr.id = rev.mr_id \
             WHERE mr.change_id = 'bbbb' ORDER BY rev.pushed_at DESC LIMIT 1",
        )
        .fetch_optional(&pool)
        .await
        .unwrap()
        .flatten();
        let new_rev = new_rev.expect("descendant has a revision");

        let raw = git_cat(&bare, &new_rev);
        assert!(
            raw.contains(&format!("parent {}", summary.new_trunk_commit)),
            "rebased commit sits on new trunk: {raw}"
        );
        assert!(
            raw.contains("change-id"),
            "change-id header preserved: {raw}"
        );
        let tree = git_in(&bare, &["rev-parse", &format!("{new_rev}^{{tree}}")]);
        let c2_tree = git_in(&bare, &["rev-parse", &format!("{c2}^{{tree}}")]);
        assert_eq!(tree, c2_tree, "descendant tree preserved");

        // two revisions recorded for bbb (original + rebased)
        let revs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM revisions rev JOIN merge_requests mr ON mr.id = rev.mr_id WHERE mr.change_id = 'bbbb'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(revs, 2, "original + rebased revision");
    }
}
