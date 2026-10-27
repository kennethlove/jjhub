// story: e25s03
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

mod settings_revoke {
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

    fn location(response: &axum::response::Response) -> String {
        response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    #[sqlx::test]
    async fn revoke_kills_the_row_and_the_token(
        pool: sqlx::PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let session = register(&pool, "alice%40example.com").await;
        let minted = post(pool.clone(), "/settings/tokens", "label=laptop", &session).await;
        assert_eq!(minted.status(), StatusCode::SEE_OTHER);

        let page = text(get(pool.clone(), "/settings", &session).await).await;
        let id = page
            .split("data-token-id=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("ledger carries the token id")
            .to_string();

        let response = post(
            pool.clone(),
            &format!("/settings/tokens/{id}/revoke"),
            "",
            &session,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&response), "/settings", "back to the resting list");

        let revoked_at: Option<chrono::DateTime<chrono::Utc>> =
            sqlx::query_scalar("SELECT revoked_at FROM tokens WHERE id = $1::uuid")
                .bind(&id)
                .fetch_optional(&pool)
                .await?
                .flatten();
        assert!(revoked_at.is_some(), "row marked revoked");

        let after = text(get(pool, "/settings", &session).await).await;
        assert!(
            !after.contains(&format!("data-token-id=\"{id}\"")),
            "row disappears from the ledger"
        );
        Ok(())
    }

    #[sqlx::test]
    async fn one_account_cannot_revoke_anothers_token(
        pool: sqlx::PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let alice = register(&pool, "alice%40example.com").await;
        let minted = post(pool.clone(), "/settings/tokens", "label=ci", &alice).await;
        assert_eq!(minted.status(), StatusCode::SEE_OTHER);
        let page = text(get(pool.clone(), "/settings", &alice).await).await;
        let id = page
            .split("data-token-id=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("ledger carries the token id")
            .to_string();

        let mallory = register(&pool, "mallory%40example.com").await;
        let response = post(
            pool.clone(),
            &format!("/settings/tokens/{id}/revoke"),
            "",
            &mallory,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "other accounts are not even told it exists"
        );

        let revoked_at: Option<chrono::DateTime<chrono::Utc>> =
            sqlx::query_scalar("SELECT revoked_at FROM tokens WHERE id = $1::uuid")
                .bind(&id)
                .fetch_optional(&pool)
                .await?
                .flatten();
        assert!(revoked_at.is_none(), "token still active");
        Ok(())
    }

    #[sqlx::test]
    async fn unknown_id_is_404(
        pool: sqlx::PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let session = register(&pool, "alice%40example.com").await;
        let response = post(
            pool,
            "/settings/tokens/00000000-0000-4000-8000-000000000000/revoke",
            "",
            &session,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        Ok(())
    }
}
