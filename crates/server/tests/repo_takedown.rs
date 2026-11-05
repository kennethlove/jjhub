// story: e30s01

mod repo_takedown {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    const SECRET: &[u8] = b"test-session-secret";

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir, local: &str) -> String {
        std::env::set_var("SESSION_SECRET", "test-session-secret");
        let response = app(pool.clone(), root)
            .oneshot(
                Request::post("/register")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "email={local}%40example.com&password=correct+horse+battery\
                         &password_confirm=correct+horse+battery"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        sqlx::query("UPDATE users SET email_verified_at = now() WHERE handle = $1")
            .bind(local)
            .execute(pool)
            .await
            .unwrap();
        let response = app(pool.clone(), root)
            .oneshot(
                Request::post("/login")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "email={local}%40example.com&password=correct+horse+battery"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        cookie_of(&response)
    }

    fn cookie_of(response: &axum::response::Response) -> String {
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

    fn csrf(cookie: &str) -> String {
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(SECRET, value)
    }

    async fn get(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        path: &str,
        cookie: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse().unwrap());
        }
        app(pool.clone(), root).oneshot(request).await.unwrap()
    }

    async fn post(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        path: &str,
        body: &str,
        cookie: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::post(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body.to_string()))
            .unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse().unwrap());
        }
        app(pool.clone(), root).oneshot(request).await.unwrap()
    }

    async fn body_of(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    async fn make_operator(pool: &sqlx::PgPool, handle: &str) {
        sqlx::query("UPDATE users SET is_operator = true WHERE handle = $1")
            .bind(handle)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn create_public_repo(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        cookie: &str,
        name: &str,
    ) {
        let body = format!("name={name}&visibility=public&_csrf={}", csrf(cookie));
        let response = post(pool, root, "/repos", &body, Some(cookie)).await;
        let status = response.status();
        if status != StatusCode::SEE_OTHER {
            let text = body_of(response).await;
            panic!("create repo failed {status}: {text}");
        }
    }

    async fn take_down(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        cookie: &str,
        name: &str,
    ) -> axum::response::Response {
        let body = format!("owner=alice&name={name}&_csrf={}", csrf(cookie));
        post(pool, root, "/admin/takedown", &body, Some(cookie)).await
    }

    async fn restore(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        cookie: &str,
        name: &str,
    ) -> axum::response::Response {
        let body = format!("owner=alice&name={name}&_csrf={}", csrf(cookie));
        post(pool, root, "/admin/takedown/restore", &body, Some(cookie)).await
    }

    async fn action_count(pool: &sqlx::PgPool, action: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM moderation_actions WHERE action = $1")
            .bind(action)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn taken_down(pool: &sqlx::PgPool) -> bool {
        sqlx::query_scalar(
            "SELECT taken_down_at IS NOT NULL FROM repositories WHERE name = 'doxrepo'",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[sqlx::test]
    async fn taken_down_repo_hides_from_strangers_but_not_owner_or_operator(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice = register(&pool, &root, "alice").await;
        let bob = register(&pool, &root, "bob").await;
        let carol = register(&pool, &root, "carol").await;
        make_operator(&pool, "carol").await;
        create_public_repo(&pool, &root, &alice, "doxrepo").await;

        // baseline: the public repo is open to everyone
        assert_eq!(
            get(&pool, &root, "/alice/doxrepo", None).await.status(),
            StatusCode::OK
        );
        assert_eq!(
            get(&pool, &root, "/alice/doxrepo", Some(&bob))
                .await
                .status(),
            StatusCode::OK
        );
        // the tree resolver on an empty repo answers "unknown revision"
        // (filesystem check); the MR list renders. Both give the wall
        // something other than the wall's own 404 to distinguish from.
        let before = body_of(get(&pool, &root, "/alice/doxrepo/tree/trunk", None).await).await;
        assert!(
            before.contains("unknown revision"),
            "baseline tree: {before}"
        );
        assert_eq!(
            get(&pool, &root, "/alice/doxrepo/merge-requests", None)
                .await
                .status(),
            StatusCode::OK
        );

        let response = take_down(&pool, &root, &carol, "doxrepo").await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert!(taken_down(&pool).await);

        // every browse surface refuses the stranger: the root page
        // (its own resolver), and the search seam (visible_repo)
        for path in [
            "/alice/doxrepo",
            "/alice/doxrepo/search",
            "/alice/doxrepo/operations",
            "/alice/doxrepo/merge-requests",
        ] {
            assert_eq!(
                get(&pool, &root, path, None).await.status(),
                StatusCode::NOT_FOUND,
                "anonymous {path} must be refused"
            );
            assert_eq!(
                get(&pool, &root, path, Some(&bob)).await.status(),
                StatusCode::NOT_FOUND,
                "stranger {path} must be refused"
            );
        }
        // the tree resolver's refusal comes from the wall, not the
        // filesystem: its answer changes when the wall goes up
        let after = body_of(get(&pool, &root, "/alice/doxrepo/tree/trunk", None).await).await;
        assert!(
            after.contains("repository not found") && !after.contains("unknown revision"),
            "tree must answer with the wall's 404: {after}"
        );

        // owner and operator still see it
        assert_eq!(
            get(&pool, &root, "/alice/doxrepo", Some(&alice))
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            get(&pool, &root, "/alice/doxrepo", Some(&carol))
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            get(&pool, &root, "/alice/doxrepo/merge-requests", Some(&carol))
                .await
                .status(),
            StatusCode::OK
        );

        assert_eq!(action_count(&pool, "takedown").await, 1);
    }

    #[sqlx::test]
    async fn restore_brings_the_repo_back_and_records_its_own_row(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice = register(&pool, &root, "alice").await;
        let carol = register(&pool, &root, "carol").await;
        make_operator(&pool, "carol").await;
        create_public_repo(&pool, &root, &alice, "doxrepo").await;

        assert_eq!(
            take_down(&pool, &root, &carol, "doxrepo").await.status(),
            StatusCode::SEE_OTHER
        );
        assert_eq!(
            restore(&pool, &root, &carol, "doxrepo").await.status(),
            StatusCode::SEE_OTHER
        );

        assert!(!taken_down(&pool).await);
        assert_eq!(
            get(&pool, &root, "/alice/doxrepo", None).await.status(),
            StatusCode::OK
        );
        assert_eq!(action_count(&pool, "takedown").await, 1);
        assert_eq!(action_count(&pool, "restore").await, 1);
    }

    #[sqlx::test]
    async fn a_non_operator_cannot_take_anything_down(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice = register(&pool, &root, "alice").await;
        let bob = register(&pool, &root, "bob").await;
        create_public_repo(&pool, &root, &alice, "doxrepo").await;

        let response = take_down(&pool, &root, &bob, "doxrepo").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(!taken_down(&pool).await);
        assert_eq!(action_count(&pool, "takedown").await, 0);
        assert_eq!(
            get(&pool, &root, "/alice/doxrepo", None).await.status(),
            StatusCode::OK
        );
    }
}
