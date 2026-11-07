// story: e23s01 — signup is gated on the click: no session until
// /verify, expiry explains the retry, login resends for the unverified.

mod email_confirmation {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    const SECRET: &str = "email-confirm-suite-secret";

    #[derive(Clone, Default)]
    struct Recorded {
        mails: Arc<Mutex<Vec<(String, String, String)>>>, // (to, subject, body)
    }

    impl Recorded {
        fn last(&self) -> (String, String, String) {
            self.mails.lock().unwrap().last().cloned().expect("a mail")
        }
        fn count(&self) -> usize {
            self.mails.lock().unwrap().len()
        }
    }

    struct RecordingMailer(Recorded);

    #[async_trait::async_trait]
    impl server::email::Mailer for RecordingMailer {
        async fn send(
            &self,
            to: &str,
            subject: &str,
            body: &str,
        ) -> Result<(), server::email::MailError> {
            self.0.mails.lock().unwrap().push((
                to.to_string(),
                subject.to_string(),
                body.to_string(),
            ));
            Ok(())
        }
    }

    async fn app(pool: &sqlx::PgPool, recorded: &Recorded) -> axum::Router {
        server::app_with_mailer(
            pool.clone(),
            "unused-storage".to_string(),
            Arc::new(RecordingMailer(recorded.clone())),
        )
    }

    async fn post(
        app: &axum::Router,
        path: &str,
        body: String,
        cookie: Option<&str>,
    ) -> axum::response::Response {
        let mut request =
            Request::post(path).header("content-type", "application/x-www-form-urlencoded");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        app.clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap()
    }

    async fn get(app: &axum::Router, path: &str) -> axum::response::Response {
        app.clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    fn session_cookie(response: &axum::response::Response) -> Option<String> {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|v| {
                v.to_str()
                    .ok()
                    .and_then(|s| s.split(';').next())
                    .filter(|s| s.starts_with("session="))
                    .map(String::from)
            })
    }

    fn token_from(mail: &(String, String, String)) -> String {
        // story: e34s01 — mails carry the path form; /verify?token= still
        // verifies when hand-built.
        let start = mail.2.find("/verify/").expect("link in mail") + "/verify/".len();
        let rest = &mail.2[start..];
        rest.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .unwrap()
            .to_string()
    }

