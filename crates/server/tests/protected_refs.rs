// story: e10s01
//! Server-side protected-ref policy: the set is derived from
//! `repositories.trunk` plus `refs/tags/*`, and rewrites of protected refs
//! are refused at the push gate.

mod protect_config {
    use server::receive_guard::{protected_refs, ref_is_protected};

    async fn seed(pool: &sqlx::PgPool, trunk: Option<&str>) -> uuid::Uuid {
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let repo_id: uuid::Uuid = match trunk {
            Some(trunk) => sqlx::query_scalar(
                "INSERT INTO repositories (owner_id, name, trunk) VALUES ($1, 'demo', $2) RETURNING id",
            )
            .bind(user_id)
            .bind(trunk)
            .fetch_one(pool)
            .await
            .unwrap(),
            None => sqlx::query_scalar(
                "INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo') RETURNING id",
            )
            .bind(user_id)
            .fetch_one(pool)
            .await
            .unwrap(),
        };
        repo_id
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn default_trunk_and_tags_are_protected(pool: sqlx::PgPool) {
        let repo_id = seed(&pool, None).await;
        let set = protected_refs(&pool, repo_id).await.unwrap();
        assert_eq!(
            set,
            vec!["refs/heads/trunk".to_string(), "refs/tags/*".to_string()]
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn custom_trunk_is_honored(pool: sqlx::PgPool) {
        let repo_id = seed(&pool, Some("main")).await;
        let set = protected_refs(&pool, repo_id).await.unwrap();
        assert!(set.contains(&"refs/heads/main".to_string()), "{set:?}");
        assert!(!set.contains(&"refs/heads/trunk".to_string()), "{set:?}");
    }

    #[test]
    fn set_matching_covers_trunk_exact_and_tag_prefix() {
        let set = vec!["refs/heads/trunk".to_string(), "refs/tags/*".to_string()];
        assert!(ref_is_protected("refs/heads/trunk", &set));
        assert!(ref_is_protected("refs/tags/v1.0.0", &set));
        assert!(!ref_is_protected("refs/heads/trunk-copy", &set));
        assert!(!ref_is_protected("refs/heads/change-abc", &set));
    }
}

mod protect_reject {
    use server::receive_guard::{check_protected_refs, PushedRef};

    async fn seed(pool: &sqlx::PgPool) -> uuid::Uuid {
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query_scalar(
            "INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo') RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    fn git_repo_with_commits() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            out
        };
        run(&["init", "-q", "-b", "trunk"]);
        run(&["config", "user.email", "alice@example.com"]);
        run(&["config", "user.name", "Alice"]);
        std::fs::write(dir.path().join("file"), "one").unwrap();
        run(&["add", "file"]);
        run(&["commit", "-q", "-m", "one"]);
        let base = String::from_utf8(run(&["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_string();
        std::fs::write(dir.path().join("file"), "two").unwrap();
        run(&["commit", "-q", "-am", "two"]);
        let head = String::from_utf8(run(&["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_string();
        // a sibling commit: descendant of neither base nor head
        run(&["checkout", "-q", "-b", "side", &base]);
        std::fs::write(dir.path().join("other"), "x").unwrap();
        run(&["add", "other"]);
        run(&["commit", "-q", "-m", "side"]);
        let side = String::from_utf8(run(&["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_string();
        (dir, side, head)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn non_fast_forward_trunk_update_is_rejected(pool: sqlx::PgPool) {
        let repo_id = seed(&pool).await;
        let (dir, sibling, _head) = git_repo_with_commits();
        let updates = vec![PushedRef {
            old: "a".repeat(40),
            new: sibling,
            ref_name: "refs/heads/trunk".to_string(),
        }];
        let rejections = check_protected_refs(&pool, repo_id, &updates, dir.path())
            .await
            .unwrap();
        assert_eq!(rejections.len(), 1, "{rejections:?}");
        assert!(
            rejections[0].contains("refs/heads/trunk"),
            "names the ref: {}",
            rejections[0]
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn protected_ref_deletion_is_rejected(pool: sqlx::PgPool) {
        let repo_id = seed(&pool).await;
        let updates = vec![PushedRef {
            old: "a".repeat(40),
            new: "0".repeat(40),
            ref_name: "refs/tags/v1.0.0".to_string(),
        }];
        let rejections = check_protected_refs(&pool, repo_id, &updates, std::path::Path::new("."))
            .await
            .unwrap();
        assert_eq!(rejections.len(), 1, "{rejections:?}");
        assert!(
            rejections[0].contains("refs/tags/v1.0.0"),
            "names the ref: {}",
            rejections[0]
        );
    }
}

mod protect_allow {
    use server::receive_guard::{check_protected_refs, PushedRef};

    async fn seed(pool: &sqlx::PgPool) -> uuid::Uuid {
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query_scalar(
            "INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo') RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    fn linear_repo() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            out
        };
        run(&["init", "-q", "-b", "trunk"]);
        run(&["config", "user.email", "alice@example.com"]);
        run(&["config", "user.name", "Alice"]);
        std::fs::write(dir.path().join("file"), "one").unwrap();
        run(&["add", "file"]);
        run(&["commit", "-q", "-m", "one"]);
        let base = String::from_utf8(run(&["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_string();
        std::fs::write(dir.path().join("file"), "two").unwrap();
        run(&["commit", "-q", "-am", "two"]);
        let head = String::from_utf8(run(&["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_string();
        (dir, base, head)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn trunk_creation_passes(pool: sqlx::PgPool) {
        let repo_id = seed(&pool).await;
        let updates = vec![PushedRef {
            old: "0".repeat(40),
            new: "b".repeat(40),
            ref_name: "refs/heads/trunk".to_string(),
        }];
        let rejections = check_protected_refs(&pool, repo_id, &updates, std::path::Path::new("."))
            .await
            .unwrap();
        assert!(rejections.is_empty(), "{rejections:?}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn trunk_fast_forward_passes(pool: sqlx::PgPool) {
        let repo_id = seed(&pool).await;
        let (dir, base, head) = linear_repo();
        let updates = vec![PushedRef {
            old: base,
            new: head,
            ref_name: "refs/heads/trunk".to_string(),
        }];
        let rejections = check_protected_refs(&pool, repo_id, &updates, dir.path())
            .await
            .unwrap();
        assert!(rejections.is_empty(), "{rejections:?}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn change_branch_updates_produce_no_protected_rejection(pool: sqlx::PgPool) {
        let repo_id = seed(&pool).await;
        let updates = vec![PushedRef {
            old: "c".repeat(40),
            // sibling of old: a rewrite, but change branches are not protected
            new: "d".repeat(40),
            ref_name: "refs/heads/change-abc".to_string(),
        }];
        let rejections = check_protected_refs(&pool, repo_id, &updates, std::path::Path::new("."))
            .await
            .unwrap();
        assert!(rejections.is_empty(), "{rejections:?}");
    }
}

mod admit {
    use server::receive_guard::{admit_push, PushedRef};

    async fn seed(pool: &sqlx::PgPool) -> (uuid::Uuid, uuid::Uuid) {
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let repo_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo') RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        (repo_id, user_id)
    }

    fn repo_with_commit(body: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str], input: Option<&str>| {
            let mut cmd = std::process::Command::new("git");
            cmd.args(args)
                .current_dir(dir.path())
                .env("GIT_CONFIG_NOSYSTEM", "1");
            if let Some(input) = input {
                use std::io::Write;
                let mut child = cmd
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(input.as_bytes())
                    .unwrap();
                child.wait_with_output().unwrap()
            } else {
                cmd.output().unwrap()
            }
        };
        let out = run(&["init", "-q", "-b", "trunk"], None);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = run(
            &["hash-object", "-t", "commit", "-w", "--stdin"],
            Some(body),
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let commit = String::from_utf8(out.stdout).unwrap().trim().to_string();
        (dir, commit)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn both_guard_rules_run_on_one_push(pool: sqlx::PgPool) {
        let (repo_id, alice) = seed(&pool).await;
        server::mr::upsert_after_push(
            &pool,
            repo_id,
            alice,
            vec![server::mr::PushedChange {
                change_id: "aaaa".into(),
                commit_id: "c1".into(),
                author_email: "alice@example.com".into(),
            }],
        )
        .await
        .unwrap();

        // jj-shaped commit: change-id preserved, committer is foreign —
        // the open-MR identity rule must catch it
        let body = "tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904\n\
                    author Alice <alice@example.com> 1700000000 +0000\n\
                    committer Mallory <mallory@example.com> 1700000000 +0000\n\
                    change-id aaaa\n\
                    \n\
                    rewrite MR change\n";
        let (dir, commit) = repo_with_commit(body);

        let updates = vec![
            PushedRef {
                old: "0".repeat(40),
                new: commit,
                ref_name: "refs/heads/change-xyz".to_string(),
            },
            PushedRef {
                old: "a".repeat(40),
                new: "0".repeat(40),
                ref_name: "refs/heads/trunk".to_string(),
            },
        ];
        let reasons = admit_push(&pool, repo_id, &updates, dir.path())
            .await
            .unwrap();
        assert_eq!(reasons.len(), 2, "{reasons:?}");
        assert!(
            reasons.iter().any(|r| r.contains("refs/heads/trunk")),
            "protected-ref rule ran: {reasons:?}"
        );
        assert!(
            reasons.iter().any(|r| r.contains("aaaa")),
            "identity rule ran: {reasons:?}"
        );
    }
}
