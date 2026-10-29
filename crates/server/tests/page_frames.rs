// e29 — every page renders the shared frame: site nav first, content in
// wrap containers, footer last. Covers the browse pages users click into
// from the repo landing plus search/operations/MR list. Detail routes
// (mr_detail, operation_detail, token_details) share the exact template
// pattern and ride on the gates; the list pages pin the frame contract.
mod page_frames {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn seed(root: &std::path::Path) -> std::path::PathBuf {
        let bare = root.join("bare.git");
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .current_dir(&seed)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "--bare", bare.to_str().unwrap()]);
        git(&["init", "-b", "trunk"]);
        std::fs::write(seed.join("README.md"), "# hello\n").unwrap();
        git(&["add", "."]);
        git(&[
            "-c",
            "user.email=a@e.com",
            "-c",
            "user.name=A",
            "commit",
            "-m",
            "one",
        ]);
        git(&["push", bare.to_str().unwrap(), "trunk"]);
        bare
    }

    async fn mount(pool: &sqlx::PgPool, root: &tempfile::TempDir) {
        let bare = seed(root.path());
        let target = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::rename(&bare, &target).unwrap();
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        ).fetch_one(pool).await.unwrap();
        let repo_id: i64 = sqlx::query_scalar(
            "INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo') RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        // one open merge request so the MR list page renders its rows
        sqlx::query(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, 'kqvwpzrsmpop')",
        )
        .bind(repo_id)
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn get(pool: &sqlx::PgPool, root: &std::path::Path, path: &str) -> String {
        let response = server::app_with_storage(pool.clone(), root.to_str().unwrap().to_string())
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "GET {path}");
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    fn assert_frame(body: &str, page: &str) {
        let header = body
            .find("class=\"site-header\"")
            .unwrap_or_else(|| panic!("{page}: site nav missing: {body}"));
        let first_content = body
            .find("<main")
            .unwrap_or_else(|| panic!("{page}: no <main> content region: {body}"));
        assert!(header < first_content, "{page}: nav renders before content");
        let wraps = body.matches("class=\"wrap").count();
        assert!(wraps >= 3, "{page}: needs nav/main/footer wraps, got {wraps}");
        assert!(
            body.contains("class=\"site-footer\""),
            "{page}: footer missing: {body}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn tree_page_carries_the_frame(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root).await;
        let body = get(&pool, root.path(), "/alice/demo/tree").await;
        assert_frame(&body, "tree");
        // navigation stays inside the browse view
        assert!(body.contains("/alice/demo/tree/trunk/docs"), "dir link");
        assert!(body.contains("/alice/demo/blob/trunk/README.md"), "file link");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn blob_page_carries_the_frame(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root).await;
        let body = get(&pool, root.path(), "/alice/demo/blob/trunk/README.md").await;
        assert_frame(&body, "blob");
        assert!(body.contains("hello"), "file content shown");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn search_page_carries_the_frame(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root).await;
        let body = get(&pool, root.path(), "/alice/demo/search").await;
        assert_frame(&body, "search");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn operations_page_carries_the_frame(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root).await;
        let body = get(&pool, root.path(), "/alice/demo/operations").await;
        assert_frame(&body, "operations");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mr_list_carries_the_frame(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root).await;
        let body = get(&pool, root.path(), "/alice/demo/merge-requests").await;
        assert_frame(&body, "merge request list");
    }
}
