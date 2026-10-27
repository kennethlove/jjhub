// story: e25s04
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

mod settings_delete {
    use super::*;

    fn csrf(cookie: &str) -> String {
        let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "settings-suite".into());
        let value = cookie
            .split(';')
            .next()
            .unwrap()
            .strip_prefix("session=")
            .unwrap_or(cookie);
        server::session::csrf_token(secret.as_bytes(), value)
    }

    async fn post(
        pool: sqlx::PgPool,
        path: &str,
        body: &str,
        cookie: &str,
    ) -> axum::response::Response {
        let mut request = Request::post(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!("{body}&_csrf={}", csrf(cookie))))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        server::app(pool).oneshot(request).await.unwrap()
    }

    async fn get(pool: sqlx::PgPool, path: &str, cookie: &str) -> axum::response::Response {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        server::app(pool).oneshot(request).await.unwrap()
    }

    async fn text(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn set_cookie(response: &axum::response::Response, name: &str) -> String {
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter(|v| v.starts_with(&format!("{name}=")))
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn location(response: &axum::response::Response) -> String {
        response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    async fn register(pool: &sqlx::PgPool, email: &str) -> String {
        std::env::set_var("SESSION_SECRET", "settings-suite");
        let response = post(
            pool.clone(),
            "/register",
            &format!(
                "email={email}&password=correct+horse+battery&\
                 password_confirm=correct+horse+battery"
            ),
            "",
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

    #[sqlx::test]
    async fn wrong_password_refuses_and_keeps_the_confirm_step(
        pool: sqlx::PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let session = register(&pool, "del%40example.com").await;

        let response = post(
            pool.clone(),
            "/settings/account",
            "password=definitely+wrong",
            &session,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            location(&response),
            "/settings#delete-2",
            "back to step two, state still open"
        );
        let del_err = set_cookie(&response, "del_err");
        assert!(!del_err.is_empty(), "one-shot error is set");
        assert!(!del_err.contains("Max-Age=0"), "and it is live");

        let page = text(get(pool, "/settings", &session).await).await;
        assert!(page.contains("Are you sure?"), "account survives");
        Ok(())
    }

    #[sqlx::test]
    async fn correct_password_deletes_the_account_and_ends_the_session(
        pool: sqlx::PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let session = register(&pool, "del%40example.com").await;

        let response = post(
            pool.clone(),
            "/settings/account",
            "password=correct+horse+battery",
            &session,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&response), "/", "lands on the home page");
        assert!(
            set_cookie(&response, "session").contains("Max-Age=0"),
            "session cookie cleared"
        );

        let res = get(pool.clone(), "/settings", &session).await;
        assert_ne!(res.status(), StatusCode::OK, "old session rejected");

        let gone: i64 =
            sqlx::query_scalar("SELECT count(*) FROM users WHERE email = 'del@example.com'::citext")
                .fetch_one(&pool)
                .await?;
        assert_eq!(gone, 0, "user row deleted");
        Ok(())
    }

    #[sqlx::test]
    async fn deletion_cascades_the_users_tokens(
        pool: sqlx::PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let session = register(&pool, "del%40example.com").await;
        let minted = post(pool.clone(), "/settings/tokens", "label=minted", &session).await;
        assert_eq!(minted.status(), StatusCode::SEE_OTHER);

        let response = post(
            pool.clone(),
            "/settings/account",
            "password=correct+horse+battery",
            &session,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let tokens: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM tokens t
             JOIN users u ON u.id = t.user_id
             WHERE u.email = 'del@example.com'::citext",
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(tokens, 0, "tokens cascade with the user");
        Ok(())
    }
}
