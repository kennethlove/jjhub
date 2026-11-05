// story: e31s01

mod captcha_signup {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    static ENV: std::sync::LazyLock<tokio::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

    // One process-wide lock: the suite toggles verifier env that the
    // handler reads per request, and cargo test runs tests in threads.
    struct EnvGuard {
        _lock: tokio::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        async fn configured(siteverify_url: &str) -> Self {
            let _lock = ENV.lock().await;
            std::env::set_var("SESSION_SECRET", "test-session-secret");
            std::env::set_var("JJHUB_CAPTCHA_SITE_KEY", "cfg-site-key");
            std::env::set_var("JJHUB_CAPTCHA_SECRET_KEY", "cfg-secret-key");
            std::env::set_var("JJHUB_CAPTCHA_SITEVERIFY_URL", siteverify_url);
            Self { _lock }
        }

        async fn unconfigured() -> Self {
            let _lock = ENV.lock().await;
            std::env::set_var("SESSION_SECRET", "test-session-secret");
            std::env::remove_var("JJHUB_CAPTCHA_SITE_KEY");
            std::env::remove_var("JJHUB_CAPTCHA_SECRET_KEY");
            std::env::remove_var("JJHUB_CAPTCHA_SITEVERIFY_URL");
            Self { _lock }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // Fields drop after this, so the lock is still held.
            std::env::remove_var("JJHUB_CAPTCHA_SITE_KEY");
            std::env::remove_var("JJHUB_CAPTCHA_SECRET_KEY");
            std::env::remove_var("JJHUB_CAPTCHA_SITEVERIFY_URL");
        }
    }

    async fn siteverify_stub(success: bool) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1: 0".replace(' ', ""))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let router = axum::Router::new().route(
            "/siteverify",
            axum::routing::post(move || async move {
                (
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    format!("{{\"success\":{success}}}"),
                )
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{addr}/siteverify")
    }

    fn signup_body(email: &str, token: Option<&str>) -> String {
        let mut body = format!(
            "email={email}&password=correct+horse+battery\
             &password_confirm=correct+horse+battery"
        );
        if let Some(token) = token {
            body.push_str(&format!("&cf-turnstile-response={token}"));
        }
        body
    }

    async fn get(pool: &sqlx::PgPool, path: &str) -> axum::response::Response {
        server::app(pool.clone())
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn post(pool: &sqlx::PgPool, body: String) -> axum::response::Response {
        server::app(pool.clone())
            .oneshot(
                Request::post("/register")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn body_of(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8_lossy(&bytes).to_string()
    }

    async fn user_count(pool: &sqlx::PgPool, email: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM users WHERE email = $1")
            .bind(email)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[sqlx::test]
    async fn configured_missing_token_creates_no_account(pool: sqlx::PgPool) {
        let stub = siteverify_stub(true).await;
        let _env = EnvGuard::configured(&stub).await;

        let response = get(&pool, "/register").await;
        assert_eq!(response.status(), StatusCode::OK);
        let page = body_of(response).await;
        assert!(
            page.contains("data-sitekey=\"cfg-site-key\""),
            "widget missing: {page}"
        );

        let response = post(&pool, signup_body("alice@example.com", None)).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let page = body_of(response).await;
        assert!(page.contains("challenge"), "retry message missing: {page}");
        assert_eq!(user_count(&pool, "alice@example.com").await, 0);
    }

    #[sqlx::test]
    async fn configured_verifier_rejection_creates_no_account(pool: sqlx::PgPool) {
        let stub = siteverify_stub(false).await;
        let _env = EnvGuard::configured(&stub).await;

        let response = post(&pool, signup_body("bob@example.com", Some("stale-token"))).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let page = body_of(response).await;
        assert!(page.contains("challenge"), "retry message missing: {page}");
        assert_eq!(user_count(&pool, "bob@example.com").await, 0);
    }

    #[sqlx::test]
    async fn configured_unreachable_verifier_fails_closed(pool: sqlx::PgPool) {
        let _env = EnvGuard::configured("http://127.0.0.1:9/siteverify").await;

        let response = post(&pool, signup_body("carol@example.com", Some("any-token"))).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(user_count(&pool, "carol@example.com").await, 0);
    }

    #[sqlx::test]
    async fn configured_success_creates_the_account(pool: sqlx::PgPool) {
        let stub = siteverify_stub(true).await;
        let _env = EnvGuard::configured(&stub).await;

        let response = post(&pool, signup_body("dave@example.com", Some("good-token"))).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(user_count(&pool, "dave@example.com").await, 1);
    }

    #[sqlx::test]
    async fn unconfigured_signup_is_exactly_as_before(pool: sqlx::PgPool) {
        let _env = EnvGuard::unconfigured().await;

        let response = get(&pool, "/register").await;
        assert_eq!(response.status(), StatusCode::OK);
        let page = body_of(response).await;
        assert!(!page.contains("cf-turnstile"), "widget leaked: {page}");
        assert!(
            !page.contains("challenges.cloudflare.com"),
            "widget leaked: {page}"
        );
        assert!(page.contains("Create your account"), "form changed: {page}");

        let response = post(&pool, signup_body("erin@example.com", None)).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(user_count(&pool, "erin@example.com").await, 1);
    }
}
