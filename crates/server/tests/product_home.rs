// story: e38s01 — the logged-out home page is the self-hoster product
// page: approved-copy markers (hero, what-ships headings, deploy
// pointer), old landing wording gone; the session-carrying dashboard
// branch is untouched.
mod product_home {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn get(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        path: &str,
        cookie: Option<&str>,
    ) -> String {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse().unwrap());
        }
        let response = server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    async fn register(pool: sqlx::PgPool, root: &tempfile::TempDir) -> String {
        std::env::set_var("SESSION_SECRET", "product-home-suite-secret");
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "email=bob%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
            ))
            .unwrap();
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
                .await
                .unwrap();
        response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn logged_out_home_is_the_self_hoster_product_page(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let html = get(pool, &root, "/", None).await;

        // hero (copy draft, reviewed 2026-11-07)
        assert!(
            html.contains("Run your own forge, built around Jujutsu."),
            "hero headline: {html}"
        );
        assert!(
            !html.contains("A forge built around Jujutsu."),
            "old hero must be gone: {html}"
        );

        // what ships today — headings from the shipped-facts list
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
        assert!(
            html.contains("Rate limits and SSH transport"),
            "shipped: limits: {html}"
        );
        assert!(html.contains("Git smart HTTP"), "shipped: git http: {html}");

        // deploy pointer
        assert!(
            html.contains("One docker compose up"),
            "deploy pointer: {html}"
        );
        assert!(html.contains("SMTP"), "deploy config: {html}");

        // story: e38s02 — auth surface: exactly one quiet way in,
        // nothing that asks the visitor to join.
        assert!(html.contains("href=\"/login\""), "login affordance: {html}");
        assert_eq!(
            html.matches("href=\"/login\"").count(),
            1,
            "exactly one /login link: {html}"
        );
        assert!(
            !html.contains("href=\"/register\""),
            "no register href: {html}"
        );
        assert!(!html.contains("Get started"), "no signup CTA: {html}");
        assert!(!html.contains("Sign up"), "no signup wording: {html}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn session_carrying_home_still_returns_the_dashboard(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(pool.clone(), &root).await;
        let html = get(pool, &root, "/", Some(&cookie)).await;
        assert!(
            html.contains("class=\"wrap dash-home\""),
            "dashboard branch: {html}"
        );
        assert!(html.contains("action=\"/repos\""), "create form: {html}");
        assert!(
            !html.contains("Run your own forge"),
            "product page must not replace the dashboard: {html}"
        );
    }
}
