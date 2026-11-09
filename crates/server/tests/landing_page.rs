// story: e38s01 — the logged-out home is the self-hoster product page:
// hero claim, what-ships rows, deploy pointer, footer; visitor links
// (/register, /login) survive and no create form appears.
mod landing_page {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn get(pool: sqlx::PgPool, root: &tempfile::TempDir) -> String {
        let request = Request::get("/").body(Body::empty()).unwrap();
        let response = server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn visitor_landing_renders_approved_design(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let html = get(pool, &root).await;

        // hero (e38s01 copy draft)
        assert!(
            html.contains("Run your own forge, built around Jujutsu."),
            "hero: {html}"
        );
        assert!(html.contains("change-ids"), "hero sub: {html}");

        // what ships today — the shipped-facts rows
        assert!(html.contains("What ships today"), "shipped section: {html}");
        assert!(
            html.contains("Change-anchored merge reviews"),
            "shipped: merge reviews: {html}"
        );
        assert!(html.contains("Mail that delivers"), "shipped: mail: {html}");
        assert!(
            html.contains("Moderation built in"),
            "shipped: moderation: {html}"
        );

        // deploy pointer
        assert!(html.contains("One docker compose up"), "deploy: {html}");
        assert!(html.contains("SMTP"), "deploy config: {html}");
        assert!(
            html.contains("docker compose up -d"),
            "deploy command: {html}"
        );

        // footer
        assert!(html.contains("Apache License 2.0"), "footer: {html}");

        // old e23s01 landing is gone
        assert!(
            !html.contains("A forge built around Jujutsu."),
            "old hero gone: {html}"
        );
        assert!(
            !html.contains("Early development"),
            "old status band gone: {html}"
        );

        // visitor paths survive — one quiet way in, nothing to join
        assert!(html.contains("/login"), "login link: {html}");
        assert!(!html.contains("/register"), "no register link: {html}");
        assert!(
            !html.contains("action=\"/repos\""),
            "no create form: {html}"
        );
    }
}
