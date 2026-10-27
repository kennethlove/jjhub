// story: e01s02
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

mod token_crud {
    use super::*;

    fn csrf(cookie: &str) -> String {
        let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "tokens-suite".into());
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(secret.as_bytes(), value)
    }

    async fn post(
        pool: sqlx::PgPool,
        path: &str,
        body: &str,
        cookie: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::post(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "{body}&_csrf={}",
                csrf(cookie.unwrap_or(""))
            )))
            .unwrap();
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                0,
            ))));
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse::<axum::http::HeaderValue>().unwrap());
        }
        server::app(pool).oneshot(request).await.unwrap()
    }

    async fn register(pool: &sqlx::PgPool) -> String {
        std::env::set_var("SESSION_SECRET", "tokens-suite");
        let response = post(
            pool.clone(),
            "/register",
            "email=bob%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
            None,
        )
        .await;
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

    async fn get(pool: sqlx::PgPool, path: &str, cookie: &str) -> axum::response::Response {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse::<axum::http::HeaderValue>().unwrap());
        server::app(pool).oneshot(request).await.unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn settings_page_has_token_form(pool: sqlx::PgPool) {
        let cookie = register(&pool).await;
        let response = get(pool, "/settings", &cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(
            html.contains("token"),
            "settings must have token UI: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn token_shown_once_then_prefix_only(pool: sqlx::PgPool) {
        let cookie = register(&pool).await;
        let response = post(
            pool.clone(),
            "/settings/tokens",
            "label=laptop",
            Some(&cookie),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let set_cookie = response
            .headers()
            .get("set-cookie")
            .expect("new token flash cookie")
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            set_cookie.contains("new_token="),
            "full token delivered once: {set_cookie}"
        );

        let response = get(pool, "/settings", &cookie).await;
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(html.contains("laptop"), "token label listed");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn create_repo_token_route_and_disclosure(pool: sqlx::PgPool) {
        // story: e21s03 — the repo-page mint form moves behind an
        // "Advanced" disclosure; the route itself is unchanged.
        let cookie = register(&pool).await;
        // absolute temp storage root — default ./data/repos breaks jj
        // workspace init with a relative store path
        let _root = tempfile::tempdir().unwrap();
        let app =
            server::app_with_storage(pool.clone(), _root.path().to_str().unwrap().to_string());

        let mut created_request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("cookie", cookie.parse::<axum::http::HeaderValue>().unwrap())
            .body(Body::from(format!(
                "name=snippet&visibility=private&_csrf={}",
                csrf(&cookie)
            )))
            .unwrap();
        created_request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                0,
            ))));
        let created = app.clone().oneshot(created_request).await.unwrap();
        let created_status = created.status();
        let created_body = String::from_utf8_lossy(
            &axum::body::to_bytes(created.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .to_string();
        assert_eq!(
            created_status,
            StatusCode::SEE_OTHER,
            "create repo failed: {created_body}"
        );

        let page_request = Request::get("/bob/snippet")
            .header("cookie", cookie.parse::<axum::http::HeaderValue>().unwrap())
            .body(Body::empty())
            .unwrap();
        let page = app.clone().oneshot(page_request).await.unwrap();
        assert_eq!(page.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(page.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8_lossy(&bytes);
        let details = html.find("<details").expect("disclosure present");
        let summary = html
            .find("<summary>Advanced</summary>")
            .expect("advanced summary present");
        let form = html
            .find(r#"action="/bob/snippet/tokens""#)
            .expect("mint form still rendered");
        let close = html.find("</details>").expect("disclosure closes");
        assert!(
            details < summary && summary < form && form < close,
            "form must sit inside the closed Advanced disclosure: {html}"
        );

        // route unchanged: the mint still works from inside the disclosure
        let mut mint_request = Request::post("/bob/snippet/tokens")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("cookie", cookie.parse::<axum::http::HeaderValue>().unwrap())
            .body(Body::from(format!(
                "label=repo-mint&_csrf={}",
                csrf(&cookie)
            )))
            .unwrap();
        mint_request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                0,
            ))));
        let mint = app.clone().oneshot(mint_request).await.unwrap();
        assert_eq!(mint.status(), StatusCode::SEE_OTHER);
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM tokens WHERE label = 'repo-mint'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 1, "route and its POST contract unchanged");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn revoked_token_stops_authenticating(pool: sqlx::PgPool) {
        let (plaintext, token_id) = server::token::insert_test_token(&pool).await;
        assert!(server::token::authenticate(&pool, &plaintext, None)
            .await
            .is_some());
        server::token::revoke(&pool, token_id).await;
        assert!(server::token::authenticate(&pool, &plaintext, None)
            .await
            .is_none());
    }
}
