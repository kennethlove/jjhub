// story: e01s02
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use tower::ServiceExt;

mod token_auth_transport {
    use super::*;

    fn basic_auth(username: &str, token: &str) -> String {
        let credentials = format!("{username}:{token}");
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(credentials)
        )
    }

    async fn get(pool: sqlx::PgPool, path: &str, auth: &str) -> axum::response::Response {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        request
            .headers_mut()
            .insert("authorization", auth.parse().unwrap());
        server::app(pool).oneshot(request).await.unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn valid_token_authenticates_transport(pool: sqlx::PgPool) {
        let (plaintext, _id) = server::token::insert_test_token(&pool).await;
        let response = get(
            pool.clone(),
            "/git/whoami",
            &basic_auth("x-access-token", &plaintext),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["user_id"].is_string(), "whoami returns user id");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn revoked_token_rejected_by_transport(pool: sqlx::PgPool) {
        let (plaintext, token_id) = server::token::insert_test_token(&pool).await;
        server::token::revoke(&pool, token_id).await;
        let response = get(
            pool,
            "/git/whoami",
            &basic_auth("x-access-token", &plaintext),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn garbage_token_rejected(pool: sqlx::PgPool) {
        let response = get(
            pool,
            "/git/whoami",
            &basic_auth("x-access-token", "jjh_notarealtoken"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn wrong_username_rejected(pool: sqlx::PgPool) {
        let (plaintext, _id) = server::token::insert_test_token(&pool).await;
        let response = get(pool, "/git/whoami", &basic_auth("wronguser", &plaintext)).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
