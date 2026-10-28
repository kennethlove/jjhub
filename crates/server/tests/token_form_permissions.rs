// story: e22s03 — settings create form permissions: at least one
// scope; empty selection floors to read+write, never no-scope.

mod form_permissions {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    fn register_form(handle: &str) -> String {
        format!(
            "email={handle}%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery"
        )
    }

    async fn post_form(
        app: &axum::Router,
        path: &str,
        cookie: &str,
        csrf: &str,
        body: String,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .header(header::COOKIE, cookie)
                    .body(Body::from(format!("{body}&_csrf={csrf}")))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn register(app: &axum::Router, pool: &sqlx::PgPool, handle: &str) -> String {
        let response = app
            .clone()
            .oneshot(
                Request::post("/register")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(register_form(handle)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|v| v.to_str().ok().and_then(|s| s.split(';').next()).map(String::from))
            .expect("session cookie");
        let _ = pool;
        cookie
    }

    async fn csrf_for(cookie: &str) -> String {
        server::session::csrf_token(
            b"token-form-suite-secret",
            cookie.strip_prefix("session=").unwrap_or(cookie),
        )
    }

    async fn newest_scopes(pool: &sqlx::PgPool, handle: &str) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT t.scopes FROM tokens t JOIN users u ON u.id = t.user_id
             WHERE u.handle = $1 ORDER BY t.created_at DESC LIMIT 1",
        )
        .bind(handle)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn explicit_read_only_sticks(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", "token-form-suite-secret");
        let app = server::app(pool.clone());
        let cookie = register(&app, &pool, "formread").await;
        let csrf = csrf_for(&cookie).await;

        let response = post_form(
            &app,
            "/settings/tokens",
            &cookie,
            &csrf,
            "label=ro&read=on".to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        assert_eq!(
            newest_scopes(&pool, "formread").await,
            vec!["read".to_string()],
            "explicit read-only scope is stored as-is"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn empty_selection_floors_to_read_write(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", "token-form-suite-secret");
        let app = server::app(pool.clone());
        let cookie = register(&app, &pool, "formempty").await;
        let csrf = csrf_for(&cookie).await;

        // no scope checkboxes -> the read+write floor, never no-scope
        let response = post_form(
            &app,
            "/settings/tokens",
            &cookie,
            &csrf,
            "label=basic".to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let mut scopes = newest_scopes(&pool, "formempty").await;
        scopes.sort();
        assert_eq!(scopes, vec!["read".to_string(), "write".to_string()]);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn past_expiry_is_rejected_with_422(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", "token-form-suite-secret");
        let app = server::app(pool.clone());
        let cookie = register(&app, &pool, "formpast").await;
        let csrf = csrf_for(&cookie).await;

        let response = post_form(
            &app,
            "/settings/tokens",
            &cookie,
            &csrf,
            "label=stale&expires=2000-01-01".to_string(),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "a date in the past is a form error, not a mint"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn allowlist_is_stored_and_scopes_survive(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", "token-form-suite-secret");
        let app = server::app(pool.clone());
        let cookie = register(&app, &pool, "formallow").await;
        let csrf = csrf_for(&cookie).await;

        let response = post_form(
            &app,
            "/settings/tokens",
            &cookie,
            &csrf,
            "label=offsite&read=on&allowed_inets=10.0.0.5".to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let (scopes, allowlist): (Vec<String>, Option<Vec<String>>) = sqlx::query_as(
            "SELECT t.scopes, t.allowed_inets::text[] FROM tokens t JOIN users u ON u.id = t.user_id
             WHERE u.handle = 'formallow' ORDER BY t.created_at DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(scopes, vec!["read".to_string()]);
        assert_eq!(allowlist.as_deref(), Some(&["10.0.0.5".to_string()][..]));
    }
}
