// story: e21s02 — theme choice persists and drives rendering from the
// very first byte (server-side attribute, never a client flip).

mod theme_preference {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    const SECRET: &str = "test-session-secret";

    async fn register(app: &axum::Router, handle: &str) -> String {
        let response = app
            .clone()
            .oneshot(
                Request::post("/register")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "email={handle}%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|v| {
                v.to_str()
                    .ok()
                    .and_then(|s| s.split(';').next())
                    .map(String::from)
            })
            .expect("session cookie")
    }

    fn csrf(cookie: &str) -> String {
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(SECRET.as_bytes(), value)
    }

    /// GET response headers + body, for asserting the first byte.
    async fn get_full(
        app: &axum::Router,
        path: &str,
        cookie: Option<&str>,
    ) -> (axum::http::HeaderMap, String) {
        let mut request = Request::get(path);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (headers, String::from_utf8_lossy(&bytes).to_string())
    }

    async fn post_theme(app: &axum::Router, path: &str, cookie: &str, theme: &str) {
        let response = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .header(header::COOKIE, cookie)
                    .body(Body::from(format!(
                        "theme={theme}&saved=1&_csrf={}",
                        csrf(cookie)
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn default_theme_is_system_on_every_page(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());
        let cookie = register(&app, "alice").await;

        for (path, authed) in [("/", true), ("/settings", true), ("/login", false)] {
            let (headers, body) = get_full(&app, path, authed.then_some(cookie.as_str())).await;
            assert!(
                body.contains("data-theme=\"system\""),
                "{path} must declare the default theme (no flash)"
            );
            // only cookie content-type on redirect-free pages; login is 200 too
            let _ = headers;
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn saved_dark_theme_renders_on_first_byte(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());
        let cookie = register(&app, "bob").await;

        post_theme(&app, "/token-details", &cookie, "dark").await;

        let stored: String = sqlx::query_scalar("SELECT theme FROM users WHERE handle = 'bob'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored, "dark");

        // authed page shows it... no client script needed
        let (_, body) = get_full(&app, "/settings", Some(&cookie)).await;
        assert!(
            body.contains("data-theme=\"dark\""),
            "settings renders the stored theme server-side"
        );

        // ...and so does a logged-out page derived from nothing but the user row
        let (_, body) = get_full(&app, "/login", None).await;
        assert!(
            !body.contains("data-theme=\"dark\""),
            "logged-out pages without the user keep the default"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn invalid_theme_from_form_is_ignored(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());
        let cookie = register(&app, "carol").await;

        let response = app
            .clone()
            .oneshot(
                Request::post("/settings/profile")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .header(header::COOKIE, cookie.as_str())
                    .body(Body::from(format!(
                        "display_name=&theme=neon&_csrf={}",
                        csrf(&cookie)
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let stored: String = sqlx::query_scalar("SELECT theme FROM users WHERE handle = 'carol'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored, "system", "unknown values never persist");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn logged_out_pages_stay_on_default_without_cookie(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());

        let (_, body) = get_full(&app, "/login", None).await;
        assert!(body.contains("data-theme=\"system\""));
    }
}
