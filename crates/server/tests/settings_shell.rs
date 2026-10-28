// story: e25s01
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

mod settings_shell {
    use super::*;

    fn csrf(cookie: &str) -> String {
        let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "settings-suite".into());
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
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

    async fn register(pool: &sqlx::PgPool) -> String {
        std::env::set_var("SESSION_SECRET", "settings-suite");
        let response = post(
            pool.clone(),
            "/register",
            "email=alice%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
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
    async fn settings_renders_the_designed_panels(
        pool: sqlx::PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cookie = register(&pool).await;
        let response = get(pool.clone(), "/settings", &cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = text(response).await;

        // Account panel: read-only rows and the password state link.
        assert!(body.contains("<h1>Your preferences</h1>"), "page h1");
        assert!(body.contains(">Email<"), "email row");
        assert!(
            body.contains("Your handle appears in URLs"),
            "handle helper"
        );
        assert!(body.contains(">Member since<"), "member-since row");
        assert!(
            body.contains(r##"href="#password""##),
            "change-password link"
        );
        assert!(body.contains(r#"id="password""#), "password state");
        assert!(
            body.contains(r#"action="/settings/password""#),
            "password form"
        );

        // Kept features: profile form and regenerate button (user decision).
        assert!(
            body.contains(r#"action="/settings/profile""#),
            "profile form"
        );
        assert!(body.contains(r#"name="theme""#), "theme radios");
        assert!(
            body.contains(r#"action="/settings/regenerate""#),
            "regenerate"
        );

        // Tokens panel: generate form, reveal state, ledger columns.
        assert!(
            body.contains(r#"aria-labelledby="tokens-heading""#),
            "tokens panel"
        );
        assert!(body.contains(r#"id="token-new""#), "reveal state");
        assert!(
            body.contains(r#"action="/settings/tokens""#),
            "generate form"
        );
        assert!(body.contains(">Prefix<"), "ledger prefix column");
        assert!(body.contains(">Created<"), "ledger created column");

        // Danger zone is last; both confirm states are :target links.
        let tokens = body
            .find(r#"aria-labelledby="tokens-heading""#)
            .expect("tokens panel present");
        let danger = body
            .find(r#"aria-labelledby="danger-heading""#)
            .expect("danger panel present");
        assert!(tokens < danger, "danger zone comes last");
        assert!(body.contains(r##"href="#delete-1""##), "delete step 1 link");
        assert!(
            body.contains(r#"action="/settings/account""#),
            "delete form"
        );
        assert!(
            body.matches("No, cancel").count() >= 2,
            "both confirm states cancel"
        );
        assert!(
            body.contains("0 repositories"),
            "repo count in confirm copy"
        );
        assert!(
            body.contains(r#"aria-current="page""#),
            "nav marks Settings current"
        );

        // Member since renders as an ISO date.
        assert!(
            body.contains(r#"class="dd-sm">20"#),
            "member-since value rendered"
        );

        Ok(())
    }

    #[sqlx::test]
    async fn profile_form_saves_display_name_and_theme(
        pool: sqlx::PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cookie = register(&pool).await;
        let response = post(
            pool.clone(),
            "/settings/profile",
            "display_name=Alice+Doe&theme=dark",
            &cookie,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers().get("location").unwrap(),
            "/settings",
            "redirects back"
        );

        let body = text(get(pool, "/settings", &cookie).await).await;
        assert!(body.contains(r#"value="Alice Doe""#), "display name echo");
        assert!(
            body.contains(r#"name="theme" value="dark" checked"#),
            "dark theme checked"
        );
        Ok(())
    }

    #[sqlx::test]
    async fn regenerate_revokes_every_token_and_hands_out_one(
        pool: sqlx::PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cookie = register(&pool).await;
        for label in ["one", "two"] {
            let response = post(
                pool.clone(),
                "/settings/tokens",
                &format!("label={label}"),
                &cookie,
            )
            .await;
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
        }
        let active_before: i64 =
            sqlx::query_scalar("SELECT count(*) FROM tokens WHERE revoked_at IS NULL")
                .fetch_one(&pool)
                .await?;
        // story: e21s01 — signup already minted the account token
        assert_eq!(active_before, 3, "signup token plus the two mints");

        let response = post(pool.clone(), "/settings/regenerate", "", &cookie).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let set_cookie = response
            .headers()
            .get("set-cookie")
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        assert!(
            set_cookie.contains("new_token="),
            "replacement plaintext rides the one-time cookie"
        );

        let active_after: i64 =
            sqlx::query_scalar("SELECT count(*) FROM tokens WHERE revoked_at IS NULL")
                .fetch_one(&pool)
                .await?;
        let revoked_after: i64 =
            sqlx::query_scalar("SELECT count(*) FROM tokens WHERE revoked_at IS NOT NULL")
                .fetch_one(&pool)
                .await?;
        assert_eq!(active_after, 1, "exactly the replacement stays active");
        assert_eq!(revoked_after, 3, "every pre-rotation token is revoked");

        let body = text(get(pool, "/settings", &cookie).await).await;
        assert!(
            !body.contains(">one<") && !body.contains(">two<"),
            "ledger lists only the active replacement"
        );
        Ok(())
    }
}
