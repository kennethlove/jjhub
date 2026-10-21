// story: e06s01
mod site_nav {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        std::env::set_var("SESSION_SECRET", "shell-suite-secret");
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "email=alice%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
            ))
            .unwrap();
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
                .await
                .unwrap();
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
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn nav_present_on_settings_and_repo_pages(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        // create repo for the detail page
        let secret = std::env::var("SESSION_SECRET").unwrap();
        let value = cookie.strip_prefix("session=").unwrap();
        let token = server::session::csrf_token(secret.as_bytes(), value);
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "name=demo&visibility=public&_csrf={token}"
            )))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap();

        for path in ["/settings", "/alice/demo"] {
            let html = get(&pool, &root, path, Some(&cookie)).await;
            assert!(
                html.contains("<nav>") && html.contains("href=\"/settings\""),
                "nav on {path}: {html}"
            );
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn repo_page_shows_origin_snippets(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        let secret = std::env::var("SESSION_SECRET").unwrap();
        let value = cookie.strip_prefix("session=").unwrap();
        let token = server::session::csrf_token(secret.as_bytes(), value);
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "name=demo&visibility=public&_csrf={token}"
            )))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap();

        let html = get(&pool, &root, "/alice/demo", Some(&cookie)).await;
        assert!(
            html.contains("jj git remote add origin"),
            "remote add snippet: {html}"
        );
        assert!(
            html.contains("x-access-token:&lt;TOKEN&gt;"),
            "token placeholder, never a real token: {html}"
        );
        assert!(
            html.contains("/git/alice/demo.git"),
            "clone path present: {html}"
        );
    }
}

// story: e12s02 — every shell page carries the token stylesheet link.
mod site_shell_css {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const LINK: &str = "<link rel=\"stylesheet\" href=\"/assets/app.css\">";

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        std::env::set_var("SESSION_SECRET", "shell-suite-secret");
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "email=alice%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
            ))
            .unwrap();
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
                .await
                .unwrap();
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
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn site_shell_css_public_pages_link_stylesheet(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        for path in ["/", "/login", "/register"] {
            let html = get(&pool, &root, path, None).await;
            assert!(html.contains(LINK), "stylesheet link on {path}: {html}");
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn site_shell_css_settings_page_links_stylesheet(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        let html = get(&pool, &root, "/settings", Some(&cookie)).await;
        assert!(html.contains(LINK), "stylesheet link on /settings: {html}");
    }
}

// story: e12s02 — repo, tree, blob, and MR pages migrated to tokens:
// stylesheet link (MR pages are standalone documents, so their heads get
// the link too) and token component classes, with no inline styles left.
mod site_repo_token_pages {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const LINK: &str = "<link rel=\"stylesheet\" href=\"/assets/app.css\">";

    fn seed(root: &std::path::Path) -> std::path::PathBuf {
        let bare = root.join("bare.git");
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .current_dir(&seed)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "--bare", bare.to_str().unwrap()]);
        git(&["init", "-b", "trunk"]);
        std::fs::write(seed.join("README.md"), "# hello\n").unwrap();
        git(&["add", "."]);
        git(&[
            "-c",
            "user.email=a@e.com",
            "-c",
            "user.name=A",
            "commit",
            "-m",
            "one",
        ]);
        git(&["push", bare.to_str().unwrap(), "trunk"]);
        bare
    }

    async fn mount(pool: &sqlx::PgPool, root: &tempfile::TempDir) {
        let bare = seed(root.path());
        let target = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::rename(&bare, &target).unwrap();
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        ).fetch_one(pool).await.unwrap();
        sqlx::query("INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo')")
            .bind(user_id)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn get(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        path: &str,
    ) -> (StatusCode, String) {
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
        let status = response.status();
        let body =
            String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
                .into_owned();
        (status, body)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn site_repo_tree_blob_pages_use_token_classes(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root).await;

        let (status, repo) = get(&pool, &root, "/alice/demo").await;
        assert_eq!(status, StatusCode::OK);
        assert!(repo.contains(LINK), "repo link: {repo}");
        assert!(repo.contains("class=\"code-block\""), "repo code: {repo}");
        assert!(repo.contains("class=\"mr-list\""), "repo MR list: {repo}");
        assert!(!repo.contains("style="), "no inline styles: {repo}");

        let (status, tree) = get(&pool, &root, "/alice/demo/tree").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            tree.contains("class=\"file-list\""),
            "tree file list: {tree}"
        );

        let (status, blob) = get(&pool, &root, "/alice/demo/blob/trunk/README.md").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            blob.contains("class=\"code-block\""),
            "blob code block: {blob}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn site_mr_pages_standalone_docs_link_stylesheet(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root).await;

        let (status, list) = get(&pool, &root, "/alice/demo/merge-requests").await;
        assert_eq!(status, StatusCode::OK);
        assert!(list.contains(LINK), "MR list link: {list}");
        assert!(list.contains("class=\"mr-stack\""), "MR stack list: {list}");

        let mut context = tera::Context::new();
        context.insert("owner", "alice");
        context.insert("name", "demo");
        context.insert("change_id", "abc123");
        context.insert("mr_state", "open");
        context.insert("csrf", "token");
        context.insert("badges", &Vec::<String>::new());
        context.insert("sections", &Vec::<String>::new());
        context.insert("review_href", &Option::<String>::None);
        let detail = server::render::render("mr_detail.html", &context);
        assert!(detail.contains(LINK), "MR detail link: {detail}");
        assert!(detail.contains("<body>"), "MR detail document: {detail}");
    }
}
