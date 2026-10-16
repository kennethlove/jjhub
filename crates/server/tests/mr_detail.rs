// story: e03s02
mod mr_detail {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn git_in(cwd: &std::path::Path, args: &[&str]) {
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
    }

    /// Seed trunk + two commits whose file content includes an HTML payload
    /// (XSS guard) and return (trunk_commit, head_commit).
    async fn seed(bare: &std::path::Path) -> (String, String) {
        let seed = bare.join("../seed");
        std::fs::create_dir_all(&seed).unwrap();
        git_in(&seed, &["init", "-b", "trunk"]);
        std::fs::write(seed.join("hello.txt"), "first line\n").unwrap();
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
                "one",
            ],
        );
        let trunk_commit = rev_parse(&seed);
        // second commit: add a file whose NAME and content carry html
        std::fs::write(
            seed.join("evil.txt"),
            "<script>alert(1)</script>\nkept line\n",
        )
        .unwrap();
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
                "two",
            ],
        );
        let head_commit = rev_parse(&seed);
        git_in(&seed, &["push", bare.to_str().unwrap(), "trunk"]);
        (trunk_commit, head_commit)
    }

    fn rev_parse(dir: &std::path::Path) -> String {
        String::from_utf8_lossy(
            &std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(dir)
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string()
    }

    async fn seed_repo_with_revisions(
        pool: &sqlx::PgPool,
        commits: &[String],
    ) -> (String, String, String) {
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
        let mr_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, 'aaaa') RETURNING id",
        )
        .bind(repo_id)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        for commit in commits {
            sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2)")
                .bind(mr_id)
                .bind(commit)
                .execute(pool)
                .await
                .unwrap();
        }
        ("alice".to_string(), "demo".to_string(), "aaaa".to_string())
    }

    async fn get(
        pool: sqlx::PgPool,
        root: &std::path::Path,
        path: &str,
    ) -> axum::response::Response {
        server::app_with_storage(pool, root.to_str().unwrap().to_string())
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn detail_lists_revisions_newest_first_with_diffs(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let (trunk_commit, head_commit) = seed(&bare).await;
        let (owner, name, change) =
            seed_repo_with_revisions(&pool, &[trunk_commit.clone(), head_commit.clone()]).await;

        let response = get(
            pool,
            root.path(),
            &format!("/{owner}/{name}/pulls/{change}"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let html =
            String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
                .into_owned();

        // newest revision first
        let head_pos = html.find(&head_commit).expect("head revision rendered");
        let trunk_pos = html.find(&trunk_commit).expect("trunk revision rendered");
        assert!(head_pos < trunk_pos, "newest revision first: {html}");

        // diff of the head revision is rendered
        assert!(html.contains("kept line"), "added line in diff: {html}");
        assert!(
            html.contains("first line"),
            "context/other file content visible"
        );

        // XSS guard: repo content never renders as live html
        assert!(
            !html.contains("<script>"),
            "file content must be escaped, got: {html}"
        );
        assert!(
            html.contains("&lt;script&gt;"),
            "escaped payload shown as text"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn detail_unknown_change_is_404(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let response = get(pool, root.path(), "/alice/demo/pulls/zzzz").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
