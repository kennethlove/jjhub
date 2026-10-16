// story: e01s01

mod session_auth_login {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;
    async fn post(pool: sqlx::PgPool, path: &str, body: &str) -> axum::response::Response {
        server::app(pool)
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn register(pool: &sqlx::PgPool) -> String {
        std::env::set_var("SESSION_SECRET", "login-suite-secret");
        let response = post(
            pool.clone(),
            "/register",
            "email=bob%40example.com&password=correct+horse+battery",
        )
        .await;
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
    async fn login_with_correct_password_sets_session(pool: sqlx::PgPool) {
        register(&pool).await;
        let response = post(
            pool.clone(),
            "/login",
            "email=bob%40example.com&password=correct+horse+battery",
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(location, "/settings");
        assert!(response.headers().contains_key("set-cookie"));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn login_with_wrong_password_is_401(pool: sqlx::PgPool) {
        register(&pool).await;
        let response = post(
            pool,
            "/login",
            "email=bob%40example.com&password=wrong-password",
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn login_unknown_email_is_401(pool: sqlx::PgPool) {
        let response = post(
            pool,
            "/login",
            "email=nobody%40example.com&password=correct+horse+battery",
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn logout_clears_session(pool: sqlx::PgPool) {
        let cookie = register(&pool).await;
        // logout is session-authed, so it carries the token
        let secret = std::env::var("SESSION_SECRET").unwrap_or_default();
        let value = cookie.strip_prefix("session=").unwrap_or(&cookie);
        let token = server::session::csrf_token(secret.as_bytes(), value);
        let mut request = Request::post("/logout")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!("_csrf={token}")))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response = server::app(pool).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let set_cookie = response
            .headers()
            .get("set-cookie")
            .expect("logout must clear cookie")
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            set_cookie.contains("Max-Age=0"),
            "cookie must be expired: {set_cookie}"
        );
    }
}
