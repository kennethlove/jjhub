// story: e10s03
//! Audit trail for refused pushes: whatever `admit_push` refuses is
//! persisted to `push_rejections` with the pushing identity and the ref,
//! and a failed insert never turns a refusal into an acceptance.

mod protect_audit {
    use server::receive_guard::{admit_push, record_rejections, PushedRef};

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

    /// A jj-shaped commit object (has a `change-id` header) written into a
    /// throwaway repo so the guard's `git cat-file` can read it back.
    fn repo_with_commit(body: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str], input: Option<&str>| {
            let mut cmd = std::process::Command::new("git");
            cmd.args(args)
                .current_dir(dir.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null");
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

    fn jj_commit_body(change_id: &str, committer: &str) -> String {
        format!(
            "tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904\n\
             author Alice <alice@example.com> 1700000000 +0000\n\
             committer {committer} 1700000000 +0000\n\
             change-id {change_id}\n\
             \n\
             rewrite trunk\n"
        )
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn rejected_push_is_recorded_with_identity_and_ref(pool: sqlx::PgPool) {
        let (repo_id, _alice) = seed(&pool).await;
        let (dir, commit) = repo_with_commit(&jj_commit_body("zzzz", "Alice <alice@example.com>"));
        let updates = vec![PushedRef {
            old: "a".repeat(40),
            new: commit.clone(),
            ref_name: "refs/heads/trunk".to_string(),
        }];

        let rejections = admit_push(&pool, repo_id, &updates, dir.path())
            .await
            .unwrap();
        assert_eq!(rejections.len(), 1, "{rejections:?}");
        record_rejections(&pool, repo_id, &rejections)
            .await
            .unwrap();

        let rows: Vec<(
            String,
            String,
            String,
            String,
            String,
            chrono::DateTime<chrono::Utc>,
        )> = sqlx::query_as(
            "SELECT ref_name, old_sha, new_sha, identity, reason, created_at \
                 FROM push_rejections WHERE repo_id = $1",
        )
        .bind(repo_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        let (ref_name, old_sha, new_sha, identity, reason, created_at) = &rows[0];
        assert_eq!(ref_name, "refs/heads/trunk");
        assert_eq!(old_sha, &"a".repeat(40));
        assert_eq!(new_sha, &commit);
        assert_eq!(identity, "alice@example.com", "pushing identity recorded");
        assert!(
            reason.contains("refs/heads/trunk"),
            "reason names the ref: {reason}"
        );
        assert!(created_at.timestamp() > 0, "server-side timestamp set");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn protected_ref_deletion_records_unknown_identity(pool: sqlx::PgPool) {
        let (repo_id, _alice) = seed(&pool).await;
        let updates = vec![PushedRef {
            old: "b".repeat(40),
            new: "0".repeat(40),
            ref_name: "refs/tags/v1.0.0".to_string(),
        }];

        let rejections = admit_push(&pool, repo_id, &updates, std::path::Path::new("."))
            .await
            .unwrap();
        assert_eq!(rejections.len(), 1, "{rejections:?}");
        record_rejections(&pool, repo_id, &rejections)
            .await
            .unwrap();

        let (ref_name, identity): (String, String) =
            sqlx::query_as("SELECT ref_name, identity FROM push_rejections WHERE repo_id = $1")
                .bind(repo_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(ref_name, "refs/tags/v1.0.0");
        assert_eq!(identity, "unknown", "no parseable commit, still recorded");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn identity_violation_is_recorded(pool: sqlx::PgPool) {
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
        let (dir, commit) =
            repo_with_commit(&jj_commit_body("aaaa", "Mallory <mallory@example.com>"));
        let updates = vec![PushedRef {
            old: "0".repeat(40),
            new: commit,
            ref_name: "refs/heads/change-xyz".to_string(),
        }];

        let rejections = admit_push(&pool, repo_id, &updates, dir.path())
            .await
            .unwrap();
        assert_eq!(rejections.len(), 1, "{rejections:?}");
        record_rejections(&pool, repo_id, &rejections)
            .await
            .unwrap();

        let (ref_name, identity, reason): (String, String, String) = sqlx::query_as(
            "SELECT ref_name, identity, reason FROM push_rejections WHERE repo_id = $1",
        )
        .bind(repo_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(ref_name, "refs/heads/change-xyz");
        assert_eq!(identity, "alice@example.com");
        assert!(reason.contains("aaaa"), "reason names the change: {reason}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn accepted_push_writes_no_audit_row(pool: sqlx::PgPool) {
        let (repo_id, _alice) = seed(&pool).await;
        let (dir, base, head) = {
            let dir = tempfile::tempdir().unwrap();
            let run = |args: &[&str]| {
                let out = std::process::Command::new("git")
                    .args(args)
                    .current_dir(dir.path())
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_SYSTEM", "/dev/null")
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
        };
        let updates = vec![PushedRef {
            old: base,
            new: head,
            ref_name: "refs/heads/trunk".to_string(),
        }];

        let rejections = admit_push(&pool, repo_id, &updates, dir.path())
            .await
            .unwrap();
        assert!(rejections.is_empty(), "{rejections:?}");
        record_rejections(&pool, repo_id, &rejections)
            .await
            .unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_rejections")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "accepted push leaves the audit table empty");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn insert_failure_does_not_unrefuse_the_push(pool: sqlx::PgPool) {
        let (repo_id, _alice) = seed(&pool).await;
        let updates = vec![PushedRef {
            old: "c".repeat(40),
            new: "0".repeat(40),
            ref_name: "refs/heads/trunk".to_string(),
        }];
        let rejections = admit_push(&pool, repo_id, &updates, std::path::Path::new("."))
            .await
            .unwrap();
        assert_eq!(rejections.len(), 1, "{rejections:?}");

        sqlx::query("DROP TABLE push_rejections")
            .execute(&pool)
            .await
            .unwrap();
        let record = record_rejections(&pool, repo_id, &rejections).await;
        assert!(record.is_err(), "insert into a missing table must fail");
        // the hook path: refusal reasons survive the failed insert and the
        // push is still refused
        assert!(!rejections.is_empty(), "push stays refused");
    }
}
