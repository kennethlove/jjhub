// story: e37s01 — every mail ships an HTML arm and a plain-text arm
// rendered from one context, so wording, links, and neutral wording
// match by construction.

mod mail_html_text {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    const SECRET: &str = "mail-html-text-secret";

    #[derive(Clone, Default)]
    struct Recorded {
        // (to, subject, text arm, html arm)
        mails: Arc<Mutex<Vec<(String, String, String, String)>>>,
    }

    impl Recorded {
        fn last(&self) -> (String, String, String, String) {
            self.mails
                .lock()
                .unwrap()
                .last()
                .cloned()
                .expect("a mail")
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
            text: &str,
            html: &str,
        ) -> Result<(), server::email::MailError> {
            self.0.mails.lock().unwrap().push((
                to.to_string(),
                subject.to_string(),
                text.to_string(),
                html.to_string(),
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

    async fn post(app: &axum::Router, path: &str, body: String) -> axum::response::Response {
        let request =
            Request::post(path).header("content-type", "application/x-www-form-urlencoded");
        app.clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap()
    }

    fn pair(name: &str, ctx: &tera::Context) -> (String, String) {
        server::render::mail_pair(name, ctx).expect("pair renders")
    }

    fn token_in(arm: &str, prefix: &str) -> String {
        let start = arm.find(prefix).expect("link in arm") + prefix.len();
        let rest = &arm[start..];
        rest.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .unwrap()
            .to_string()
    }

    #[test]
    fn verify_pair_carries_the_link_in_both_arms() {
        let mut ctx = tera::Context::new();
        ctx.insert("link", "http://mail.test/verify/vfy_abc123");
        let (text, html) = pair("verify", &ctx);
        assert!(
            text.contains("http://mail.test/verify/vfy_abc123"),
            "text arm carries the link: {text}"
        );
        assert!(
            html.contains("href=\"http://mail.test/verify/vfy_abc123\""),
            "html arm links the same URL: {html}"
        );
        assert!(!html.contains("<link "), "html arm is self-contained");
        assert!(!html.contains("@import"), "html arm is self-contained");
        assert!(html.contains("style="), "html arm styles inline");
    }

    #[test]
    fn reset_pair_carries_the_link_in_both_arms() {
        let mut ctx = tera::Context::new();
        ctx.insert("link", "http://mail.test/reset/rst_abc123");
        let (text, html) = pair("reset", &ctx);
        assert!(
            text.contains("http://mail.test/reset/rst_abc123"),
            "text arm carries the link: {text}"
        );
        assert!(
            html.contains("href=\"http://mail.test/reset/rst_abc123\""),
            "html arm links the same URL: {html}"
        );
    }

    #[test]
    fn moderation_pair_quotes_the_reason_in_both_arms() {
        let mut ctx = tera::Context::new();
        ctx.insert("action", "unsuspend");
        ctx.insert("reason", "triaged by ops");
        let (text, html) = pair("moderation", &ctx);
        assert!(
            text.contains("triaged by ops"),
            "text arm quotes the operator reason: {text}"
        );
        assert!(
            html.contains("triaged by ops"),
            "html arm quotes the operator reason: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn signup_records_both_arms_with_the_same_link(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let recorded = Recorded::default();
        let app = app(&pool, &recorded).await;

        let body = format!(
            "email=pair%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery"
        );
        let response = post(&app, "/register", body).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        assert_eq!(recorded.count(), 1, "one mail, two arms");
        let (to, _subject, text, html) = recorded.last();
        assert_eq!(to, "pair@example.com");
        let text_token = token_in(&text, "/verify/");
        let html_token = token_in(&html, "/verify/");
        assert_eq!(text_token, html_token, "both arms carry one link");
        assert!(
            html.contains(&format!("href=\"http://localhost:3000/verify/{text_token}\"")),
            "html arm carries the absolute link: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unknown_address_resend_records_no_arm(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let recorded = Recorded::default();
        let app = app(&pool, &recorded).await;

        let response = post(&app, "/verify-resend", "email=ghost%40example.com".to_string()).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            recorded.count(),
            0,
            "neutral answer: no arm leaves for an unknown address"
        );
    }
}
