// story: jjhub-qi7 (e24s01) — htmx fragment negotiation for the auth cards

mod auth_htmx {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn get(pool: sqlx::PgPool, path: &str, htmx: bool) -> axum::response::Response {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
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

    #[sqlx::test(migrations = "./migrations")]
    async fn register_full_page_without_htmx_header(pool: sqlx::PgPool) {
        let response = get(pool, "/register", false).await;
        assert_eq!(response.status(), StatusCode::OK);
        let html = body(response).await;
        assert!(html.contains("<!doctype html>"), "full document expected");
        assert!(html.contains("/register"));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn register_returns_card_fragment_for_htmx(pool: sqlx::PgPool) {
        let response = get(pool, "/register", true).await;
        assert_eq!(response.status(), StatusCode::OK);
        let html = body(response).await;
        assert!(
            !html.contains("<!doctype html>"),
            "fragment must not be a full document: {html}"
        );
        assert!(
            html.contains("auth-card"),
            "fragment carries the card markup"
        );
        assert!(html.contains("Create your account"));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn login_returns_card_fragment_for_htmx(pool: sqlx::PgPool) {
        let response = get(pool, "/login", true).await;
        assert_eq!(response.status(), StatusCode::OK);
        let html = body(response).await;
        assert!(!html.contains("<!doctype html>"));
        assert!(html.contains("auth-card"));
        assert!(html.contains("Log in"));
    }
}
