// story: e21s02 — gravatar is derived from the lowercased, trimmed
// email (md5) and never stored anywhere.

mod gravatar {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    const SECRET: &str = "test-session-secret";
    /// md5("user@example.com") — fixed fixture, no md5 crate needed in tests
    const HASH: &str = "b58996c504c5638798eb6b511e6f49af";

    async fn register(app: &axum::Router, handle: &str, email: &str) -> String {
        let response = app
            .clone()
            .oneshot(
                Request::post("/register")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "email={email}&password=correct+horse+battery&password_confirm=correct+horse+battery"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let _ = handle;
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

    #[sqlx::test(migrations = "./migrations")]
    async fn details_screen_shows_derived_avatar(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());
        // email mixed case — must be lowercased before hashing
        let cookie = register(&app, "alice", "User%40Example.COM").await;

        let html = get(&app, "/token-details", &cookie).await;
        assert!(
            html.contains(&format!("https://www.gravatar.com/avatar/{HASH}")),
            "avatar URL is the md5 of the lowercased, trimmed email"
        );
        assert!(html.contains("gravatar.com"), "link credits gravatar");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn avatar_hash_is_never_stored(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());
        let cookie = register(&app, "bob", "bob%40example.com").await;

        // render once so anything that would have persisted has persisted
        let _ = get(&app, "/token-details", &cookie).await;

        let columns: Vec<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns WHERE table_name = 'users'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(
            !columns
                .iter()
                .any(|c| c.contains("gravatar") || c.contains("avatar")),
            "no avatar column on users — derivation only, per spec"
        );

        let rows: Vec<(String,)> = sqlx::query_as("SELECT email FROM users")
            .fetch_all(&pool)
            .await
            .unwrap();
        for (email,) in rows {
            assert!(!email.contains(HASH), "email column untouched");
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn whitespace_case_variants_hash_identically(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());
        let cookie = register(&app, "carol", "  User%40Example.com  ").await;

        let html = get(&app, "/token-details", &cookie).await;
        assert!(
            html.contains(&format!("/avatar/{HASH}")),
            "trim + lowercase happens before hashing"
        );
    }
}
