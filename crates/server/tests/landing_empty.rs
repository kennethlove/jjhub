// story: e13s02 — a repo whose trunk has no commits shows the Files
// section's onboarding block: no entry list, no dead tree/blob links.
mod landing_empty {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// a bare repo that was never pushed: trunk has no commits
    fn seed_empty(root: &std::path::Path) -> std::path::PathBuf {
        let bare = root.join("bare.git");
        let out = std::process::Command::new("git")
            .args(["init", "--bare", bare.to_str().unwrap()])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git init: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        bare
    }

    async fn mount(pool: &sqlx::PgPool, root: &tempfile::TempDir) {
        let bare = seed_empty(root.path());
        let target = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::rename(&bare, &target).unwrap();
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        ).fetch_one(pool).await.unwrap();
        sqlx::query("INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo')")
            .bind(user_id)
            .execute(pool)
            .await
            .unwrap();
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn empty_repo_renders_onboarding(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root).await;

        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(Request::get("/alice/demo").body(Body::empty()).unwrap())
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let html =
            String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
                .into_owned();

        assert!(
            html.contains("<h2>Files</h2>"),
            "Files section kept: {html}"
        );
        assert!(
            html.contains("No files yet") && html.contains("push your first commit"),
            "onboarding block rendered: {html}"
        );
        assert!(
            !html.contains(r#"class="file-list""#),
            "no entry list rendered: {html}"
        );
        assert!(
            !html.contains("/alice/demo/tree") && !html.contains("/alice/demo/blob"),
            "no dead tree or blob links: {html}"
        );
    }
}
