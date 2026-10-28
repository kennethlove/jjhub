// story: e21s01 — signup mints exactly one transport token (stored
// hashed, label "signup"), hands the plaintext over through the
// one-time handoff, and redirects to the details screen instead of
// the old landing.

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
    async fn signup_mints_one_hashed_token_and_redirects_to_details(pool: sqlx::PgPool) {
        let response = post(
            pool.clone(),
            "/register",
            "email=bob%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
        )
        .await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(
            location, "/token-details",
            "signup lands on the one-time details screen"
        );

        let (user_id,): (uuid::Uuid,) = sqlx::query_as("SELECT id FROM users WHERE handle = 'bob'")
            .fetch_one(&pool)
            .await
            .unwrap();

        // exactly one transport token, label fixed, hash only
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT label, token_hash FROM tokens WHERE user_id = $1")
                .bind(user_id)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rows.len(), 1, "exactly one auto-issued token per signup");
        assert_eq!(rows[0].0, "signup");
        assert!(
            !rows[0].1.starts_with("jjh_"),
            "stored value must be a hash, never the plaintext"
        );

        // the plaintext waits in the one-time handoff, keyed to the token row
        let (handoff_plain, handoff_token): (String, uuid::Uuid) = sqlx::query_as(
            "SELECT h.plaintext, h.token_id FROM token_details_handoff h WHERE h.user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("handoff row must exist after signup");
        assert!(handoff_plain.starts_with("jjh_"));
        assert!(
            !rows[0].1.contains(&handoff_plain),
            "hash must not contain the plaintext"
        );

        let (token_id,): (uuid::Uuid,) = sqlx::query_as("SELECT id FROM tokens WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(handoff_token, token_id, "handoff names the minted token");

        // plaintext never leaks through the redirect
        let raw = format!("{:?}", response.headers());
        assert!(
            !raw.contains(&handoff_plain),
            "plaintext must not appear in response headers"
        );
    }
}
