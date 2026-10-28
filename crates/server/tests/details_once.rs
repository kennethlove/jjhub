// story: e21s01 — the details screen renders the plaintext exactly
// once; completion blocks re-render forever; an abandoned screen
// does not re-trigger on later visits.

mod details_once {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    const SECRET: &str = "test-session-secret";

    fn register_form(handle: &str) -> String {
        format!(
            "email={handle}%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery"
        )
    }

    /// Register a fresh account; returns (session cookie, plaintext).
    async fn register(pool: &sqlx::PgPool, app: &axum::Router, handle: &str) -> (String, String) {
        let response = app
            .clone()
            .oneshot(
                Request::post("/register")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(register_form(handle)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|v| {
                v.to_str()
                    .ok()
                    .and_then(|s| s.split(';').next())
                    .map(String::from)
            })
            .expect("session cookie");
        let (user_id,): (uuid::Uuid,) = sqlx::query_as("SELECT id FROM users WHERE handle = $1")
            .bind(handle)
            .fetch_one(pool)
            .await
            .unwrap();
        let plaintext: String =
            sqlx::query_scalar("SELECT plaintext FROM token_details_handoff WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(pool)
                .await
                .expect("handoff row right after signup");
        (cookie, plaintext)
    }

    async fn get(app: &axum::Router, path: &str, cookie: &str) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::get(path)
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    fn csrf(cookie: &str) -> String {
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(SECRET.as_bytes(), value)
    }

    async fn complete(app: &axum::Router, cookie: &str) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::post("/token-details")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .header(header::COOKIE, cookie)
                    .body(Body::from(format!("saved=1&_csrf={}", csrf(cookie))))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn body(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8_lossy(&bytes).to_string()
    }

    async fn handoff_count(pool: &sqlx::PgPool) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM token_details_handoff")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn renders_once_then_never_again(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());
        let (cookie, plaintext) = register(&pool, &app, "carol").await;

        // first render: plaintext + copy control on the page
        let response = get(&app, "/token-details", &cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        let html = body(response).await;
        assert!(html.contains(&plaintext), "page shows the plaintext once");
        assert!(html.contains("copy-token"), "copy-once control present");
        assert_eq!(
            handoff_count(&pool).await,
            0,
            "handoff consumed by the render"
        );

        // second visit: ordinary landing, no token anywhere
        let response = get(&app, "/token-details", &cookie).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(location, "/", "no handoff shows the ordinary landing");
        let html = body(response).await;
        assert!(!html.contains("jjh_"), "no plaintext ever again");

        // completion marks the row; still no re-render after
        let response = complete(&app, &cookie).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get("location")
                .unwrap()
                .to_str()
                .unwrap(),
            "/"
        );
        let completed: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
            "SELECT token_details_completed_at FROM users WHERE handle = 'carol'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(completed.is_some(), "completion is marked");

        let response = get(&app, "/token-details", &cookie).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get("location")
                .unwrap()
                .to_str()
                .unwrap(),
            "/",
            "completed screen never re-renders"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn abandoned_screen_does_not_retrigger(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());
        let (cookie, plaintext) = register(&pool, &app, "dave").await;

        // user views the screen once, then walks away without completing
        let response = get(&app, "/token-details", &cookie).await;
        let html = body(response).await;
        assert!(html.contains(&plaintext));

        // later visit: ordinary landing, nothing re-triggers, nothing rendered
        let response = get(&app, "/token-details", &cookie).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get("location")
                .unwrap()
                .to_str()
                .unwrap(),
            "/"
        );
        let completed: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
            "SELECT token_details_completed_at FROM users WHERE handle = 'dave'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(completed.is_none(), "abandonment is not completion");

        // settings (regenerate) remains reachable for this user
        let response = get(&app, "/settings", &cookie).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "settings stays available"
        );
    }
}
