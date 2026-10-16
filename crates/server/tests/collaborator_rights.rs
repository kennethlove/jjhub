// story: e03s04
mod collaborator_rights {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use base64::Engine;
    use tower::ServiceExt;

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
    }

    fn basic(token: &str) -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"))
        )
    }

    fn csrf(cookie: &str) -> String {
        let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "csrf-suite-secret".into());
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(secret.as_bytes(), value)
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir, email: &str) -> String {
        std::env::set_var("SESSION_SECRET", "csrf-suite-secret");
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "email={email}&password=correct+horse+battery"
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
            .body(Body::from(format!("label=e2e&_csrf={}", csrf(cookie))))
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

    async fn info_refs(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        service: &str,
        auth: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::get(format!(
            "/git/alice/demo.git/info/refs?service=git-{service}"
        ))
        .body(Body::empty())
        .unwrap();
        if let Some(auth) = auth {
            request
                .headers_mut()
                .insert("authorization", auth.parse().unwrap());
        }
        app(pool, root).oneshot(request).await.unwrap()
    }

    async fn manage_collaborators(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        cookie: &str,
        body: &str,
    ) -> axum::response::Response {
        let mut request = Request::post("/alice/demo/settings/collaborators")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body.to_string()))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        app(pool, root).oneshot(request).await.unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn collaborator_can_push_and_loses_rights_on_removal(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice = register(&pool, &root, "alice@example.com").await;
        let bob = register(&pool, &root, "bob@example.com").await;
        let response = post_form_repo(pool.clone(), &root, &alice).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let bob_token = token(&pool, &root, &bob).await;

        // stranger: rejected
        let response = info_refs(
            pool.clone(),
            &root,
            "receive-pack",
            Some(&basic(&bob_token)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // owner adds bob
        let response = manage_collaborators(
            pool.clone(),
            &root,
            &alice,
            &format!("email=bob%40example.com&_csrf={}", csrf(&alice)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        // collaborator push passes the authz gate
        let response = info_refs(
            pool.clone(),
            &root,
            "receive-pack",
            Some(&basic(&bob_token)),
        )
        .await;
        assert_ne!(response.status(), StatusCode::FORBIDDEN);

        // removal revokes immediately
        let response = manage_collaborators(
            pool.clone(),
            &root,
            &alice,
            &format!("email=bob%40example.com&remove=1&_csrf={}", csrf(&alice)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let response = info_refs(pool, &root, "receive-pack", Some(&basic(&bob_token))).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn non_owner_cannot_manage_collaborators(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice = register(&pool, &root, "alice@example.com").await;
        let bob = register(&pool, &root, "bob@example.com").await;
        post_form_repo(pool.clone(), &root, &alice).await;

        let response = manage_collaborators(
            pool,
            &root,
            &bob,
            &format!("email=carol%40example.com&_csrf={}", csrf(&bob)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn collaborator_reads_private_clone(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice = register(&pool, &root, "alice@example.com").await;
        let bob = register(&pool, &root, "bob@example.com").await;
        let response = post_form_repo(pool.clone(), &root, &alice).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        // make it private
        let mut request = Request::post("/alice/demo/settings")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "visibility=private&_csrf={}",
                csrf(&alice)
            )))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", alice.parse().unwrap());
        app(pool.clone(), &root).oneshot(request).await.unwrap();
        manage_collaborators(
            pool.clone(),
            &root,
            &alice,
            &format!("email=bob%40example.com&_csrf={}", csrf(&alice)),
        )
        .await;

        let bob_token = token(&pool, &root, &bob).await;
        let response =
            info_refs(pool.clone(), &root, "upload-pack", Some(&basic(&bob_token))).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "collaborator reads private"
        );

        // a third user with a valid token is still refused
        let carol = register(&pool, &root, "carol@example.com").await;
        let carol_token = token(&pool, &root, &carol).await;
        let response = info_refs(pool, &root, "upload-pack", Some(&basic(&carol_token))).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "stranger refused");
    }

    async fn post_form_repo(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        cookie: &str,
    ) -> axum::response::Response {
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
        app(pool, root).oneshot(request).await.unwrap()
    }
}
