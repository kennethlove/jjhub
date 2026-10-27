// story: jjhub-qi7 (e24s03) — consuming a reset link sets a new
// password once: expired, unknown, and already-used tokens are refused

mod auth_reset_password {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn get(pool: sqlx::PgPool, path: &str) -> axum::response::Response {
        server::app(pool)
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn post(pool: sqlx::PgPool, path: &str, form: &str) -> axum::response::Response {
        let request = Request::post(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(form.to_owned()))
            .unwrap();
        server::app(pool).oneshot(request).await.unwrap()
    }

    async fn body(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    async fn register(pool: &sqlx::PgPool, email: &str) {
        let form = format!("email={email}&password=oldpassword1&password_confirm=oldpassword1");
        post(pool.clone(), "/register", &form).await;
    }

    async fn mint(pool: &sqlx::PgPool, email: &str, lifetime: &str, used: bool) -> String {
        let user_id: String = sqlx::query_scalar("SELECT id::text FROM users WHERE email = $1")
            .bind(email)
            .fetch_one(pool)
            .await
            .unwrap();
        let (plaintext, _) = server::token::generate();
        let token_hash = server::token::hash(&plaintext);
        sqlx::query(
            "INSERT INTO reset_tokens (user_id, token_hash, expires_at, used_at) \
             VALUES ($1::uuid, $2, now() + $3::interval, \
                     CASE WHEN $4::bool THEN now() - interval '1 minute' ELSE NULL END)",
        )
        .bind(&user_id)
        .bind(&token_hash)
        .bind(lifetime)
        .bind(used)
        .execute(pool)
        .await
        .expect("insert reset token");
        plaintext
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn valid_token_renders_set_password_form(pool: sqlx::PgPool) {
        register(&pool, "setpw@example.com").await;
        let token = mint(&pool, "setpw@example.com", "1 hour", false).await;
        let response = get(pool, &format!("/reset-password?token={token}")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let html = body(response).await;
        assert!(html.contains("Choose a new password"), "{html}");
        assert!(html.contains(r#"name="password""#));
        assert!(html.contains(r#"name="password_confirm""#));
        assert!(html.contains(&format!(r#"name="token" value="{token}""#)));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unknown_expired_and_used_tokens_are_refused(pool: sqlx::PgPool) {
        register(&pool, "refused@example.com").await;
        let expired = mint(&pool, "refused@example.com", "-1 hour", false).await;
        let used = mint(&pool, "refused@example.com", "1 hour", true).await;

        for token in ["jjh_deadbeef".to_string(), expired, used] {
            let response = get(pool.clone(), &format!("/reset-password?token={token}")).await;
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "token {token} must be refused"
            );
            let html = body(response).await;
            assert!(html.contains("invalid or expired"), "{html}");
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn valid_token_sets_password_and_burns_link(pool: sqlx::PgPool) {
        register(&pool, "rotate@example.com").await;
        let token = mint(&pool, "rotate@example.com", "1 hour", false).await;

        let response = post(
            pool.clone(),
            "/reset-password",
            &format!("token={token}&password=newpassword9&password_confirm=newpassword9"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        // new password signs in, old one no longer does
        let login = |pw: &'static str| {
            let pool = pool.clone();
            async move {
                post(
                    pool,
                    "/login",
                    &format!("email=rotate%40example.com&password={pw}"),
                )
                .await
            }
        };
        assert_eq!(
            login("oldpassword1").await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(login("newpassword9").await.status(), StatusCode::SEE_OTHER);

        // the link is single-use
        let replay = post(
            pool.clone(),
            "/reset-password",
            &format!("token={token}&password=anotherpass1&password_confirm=anotherpass1"),
        )
        .await;
        assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mismatched_passwords_keep_the_link(pool: sqlx::PgPool) {
        register(&pool, "mismatch@example.com").await;
        let token = mint(&pool, "mismatch@example.com", "1 hour", false).await;

        let response = post(
            pool.clone(),
            "/reset-password",
            &format!("token={token}&password=firstpass1&password_confirm=secondpass1"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body(response).await;
        assert!(html.contains("passwords do not match"), "{html}");

        // old password still works: nothing burned the link
        let login = post(
            pool.clone(),
            "/login",
            "email=mismatch%40example.com&password=oldpassword1",
        )
        .await;
        assert_eq!(login.status(), StatusCode::SEE_OTHER);
    }
}
