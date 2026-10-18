// story: e05s03
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

mod review_proxy_auth {
    use super::*;
    use axum::response::IntoResponse;
    use std::sync::{Arc, Mutex};

    /// (captured request headers, path) from the mock sidecar.
    type Seen = Arc<Mutex<Option<(Vec<(String, String)>, String)>>>;

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir, upstream: &str) -> axum::Router {
        server::app_with_kata(
            pool,
            root.path().to_str().unwrap().to_string(),
            upstream.to_string(),
            "http://127.0.0.1:7878".to_string(),
        )
    }

    /// Mock kata: records the last request's headers+path, returns 200.
    async fn spawn_mock_kata() -> (String, Seen) {
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
                (StatusCode::OK, "kata-ok").into_response()
            }
        });
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{addr}"), seen)
    }

    fn csrf(cookie: &str) -> String {
        let secret =
            std::env::var("SESSION_SECRET").unwrap_or_else(|_| "review-proxy-suite".into());
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(secret.as_bytes(), value)
    }

    async fn register(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        upstream: &str,
        email: &str,
    ) -> String {
        std::env::set_var("SESSION_SECRET", "review-proxy-suite");
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "email={email}&password=correct+horse+battery&password_confirm=correct+horse+battery"
            )))
            .unwrap();
        let response = app(pool.clone(), root, upstream)
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

    async fn create_repo(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        upstream: &str,
        cookie: &str,
        body: &str,
    ) {
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!("{body}&_csrf={}", csrf(cookie))))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response = app(pool.clone(), root, upstream)
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    async fn review_get(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        upstream: &str,
        path: &str,
        cookie: Option<&str>,
        forged_trust: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse().unwrap());
        }
        if let Some(trust) = forged_trust {
            request
                .headers_mut()
                .insert("x-kata-trust", trust.parse().unwrap());
        }
        app(pool, root, upstream).oneshot(request).await.unwrap()
    }

    fn upstream_header(seen: &Seen, name: &str) -> Option<String> {
        seen.lock().unwrap().as_ref().and_then(|(headers, _)| {
            headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        })
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unauthenticated_is_login_redirect(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (upstream, _seen) = spawn_mock_kata().await;
        let response = review_get(pool, &root, &upstream, "/review/alice/demo/", None, None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            location.starts_with("/login"),
            "redirect to login: {location}"
        );
    }

    // story: e05s03 — spoofed trust header from unauthenticated client is
    // ignored (login redirect), never forwarded.
    #[sqlx::test(migrations = "./migrations")]
    async fn spoofed_trust_header_rejected(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (upstream, seen) = spawn_mock_kata().await;
        let response = review_get(
            pool,
            &root,
            &upstream,
            "/review/alice/demo/",
            None,
            Some("evil@example.com"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert!(
            seen.lock().unwrap().is_none(),
            "upstream must never see an unauthenticated request"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn logged_in_reaches_public_repo(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (upstream, seen) = spawn_mock_kata().await;
        let cookie = register(&pool, &root, &upstream, "alice@example.com").await;
        create_repo(
            &pool,
            &root,
            &upstream,
            &cookie,
            "name=demo&visibility=public",
        )
        .await;

        // client forges the identity header; the proxy must replace it
        let response = review_get(
            pool,
            &root,
            &upstream,
            "/review/alice/demo/files",
            Some(&cookie),
            Some("evil@example.com"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"kata-ok");
        assert_eq!(
            upstream_header(&seen, "x-review-author").as_deref(),
            Some("alice@example.com"),
            "proxy sets the authenticated user's identity"
        );
        let (headers, path) = seen.lock().unwrap().clone().unwrap();
        assert_eq!(path, "/files", "rest path forwarded");
        assert!(
            !headers
                .iter()
                .any(|(k, v)| k == "x-review-author" && v == "evil@example.com"),
            "client identity header stripped"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn private_repo_hidden_from_strangers(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (upstream, seen) = spawn_mock_kata().await;
        let alice = register(&pool, &root, &upstream, "alice@example.com").await;
        create_repo(
            &pool,
            &root,
            &upstream,
            &alice,
            "name=secret&visibility=private",
        )
        .await;
        let bob = register(&pool, &root, &upstream, "bob@example.com").await;

        let response = review_get(
            pool.clone(),
            &root,
            &upstream,
            "/review/alice/secret/",
            Some(&bob),
            None,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "no existence leak for private repo"
        );
        assert!(seen.lock().unwrap().is_none(), "upstream not reached");

        // owner still reaches it
        let response = review_get(
            pool,
            &root,
            &upstream,
            "/review/alice/secret/",
            Some(&alice),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    // story: e05s03 — token clients (transport token) get 401 JSON, not a
    // browser redirect.
    #[sqlx::test(migrations = "./migrations")]
    async fn unknown_repo_is_404(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (upstream, _seen) = spawn_mock_kata().await;
        let cookie = register(&pool, &root, &upstream, "alice@example.com").await;
        let response = review_get(
            pool,
            &root,
            &upstream,
            "/review/nobody/nope/",
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
