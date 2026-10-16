// story: jjhub-8bq
mod csrf_protection {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "email=alice%40example.com&password=correct+horse+battery",
            ))
            .unwrap();
        let response = server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
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

    async fn post(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
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
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap()
    }

    fn token_for(secret: &[u8], cookie: &str) -> String {
        server::session::csrf_token(secret, cookie)
    }

    const SECRET: &[u8] = b"test-session-secret";

    #[sqlx::test(migrations = "./migrations")]
    async fn post_without_token_is_forbidden(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        let response = post(
            pool,
            &root,
            "/settings/tokens",
            "label=evil",
            Some(&cookie),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn post_with_valid_token_succeeds(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        // app_with_storage derives the session secret from env; set it so the
        // rendered token and the computed token agree
        std::env::set_var("SESSION_SECRET", "test-session-secret");
        let cookie = register(&pool, &root).await;
        let token = token_for(SECRET, &cookie);
        let response = post(
            pool.clone(),
            &root,
            "/settings/tokens",
            &format!("label=fine&_csrf={token}"),
            Some(&cookie),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn forged_token_is_rejected(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        std::env::set_var("SESSION_SECRET", "test-session-secret");
        let cookie = register(&pool, &root).await;
        let response = post(
            pool,
            &root,
            "/settings/tokens",
            "label=evil&_csrf=forgedtokenvalue",
            Some(&cookie),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn settings_page_renders_hidden_token(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        std::env::set_var("SESSION_SECRET", "test-session-secret");
        let cookie = register(&pool, &root).await;
        let response = server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
            .oneshot(
                Request::get("/settings")
                    .header("cookie", cookie.parse::<axum::http::HeaderValue>().unwrap())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(
            html.contains("name=\"_csrf\""),
            "settings form carries the hidden token: {html}"
        );
        assert!(
            html.contains(&token_for(SECRET, &cookie)),
            "rendered token matches the session-derived one"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn login_and_register_stay_exempt(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        // register works with no token (pre-session)
        let response = post(
            pool.clone(),
            &root,
            "/register",
            "email=bob%40example.com&password=correct+horse+battery",
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        // login likewise
        let response = post(
            pool,
            &root,
            "/login",
            "email=bob%40example.com&password=correct+horse+battery",
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn logout_requires_token(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        std::env::set_var("SESSION_SECRET", "test-session-secret");
        let cookie = register(&pool, &root).await;
        // without token: refused
        let response = post(pool.clone(), &root, "/logout", "", Some(&cookie)).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        // with token: succeeds
        let token = token_for(SECRET, &cookie);
        let response = post(pool, &root, "/logout", &format!("_csrf={token}"), Some(&cookie)).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }
}
