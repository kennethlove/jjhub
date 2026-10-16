// story: e03s03
mod merge_endpoint {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

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

    /// trunk at T, stack C1(aaa) -> C2(bbb), MRs open, repo owned by alice.
    async fn setup(pool: &sqlx::PgPool, root: &std::path::Path) -> String {
        let bare = domain::storage_path(root.to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git_in(&seed, &["init", "-b", "trunk"]);
        std::fs::write(seed.join("base.txt"), "base\n").unwrap();
        git_in(&seed, &["add", "."]);
        let trunk = mkcommit(
            &seed,
            "0000000000000000000000000000000000000000",
            "0000trunk0000",
            "base",
        );
        std::fs::write(seed.join("one.txt"), "one\n").unwrap();
        git_in(&seed, &["add", "."]);
        let c1 = mkcommit(&seed, &trunk, "kkkkktaaaaa", "change one");
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
        git_in(&bare, &["update-ref", "refs/heads/trunk", &trunk]);

        let user_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
            .fetch_optional(pool)
            .await
            .unwrap()
            .expect("call session() before setup()");
        let repo_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo') RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        for (change, commit) in [("aaaa", c1), ("bbbb", c2)] {
            let mr_id: uuid::Uuid = sqlx::query_scalar(
                "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, $3) RETURNING id",
            )
            .bind(repo_id)
            .bind(user_id)
            .bind(change)
            .fetch_one(pool)
            .await
            .unwrap();
            sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2)")
                .bind(mr_id)
                .bind(&commit)
                .execute(pool)
                .await
                .unwrap();
        }
        "alice".to_string()
    }

    async fn session(pool: &sqlx::PgPool, root: &std::path::Path) -> String {
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "email=alice%40example.com&password=correct+horse+battery",
            ))
            .unwrap();
        let response = server::app_with_storage(pool.clone(), root.to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap();
        response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
    }

    async fn merge_post(
        pool: sqlx::PgPool,
        root: &std::path::Path,
        cookie: Option<&str>,
        path: &str,
    ) -> axum::response::Response {
        let mut request = Request::post(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(""))
            .unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse().unwrap());
        }
        server::app_with_storage(pool, root.to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn owner_merges_bottom_and_redirects(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = session(&pool, root.path()).await;
        setup(&pool, root.path()).await;

        let response = merge_post(
            pool.clone(),
            root.path(),
            Some(&cookie),
            "/alice/demo/merge-requests/aaaa/merge",
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let state: String =
            sqlx::query_scalar("SELECT state FROM merge_requests WHERE change_id = 'aaaa'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, "merged");
        // descendant bbb rebased onto new trunk, still open
        let bbb: String =
            sqlx::query_scalar("SELECT state FROM merge_requests WHERE change_id = 'bbbb'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(bbb, "open");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn merging_a_descendant_first_is_conflict(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = session(&pool, root.path()).await;
        setup(&pool, root.path()).await;
        let response = merge_post(
            pool,
            root.path(),
            Some(&cookie),
            "/alice/demo/merge-requests/bbbb/merge",
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unauthenticated_is_login_redirect(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = session(&pool, root.path()).await;
        let _ = cookie;
        setup(&pool, root.path()).await;
        let response = merge_post(
            pool,
            root.path(),
            None,
            "/alice/demo/merge-requests/aaaa/merge",
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn non_owner_is_forbidden(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice_cookie = session(&pool, root.path()).await;
        let _ = alice_cookie;
        setup(&pool, root.path()).await;
        // register a different account (bob) to attempt the merge
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "email=bob%40example.com&password=correct+horse+battery",
            ))
            .unwrap();
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
                .await
                .unwrap();
        let cookie = response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();

        let response = merge_post(
            pool,
            root.path(),
            Some(&cookie),
            "/alice/demo/merge-requests/aaaa/merge",
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
