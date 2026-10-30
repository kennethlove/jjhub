// story: e12s02 — the site stylesheet is served by the app itself: one
// embedded file, one GET route, no sidecar and no per-request file read.
mod site_css {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
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
            body.contains("--color-tertiary: #106845"),
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

    #[sqlx::test(migrations = "./migrations")]
    async fn site_css_clone_copy_button_has_visible_focus(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (_, _, body) = get_stylesheet(&pool, &root).await;
        // the copy button is an interactive control; keyboard focus must be
        // visible and its disabled state must not be mistakable for enabled
        assert!(
            body.contains(".copy-btn:focus-visible"),
            "copy button focus ring missing: {body}"
        );
        assert!(
            body.contains(".copy-btn:disabled"),
            "copy button disabled state missing: {body}"
        );
    }

    // story: e30 — the stylesheet URL is stable across deploys, so a
    // browser must never keep one deploy's CSS for the next. The origin
    // sends no-cache + an ETag; clients revalidate and get 304.
    #[sqlx::test(migrations = "./migrations")]
    async fn app_css_revalidates_instead_of_serving_stale(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let request = Request::get("/assets/app.css").body(Body::empty()).unwrap();
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cache_control = response
            .headers()
            .get(header::CACHE_CONTROL)
            .map(|v| v.to_str().unwrap_or_default().to_string())
            .unwrap_or_default();
        assert!(
            cache_control.contains("no-cache"),
            "cache-control must force revalidation: {cache_control}"
        );
        assert!(
            !cache_control.contains("max-age="),
            "no max-age: the CSS must not outlive its deploy: {cache_control}"
        );
        let etag = response
            .headers()
            .get(header::ETAG)
            .map(|v| v.to_str().unwrap_or_default().to_string())
            .unwrap_or_default();
        assert!(!etag.is_empty(), "ETag missing");

        let conditional = Request::get("/assets/app.css")
            .header(header::IF_NONE_MATCH, etag.clone())
            .body(Body::empty())
            .unwrap();
        let not_modified =
            server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
                .oneshot(conditional)
                .await
                .unwrap();
        assert_eq!(
            not_modified.status(),
            StatusCode::NOT_MODIFIED,
            "matching If-None-Match must answer 304"
        );
        assert_eq!(
            not_modified
                .headers()
                .get(header::ETAG)
                .map(|v| v.to_str().unwrap_or_default()),
            Some(etag.as_str()),
            "304 must carry the ETag"
        );
    }
}
