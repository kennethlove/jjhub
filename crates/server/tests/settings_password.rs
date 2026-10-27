// story: e25s02
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

mod settings_password {
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

    async fn register(pool: &sqlx::PgPool) -> String {
        std::env::set_var("SESSION_SECRET", "settings-suite");
        let response = post(
            pool.clone(),
            "/register",
            "email=bob%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
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

    fn set_cookie(response: &axum::response::Response, name: &str) -> String {
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter(|v| v.starts_with(&format!("{name}=")))
            .collect::<Vec<_>>()
            .join("")
    }

    #[sqlx::test]
    async fn wrong_current_password_opens_the_state_with_an_error(
        pool: sqlx::PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let session = register(&pool).await;
        let response = post(
            pool.clone(),
            "/settings/password",
            "current_password=not+it&new_password=brand+new+password&confirm_password=brand+new+password",
            &session,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers().get("location").unwrap(),
            "/settings#password",
            "redirects back with the state open"
        );
        assert!(
            set_cookie(&response, "pw_err").contains("pw_err=current"),
            "names the failure for the GET"
        );

        let cookie = format!("{session}; pw_err=current");
        let response = get(pool, "/settings", &cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            set_cookie(&response, "pw_err").contains("Max-Age=0"),
            "error cookie is one-shot"
        );
        let body = text(response).await;
        assert!(body.contains("Current password is incorrect"), "inline error");
        assert!(body.contains(r#"id="password""#), "state markup present");
        Ok(())
    }

    #[sqlx::test]
    async fn mismatched_confirmation_reports_and_keeps_the_link_alive(
        pool: sqlx::PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let session = register(&pool).await;
        let response = post(
            pool.clone(),
            "/settings/password",
            "current_password=correct+horse+battery&new_password=brand+new+password&confirm_password=different+password",
            &session,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert!(set_cookie(&response, "pw_err").contains("pw_err=mismatch"));

        let cookie = format!("{session}; pw_err=mismatch");
        let body = text(get(pool, "/settings", &cookie).await).await;
        assert!(body.contains("Passwords do not match"), "mismatch copy");
        Ok(())
    }

    #[sqlx::test]
    async fn short_new_password_uses_the_shared_floor(
        pool: sqlx::PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let session = register(&pool).await;
        let response = post(
            pool.clone(),
            "/settings/password",
            "current_password=correct+horse+battery&new_password=short&confirm_password=short",
            &session,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert!(set_cookie(&response, "pw_err").contains("pw_err=short"));

        let cookie = format!("{session}; pw_err=short");
        let body = text(get(pool, "/settings", &cookie).await).await;
        assert!(
            body.contains("password must be at least 10 characters"),
            "same floor message as registration"
        );
        Ok(())
    }

    #[sqlx::test]
    async fn success_swaps_the_password_and_the_old_one_stops_working(
        pool: sqlx::PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let session = register(&pool).await;
        let response = post(
            pool.clone(),
            "/settings/password",
            "current_password=correct+horse+battery&new_password=brand+new+password&confirm_password=brand+new+password",
            &session,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers().get("location").unwrap(),
            "/settings",
            "lands on the resting state"
        );

        let old = post(
            pool.clone(),
            "/login",
            "email=bob%40example.com&password=correct+horse+battery",
            "",
        )
        .await;
        assert_eq!(old.status(), StatusCode::UNAUTHORIZED, "old password dies");

        let fresh = post(
            pool.clone(),
            "/login",
            "email=bob%40example.com&password=brand+new+password",
            "",
        )
        .await;
        assert_eq!(fresh.status(), StatusCode::SEE_OTHER, "new password works");
        Ok(())
    }
}
