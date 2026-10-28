// story: e22s03 — defaults hold everywhere: unadorned minting is
// read+write, repo-page minting pins to its own repo, rotation
// carries the current token's permissions.

mod defaults {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    const SECRET: &str = "token-defaults-suite-secret";

    fn register_form(handle: &str) -> String {
        format!(
            "email={handle}%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery"
        )
    }

    async fn post(
        app: &axum::Router,
        path: &str,
        cookie: &str,
        body: String,
    ) -> axum::response::Response {
        let csrf = server::session::csrf_token(
            SECRET.as_bytes(),
            cookie.strip_prefix("session=").unwrap_or(cookie),
        );
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

    async fn register(app: &axum::Router, handle: &str) -> String {
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
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|v| v.to_str().ok().and_then(|s| s.split(';').next()).map(String::from))
            .expect("session cookie")
    }

    /// Mint one token with no permission fields (plain label), the way
    /// an older client (or plain curl) would.
    async fn mint_plain(app: &axum::Router, cookie: &str) -> axum::response::Response {
        post(app, "/settings/tokens", cookie, "label=plain".to_string()).await
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn token_created_before_this_story_defaults_to_read_write(
        pool: sqlx::PgPool,
    ) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());
        let cookie = register(&app, "plainy").await;
        assert_eq!(mint_plain(&app, &cookie).await.status(), StatusCode::SEE_OTHER);

        let scopes: Vec<String> = sqlx::query_scalar(
            "SELECT t.scopes FROM tokens t JOIN users u ON u.id = t.user_id
             WHERE u.handle = 'plainy' ORDER BY t.created_at DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let mut sorted = scopes;
        sorted.sort();
        assert_eq!(sorted, vec!["read".to_string(), "write".to_string()]);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn repo_page_token_is_pinned_to_its_own_repo(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let root = tempfile::tempdir().unwrap();
        let app = server::app_with_storage(
            pool.clone(),
            root.path().to_str().unwrap().to_string(),
        );
        let cookie = register(&app, "pinny").await;
        let csrf = server::session::csrf_token(
            SECRET.as_bytes(),
            cookie.strip_prefix("session=").unwrap_or(&cookie),
        );
        let response = app
            .clone()
            .oneshot(
                Request::post("/repos")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .header(header::COOKIE, &cookie)
                    .body(Body::from(format!("name=snippet&visibility=private&_csrf={csrf}")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        // mint via the repo route with NO permission fields
        let response = post(&app, "/pinny/snippet/tokens", &cookie, String::new()).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let (repo_id, scopes): (Option<uuid::Uuid>, Vec<String>) = sqlx::query_as(
            "SELECT t.repo_id, t.scopes FROM tokens t JOIN users u ON u.id = t.user_id
             WHERE u.handle = 'pinny' ORDER BY t.created_at DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(repo_id.is_some(), "repo-page mint pins to its own repo");
        let (pinned,): (uuid::Uuid,) = sqlx::query_as(
            "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id
             WHERE u.handle = 'pinny' AND r.name = 'snippet'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(repo_id, Some(pinned));
        let mut sorted = scopes;
        sorted.sort();
        assert_eq!(sorted, vec!["read".to_string(), "write".to_string()]);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn rotate_without_fields_floors_to_read_write(pool: sqlx::PgPool) {
        std::env::set_var("SESSION_SECRET", SECRET);
        let app = server::app(pool.clone());
        let cookie = register(&app, "carryy").await;
        let csrf = server::session::csrf_token(
            SECRET.as_bytes(),
            cookie.strip_prefix("session=").unwrap_or(&cookie),
        );

        // mint read-only explicitly (no expiry fields — absent = no expiry)
        let response = post(
            &app,
            "/settings/tokens",
            &cookie,
            "label=ro&read=on".to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let _ = csrf;

        // rotate with no permission fields: current perms carry over
        let response = post(&app, "/settings/regenerate", &cookie, String::new()).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let active: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM tokens t JOIN users u ON u.id = t.user_id
             WHERE u.handle = 'carryy' AND t.revoked_at IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(active, 1, "exactly one active token after rotation");
        let (active_scopes, active_expiry): (Vec<String>, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
            "SELECT t.scopes, t.expires_at FROM tokens t JOIN users u ON u.id = t.user_id
             WHERE u.handle = 'carryy' AND t.revoked_at IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let mut scopes = active_scopes;
        scopes.sort();
        assert_eq!(
            scopes,
            vec!["read".to_string(), "write".to_string()],
            "rotation without permission fields floors to read+write"
        );
        assert!(active_expiry.is_none(), "no expiry carried means no expiry set");
    }
}
