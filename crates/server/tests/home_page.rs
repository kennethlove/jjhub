// story: e06s01
mod home_page {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn csrf(cookie: &str) -> String {
        let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "home-suite-secret".into());
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
        std::env::set_var("SESSION_SECRET", "home-suite-secret");
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
    async fn logged_out_landing_offers_register_and_login(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let html = get(pool, &root, "/", None).await;
        assert!(html.contains("/register"), "register link: {html}");
        assert!(html.contains("/login"), "login link: {html}");
        // no create form for visitors
        assert!(
            !html.contains("action=\"/repos\""),
            "no create form logged out"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn logged_in_home_shows_form_and_repos(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(pool.clone(), &root).await;
        // create a repo so the list is non-empty
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
        assert!(html.contains("action=\"/repos\""), "create form: {html}");
        assert!(html.contains("/alice/demo"), "owned repo listed: {html}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn logged_in_home_carries_the_shared_frame(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(pool.clone(), &root).await;
        // owned repo exists so the dashboard list has content
        let mut create = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "name=demo&visibility=public&_csrf={}",
                csrf(&cookie)
            )))
            .unwrap();
        create
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(create)
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "create repo");
        let html = get(pool, &root, "/", Some(&cookie)).await;
        // story: e31 — the dashboard branch sits in the same frame as every
        // other page: site nav first, one wrap around the content, footer last.
        let nav = html.find("site-header").unwrap_or(usize::MAX);
        let main = html.find("<main").unwrap_or(usize::MAX);
        assert!(nav < main, "site nav renders before main: {html}");
        assert!(
            html.contains("class=\"wrap dash-home\""),
            "dashboard content sits in a wrap: {html}"
        );
        assert!(html.contains("site-footer"), "site footer present: {html}");
        // pinned dashboard content survives the reframe
        assert!(html.contains("action=\"/repos\""), "create form: {html}");
        assert!(html.contains("/alice/demo"), "owned repo listed: {html}");
    }
}
