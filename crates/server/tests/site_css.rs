// story: e12s02 — the site stylesheet is served by the app itself: one
// embedded file, one GET route, no sidecar and no per-request file read.
mod site_css {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn get_stylesheet(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
    ) -> (StatusCode, String, String) {
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(Request::get("/assets/app.css").body(Body::empty()).unwrap())
                .await
                .unwrap();
        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body =
            String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
                .into_owned();
        (status, content_type, body)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn site_css_route_serves_stylesheet(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (status, content_type, body) = get_stylesheet(&pool, &root).await;
        assert_eq!(status, StatusCode::OK, "GET /assets/app.css: {status}");
        assert!(
            content_type.starts_with("text/css"),
            "content-type: {content_type}"
        );
        assert!(!body.is_empty(), "stylesheet body must not be empty");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn site_css_stylesheet_declares_token_base(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (_, _, body) = get_stylesheet(&pool, &root).await;
        assert!(body.contains(":root"), "token block missing: {body}");
        assert!(
            body.contains("--color-tertiary: #157F52"),
            "tertiary token from DESIGN.md missing"
        );
        assert!(
            body.contains("--space-sm: 16px"),
            "spacing token from DESIGN.md missing"
        );
        assert!(
            body.contains("IBM Plex Sans"),
            "base type must use the DESIGN.md typeface: {body}"
        );
    }
}
