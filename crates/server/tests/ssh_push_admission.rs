// story: e17s02
//! SSH receive-pack runs the SAME admission chain as HTTP:
//! transport::authorize(GitOp::ReceivePack) before any spawn — no
//! SSH-specific bypass, no second implementation. Protected-refs and
//! rejection recording ride the bare repo's `server pre-receive` hook
//! (exercised end-to-end by scripts/e2e-ssh-push.sh).

use server::ssh::SshHandler;

mod ssh_push_admission {
    use super::*;
    use russh::server::Handler;

    fn bare_repo(root: &tempfile::TempDir, owner: &str, name: &str) {
        let bare = domain::storage_path(root.path().to_str().unwrap(), owner, name);
        std::fs::create_dir_all(&bare).unwrap();
        let status = std::process::Command::new("git")
            .args(["init", "--bare"])
            .arg(&bare)
            .status()
            .unwrap();
        assert!(status.success(), "git init --bare must succeed");
    }

    async fn insert_other_user(pool: &sqlx::PgPool) -> uuid::Uuid {
        sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) \
             VALUES ('other@example.com', 'x', 'other') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn insert_repo(pool: &sqlx::PgPool, owner: uuid::Uuid, name: &str, visibility: &str) {
        sqlx::query("INSERT INTO repositories (owner_id, name, visibility) VALUES ($1, $2, $3)")
            .bind(owner)
            .bind(name)
            .bind(visibility)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn tokentest_user(pool: &sqlx::PgPool) -> uuid::Uuid {
        sqlx::query_scalar("SELECT id FROM users WHERE handle = 'tokentest'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn owner_receive_pack_admitted(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (token, _tok) = server::token::insert_test_token(&pool).await;
        let owner = tokentest_user(&pool).await;
        insert_repo(&pool, owner, "demo", "private").await;
        bare_repo(&root, "tokentest", "demo");

        let mut handler =
            SshHandler::with_storage(pool.clone(), root.path().to_str().unwrap().to_string());
        let auth = handler.auth_password("tokentest", &token).await.unwrap();
        assert!(matches!(auth, russh::server::Auth::Accept));

        let (child, _stdin) = handler
            .run_git_exec("git-receive-pack '/tokentest/demo.git'")
            .await
            .expect("owner push must be admitted through the shared chain");
        drop(child); // kill_on_drop
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn anonymous_receive_pack_unauthorized(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (_token, _tok) = server::token::insert_test_token(&pool).await;
        let owner = tokentest_user(&pool).await;
        insert_repo(&pool, owner, "demo", "public").await;
        bare_repo(&root, "tokentest", "demo");

        let mut handler = SshHandler::with_storage(pool, root.path().to_str().unwrap().to_string());
        let err = handler
            .run_git_exec("git-receive-pack '/tokentest/demo.git'")
            .await
            .expect_err("anonymous push must be refused before spawn");
        assert_eq!(err, "unauthorized");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn stranger_receive_pack_forbidden(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (_token, _tok) = server::token::insert_test_token(&pool).await;
        let owner = tokentest_user(&pool).await;
        let other = insert_other_user(&pool).await;
        insert_repo(&pool, owner, "demo", "public").await;
        bare_repo(&root, "tokentest", "demo");

        let other_token = server::token::create(&pool, other, "stranger")
            .await
            .plaintext;
        let mut handler = SshHandler::with_storage(pool, root.path().to_str().unwrap().to_string());
        let auth = handler.auth_password("other", &other_token).await.unwrap();
        assert!(matches!(auth, russh::server::Auth::Accept));

        let err = handler
            .run_git_exec("git-receive-pack '/tokentest/demo.git'")
            .await
            .expect_err("non-collaborator push must be refused before spawn");
        assert_eq!(err, "forbidden");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn anonymous_upload_pack_public_still_clonable(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (_token, _tok) = server::token::insert_test_token(&pool).await;
        let owner = tokentest_user(&pool).await;
        insert_repo(&pool, owner, "demo", "public").await;
        bare_repo(&root, "tokentest", "demo");

        let mut handler = SshHandler::with_storage(pool, root.path().to_str().unwrap().to_string());
        let (child, _stdin) = handler
            .run_git_exec("git-upload-pack '/tokentest/demo.git'")
            .await
            .expect("anonymous clone of a public repo must stay admitted");
        drop(child);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn receive_pack_unknown_repo_refused(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (_token, _tok) = server::token::insert_test_token(&pool).await;
        let owner = tokentest_user(&pool).await;
        insert_repo(&pool, owner, "demo", "public").await;
        bare_repo(&root, "tokentest", "demo");

        let mut handler = SshHandler::with_storage(pool, root.path().to_str().unwrap().to_string());
        let err = handler
            .run_git_exec("git-receive-pack '/tokentest/missing.git'")
            .await
            .expect_err("unknown repo must be refused");
        assert_eq!(err, "repository not found");
    }

    /// run one git plumbing command, return raw stdout (pack bytes etc).
    fn git_raw_bytes(dir: &std::path::Path, args: &[&str], input: &str) -> Vec<u8> {
        use std::io::Write as _;
        let mut child = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    }

    /// run one git plumbing command, return trimmed stdout.
    fn git_raw(dir: &std::path::Path, args: &[&str], input: &str) -> String {
        String::from_utf8(git_raw_bytes(dir, args, input))
            .unwrap()
            .trim()
            .to_string()
    }

    /// story: e17s02 — AC t1: a push driven through the SSH exec path
    /// runs the bare repo's `server pre-receive` hook (the same chain as
    /// HTTP): a non-fast-forward trunk rewrite is refused, and BOTH the
    /// protected-ref and open-MR identity rules land in push_rejections.
    #[sqlx::test(migrations = "./migrations")]
    async fn protected_and_identity_violations_refused_and_recorded(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (token, _tok) = server::token::insert_test_token(&pool).await;
        let owner = tokentest_user(&pool).await;
        insert_repo(&pool, owner, "demo", "private").await;
        bare_repo(&root, "tokentest", "demo");
        let bare = domain::storage_path(root.path().to_str().unwrap(), "tokentest", "demo");
        let bare = std::path::Path::new(&bare);

        let owner_email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
            .bind(owner)
            .fetch_one(&pool)
            .await
            .unwrap();
        let change = "c1a0b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3";

        // objects the hook inspects: old head (owner identity) and the
        // foreign-committer rewrite carrying the same change-id
        let tree = git_raw(bare, &["mktree"], "");
        let old_commit = git_raw(
            bare,
            &["hash-object", "-t", "commit", "-w", "--stdin"],
            &format!(
                "tree {tree}\nauthor e2e <{owner_email}> 1700000000 +0000\n\
                 committer e2e <{owner_email}> 1700000000 +0000\n\
                 change-id {change}\n\nbase change\n"
            ),
        );
        let new_commit = git_raw(
            bare,
            &["hash-object", "-t", "commit", "-w", "--stdin"],
            &format!(
                "tree {tree}\nauthor e2e <{owner_email}> 1700000000 +0000\n\
                 committer mallory <mallory@evil.example> 1700000000 +0000\n\
                 change-id {change}\n\nforeign rewrite\n"
            ),
        );
        git_raw(bare, &["update-ref", "refs/heads/trunk", &old_commit], "");
        let repo_id: uuid::Uuid =
            sqlx::query_scalar("SELECT id FROM repositories WHERE owner_id = $1 AND name = 'demo'")
                .bind(owner)
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, $3)",
        )
        .bind(repo_id)
        .bind(owner)
        .bind(change)
        .execute(&pool)
        .await
        .unwrap();

        // the bare repo's hook IS the production pre-receive entry point;
        // it needs the per-test database (sqlx::test only swaps the pool)
        // and the storage root this handler was built with
        let db_name = pool.connect_options().get_database().unwrap().to_string();
        let base_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let db_url = format!(
            "{}/{}",
            &base_url[..base_url.rfind('/').expect("url has a path")],
            db_name
        );
        let hooks = bare.join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-receive");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\nexport JJHUB_STORAGE_ROOT='{}'\nexport DATABASE_URL='{}'\nexec '{}' pre-receive\n",
                root.path().display(),
                db_url,
                env!("CARGO_BIN_EXE_server")
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let mut handler =
            SshHandler::with_storage(pool.clone(), root.path().to_str().unwrap().to_string());
        let auth = handler.auth_password("tokentest", &token).await.unwrap();
        assert!(matches!(auth, russh::server::Auth::Accept));

        let (mut child, mut stdin) = handler
            .run_git_exec("git-receive-pack '/tokentest/demo.git'")
            .await
            .expect("owner push reaches the shared receive path");

        // pkt-line command list + flush, then the packfile: receive-pack
        // unpacks first (quarantine), runs pre-receive, then decides
        let body = format!("{old_commit} {new_commit} refs/heads/trunk\0report-status\n");
        let pkt = format!("{:04x}{}", body.len() + 4, body);
        let pack = git_raw_bytes(
            bare,
            &["pack-objects", "--stdout", "--revs"],
            &format!("{new_commit}\n"),
        );
        use tokio::io::AsyncWriteExt as _;
        stdin.write_all(pkt.as_bytes()).await.unwrap();
        stdin.write_all(b"0000").await.unwrap();
        stdin.write_all(&pack).await.unwrap();
        stdin.flush().await.unwrap();
        drop(stdin);

        // liveness: receive-pack finishes after the hook decision (its
        // exit code carries nothing — the refusal reaches the client as
        // report-status NAK and lands in push_rejections)
        tokio::time::timeout(std::time::Duration::from_secs(20), child.wait())
            .await
            .expect("receive-pack must exit after the hook decision")
            .unwrap();

        let mut hook_err = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            use tokio::io::AsyncReadExt as _;
            let _ = pipe.read_to_string(&mut hook_err).await;
        }
        let mut hook_out = String::new();
        if let Some(mut pipe) = child.stdout.take() {
            use tokio::io::AsyncReadExt as _;
            let _ = pipe.read_to_string(&mut hook_out).await;
        }

        let mut reasons: Vec<String> = sqlx::query_scalar(
            "SELECT reason FROM push_rejections WHERE repo_id = $1 ORDER BY created_at",
        )
        .bind(repo_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        reasons.sort();
        let joined = reasons.join("\n");
        assert!(
            joined.contains("non-fast-forward"),
            "protected-ref rule must refuse the trunk rewrite: {joined}\n--- hook stderr ---\n{hook_err}\n--- stdout ---\n{hook_out}"
        );
        assert!(
            joined.contains("owner's identity"),
            "identity rule must refuse the foreign rewrite: {joined}"
        );

        // refusal held: trunk still points at the old head
        let now = git_raw(bare, &["rev-parse", "refs/heads/trunk"], "");
        assert_eq!(now, old_commit, "refused push must not move trunk");
    }
}
