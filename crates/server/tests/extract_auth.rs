// story: e15s02 — the register, login, and settings pages render from
// templates, not from Rust string literals
mod extract_auth {
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
        ctx
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
        std::env::set_var("SESSION_SECRET", "auth-suite-secret");
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
    async fn register_form_matches_template(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let html = get(pool, &root, "/register", None).await;
        let mut expected = ctx("", "");
        expected.insert("error", "");
        expected.insert("email", "");
        assert_eq!(html, server::render::render("register.html", &expected));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn login_form_matches_template(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let html = get(pool, &root, "/login", None).await;
        let mut expected = ctx("", "");
        expected.insert("error", "");
        assert_eq!(html, server::render::render("login.html", &expected));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn settings_matches_template(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(pool.clone(), &root).await;
        let secret = std::env::var("SESSION_SECRET").unwrap();
        let value = cookie.strip_prefix("session=").unwrap_or(&cookie);
        let csrf = server::session::csrf_token(secret.as_bytes(), value);
        let html = get(pool.clone(), &root, "/settings", Some(&cookie)).await;
        // story: e21s01 — signup mints the account's token, so the
        // list renders it; context mirrors the handler's query shape
        let token_items: Vec<serde_json::Value> = sqlx::query_as::<_, (String, String)>(
            "SELECT label, prefix FROM tokens
                 WHERE user_id = (SELECT id FROM users WHERE handle = 'alice')
                   AND revoked_at IS NULL
                 ORDER BY created_at",
        )
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|(label, prefix)| serde_json::json!({ "label": label, "prefix": prefix }))
        .collect();
        let mut expected = ctx("alice", &csrf);
        expected.insert("email", "alice@example.com");
        expected.insert("display_name", "alice");
        expected.insert("checked_system", "checked");
        expected.insert("checked_light", "");
        expected.insert("checked_dark", "");
        expected.insert("new_token", "");
        expected.insert("tokens", &token_items);
        assert_eq!(html, server::render::render("settings.html", &expected));
    }
}
