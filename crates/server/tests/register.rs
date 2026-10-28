// story: e01s01

mod session_auth {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;
    fn form(body: &str) -> Body {
        Body::from(body.to_string())
    }

    async fn post(pool: sqlx::PgPool, path: &str, body: &str) -> axum::response::Response {
        server::app(pool)
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(form(body))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn register_creates_user_and_redirects_to_details(pool: sqlx::PgPool) {
        let response = post(
            pool.clone(),
            "/register",
            "email=Alice%40Example.COM&password=correct+horse+battery&password_confirm=correct+horse+battery",
        )
        .await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(location, "/token-details");
        assert!(
            response.headers().contains_key("set-cookie"),
            "session cookie missing"
        );

        let email: String = sqlx::query_scalar("SELECT email FROM users LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            email, "alice@example.com",
            "email must be stored normalized"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn register_rejects_short_password(pool: sqlx::PgPool) {
        let response = post(
            pool,
            "/register",
            "email=alice%40example.com&password=short&password_confirm=short",
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn register_rejects_duplicate_email(pool: sqlx::PgPool) {
        post(
            pool.clone(),
            "/register",
            "email=alice%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
        )
        .await;
        let response = post(
            pool,
            "/register",
            "email=ALICE%40example.com&password=another+long+password&password_confirm=another+long+password",
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
}
