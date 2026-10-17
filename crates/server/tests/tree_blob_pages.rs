// story: e07s01
mod tree_blob_pages {
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
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "--bare", bare.to_str().unwrap()]);
        git(&["init", "-b", "trunk"]);
        std::fs::create_dir_all(seed.join("docs")).unwrap();
        std::fs::write(seed.join("README.md"), "# hello\n<script>alert(1)</script>\n").unwrap();
        std::fs::write(seed.join("docs/guide.md"), "guide\n").unwrap();
        git(&["add", "."]);
        git(&["-c", "user.email=a@e.com", "-c", "user.name=A", "commit", "-m", "one"]);
        git(&["push", bare.to_str().unwrap(), "trunk"]);
        bare
    }

    async fn get(root: &std::path::Path, path: &str) -> (StatusCode, String) {
        let response = server::app_with_storage(
            sqlx::postgres::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
                .await
                .unwrap(),
            root.to_str().unwrap().to_string(),
        )
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
        let status = response.status();
        let body = String::from_utf8_lossy(
            &response.into_body().collect().await.unwrap().to_bytes(),
        )
        .into_owned();
        (status, body)
    }

    #[tokio::test]
    async fn tree_page_lists_and_links() {
        let root = tempfile::tempdir().unwrap();
        let bare = seed(root.path());
        // mount the seeded bare where storage_path expects it
        let target = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::rename(&bare, &target).unwrap();
        // repo row so the page handler resolves
        let pool = sqlx::postgres::PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        ).fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo')")
            .bind(user_id).execute(&pool).await.unwrap();

        let (status, html) = get(root.path(), "/alice/demo/tree").await;
        assert_eq!(status, StatusCode::OK);
        assert!(html.contains("/alice/demo/tree/trunk/docs"), "dir linked: {html}");
        assert!(html.contains("/alice/demo/blob/trunk/README.md"), "file linked: {html}");
        assert!(html.contains("README.md"), "names listed: {html}");
    }

    #[tokio::test]
    async fn blob_page_escapes_and_numbers() {
        let root = tempfile::tempdir().unwrap();
        let bare = seed(root.path());
        let target = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::rename(&bare, &target).unwrap();
        let pool = sqlx::postgres::PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        ).fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo')")
            .bind(user_id).execute(&pool).await.unwrap();

        let (status, html) = get(root.path(), "/alice/demo/blob/trunk/README.md").await;
        assert_eq!(status, StatusCode::OK);
        assert!(!html.contains("<script>"), "content escaped: {html}");
        assert!(html.contains("&lt;script&gt;"), "escaped payload visible as text: {html}");
        assert!(html.contains("hello"), "line content: {html}");
        assert!(html.contains("1"), "line numbers: {html}");
    }

    #[tokio::test]
    async fn unknown_ref_is_404() {
        let root = tempfile::tempdir().unwrap();
        let bare = seed(root.path());
        let target = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::rename(&bare, &target).unwrap();
        let pool = sqlx::postgres::PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        ).fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo')")
            .bind(user_id).execute(&pool).await.unwrap();

        let (status, _) = get(root.path(), "/alice/demo/tree/nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = get(root.path(), "/alice/demo/tree/trunk/..%2F..%2Fetc").await;
        assert!(status == StatusCode::NOT_FOUND || status == StatusCode::BAD_REQUEST, "traversal refused: {status}");
    }
}
