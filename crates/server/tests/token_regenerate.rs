// story: e21s03 — regenerate rotates the account token: revoke the
// old + issue the new in one operation, show the plaintext once,
// and the old token stops authenticating immediately.

mod token_regenerate {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

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
                .insert("cookie", cookie.parse().unwrap());
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

    fn flash_cookie(response: &axum::response::Response, name: &str) -> Option<String> {
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .find_map(|v| {
                let s = v.to_str().ok()?;
                s.strip_prefix(&format!("{name}="))
                    .map(|rest| rest.split(';').next().unwrap().to_string())
            })
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn regenerate_rotates_and_kills_the_old_token(pool: sqlx::PgPool) {
        let cookie = register(&pool).await;

        // a working account token exists (signup mint) — take a known
        // plaintext through the normal mint route so we hold the old value
        let minted = post(
            pool.clone(),
            "/settings/tokens",
            "label=laptop",
            Some(&cookie),
        )
        .await;
        assert_eq!(minted.status(), StatusCode::SEE_OTHER);
        let old = flash_cookie(&minted, "new_token").expect("old plaintext shown once");
        assert!(server::token::authenticate(&pool, &old).await.is_some());

        // rotate: one operation, new plaintext shown once
        let rotated = post(pool.clone(), "/settings/regenerate", "", Some(&cookie)).await;
        assert_eq!(rotated.status(), StatusCode::SEE_OTHER);
        let location = rotated.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(
            location, "/settings",
            "rotation lands on the copy-once banner"
        );
        let new = flash_cookie(&rotated, "new_token").expect("new plaintext shown once");
        assert_ne!(new, old, "a replacement, not the same value");

        // old token fails authentication immediately; new one works
        assert!(
            server::token::authenticate(&pool, &old).await.is_none(),
            "old token must be dead after rotation"
        );
        assert!(
            server::token::authenticate(&pool, &new).await.is_some(),
            "new token authenticates immediately"
        );

        // exactly one active token after rotation (revoke+issue atomic)
        let active: i64 =
            sqlx::query_scalar("SELECT count(*) FROM tokens WHERE revoked_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(active, 1, "rotation leaves one active token");

        // the copy-once banner on settings carries the new value
        let settings = axum::body::to_bytes(
            server::app(pool.clone())
                .oneshot({
                    let mut r = Request::get("/settings").body(Body::empty()).unwrap();
                    r.headers_mut().insert(
                        "cookie",
                        format!("{cookie}; new_token={new}")
                            .parse::<axum::http::HeaderValue>()
                            .unwrap(),
                    );
                    r
                })
                .await
                .unwrap()
                .into_body(),
            usize::MAX,
        )
        .await
        .unwrap();
        let html = String::from_utf8_lossy(&settings);
        assert!(
            html.contains(&new),
            "settings banner shows the new token once"
        );

        // regenerating again rotates again: the previous new token dies
        let again = post(pool.clone(), "/settings/regenerate", "", Some(&cookie)).await;
        assert_eq!(again.status(), StatusCode::SEE_OTHER);
        let newer = flash_cookie(&again, "new_token").expect("second rotation shows a token");
        assert_ne!(newer, new);
        assert!(server::token::authenticate(&pool, &new).await.is_none());
        assert!(server::token::authenticate(&pool, &newer).await.is_some());
    }
}
