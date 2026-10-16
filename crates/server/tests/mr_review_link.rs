// story: e05s04
mod mr_review_links {
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

    /// alice/demo with one open MR whose revision is a real commit.
    async fn setup(pool: &sqlx::PgPool, root: &std::path::Path, with_meta: bool) {
        let bare = domain::storage_path(root.to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        if with_meta {
            let meta = domain::meta_path(root.to_str().unwrap(), "alice", "demo");
            tokio::fs::create_dir_all(meta.join(".jj")).await.unwrap();
        }
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git_in(&seed, &["init", "-b", "trunk"]);
        std::fs::write(seed.join("f.txt"), "x\n").unwrap();
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
        let commit = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&seed)
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();
        git_in(&seed, &["push", bare.to_str().unwrap(), "trunk"]);

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
        sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2)")
            .bind(mr_id)
            .bind(&commit)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn get(pool: sqlx::PgPool, root: &std::path::Path, path: &str) -> String {
        let response = server::app_with_storage(pool, root.to_str().unwrap().to_string())
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn detail_page_links_to_kata_when_meta_exists(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        setup(&pool, root.path(), true).await;
        let html = get(pool, root.path(), "/alice/demo/merge-requests/aaaa").await;
        let expected = "/review/alice/demo/r/alice__demo/browse?change=aaaa";
        assert!(
            html.contains(&format!("href=\"{expected}\"")),
            "review link rendered: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn detail_page_has_no_link_without_meta(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        setup(&pool, root.path(), false).await;
        let html = get(pool, root.path(), "/alice/demo/merge-requests/aaaa").await;
        assert!(
            !html.contains("/review/alice/demo/"),
            "no dead link for pre-backfill repos: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn stack_page_links_each_open_mr(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        setup(&pool, root.path(), true).await;
        let html = get(pool, root.path(), "/alice/demo/merge-requests").await;
        assert!(
            html.contains("href=\"/review/alice/demo/r/alice__demo/browse?change=aaaa\""),
            "stack page carries the review link: {html}"
        );
    }
}
