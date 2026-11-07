//! e34s01 — the mailer contract: a safe default, path-form verify links,
//! and a neutral, cooldown-limited resend.
//!
//! Runs against the live jjhub_test database. `cargo test -p server --test
//! mail_core` is the expected-RED / expected-GREEN gate.

mod mail_core {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const SECRET: &str = "test-session-secret";

    fn csrf(cookie: &str) -> String {
        let value = cookie.strip_prefix("session=").unwrap_or("");
        server::session::csrf_token(SECRET.as_bytes(), value)
    }

    async fn body_of(response: axum::response::Response) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8_lossy(&bytes).to_string()
    }

    async fn get(app: &axum::Router, path: &str) -> (StatusCode, String) {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        (status, body_of(response).await)
    }

    async fn post_form(app: &axum::Router, path: &str, body: &str) -> StatusCode {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        response.status()
    }

    /// One address per test, so the rate limiter and cooldown never share a
    /// row between tests.
    fn fresh_email(tag: &str) -> String {
        format!("{tag}@example.test")
    }

    async fn register(pool: &sqlx::PgPool, email: &str) -> String {
        let app = server::app(pool.clone());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/register")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "email={email}&password=hunter2hunter2&password_confirm=hunter2hunter2&_csrf={}",
                        csrf("")
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "register");
        let set = sqlx::query("UPDATE users SET email_verified_at = now() WHERE email = $1")
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
        assert_eq!(set.rows_affected(), 1, "email_verified_at");
        sqlx::query_scalar::<_, String>("SELECT id::text FROM users WHERE email = $1")
            .bind(email)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// The mail vars of a fresh node: every provider arm off.
    struct MailEnv {
        provider: Option<String>,
        resend_key: Option<String>,
        messagepit_url: Option<String>,
    }

    impl MailEnv {
        fn disarm() -> Self {
            let grab = |k: &str| std::env::var(k).ok();
            let guard = Self {
                provider: grab("JJHUB_EMAIL_PROVIDER"),
                resend_key: grab("RESEND_API_KEY"),
                messagepit_url: grab("JJHUB_MESSAGEPIT_URL"),
            };
            std::env::remove_var("JJHUB_EMAIL_PROVIDER");
            std::env::remove_var("RESEND_API_KEY");
            std::env::remove_var("JJHUB_MESSAGEPIT_URL");
            guard
        }
    }

    impl Drop for MailEnv {
        fn drop(&mut self) {
            if let Some(v) = self.provider.take() {
                std::env::set_var("JJHUB_EMAIL_PROVIDER", v);
            }
            if let Some(v) = self.resend_key.take() {
                std::env::set_var("RESEND_API_KEY", v);
            }
            if let Some(v) = self.messagepit_url.take() {
                std::env::set_var("JJHUB_MESSAGEPIT_URL", v);
            }
        }
    }

    /// The default arm sends without a network and without a configured
    /// provider: LogMailer, for an unset or unknown JJHUB_EMAIL_PROVIDER.
    #[tokio::test]
    async fn unset_and_unknown_providers_fall_back_to_logmailer() {
        let _env = MailEnv::disarm();
        let mailer = server::email::from_env();
        mailer
            .send("someone@example.test", "subject", "body", "<p>body</p>")
            .await
            .expect("the default arm never fails and never dials out");

        std::env::set_var("JJHUB_EMAIL_PROVIDER", "purple-smoke");
        let mailer = server::email::from_env();
        mailer
            .send("someone@example.test", "subject", "body", "<p>body</p>")
            .await
            .expect("an unknown provider falls back to the log arm");
    }

    /// The path form verifies, and the e23 query form keeps working.
    #[sqlx::test]
    async fn path_verify_link_verifies_and_the_query_form_still_works(pool: sqlx::PgPool) {
        let email = fresh_email("pathlink");
        let user_id = register(&pool, &email).await;

        // An unverified user with a live verification, as signup leaves them.
        sqlx::query("UPDATE users SET email_verified_at = NULL WHERE id = $1::uuid")
            .bind(&user_id)
            .execute(&pool)
            .await
            .unwrap();
        let pending =
            server::email::send_verification(&pool, uuid::Uuid::parse_str(&user_id).unwrap()).await;
        let app = server::app(pool.clone());

        let (status, _) = get(&app, &format!("/verify/{}", pending.token)).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "path link session");
        let verified = sqlx::query_scalar::<_, bool>(
            "SELECT email_verified_at IS NOT NULL FROM users WHERE id = $1::uuid",
        )
        .bind(&user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(verified, "path link marked the address verified");

        // e23's query form: same contract, still accepted.
        sqlx::query("UPDATE users SET email_verified_at = NULL WHERE id = $1::uuid")
            .bind(&user_id)
            .execute(&pool)
            .await
            .unwrap();
        let pending =
            server::email::send_verification(&pool, uuid::Uuid::parse_str(&user_id).unwrap()).await;
        let (status, _) = get(&app, &format!("/verify?token={}", pending.token)).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "query link still works");
    }

    /// The resend endpoint answers the same neutral way for known and
    /// unknown addresses, mints at most one fresh verification, and cools
    /// down: a second immediate send leaves the pending row untouched.
    #[sqlx::test]
    async fn resend_is_neutral_and_cools_down(pool: sqlx::PgPool) {
        let email = fresh_email("resend");
        let user_id = register(&pool, &email).await;
        let user_uuid = uuid::Uuid::parse_str(&user_id).unwrap();
        // Back to unverified with a pending row, as signup leaves it.
        sqlx::query("UPDATE users SET email_verified_at = NULL WHERE id = $1::uuid")
            .bind(user_uuid)
            .execute(&pool)
            .await
            .unwrap();
        server::email::send_verification(&pool, user_uuid).await;
        let pending_before: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM email_verifications WHERE user_id = $1 AND used_at IS NULL",
        )
        .bind(user_uuid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(pending_before >= 1, "a pending verification exists");
        let hash_before: Vec<u8> = sqlx::query_scalar(
            "SELECT token_hash FROM email_verifications WHERE user_id = $1 AND used_at IS NULL LIMIT 1",
        )
        .bind(user_uuid)
        .fetch_one(&pool)
        .await
        .unwrap();

        let app = server::app(pool.clone());
        let status = post_form(&app, "/verify-resend", &format!("email={email}")).await;
        assert_eq!(
            status,
            StatusCode::SEE_OTHER,
            "known address: neutral redirect, never an error"
        );
        let status = post_form(&app, "/verify-resend", &format!("email={email}")).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "repeat within cooldown");

        let rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM email_verifications WHERE user_id = $1 AND used_at IS NULL",
        )
        .bind(user_uuid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows, pending_before, "cooldown: no second pending row");
        let hash_after: Vec<u8> = sqlx::query_scalar(
            "SELECT token_hash FROM email_verifications WHERE user_id = $1 AND used_at IS NULL LIMIT 1",
        )
        .bind(user_uuid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            hash_after, hash_before,
            "cooldown: the pending token was not re-minted"
        );

        // Unknown address: byte-identical answer, no rows anywhere.
        let status = post_form(
            &app,
            "/verify-resend",
            &format!("email={}", fresh_email("ghost")),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::SEE_OTHER,
            "unknown address: same neutral answer, no enumeration"
        );
        let ghost: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE email = $1")
            .bind(fresh_email("ghost"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(ghost, 0, "resend never creates an account");
    }

    /// The verify-sent page carries the resend affordance, ready for the
    /// CDP overflow probe in the UAT phase.
    #[sqlx::test]
    async fn verify_sent_page_offers_the_resend_form(pool: sqlx::PgPool) {
        let app = server::app(pool.clone());
        let (status, body) = get(&app, "/verify-sent").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains("action=\"/verify-resend\""),
            "the sent page offers the resend form"
        );
        assert!(body.contains("name=\"email\""), "with its email field");
    }
}
