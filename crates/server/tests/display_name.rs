// story: e21s02 — display name persists; NULL falls back to the
// handle in every shown location, never an empty or null string.

mod display_name {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    const SECRET: &str = "test-session-secret";

    async fn register(pool: &sqlx::PgPool, app: &axum::Router, handle: &str) -> String {
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
        let _ = pool;
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

    async fn get(app: &axum::Router, path: &str, cookie: &str) -> String {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8_lossy(&bytes).to_string()
    }

    async fn post_form(app: &axum::Router, path: &str, cookie: &str, body: String) -> StatusCode {
        let response = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .header(header::COOKIE, cookie)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        response.status()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn null_display_name_renders_handle(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());
        let cookie = register(&pool, &app, "alice").await;

        // fresh account: display_name is NULL — settings shows the handle
        let html = get(&app, "/settings", &cookie).await;
        assert!(
            html.contains("value=\"alice\""),
            "settings pre-fills the handle as the display name fallback"
        );
        assert!(
            !html.contains("null"),
            "no 'null' string leaks into the profile"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn display_name_saves_from_details_screen(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());
        let cookie = register(&pool, &app, "bob").await;

        let status = post_form(
            &app,
            "/token-details",
            &cookie,
            format!(
                "display_name=Alice+Carroll&theme=system&saved=1&_csrf={}",
                csrf(&cookie)
            ),
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER);

        let saved: Option<String> =
            sqlx::query_scalar("SELECT display_name FROM users WHERE handle = 'bob'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(saved.as_deref(), Some("Alice Carroll"));

        let html = get(&app, "/settings", &cookie).await;
        assert!(
            html.contains("value=\"Alice Carroll\""),
            "settings shows the saved name"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn clearing_name_in_settings_falls_back_to_handle(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());
        let cookie = register(&pool, &app, "carol").await;

        let status = post_form(
            &app,
            "/settings/profile",
            &cookie,
            format!("display_name=&theme=system&_csrf={}", csrf(&cookie)),
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER);

        let saved: Option<String> =
            sqlx::query_scalar("SELECT display_name FROM users WHERE handle = 'carol'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(saved.is_none(), "clearing stores NULL, not an empty string");

        let html = get(&app, "/settings", &cookie).await;
        assert!(
            html.contains("value=\"carol\""),
            "handle fallback shows again"
        );
    }
}
