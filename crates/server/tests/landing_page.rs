// story: e23s01 — the approved landing design renders for logged-out
// visitors: hero claim, MR ledger, why rows, stack band, self-hosting,
// status/roadmap, footer; visitor links (/register, /login) survive.
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

        // hero
        assert!(
            html.contains("A forge built around Jujutsu."),
            "hero: {html}"
        );
        assert!(html.contains("change-id"), "hero sub: {html}");

        // ledger signature — mono change-ids in ruled rows
        assert!(html.contains("kqvryyuz"), "ledger change-id: {html}");
        assert!(
            html.contains("rewrite-diff-viewer"),
            "ledger branch: {html}"
        );

        // why rows
        assert!(
            html.contains("Change-anchored merge requests"),
            "why rows: {html}"
        );
        assert!(html.contains("Edit-in-place review"), "why rows: {html}");

        // inverse stack band
        assert!(html.contains("Merge the bottom"), "stack band: {html}");
        assert!(html.contains("server-rebase"), "stack band: {html}");

        // self-hosting
        assert!(
            html.contains("docker compose up -d"),
            "self-hosting: {html}"
        );
        assert!(
            html.contains("SESSION_SECRET"),
            "self-hosting secrets: {html}"
        );

        // status + roadmap + footer
        assert!(html.contains("Early development"), "status badge: {html}");
        assert!(html.contains("Revset search"), "roadmap: {html}");
        assert!(html.contains("Apache License 2.0"), "footer: {html}");

        // visitor paths survive
        assert!(html.contains("/register"), "register link: {html}");
        assert!(html.contains("/login"), "login link: {html}");
        assert!(
            !html.contains("action=\"/repos\""),
            "no create form: {html}"
        );
    }
}
