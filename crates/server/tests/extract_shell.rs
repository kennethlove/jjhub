// story: e15s02 — the shell, nav, and home pages render from templates,
// not from Rust string literals
mod extract_shell {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tera::Context;
    use tower::ServiceExt;

    fn ctx(user_handle: &str, csrf: &str) -> Context {
        let mut ctx = Context::new();
        ctx.insert("user_handle", user_handle);
        ctx.insert("csrf", csrf);
        ctx.insert("theme", "system"); // e21s02: base.html declares theme
        ctx.insert("repos", &Vec::<serde_json::Value>::new());
        ctx
    }

    #[test]
    fn nav_renders_from_template() {
        assert_eq!(
            server::routes::nav_html(None, ""),
            server::render::render("nav.html", &ctx("", ""))
        );
        assert_eq!(
            server::routes::nav_html(Some("alice"), "tok"),
            server::render::render("nav.html", &ctx("alice", "tok"))
        );
    }

    fn csrf(cookie: &str) -> String {
        let secret =
            std::env::var("SESSION_SECRET").unwrap_or_else(|_| "shell-suite-secret".into());
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(secret.as_bytes(), value)
    }

    async fn get(
        pool: sqlx::PgPool,
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
        let response = server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    async fn register(pool: sqlx::PgPool, root: &tempfile::TempDir) -> String {
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

    #[sqlx::test(migrations = "./migrations")]
    async fn home_anon_matches_template(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let html = get(pool, &root, "/", None).await;
        assert_eq!(html, server::render::render("home.html", &ctx("", "")));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn home_authed_matches_template(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(pool.clone(), &root).await;
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "name=demo&visibility=public&_csrf={}",
                csrf(&cookie)
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
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "create repo");

        let html = get(pool, &root, "/", Some(&cookie)).await;
        let repos = vec![serde_json::json!({
            "owner": "alice",
            "name": "demo",
            "visibility": "public"
        })];
        let mut expected = ctx("alice", &csrf(&cookie));
        expected.insert("repos", &repos);
        assert_eq!(html, server::render::render("home.html", &expected));
    }
}
