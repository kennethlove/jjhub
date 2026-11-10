// story: kata at jjhub origin — the SPA must load, route, and fetch at
// jjhub's own origin (/r, /api, /assets) with identity injected; the
// loopback shortcut dead-ends in kata's auth/login redirect loop.
mod origin_spa {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::response::IntoResponse;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    async fn spawn_mock_kata() -> (String, Arc<Mutex<Option<(Vec<(String, String)>, String)>>>) {
        spawn_mock_kata_reply("text/plain", b"kata-ok").await
    }

    // story: e39s01 — a mock that answers with a chosen content type and
    // exact bytes, so HTML injection and non-HTML pass-through can be
    // asserted against the same seam.
    async fn spawn_mock_kata_reply(
        content_type: &'static str,
        body: &'static [u8],
    ) -> (String, Arc<Mutex<Option<(Vec<(String, String)>, String)>>>) {
        let seen = Arc::new(Mutex::new(None));
        let seen2 = seen.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = axum::Router::new().fallback(move |req: axum::extract::Request| {
            let seen = seen2.clone();
            async move {
                let headers: Vec<(String, String)> = req
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect();
                *seen.lock().unwrap() = Some((headers, req.uri().path().to_string()));
                (
                    StatusCode::OK,
                    [(axum::http::header::CONTENT_TYPE, content_type)],
                    body,
                )
                    .into_response()
            }
        });
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{addr}"), seen)
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir, email: &str) -> String {
        std::env::set_var("SESSION_SECRET", "origin-suite");
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "email={email}&password=correct+horse+battery&password_confirm=correct+horse+battery"
            )))
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

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir, upstream: &str) -> axum::Router {
        server::app_with_kata(
            pool,
            root.path().to_str().unwrap().to_string(),
            upstream.to_string(),
            "".to_string(),
        )
    }

    async fn create_repo(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        cookie: &str,
        body: &str,
        upstream: &str,
    ) {
        let secret = std::env::var("SESSION_SECRET").unwrap();
        let value = cookie.strip_prefix("session=").unwrap();
        let token = server::session::csrf_token(secret.as_bytes(), value);
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!("{body}&_csrf={token}")))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        app(pool.clone(), root, upstream)
            .oneshot(request)
            .await
            .unwrap();
    }

    async fn get(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        upstream: &str,
        path: &str,
        cookie: Option<&str>,
        spoof: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse().unwrap());
        }
        if let Some(spoof) = spoof {
            request
                .headers_mut()
                .insert("x-review-author", spoof.parse().unwrap());
        }
        app(pool.clone(), root, upstream)
            .oneshot(request)
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn spa_route_forwarded_with_identity(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (upstream, seen) = spawn_mock_kata().await;
        let cookie = register(&pool, &root, "alice@example.com").await;
        create_repo(
            &pool,
            &root,
            &cookie,
            "name=demo&visibility=public",
            &upstream,
        )
        .await;

        let response = get(
            &pool,
            &root,
            &upstream,
            "/r/alice__demo/browse?change=aaaa",
            Some(&cookie),
            Some("evil@example.com"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let (headers, path) = seen.lock().unwrap().clone().expect("upstream hit");
        assert_eq!(path, "/r/alice__demo/browse", "native kata path forwarded");
        let identity = headers
            .iter()
            .find(|(k, _)| k == "x-review-author")
            .map(|(_, v)| v.clone());
        assert_eq!(identity.as_deref(), Some("alice@example.com"));
        assert!(
            !headers
                .iter()
                .any(|(k, v)| k == "x-review-author" && v == "evil@example.com"),
            "client identity stripped"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn private_repo_hidden_on_spa_route(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (upstream, seen) = spawn_mock_kata().await;
        let alice = register(&pool, &root, "alice@example.com").await;
        create_repo(
            &pool,
            &root,
            &alice,
            "name=secret&visibility=private",
            &upstream,
        )
        .await;
        let bob = register(&pool, &root, "bob@example.com").await;

        let response = get(
            &pool,
            &root,
            &upstream,
            "/r/alice__secret/browse",
            Some(&bob),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(seen.lock().unwrap().is_none(), "upstream not reached");

        let response = get(
            &pool,
            &root,
            &upstream,
            "/r/alice__secret/browse",
            Some(&alice),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn assets_forwarded_without_session(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (upstream, seen) = spawn_mock_kata().await;

        let response = get(&pool, &root, &upstream, "/assets/index-ABC.js", None, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        let (_, path) = seen.lock().unwrap().clone().expect("upstream hit");
        assert_eq!(path, "/assets/index-ABC.js");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn proxied_html_carries_theme_link(pool: sqlx::PgPool) {
        // story: e39s01 — text/html documents from the sidecar wear the
        // injected jjhub theme stylesheet link.
        let root = tempfile::tempdir().unwrap();
        const HTML: &str = r#"<!doctype html><html><head><title>Kata</title><link rel="stylesheet" href="/assets/index-DRn4R2rA.css"></head><body><div id="app"></div></body></html>"#;
        let (upstream, _) =
            spawn_mock_kata_reply("text/html; charset=utf-8", HTML.as_bytes()).await;
        let cookie = register(&pool, &root, "alice@example.com").await;
        create_repo(
            &pool,
            &root,
            &cookie,
            "name=demo&visibility=public",
            &upstream,
        )
        .await;

        let response = get(
            &pool,
            &root,
            &upstream,
            "/r/alice__demo/browse",
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(
            html.contains("/assets/kata-theme.css"),
            "theme stylesheet link injected: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn chrome_stylesheet_carries_header_wrap(pool: sqlx::PgPool) {
        // user report (2026-11-10): the injected header renders
        // `.wrap header-inner`, but app.css's `.wrap` never reaches the
        // review surface, so the chrome lost its inline padding and sat
        // flush to the viewport. The chrome sheet must ship the rule.
        let root = tempfile::tempdir().unwrap();
        let (upstream, _) =
            spawn_mock_kata_reply("text/html; charset=utf-8", b"<html></html>").await;

        let response = get(
            &pool,
            &root,
            &upstream,
            "/assets/kata-chrome.css",
            None,
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let css = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(css.contains(".wrap"), "header wrap rule shipped: {css}");
        assert!(
            css.contains("--space-lg"),
            "wrap padding token defined: {css}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn proxied_html_carries_site_chrome(pool: sqlx::PgPool) {
        // story: e40s03 — session HTML wears the chrome stylesheet link and
        // the rendered site nav right before kata's `#app`; the e39 theme
        // link, the byte-identical guards and the no-session redirect all
        // stay as they were (same suite).
        let root = tempfile::tempdir().unwrap();
        const HTML: &str = r#"<!doctype html><html><head><title>Kata</title><link rel="stylesheet" href="/assets/index-DRn4R2rA.css"></head><body><div id="app"></div></body></html>"#;
        let (upstream, _) =
            spawn_mock_kata_reply("text/html; charset=utf-8", HTML.as_bytes()).await;
        let cookie = register(&pool, &root, "alice@example.com").await;
        create_repo(
            &pool,
            &root,
            &cookie,
            "name=demo&visibility=public",
            &upstream,
        )
        .await;

        let response = get(
            &pool,
            &root,
            &upstream,
            "/r/alice__demo/browse",
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(
            html.contains("/assets/kata-theme.css"),
            "e39 theme link still injected: {html}"
        );
        assert!(
            html.contains("/assets/kata-chrome.css"),
            "chrome stylesheet link injected: {html}"
        );
        assert_eq!(
            html.matches("site-header").count(),
            1,
            "site nav injected exactly once: {html}"
        );
        let app_at = html.find("<div id=\"app\"").expect("#app present");
        let nav_at = html.find("site-header").expect("nav injected");
        assert!(nav_at < app_at, "site nav precedes kata's #app");
        assert!(
            html.contains("action=\"/logout\"") && html.matches("name=\"_csrf\"").count() == 1,
            "nav renders the csrf logout form: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn api_json_passes_through_byte_identical(pool: sqlx::PgPool) {
        // story: e39s01 — API JSON is not HTML: no injection, bytes intact.
        let root = tempfile::tempdir().unwrap();
        const JSON: &[u8] = b"{\"who\":\"alice\",\"n\":42}";
        let (upstream, _) = spawn_mock_kata_reply("application/json", JSON).await;
        let cookie = register(&pool, &root, "alice@example.com").await;
        create_repo(
            &pool,
            &root,
            &cookie,
            "name=demo&visibility=public",
            &upstream,
        )
        .await;

        let response = get(
            &pool,
            &root,
            &upstream,
            "/api/repos/alice__demo/browse/log",
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), JSON, "JSON bytes untouched");
        assert!(!bytes.windows(14).any(|w| w == b"kata-theme"));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn asset_bytes_pass_through_byte_identical(pool: sqlx::PgPool) {
        // story: e39s01 — asset bytes (non-UTF8 payload) stay untouched.
        let root = tempfile::tempdir().unwrap();
        const ASSET: &[u8] = b"\x00\x01\xff\xfe export const x = \"\\u00e9\";\x80";
        let (upstream, _) = spawn_mock_kata_reply("application/javascript", ASSET).await;

        let response = get(&pool, &root, &upstream, "/assets/index-ABC.js", None, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), ASSET, "asset bytes untouched");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn api_repo_scoped_requires_session(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (upstream, seen) = spawn_mock_kata().await;
        let cookie = register(&pool, &root, "alice@example.com").await;
        create_repo(
            &pool,
            &root,
            &cookie,
            "name=demo&visibility=public",
            &upstream,
        )
        .await;

        // no session: refused, upstream untouched
        let response = get(
            &pool,
            &root,
            &upstream,
            "/api/repos/alice__demo/browse/log",
            None,
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert!(seen.lock().unwrap().is_none());

        // with session: forwarded with identity
        let response = get(
            &pool,
            &root,
            &upstream,
            "/api/repos/alice__demo/browse/log",
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let (headers, path) = seen.lock().unwrap().clone().expect("upstream hit");
        assert_eq!(path, "/api/repos/alice__demo/browse/log");
        assert!(headers.iter().any(|(k, _)| k == "x-review-author"));
    }
}
