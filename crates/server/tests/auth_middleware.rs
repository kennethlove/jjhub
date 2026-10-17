// story: e01s01

mod auth_middleware {
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
        let response = post(
            pool.clone(),
            "/register",
            "email=bob%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
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
    async fn settings_redirects_to_login_without_session(pool: sqlx::PgPool) {
        let response = server::app(pool)
            .oneshot(Request::get("/settings").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(location, "/login");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn settings_rejects_tampered_cookie(pool: sqlx::PgPool) {
        let mut request = Request::get("/settings").body(Body::empty()).unwrap();
        request.headers_mut().insert(
            "cookie",
            "session=11111111-1111-1111-1111-111111111111.9999999999.deadbeef"
                .parse()
                .unwrap(),
        );
        let response = server::app(pool).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(location, "/login");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn settings_renders_with_session_cookie(pool: sqlx::PgPool) {
        let cookie = register(&pool).await;
        let mut request = Request::get("/settings").body(Body::empty()).unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response = server::app(pool).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
