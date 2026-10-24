// story: e17s02
//! The ssh:// URL renders in the repo page's clone block ONLY when the
//! SSH listener is enabled; disabled means no ssh:// anywhere.

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

mod ssh_url_snippet {
    use super::*;
    use server::ssh::SshConfig;

    fn ssh_config(enabled: bool) -> SshConfig {
        SshConfig {
            enabled,
            bind: format!("127.0.0.1:{}", server::ssh::DEFAULT_SSH_PORT)
                .parse()
                .unwrap(),
            host_key_dir: std::path::PathBuf::from("/nonexistent-host-keys"),
        }
    }

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir, ssh: SshConfig) -> axum::Router {
        server::app_with_ssh(
            pool,
            b"snippet-secret".to_vec(),
            root.path().to_str().unwrap().to_string(),
            String::new(),
            String::new(),
            ssh,
        )
    }

    async fn body_of(response: axum::response::Response) -> String {
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn csrf(cookie: &str) -> String {
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(b"snippet-secret", value)
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir, ssh: SshConfig) -> String {
        let response = app(pool.clone(), root, ssh)
            .oneshot(
                Request::post("/register")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "email=alice%40example.com&password=correct+horse+battery\
                         &password_confirm=correct+horse+battery",
                    ))
                    .unwrap(),
            )
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
        ssh: SshConfig,
        cookie: &str,
    ) {
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "name=demo&visibility=public&_csrf={}",
                csrf(cookie)
            )))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        app(pool.clone(), root, ssh).oneshot(request).await.unwrap();
    }

    async fn handle_of(pool: &sqlx::PgPool) -> String {
        sqlx::query_scalar("SELECT handle FROM users LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn repo_page(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        ssh: SshConfig,
        handle: &str,
    ) -> String {
        let request = Request::get(format!("/{handle}/demo"))
            .header("host", "example.test:3900")
            .body(Body::empty())
            .unwrap();
        let response = app(pool.clone(), root, ssh).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        body_of(response).await
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn enabled_renders_ssh_url_with_ssh_port(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root, ssh_config(true)).await;
        create_repo(&pool, &root, ssh_config(true), &cookie).await;
        let handle = handle_of(&pool).await;

        let body = repo_page(&pool, &root, ssh_config(true), &handle).await;
        let expected = format!(
            "ssh://{handle}@example.test:{}/",
            server::ssh::DEFAULT_SSH_PORT
        );
        assert!(
            body.contains(&expected),
            "enabled listener must render {expected:?}, body was:\n{body}"
        );
        assert!(
            body.contains(&format!("{handle}/demo.git")),
            "ssh URL must point at the repo path, body was:\n{body}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn disabled_renders_no_ssh_url(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root, ssh_config(false)).await;
        create_repo(&pool, &root, ssh_config(false), &cookie).await;
        let handle = handle_of(&pool).await;

        let body = repo_page(&pool, &root, ssh_config(false), &handle).await;
        assert!(
            !body.contains("ssh://"),
            "disabled listener must render no ssh:// URL, body was:\n{body}"
        );
        // The HTTP clone instructions must survive.
        assert!(
            body.contains(&format!("http://example.test:3900/git/{handle}/demo.git")),
            "HTTP clone line must stay, body was:\n{body}"
        );
    }
}
