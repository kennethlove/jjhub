// story: e28s01
//! Protected heads configuration: stored patterns merge with the
//! immutable defaults, the Operations panel manages them behind session
//! + CSRF, and the admission gate enforces the merged set.

mod resolver {
    use server::receive_guard::protected_refs;

    async fn seed(pool: &sqlx::PgPool) -> uuid::Uuid {
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query_scalar(
            "INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo') RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn store_pattern(pool: &sqlx::PgPool, repo_id: uuid::Uuid, pattern: &str) {
        sqlx::query("INSERT INTO protected_refs (repo_id, pattern) VALUES ($1, $2)")
            .bind(repo_id)
            .bind(pattern)
            .execute(pool)
            .await
            .unwrap();
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn stored_pattern_merges_with_defaults(pool: sqlx::PgPool) {
        let repo_id = seed(&pool).await;
        store_pattern(&pool, repo_id, "refs/heads/release/*").await;
        let set = protected_refs(&pool, repo_id).await.unwrap();
        assert_eq!(
            set,
            vec![
                "refs/heads/trunk".to_string(),
                "refs/tags/*".to_string(),
                "refs/heads/release/*".to_string(),
            ],
            "{set:?}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn defaults_survive_when_no_rows_are_stored(pool: sqlx::PgPool) {
        let repo_id = seed(&pool).await;
        let set = protected_refs(&pool, repo_id).await.unwrap();
        assert_eq!(
            set,
            vec!["refs/heads/trunk".to_string(), "refs/tags/*".to_string()],
            "{set:?}"
        );
    }
}

mod admission {
    use server::receive_guard::{check_protected_refs, PushedRef};

    async fn seed(pool: &sqlx::PgPool) -> uuid::Uuid {
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query_scalar(
            "INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo') RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    fn git_repo_with_commits() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            out
        };
        run(&["init", "-q", "-b", "trunk"]);
        run(&["config", "user.email", "alice@example.com"]);
        run(&["config", "user.name", "Alice"]);
        std::fs::write(dir.path().join("file"), "one").unwrap();
        run(&["add", "file"]);
        run(&["commit", "-q", "-m", "one"]);
        let base = String::from_utf8(run(&["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_string();
        std::fs::write(dir.path().join("file"), "two").unwrap();
        run(&["commit", "-q", "-am", "two"]);
        let head = String::from_utf8(run(&["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_string();
        run(&["checkout", "-q", "-b", "side", &base]);
        std::fs::write(dir.path().join("other"), "x").unwrap();
        run(&["add", "other"]);
        run(&["commit", "-q", "-m", "side"]);
        let side = String::from_utf8(run(&["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_string();
        (dir, side, head)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn stored_pattern_refuses_non_fast_forward(pool: sqlx::PgPool) {
        let repo_id = seed(&pool).await;
        sqlx::query("INSERT INTO protected_refs (repo_id, pattern) VALUES ($1, $2)")
            .bind(repo_id)
            .bind("refs/heads/release/*")
            .execute(&pool)
            .await
            .unwrap();
        let (dir, sibling, _head) = git_repo_with_commits();
        let updates = vec![PushedRef {
            old: "a".repeat(40),
            new: sibling,
            ref_name: "refs/heads/release/1.0".to_string(),
        }];
        let rejections = check_protected_refs(&pool, repo_id, &updates, dir.path())
            .await
            .unwrap();
        assert_eq!(rejections.len(), 1, "{rejections:?}");
        assert!(
            rejections[0].contains("refs/heads/release/1.0"),
            "names the ref: {}",
            rejections[0]
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn stored_pattern_refuses_deletion(pool: sqlx::PgPool) {
        let repo_id = seed(&pool).await;
        sqlx::query("INSERT INTO protected_refs (repo_id, pattern) VALUES ($1, $2)")
            .bind(repo_id)
            .bind("refs/heads/release/*")
            .execute(&pool)
            .await
            .unwrap();
        let updates = vec![PushedRef {
            old: "a".repeat(40),
            new: "0".repeat(40),
            ref_name: "refs/heads/release/1.0".to_string(),
        }];
        let rejections = check_protected_refs(&pool, repo_id, &updates, std::path::Path::new("."))
            .await
            .unwrap();
        assert_eq!(rejections.len(), 1, "{rejections:?}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unprotected_ref_still_passes_with_stored_pattern(pool: sqlx::PgPool) {
        let repo_id = seed(&pool).await;
        sqlx::query("INSERT INTO protected_refs (repo_id, pattern) VALUES ($1, $2)")
            .bind(repo_id)
            .bind("refs/heads/release/*")
            .execute(&pool)
            .await
            .unwrap();
        let (dir, sibling, _head) = git_repo_with_commits();
        let updates = vec![PushedRef {
            old: "a".repeat(40),
            new: sibling,
            ref_name: "refs/heads/feature".to_string(),
        }];
        let rejections = check_protected_refs(&pool, repo_id, &updates, dir.path())
            .await
            .unwrap();
        assert!(rejections.is_empty(), "{rejections:?}");
    }
}

mod panel_and_routes {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
    }

    fn csrf(cookie: &str) -> String {
        let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "csrf-suite-secret".into());
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(secret.as_bytes(), value)
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

    async fn body_of(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    // Tera escapes `/` as &#x2F;; decode before asserting on ref names
    fn decoded(body: &str) -> String {
        body.replace("&#x2F;", "/").replace("&#x27;", "'")
    }

    async fn session(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        std::env::set_var("SESSION_SECRET", "csrf-suite-secret");
        let response = app(pool.clone(), root)
            .oneshot(
                Request::post("/register")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "email=alice%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        // story: e23s01 — signup no longer sets a session: confirm, then log in
        sqlx::query("UPDATE users SET email_verified_at = now() WHERE handle = 'alice'")
            .execute(pool)
            .await
            .unwrap();
        let response = app(pool.clone(), root)
            .oneshot(
                Request::post("/login")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "email=alice%40example.com&password=correct+horse+battery",
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        cookie_of(&response)
    }

    async fn create_repo(pool: &sqlx::PgPool, root: &tempfile::TempDir, cookie: &str) {
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
        let response = app(pool.clone(), root).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "create repo");
    }

    async fn post_form(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        path: &str,
        cookie: Option<&str>,
        body: String,
    ) -> axum::response::Response {
        let mut request = Request::post(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse().unwrap());
        }
        app(pool.clone(), root).oneshot(request).await.unwrap()
    }

    async fn get(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        path: &str,
        cookie: Option<&str>,
    ) -> String {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse().unwrap());
        }
        let response = app(pool.clone(), root).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "GET {path}");
        body_of(response).await
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn panel_lists_defaults_and_form_for_owner(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = session(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;

        let body = get(&pool, &root, "/alice/demo/operations", Some(&cookie)).await;
        let body = decoded(&body);
        assert!(
            body.contains("<li>refs/heads/trunk</li>"),
            "defaults listed"
        );
        assert!(body.contains("<li>refs/tags/*</li>"), "defaults listed");
        assert!(
            body.contains("/alice/demo/protected-refs"),
            "add form present"
        );
        // no stored rows yet, so no remove affordances
        assert!(
            !body.contains("/protected-refs/remove"),
            "no remove forms without stored rows"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn panel_shows_added_pattern(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = session(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;

        let response = post_form(
            &pool,
            &root,
            "/alice/demo/protected-refs",
            Some(&cookie),
            format!("pattern=refs%2Fheads%2Frelease%2F*&_csrf={}", csrf(&cookie)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "add accepted");

        let body = get(&pool, &root, "/alice/demo/operations", Some(&cookie)).await;
        let body = decoded(&body);
        assert!(
            body.contains("<li>refs/heads/release/*</li>"),
            "pattern listed"
        );
        assert!(
            body.contains("/alice/demo/protected-refs/remove"),
            "remove form for the stored row"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn add_requires_session(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = session(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;

        let response = post_form(
            &pool,
            &root,
            "/alice/demo/protected-refs",
            None,
            "pattern=refs%2Fheads%2Frelease%2F*".into(),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::SEE_OTHER,
            "redirect to login"
        );
        assert_eq!(
            response.headers().get("location").unwrap(),
            "/login",
            "anonymous add is refused"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn add_requires_csrf(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = session(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;

        let response = post_form(
            &pool,
            &root,
            "/alice/demo/protected-refs",
            Some(&cookie),
            "pattern=refs%2Fheads%2Frelease%2F*".into(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "bad csrf");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn remove_requires_csrf(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = session(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;

        let response = post_form(
            &pool,
            &root,
            "/alice/demo/protected-refs/remove",
            Some(&cookie),
            "pattern=refs%2Fheads%2Frelease%2F*".into(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "bad csrf");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn add_then_remove_roundtrip(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = session(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;

        let response = post_form(
            &pool,
            &root,
            "/alice/demo/protected-refs",
            Some(&cookie),
            format!("pattern=refs%2Fheads%2Frelease%2F*&_csrf={}", csrf(&cookie)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "add accepted");

        let response = post_form(
            &pool,
            &root,
            "/alice/demo/protected-refs/remove",
            Some(&cookie),
            format!("pattern=refs%2Fheads%2Frelease%2F*&_csrf={}", csrf(&cookie)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "remove accepted");

        let body = get(&pool, &root, "/alice/demo/operations", Some(&cookie)).await;
        let body = decoded(&body);
        assert!(
            !body.contains("<li>refs/heads/release/*</li>"),
            "pattern gone from panel"
        );
        assert!(body.contains("<li>refs/heads/trunk</li>"), "defaults stay");

        let repo_id: uuid::Uuid = sqlx::query_scalar(
            "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = 'alice' AND r.name = 'demo'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT pattern FROM protected_refs WHERE repo_id = $1")
                .bind(repo_id)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(rows.is_empty(), "row deleted: {rows:?}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn non_owner_cannot_add(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = session(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;

        // bob registers and gets his own session
        std::env::set_var("SESSION_SECRET", "csrf-suite-secret");
        let response = app(pool.clone(), &root)
            .oneshot(
                Request::post("/register")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "email=bob%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        sqlx::query("UPDATE users SET email_verified_at = now() WHERE handle = 'bob'")
            .execute(&pool)
            .await
            .unwrap();
        let response = app(pool.clone(), &root)
            .oneshot(
                Request::post("/login")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "email=bob%40example.com&password=correct+horse+battery",
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let bob = cookie_of(&response);

        let response = post_form(
            &pool,
            &root,
            "/alice/demo/protected-refs",
            Some(&bob),
            format!("pattern=refs%2Fheads%2Frelease%2F*&_csrf={}", csrf(&bob)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "not owner");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn invalid_pattern_is_rejected(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = session(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;

        let response = post_form(
            &pool,
            &root,
            "/alice/demo/protected-refs",
            Some(&cookie),
            format!("pattern=trunk&_csrf={}", csrf(&cookie)),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "pattern must live under refs/"
        );
    }
}