    async fn register_form(app: &axum::Router, handle: &str) -> String {
        let body = format!(
            "email={handle}%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery"
        );
        let response = post(app, "/register", body, None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        response
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn signup_sends_the_link_and_gives_no_session(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let recorded = Recorded::default();
        let app = app(&pool, &recorded).await;

        let response = post(
            &app,
            "/register",
            "email=bob%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery".to_string(),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get(header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "/verify-sent",
            "signup lands on the verify-sent page"
        );
        assert!(
            session_cookie(&response).is_none(),
            "no session before the click"
        );

        let (to, subject, body) = recorded.last();
        assert_eq!(to, "bob@example.com");
        assert!(subject.to_lowercase().contains("confirm"));
        assert!(body.contains("/verify/"), "the path-form link");

        // the stored row is a hash, never the plaintext
        let plaintext = token_from(&recorded.last());
        let stored: Option<String> = sqlx::query_scalar(
            "SELECT encode(token_hash, 'hex') FROM email_verifications ev
             JOIN users u ON u.id = ev.user_id
             WHERE u.handle = 'bob' AND ev.used_at IS NULL",
        )
        .fetch_one(&pool)
        .await
        .ok()
        .flatten();
        let stored = stored.expect("a pending verification row");
        assert!(!stored.contains(&plaintext), "plaintext never stored");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn verify_click_confirms_and_signs_in(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let recorded = Recorded::default();
        let app = app(&pool, &recorded).await;
        register_form(&app, "carol").await;
        let token = token_from(&recorded.last());

        let response = get(&app, &format!("/verify?token={token}")).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get(header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "/token-details"
        );
        assert!(session_cookie(&response).is_some(), "the click signs in");

        let verified: Option<chrono::DateTime<chrono::Utc>> =
            sqlx::query_scalar("SELECT email_verified_at FROM users WHERE handle = 'carol'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(verified.is_some(), "email_verified_at is set");
        let used: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
            "SELECT ev.used_at FROM email_verifications ev
             JOIN users u ON u.id = ev.user_id WHERE u.handle = 'carol'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(used.is_some(), "the row is consumed");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn expired_link_explains_the_retry(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let recorded = Recorded::default();
        let app = app(&pool, &recorded).await;
        register_form(&app, "dave").await;
        let token = token_from(&recorded.last());
        // expire only the still-pending row (a prior verification may exist)
        sqlx::query(
            "UPDATE email_verifications SET expires_at = now() - interval '1 day' WHERE used_at IS NULL",
        )
        .execute(&pool)
        .await
        .unwrap();

        let response = get(&app, &format!("/verify?token={token}")).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8_lossy(&bytes);
        assert!(
            html.to_lowercase().contains("expired"),
            "expiry explains the retry: {html}"
        );
        let verified: Option<chrono::DateTime<chrono::Utc>> =
            sqlx::query_scalar("SELECT email_verified_at FROM users WHERE handle = 'dave'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(verified.is_none(), "expiry never verifies");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn a_used_link_is_one_time(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let recorded = Recorded::default();
        let app = app(&pool, &recorded).await;
        register_form(&app, "erin").await;
        let token = token_from(&recorded.last());

        let first = get(&app, &format!("/verify?token={token}")).await;
        assert_eq!(first.status(), StatusCode::SEE_OTHER);
        let again = get(&app, &format!("/verify?token={token}")).await;
        assert_eq!(
            again.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "second click is refused"
        );
        let bytes = axum::body::to_bytes(again.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&bytes)
                .to_lowercase()
                .contains("expired")
                || String::from_utf8_lossy(&bytes)
                    .to_lowercase()
                    .contains("used"),
            "reuse explains itself"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn login_while_unverified_resends_and_refuses(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let recorded = Recorded::default();
        let app = app(&pool, &recorded).await;
        register_form(&app, "frank").await;

        let response = post(
            &app,
            "/login",
            "email=frank%40example.com&password=correct+horse+battery".to_string(),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            session_cookie(&response).is_none(),
            "no session while unverified"
        );
        assert_eq!(recorded.count(), 2, "the attempt sent a fresh link");

        // the fresh link works
        let fresh = token_from(&recorded.last());
        let response = get(&app, &format!("/verify?token={fresh}")).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get(header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "/token-details"
        );

        // and login now proceeds to the landing the flow promises
        let response = post(
            &app,
            "/login",
            "email=frank%40example.com&password=correct+horse+battery".to_string(),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert!(session_cookie(&response).is_some());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn signup_link_is_absolute_and_honors_origin(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let recorded = Recorded::default();
        let app = app(&pool, &recorded).await;

        // no Host header: the local default
        let response = post(
            &app,
            "/register",
            "email=abs1%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery".to_string(),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let (_, _, body) = recorded.last();
        assert!(
            body.contains("http://localhost:3000/verify/vfy_"),
            "recipient gets a clickable link: {body}"
        );

        // a forwarded https origin wins over plain http
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .header(header::HOST, "mail.test")
            .header("x-forwarded-proto", "https");
        let response = app
            .clone()
            .oneshot(
                request
                    .body(Body::from(
                        "email=abs2%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let (_, _, body) = recorded.last();
        assert!(
            body.contains("https://mail.test/verify/vfy_"),
            "scheme and host come from the request: {body}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn resend_and_login_send_absolute_links(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let recorded = Recorded::default();
        let app = app(&pool, &recorded).await;
        register_form(&app, "gina").await;

        // unverified login resends a clickable link
        let response = post(
            &app,
            "/login",
            "email=gina%40example.com&password=correct+horse+battery".to_string(),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(recorded.count(), 2, "login resent a fresh link");
        let (_, _, body) = recorded.last();
        assert!(
            body.contains("http://localhost:3000/verify/vfy_"),
            "login resend is clickable: {body}"
        );

        // age the cooldown so the explicit resend actually fires
        sqlx::query("UPDATE email_verifications SET created_at = now() - interval '2 minutes'")
            .execute(&pool)
            .await
            .unwrap();
        let response = post(
            &app,
            "/verify-resend",
            "email=gina%40example.com".to_string(),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(recorded.count(), 3, "the resend sent a fresh link");
        let (_, _, body) = recorded.last();
        assert!(
            body.contains("Open http://localhost:3000/verify/vfy_"),
            "resend page link is clickable: {body}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn resend_confirmation_is_visible_and_neutral(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let recorded = Recorded::default();
        let app = app(&pool, &recorded).await;
        register_form(&app, "iris").await;
        sqlx::query("UPDATE email_verifications SET created_at = now() - interval '2 minutes'")
            .execute(&pool)
            .await
            .unwrap();

        let response = post(
            &app,
            "/verify-resend",
            "email=iris%40example.com".to_string(),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(location, "/verify-sent?resent=1", "lands with the notice");
        assert_eq!(recorded.count(), 2, "the resend sent a fresh link");

        let notice = "fresh confirmation link is on its way";
        let response = get(&app, "/verify-sent?resent=1").await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&bytes).contains(notice),
            "the resend leaves a visible confirmation"
        );

        // an unknown address answers identically — no enumeration
        let response = post(
            &app,
            "/verify-resend",
            "email=ghost%40example.com".to_string(),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get(header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "/verify-sent?resent=1",
            "unknown address gets the same answer"
        );
        assert_eq!(recorded.count(), 2, "no mail for an unknown address");

        // the plain signup landing carries no notice
        let response = get(&app, "/verify-sent").await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&bytes).contains(notice),
            "no resend notice without the flag"
        );
    }
}
