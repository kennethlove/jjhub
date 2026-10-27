// story: jjhub-qi7 (e24s02) — POST /reset issues a logged, expiring
// reset token and answers identically for known and unknown emails

mod auth_reset {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn send(
        pool: sqlx::PgPool,
        path: &str,
        form: &str,
        htmx: bool,
    ) -> axum::response::Response {
        let mut request = Request::post(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(form.to_owned()))
            .unwrap();
        if htmx {
            request
                .headers_mut()
                .insert("hx-request", "true".parse().unwrap());
        }
        server::app(pool).oneshot(request).await.unwrap()
    }

    async fn body(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    async fn register(pool: &sqlx::PgPool, email: &str) {
        let form = format!("email={email}&password=hunter2tracker&password_confirm=hunter2tracker");
        send(pool.clone(), "/register", &form, false).await;
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unknown_email_answers_with_sent_card_and_no_row(pool: sqlx::PgPool) {
        let response = send(pool.clone(), "/reset", "email=nobody%40example.com", true).await;
        assert_eq!(response.status(), StatusCode::OK);
        let html = body(response).await;
        assert!(
            !html.contains("<!doctype html>"),
            "fragment expected: {html}"
        );
        assert!(html.contains("auth-card"));
        assert!(html.contains("Check your inbox"), "{html}");

        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM reset_tokens")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 0, "no token for an unknown account");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn known_email_gets_identical_answer_plus_token(pool: sqlx::PgPool) {
        register(&pool, "resetme@example.com").await;

        let known = send(pool.clone(), "/reset", "email=resetme%40example.com", true).await;
        assert_eq!(known.status(), StatusCode::OK);
        let known_body = body(known).await;

        let unknown = send(pool.clone(), "/reset", "email=nobody%40example.com", true).await;
        assert_eq!(unknown.status(), StatusCode::OK);
        let unknown_body = body(unknown).await;

        // anti-enumeration: the two answers must be indistinguishable
        assert_eq!(known_body, unknown_body);

        let (count, not_expired, unused): (i64, i64, i64) = sqlx::query_as(
            "SELECT count(*), count(*) FILTER (WHERE expires_at > now() + interval '55 minutes'), \
             count(*) FILTER (WHERE used_at IS NULL) FROM reset_tokens",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1, "one token for the real account");
        assert_eq!(not_expired, 1, "expires about an hour out");
        assert_eq!(unused, 1, "freshly issued token is unused");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn native_post_gets_full_sent_page(pool: sqlx::PgPool) {
        let response = send(pool.clone(), "/reset", "email=ghost%40example.com", false).await;
        assert_eq!(response.status(), StatusCode::OK);
        let html = body(response).await;
        assert!(html.contains("<!doctype html>"));
        assert!(html.contains("/assets/app.css"));
        assert!(html.contains("Check your inbox"));
    }
}
