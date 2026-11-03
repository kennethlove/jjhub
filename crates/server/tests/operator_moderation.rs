// story: e29s01

mod operator_moderation {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    const SECRET: &[u8] = b"test-session-secret";

    async fn register(pool: &sqlx::PgPool, local: &str) -> String {
        // one secret for every app this test builds, so the rendered
        // CSRF token and the computed one always agree
        std::env::set_var("SESSION_SECRET", "test-session-secret");
        let response = server::app(pool.clone())
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
        // story: e23s01 — signup no longer sets a session: confirm by
        // hand, then log in to get the cookie.
        sqlx::query("UPDATE users SET email_verified_at = now() WHERE handle = $1")
            .bind(local)
            .execute(pool)
            .await
            .unwrap();
        let response = server::app(pool.clone())
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
        path: &str,
        cookie: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse().unwrap());
        }
        server::app(pool.clone()).oneshot(request).await.unwrap()
    }

    async fn post(
        pool: &sqlx::PgPool,
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
        server::app(pool.clone()).oneshot(request).await.unwrap()
    }

    fn location(response: &axum::response::Response) -> String {
        response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    }

    async fn body_of(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8_lossy(&bytes).to_string()
    }

    async fn login(pool: &sqlx::PgPool, local: &str, password: &str) -> (StatusCode, String) {
        let response = server::app(pool.clone())
            .oneshot(
                Request::post("/login")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "email={local}%40example.com&password={password}"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        (status, body_of(response).await)
    }

    async fn make_operator(pool: &sqlx::PgPool, local: &str) -> String {
        let cookie = register(pool, local).await;
        sqlx::query("UPDATE users SET is_operator = true WHERE handle = $1")
            .bind(local)
            .execute(pool)
            .await
            .unwrap();
        cookie
    }

    async fn actions(pool: &sqlx::PgPool) -> Vec<(uuid::Uuid, uuid::Uuid, String)> {
        sqlx::query_as("SELECT actor_id, target_id, action FROM moderation_actions")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    async fn id_of(pool: &sqlx::PgPool, local: &str) -> uuid::Uuid {
        sqlx::query_scalar("SELECT id FROM users WHERE handle = $1")
            .bind(local)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn env_var_designates_the_bootstrap_operator(pool: sqlx::PgPool) {
        std::env::set_var("JJHUB_OPERATOR", "envboss");
        let cookie = register(&pool, "envboss").await;
        let response = get(&pool, "/admin", Some(&cookie)).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn column_operator_reaches_admin_and_sees_accounts(pool: sqlx::PgPool) {
        let _ = register(&pool, "alice").await;
        let cookie = make_operator(&pool, "bob").await;
        let response = get(&pool, "/admin", Some(&cookie)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_of(response).await;
        assert!(body.contains("alice"), "admin lists accounts: {body}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn ordinary_user_is_shown_nothing_on_admin(pool: sqlx::PgPool) {
        let cookie = register(&pool, "mallory").await;
        let response = get(&pool, "/admin", Some(&cookie)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let token = csrf(&cookie);
        let response = post(
            &pool,
            "/admin/suspend",
            &format!("target=alice&_csrf={token}"),
            Some(&cookie),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn moderation_posts_require_session_and_csrf(pool: sqlx::PgPool) {
        let response = post(&pool, "/admin/suspend", "target=alice", None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&response), "/login");

        let cookie = register(&pool, "bob").await;
        let response = post(&pool, "/admin/suspend", "target=alice", Some(&cookie)).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let response = post(
            &pool,
            "/admin/suspend",
            "target=alice&_csrf=forgedtokenvalue",
            Some(&cookie),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn operator_suspends_and_the_session_and_login_die(pool: sqlx::PgPool) {
        let victim = register(&pool, "alice").await;
        let response = get(&pool, "/settings", Some(&victim)).await;
        assert_eq!(response.status(), StatusCode::OK);

        let actor = make_operator(&pool, "bob").await;
        let token = csrf(&actor);
        let response = post(
            &pool,
            "/admin/suspend",
            &format!("target=alice&_csrf={token}"),
            Some(&actor),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&response), "/admin");

        // the live session dies on its next request
        let response = get(&pool, "/settings", Some(&victim)).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&response), "/login");

        // login answers exactly like a bad password — no enumeration
        let (suspended, suspended_body) = login(&pool, "alice", "correct+horse+battery").await;
        let (wrong, wrong_body) = login(&pool, "alice", "not-the-password").await;
        assert_eq!(suspended, wrong);
        assert_eq!(suspended, StatusCode::UNAUTHORIZED);
        assert_eq!(suspended_body, wrong_body);

        // one audit row: actor, target, action
        let rows = actions(&pool).await;
        assert_eq!(rows.len(), 1, "one audit row: {rows:?}");
        assert_eq!(rows[0].0, id_of(&pool, "bob").await);
        assert_eq!(rows[0].1, id_of(&pool, "alice").await);
        assert_eq!(rows[0].2, "suspend");

        // the admin page shows the state
        let response = get(&pool, "/admin", Some(&actor)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_of(response).await;
        assert!(
            body.to_lowercase().contains("suspended"),
            "admin marks the suspended account: {body}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unsuspend_restores_the_same_account_and_session(pool: sqlx::PgPool) {
        let victim = register(&pool, "alice").await;
        let actor = make_operator(&pool, "bob").await;
        let token = csrf(&actor);
        let response = post(
            &pool,
            "/admin/suspend",
            &format!("target=alice&_csrf={token}"),
            Some(&actor),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let response = get(&pool, "/settings", Some(&victim)).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let token = csrf(&actor);
        let response = post(
            &pool,
            "/admin/unsuspend",
            &format!("target=alice&_csrf={token}"),
            Some(&actor),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&response), "/admin");

        // the same account, the same session cookie, works again
        let response = get(&pool, "/settings", Some(&victim)).await;
        assert_eq!(response.status(), StatusCode::OK);

        let rows = actions(&pool).await;
        assert_eq!(rows.len(), 2, "both actions audited: {rows:?}");
        assert_eq!(rows[1].2, "unsuspend");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn operators_are_never_suspendable(pool: sqlx::PgPool) {
        let carol = make_operator(&pool, "carol").await;
        let token = csrf(&carol);

        // another operator as target — the role must stay lockout-proof
        let _dave = register(&pool, "dave").await;
        sqlx::query("UPDATE users SET is_operator = true WHERE handle = 'dave'")
            .execute(&pool)
            .await
            .unwrap();
        let response = post(
            &pool,
            "/admin/suspend",
            &format!("target=dave&_csrf={token}"),
            Some(&carol),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // self-suspend — the lockout guard covers the actor too
        let token = csrf(&carol);
        let response = post(
            &pool,
            "/admin/suspend",
            &format!("target=carol&_csrf={token}"),
            Some(&carol),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let rows = actions(&pool).await;
        assert!(rows.is_empty(), "refusals never audit: {rows:?}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn suspended_session_is_walled_out_of_review_proxy(pool: sqlx::PgPool) {
        let victim = register(&pool, "alice").await;
        // an active session is not bounced to /login by the proxy
        let response = get(&pool, "/review/alice/demo", Some(&victim)).await;
        let active_is_login_redirect =
            response.status() == StatusCode::SEE_OTHER && location(&response) == "/login";
        assert!(
            !active_is_login_redirect,
            "active session reaches the proxy"
        );

        let actor = make_operator(&pool, "bob").await;
        let token = csrf(&actor);
        let response = post(
            &pool,
            "/admin/suspend",
            &format!("target=alice&_csrf={token}"),
            Some(&actor),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let response = get(&pool, "/review/alice/demo", Some(&victim)).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&response), "/login");
    }

    // ---- story: e29s02 — transport refusal ----------------------

    fn basic(plaintext: &str) -> String {
        use base64::Engine;
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{plaintext}"))
        )
    }

    async fn create_private_repo(pool: &sqlx::PgPool, root: &tempfile::TempDir, cookie: &str) {
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "name=demo&visibility=private&_csrf={}",
                csrf(cookie)
            )))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    async fn suspend(pool: &sqlx::PgPool, handle: &str) {
        sqlx::query("UPDATE users SET suspended_at = now() WHERE handle = $1")
            .bind(handle)
            .execute(pool)
            .await
            .unwrap();
    }

    /// GET the upload-pack advertisement with a Basic token.
    async fn info_refs(pool: &sqlx::PgPool, root: &tempfile::TempDir, token: &str) -> StatusCode {
        let mut request = Request::get("/git/alice/demo.git/info/refs?service=git-upload-pack")
            .header(axum::http::header::AUTHORIZATION, basic(token))
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                0,
            ))));
        server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap()
            .status()
    }

    /// POST receive-pack — refusal must happen before any spawn.
    async fn push_status(pool: &sqlx::PgPool, root: &tempfile::TempDir, token: &str) -> StatusCode {
        let mut request = Request::post("/git/alice/demo.git/git-receive-pack")
            .header(axum::http::header::AUTHORIZATION, basic(token))
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                0,
            ))));
        server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap()
            .status()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn suspended_account_token_fails_closed_at_authenticate(pool: sqlx::PgPool) {
        let _cookie = register(&pool, "alice").await;
        let user_id = id_of(&pool, "alice").await;
        let live = server::token::create(&pool, user_id, "live").await;
        assert!(
            server::token::authenticate(&pool, &live.plaintext, None)
                .await
                .is_some(),
            "active account still authenticates"
        );
        suspend(&pool, "alice").await;
        assert!(
            server::token::authenticate(&pool, &live.plaintext, None)
                .await
                .is_none(),
            "suspended account's token fails closed to None"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn suspended_account_token_refused_on_smart_http(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, "alice").await;
        create_private_repo(&pool, &root, &cookie).await;
        let user_id = id_of(&pool, "alice").await;
        let token = server::token::create(&pool, user_id, "clone").await;
        assert_eq!(
            info_refs(&pool, &root, &token.plaintext).await,
            StatusCode::OK,
            "active account's clone behaves as before"
        );
        suspend(&pool, "alice").await;
        assert_eq!(
            info_refs(&pool, &root, &token.plaintext).await,
            StatusCode::UNAUTHORIZED,
            "suspended clone refused"
        );
        assert_eq!(
            push_status(&pool, &root, &token.plaintext).await,
            StatusCode::UNAUTHORIZED,
            "suspended push refused"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn suspended_account_token_refused_on_ssh_before_spawn(pool: sqlx::PgPool) {
        use russh::server::Handler;
        let root = tempfile::tempdir().unwrap();
        let _cookie = register(&pool, "alice").await;
        let user_id = id_of(&pool, "alice").await;
        let token = server::token::create(&pool, user_id, "ssh").await;
        let mut handler = server::ssh::SshHandler::with_storage(
            pool.clone(),
            root.path().to_str().unwrap().to_string(),
        );
        let auth = handler
            .auth_password("alice", &token.plaintext)
            .await
            .unwrap();
        assert!(
            matches!(auth, russh::server::Auth::Accept),
            "active account's SSH auth accepted"
        );
        suspend(&pool, "alice").await;
        let auth = handler
            .auth_password("alice", &token.plaintext)
            .await
            .unwrap();
        assert!(
            matches!(auth, russh::server::Auth::Reject { .. }),
            "suspended account rejected before any git process spawns"
        );
    }
}
