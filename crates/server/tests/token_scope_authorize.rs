// story: e22s02 — scope and repo-pin enforcement lives in the shared
// admission decision: read-only never pushes, a pinned token touches
// only its repo, and a denied create never happens. Both transports.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use tower::ServiceExt;

mod token_scope_authorize {
    use super::*;
    use russh::server::Handler;

    const SECRET: &str = "token-scope-authorize-suite";

    fn basic(plaintext: &str) -> String {
        use base64::Engine;
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD
                .encode(format!("x-access-token:{plaintext}"))
        )
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        std::env::set_var("SESSION_SECRET", SECRET);
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "email=alice%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
            ))
            .unwrap();
        let response = server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|v| v.to_str().ok().and_then(|s| s.split(';').next()).map(String::from))
            .expect("session cookie")
    }

    async fn csrf(cookie: &str) -> String {
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(SECRET.as_bytes(), value)
    }

    async fn create_repo(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        cookie: &str,
        name: &str,
    ) {
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "name={name}&visibility=private&_csrf={}",
                csrf(cookie).await
            )))
            .unwrap();
        request.headers_mut().insert("cookie", cookie.parse().unwrap());
        let response = server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    async fn alice_id(pool: &sqlx::PgPool) -> uuid::Uuid {
        sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn set_scopes(pool: &sqlx::PgPool, token_id: uuid::Uuid, value: &str) {
        let literal = format!("'{{{value}}}'::text[]");
        sqlx::query(&format!(
            "UPDATE tokens SET scopes = {literal} WHERE id = $1"
        ))
        .bind(token_id)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn info_refs(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        repo: &str,
        token: &str,
    ) -> StatusCode {
        let mut request = Request::get(format!(
            "/git/alice/{repo}.git/info/refs?service=git-upload-pack"
        ))
        .header(header::AUTHORIZATION, basic(token))
        .body(Body::empty())
        .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 0))));
        server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap()
            .status()
    }

    async fn push(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        repo: &str,
        token: &str,
    ) -> axum::response::Response {
        let mut request = Request::post(format!("/git/alice/{repo}.git/git-receive-pack"))
            .header(header::AUTHORIZATION, basic(token))
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 0))));
        server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn read_only_token_clones_but_cannot_push(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie, "demo").await;
        let token = server::token::create(&pool, alice_id(&pool).await, "ro").await;
        set_scopes(&pool, token.id, "read").await;

        assert_eq!(
            info_refs(&pool, &root, "demo", &token.plaintext).await,
            StatusCode::OK,
            "read scope advertises the clone"
        );
        let response = push(&pool, &root, "demo", &token.plaintext).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn read_only_token_cannot_push_to_create(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        let token = server::token::create(&pool, alice_id(&pool).await, "ro").await;
        set_scopes(&pool, token.id, "read").await;

        let response = push(&pool, &root, "brandnew", &token.plaintext).await;
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "write scope gates admission itself"
        );
        let rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM repositories r
             JOIN users u ON u.id = r.owner_id
             WHERE u.handle = 'alice' AND r.name = 'brandnew'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows, 0, "denied create never creates");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn pinned_token_only_touches_its_repo(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie, "pinned").await;
        create_repo(&pool, &root, &cookie, "other").await;
        let pinned_id: uuid::Uuid = sqlx::query_scalar(
            "SELECT r.id FROM repositories r
             JOIN users u ON u.id = r.owner_id
             WHERE u.handle = 'alice' AND r.name = 'pinned'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let token = server::token::create(&pool, alice_id(&pool).await, "pin").await;
        sqlx::query("UPDATE tokens SET repo_id = $2 WHERE id = $1")
            .bind(token.id)
            .bind(pinned_id)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            info_refs(&pool, &root, "pinned", &token.plaintext).await,
            StatusCode::OK,
            "the pinned repo serves its token"
        );
        assert_eq!(
            info_refs(&pool, &root, "other", &token.plaintext).await,
            StatusCode::FORBIDDEN,
            "every other repo refuses the pinned token — even private-read own repos"
        );
        let response = push(&pool, &root, "other", &token.plaintext).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    /// SSH: scope gates the exec admission with the same rules.
    #[sqlx::test(migrations = "./migrations")]
    async fn ssh_scope_gates_push_and_admission(pool: sqlx::PgPool) {
        let (plaintext, token_id) = server::token::insert_test_token(&pool).await;
        let user_id: uuid::Uuid =
            sqlx::query_scalar("SELECT id FROM users WHERE handle = 'tokentest'")
                .fetch_one(&pool)
                .await
                .unwrap();
        set_scopes(&pool, token_id, "read").await;

        // seed a bare repo + rows so read admission has somewhere to go
        let root = tempfile::tempdir().unwrap();
        let bare = domain::storage_path(root.path().to_str().unwrap(), "tokentest", "demo");
        std::fs::create_dir_all(&bare).unwrap();
        let out = std::process::Command::new("git")
            .args(["init", "--bare"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .arg(&bare)
            .output()
            .unwrap();
        assert!(out.status.success());
        sqlx::query(
            "INSERT INTO repositories (owner_id, name, visibility)
             VALUES ($1, 'demo', 'private')",
        )
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

        let mut handler =
            server::ssh::SshHandler::with_storage(pool, root.path().to_str().unwrap().to_string());
        let auth = handler.auth_password("tokentest", &plaintext).await.unwrap();
        assert!(matches!(auth, russh::server::Auth::Accept));

        let denied = handler
            .run_git_exec("git-receive-pack '/tokentest/demo.git'")
            .await
            .unwrap_err();
        assert_eq!(
            denied, "forbidden",
            "write scope denial reuses today's forbidden shape"
        );

        let (child, _stdin) = handler
            .run_git_exec("git-upload-pack '/tokentest/demo.git'")
            .await
            .expect("read scope still clones");
        drop(child);
    }
}
