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
                "email=alice%40example.com&password=correct+horse+battery",
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
