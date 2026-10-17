// story: e04s02
mod status_api {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use base64::Engine;
    use tower::ServiceExt;

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
    }

    fn csrf(cookie: &str) -> String {
        let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "csrf-suite-secret".into());
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(secret.as_bytes(), value)
    }

    fn basic(token: &str) -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"))
        )
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir, email: &str) -> String {
        std::env::set_var("SESSION_SECRET", "csrf-suite-secret");
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "email={email}&password=correct+horse+battery&password_confirm=correct+horse+battery"
            )))
            .unwrap();
        let response = app(pool.clone(), root).oneshot(request).await.unwrap();
        response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
    }

    async fn token(pool: &sqlx::PgPool, root: &tempfile::TempDir, cookie: &str) -> String {
        let mut request = Request::post("/settings/tokens")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!("label=ci&_csrf={}", csrf(cookie))))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response = app(pool.clone(), root).oneshot(request).await.unwrap();
        response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split("new_token=")
            .nth(1)
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
    }

    /// Create alice's repo and an MR whose only revision is `commit`.
    async fn seed(pool: &sqlx::PgPool, root: &tempfile::TempDir, cookie: &str, commit: &str) {
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "name=demo&visibility=public&_csrf={}",
                csrf(cookie)
            )))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        app(pool.clone(), root).oneshot(request).await.unwrap();

        let repo_id: uuid::Uuid = sqlx::query_scalar(
            "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id \
             WHERE u.handle = 'alice' AND r.name = 'demo'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let mr_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) \
             VALUES ($1, (SELECT id FROM users WHERE handle = 'alice'), 'aaaa') RETURNING id",
        )
        .bind(repo_id)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2)")
            .bind(mr_id)
            .bind(commit)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn post_status(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        auth: Option<&str>,
        body: String,
    ) -> axum::response::Response {
        let mut request = Request::post("/alice/demo/merge-requests/aaaa/statuses")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();
        if let Some(auth) = auth {
            request
                .headers_mut()
                .insert("authorization", auth.parse().unwrap());
        }
        app(pool, root).oneshot(request).await.unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn statuses_upsert_per_revision_and_context(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice = register(&pool, &root, "alice@example.com").await;
        seed(&pool, &root, &alice, "caaaa1").await;
        let tok = token(&pool, &root, &alice).await;

        let response = post_status(
            pool.clone(),
            &root,
            Some(&basic(&tok)),
            "revision=caaaa1&context=ci%2Fbuild&state=success&target_url=https%3A%2F%2Fci%2F1"
                .to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);

        let (count, state): (i64, String) =
            sqlx::query_as("SELECT COUNT(*), MIN(state) FROM statuses WHERE context = 'ci/build'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((count, state.as_str()), (1, "success"));

        // same (revision, context) again: one row, state replaced
        let response = post_status(
            pool.clone(),
            &root,
            Some(&basic(&tok)),
            "revision=caaaa1&context=ci%2Fbuild&state=failure".to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let (count, state): (i64, String) =
            sqlx::query_as("SELECT COUNT(*), MIN(state) FROM statuses WHERE context = 'ci/build'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((count, state.as_str()), (1, "failure"));

        // invalid state rejected
        let response = post_status(
            pool.clone(),
            &root,
            Some(&basic(&tok)),
            "revision=caaaa1&context=ci%2Flint&state=bogus".to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        // empty context rejected (DB CHECK backs this up too)
        let response = post_status(
            pool,
            &root,
            Some(&basic(&tok)),
            "revision=caaaa1&context=++&state=pending".to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn status_needs_write_token_and_known_revision(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice = register(&pool, &root, "alice@example.com").await;
        let bob = register(&pool, &root, "bob@example.com").await;
        seed(&pool, &root, &alice, "caaaa1").await;
        let alice_tok = token(&pool, &root, &alice).await;
        let bob_tok = token(&pool, &root, &bob).await;

        // no credentials
        let response = post_status(
            pool.clone(),
            &root,
            None,
            "revision=caaaa1&context=ci&state=success".to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // garbage credentials
        let response = post_status(
            pool.clone(),
            &root,
            Some("Basic bm90LWEtdG9rZW46eA=="),
            "revision=caaaa1&context=ci&state=success".to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // valid token, but its user has no write access to the repo
        let response = post_status(
            pool.clone(),
            &root,
            Some(&basic(&bob_tok)),
            "revision=caaaa1&context=ci&state=success".to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // write token, but the revision does not exist on this MR
        let response = post_status(
            pool,
            &root,
            Some(&basic(&alice_tok)),
            "revision=zzzz99&context=ci&state=success".to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
