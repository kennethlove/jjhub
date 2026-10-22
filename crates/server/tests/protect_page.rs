// story: e10s02
//! The repo settings surface lists the protected ref set (the e10s01
//! resolver's output) read-only, beside visibility and trunk.

mod protect_page {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn seed(pool: &sqlx::PgPool, trunk: Option<&str>) {
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        match trunk {
            Some(trunk) => {
                sqlx::query(
                    "INSERT INTO repositories (owner_id, name, trunk) VALUES ($1, 'demo', $2)",
                )
                .bind(user_id)
                .bind(trunk)
                .execute(pool)
                .await
                .unwrap();
            }
            None => {
                sqlx::query("INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo')")
                    .bind(user_id)
                    .execute(pool)
                    .await
                    .unwrap();
            }
        }
    }

    async fn get(pool: sqlx::PgPool, root: &tempfile::TempDir, path: &str) -> String {
        let request = Request::get(path).body(Body::empty()).unwrap();
        let response = server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // Tera escapes `/` as &#x2F;; the browser renders it as-is
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .replace("&#x2F;", "/")
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn settings_surface_lists_default_protected_set(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        seed(&pool, None).await;

        let html = get(pool, &root, "/alice/demo").await;
        assert!(
            html.contains("refs/heads/trunk"),
            "default trunk protected: {html}"
        );
        assert!(
            html.contains("refs/tags/*"),
            "tag pattern protected: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn settings_surface_lists_custom_trunk(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        seed(&pool, Some("main")).await;

        let html = get(pool, &root, "/alice/demo").await;
        assert!(
            html.contains("refs/heads/main"),
            "custom trunk protected: {html}"
        );
        assert!(
            !html.contains("refs/heads/trunk"),
            "default trunk absent: {html}"
        );
    }
}
