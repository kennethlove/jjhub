// story: e32s01

mod registration_gate {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    const SECRET: &[u8] = b"test-session-secret";

    async fn register(pool: &sqlx::PgPool, local: &str) -> String {
        std::env::set_var("SESSION_SECRET", "test-session-secret");
        let response = server::app(pool.clone())
            .oneshot(
                Request::post("/register")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "email={local}%40example.com&password=correct+horse+battery\
                         &password_confirm=correct+horse+battery"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        sqlx::query("UPDATE users SET email_verified_at = now() WHERE handle = $1")
            .bind(local)
            .execute(pool)
            .await
            .unwrap();
        let response = server::app(pool.clone())
            .oneshot(
                Request::post("/login")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "email={local}%40example.com&password=correct+horse+battery"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        cookie_of(&response)
    }

    fn cookie_of(response: &axum::response::Response) -> String {
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

    fn csrf(cookie: &str) -> String {
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(SECRET, value)
    }

    async fn get(
        pool: &sqlx::PgPool,
        path: &str,
        cookie: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse().unwrap());
        }
        server::app(pool.clone()).oneshot(request).await.unwrap()
    }

    async fn post(
        pool: &sqlx::PgPool,
        path: &str,
        body: &str,
        cookie: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::post(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body.to_string()))
            .unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse().unwrap());
        }
        server::app(pool.clone()).oneshot(request).await.unwrap()
    }

    async fn location(response: &axum::response::Response) -> String {
        response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    }

    async fn body_of(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8_lossy(&bytes).to_string()
    }

    async fn try_register(pool: &sqlx::PgPool, local: &str) -> axum::response::Response {
        server::app(pool.clone())
            .oneshot(
                Request::post("/register")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "email={local}%40example.com&password=correct+horse+battery\
                         &password_confirm=correct+horse+battery"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn make_operator(pool: &sqlx::PgPool, local: &str) -> String {
        let cookie = register(pool, local).await;
        sqlx::query("UPDATE users SET is_operator = true WHERE handle = $1")
            .bind(local)
            .execute(pool)
            .await
            .unwrap();
        cookie
    }

    async fn close_registration(pool: &sqlx::PgPool) {
        sqlx::query(
            "INSERT INTO site_settings (key, value) \
             VALUES ('registration_closed', 'true') \
             ON CONFLICT (key) DO UPDATE SET value = 'true'",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn user_count(pool: &sqlx::PgPool, local: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM users WHERE handle = $1")
            .bind(local)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn close_rows(pool: &sqlx::PgPool) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM moderation_actions \
             WHERE action = 'registration_close'",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn closed_registration_redirects_to_login_and_creates_no_row(pool: sqlx::PgPool) {
        close_registration(&pool).await;
        let response = try_register(&pool, "latecomer").await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&response).await, "/login?notice=closed");
        assert_eq!(user_count(&pool, "latecomer").await, 0);

        // the message lands where the signup did
        let body = body_of(get(&pool, "/login?notice=closed", None).await).await;
        assert!(
            body.to_lowercase().contains("closed"),
            "login names the closed state: {body}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn open_registration_creates_account(pool: sqlx::PgPool) {
        let response = try_register(&pool, "welcome").await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(user_count(&pool, "welcome").await, 1);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn operator_toggle_closes_then_opens_with_audit(pool: sqlx::PgPool) {
        let operator = make_operator(&pool, "alice").await;
        let body = format!("action=close&_csrf={}", csrf(&operator));

        let response = post(&pool, "/admin/registration", &body, Some(&operator)).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&response).await, "/admin");
        assert_eq!(close_rows(&pool).await, 1);
        assert_eq!(
            try_register(&pool, "latecomer").await.status(),
            StatusCode::SEE_OTHER
        );

        let body = format!("action=open&_csrf={}", csrf(&operator));
        let response = post(&pool, "/admin/registration", &body, Some(&operator)).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            try_register(&pool, "latecomer").await.status(),
            StatusCode::SEE_OTHER
        );
        let open_rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM moderation_actions \
             WHERE action = 'registration_open'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(open_rows, 1);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn non_operator_cannot_toggle(pool: sqlx::PgPool) {
        let bob = register(&pool, "bob").await;
        let body = format!("action=close&_csrf={}", csrf(&bob));
        let response = post(&pool, "/admin/registration", &body, Some(&bob)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(close_rows(&pool).await, 0);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn live_sessions_survive_closing(pool: sqlx::PgPool) {
        let operator = make_operator(&pool, "alice").await;
        register(&pool, "bob").await;
        close_registration(&pool).await;
        // existing sessions keep working: no logout, no redirect wall
        let response = get(&pool, "/settings", Some(&operator)).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn toggle_requires_session(pool: sqlx::PgPool) {
        let response = post(&pool, "/admin/registration", "action=close", None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&response).await, "/login");
        assert_eq!(close_rows(&pool).await, 0);
    }
}
