// story: e22s02 — authentication stage enforces expiry and the IP
// allowlist: dead tokens die before any git work, on both transports.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use tower::ServiceExt;

mod token_restrictions {
    use super::*;
    use russh::server::Handler;

    const SECRET: &str = "token-restrictions-suite";

    fn basic(plaintext: &str) -> String {
        use base64::Engine;
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{plaintext}"))
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
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|v| {
                v.to_str()
                    .ok()
                    .and_then(|s| s.split(';').next())
                    .map(String::from)
            })
            .expect("session cookie")
    }

    async fn csrf(cookie: &str) -> String {
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(SECRET.as_bytes(), value)
    }

    async fn create_repo(pool: &sqlx::PgPool, root: &tempfile::TempDir, cookie: &str) {
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "name=demo&visibility=private&_csrf={}",
                csrf(cookie).await
            )))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    /// GET the upload-pack advertisement with a Basic token from `ip`.
    async fn info_refs(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        token: &str,
        ip: [u8; 4],
    ) -> StatusCode {
        let mut request = Request::get("/git/alice/demo.git/info/refs?service=git-upload-pack")
            .header(header::AUTHORIZATION, basic(token))
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(std::net::SocketAddr::from((ip, 0))));
        server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap()
            .status()
    }

    /// POST receive-pack from `ip` — denial must happen before spawn.
    async fn push(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        token: &str,
        ip: [u8; 4],
    ) -> StatusCode {
        let mut request = Request::post("/git/alice/demo.git/git-receive-pack")
            .header(header::AUTHORIZATION, basic(token))
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(std::net::SocketAddr::from((ip, 0))));
        server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap()
            .status()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn expired_token_dies_at_authentication(pool: sqlx::PgPool) {
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle)
             VALUES ('exp@example.com', 'x', 'expuser') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let live = server::token::create(&pool, user_id, "live").await;
        let dead = server::token::create(&pool, user_id, "dead").await;
        sqlx::query("UPDATE tokens SET expires_at = now() - interval '1 hour' WHERE id = $1")
            .bind(dead.id)
            .execute(&pool)
            .await
            .unwrap();

        assert!(
            server::token::authenticate(&pool, &live.plaintext, None)
                .await
                .is_some(),
            "unexpired token still authenticates"
        );
        assert!(
            server::token::authenticate(&pool, &dead.plaintext, None)
                .await
                .is_none(),
            "expired token dies at authentication, same stage as revoked"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn http_expired_token_gets_no_service(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;
        let user_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
            .fetch_one(&pool)
            .await
            .unwrap();
        let dead = server::token::create(&pool, user_id, "dead").await;
        sqlx::query("UPDATE tokens SET expires_at = now() - interval '1 minute' WHERE id = $1")
            .bind(dead.id)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            info_refs(&pool, &root, &dead.plaintext, [127, 0, 0, 1]).await,
            StatusCode::UNAUTHORIZED,
            "expired token gets the invalid-token shape, never an advertisement"
        );
        assert_eq!(
            push(&pool, &root, &dead.plaintext, [127, 0, 0, 1]).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn allowlist_gates_the_client_ip(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;
        let user_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
            .fetch_one(&pool)
            .await
            .unwrap();
        let listed = server::token::create(&pool, user_id, "listed").await;
        sqlx::query("UPDATE tokens SET allowed_inets = '{127.0.0.1}'::inet[] WHERE id = $1")
            .bind(listed.id)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            info_refs(&pool, &root, &listed.plaintext, [127, 0, 0, 1]).await,
            StatusCode::OK,
            "on-list IP passes"
        );
        assert_eq!(
            info_refs(&pool, &root, &listed.plaintext, [10, 9, 9, 7]).await,
            StatusCode::UNAUTHORIZED,
            "off-list IP fails closed — indistinguishable from an invalid token"
        );
        assert_eq!(
            push(&pool, &root, &listed.plaintext, [10, 9, 9, 7]).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn ssh_password_stage_rejects_dead_and_off_list_tokens(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle)
             VALUES ('sshexp@example.com', 'x', 'sshexpuser') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let live = server::token::create(&pool, user_id, "live").await;
        let dead = server::token::create(&pool, user_id, "dead").await;
        let listed = server::token::create(&pool, user_id, "listed").await;
        sqlx::query(
            "UPDATE tokens SET expires_at = now() - interval '1 day'
             WHERE id = $1",
        )
        .bind(dead.id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE tokens SET allowed_inets = '{127.0.0.1}'::inet[] WHERE id = $1")
            .bind(listed.id)
            .execute(&pool)
            .await
            .unwrap();

        let mut handler =
            server::ssh::SshHandler::with_storage(pool, root.path().to_str().unwrap().to_string())
                .with_peer(std::net::SocketAddr::from(([127, 0, 0, 1], 0)));
        let auth = handler
            .auth_password("sshexpuser", &live.plaintext)
            .await
            .unwrap();
        assert!(
            matches!(auth, russh::server::Auth::Accept),
            "live token accepted"
        );

        let auth = handler
            .auth_password("sshexpuser", &dead.plaintext)
            .await
            .unwrap();
        assert!(
            !matches!(auth, russh::server::Auth::Accept),
            "expired token rejected"
        );

        let auth = handler
            .auth_password("sshexpuser", &listed.plaintext)
            .await
            .unwrap();
        assert!(
            matches!(auth, russh::server::Auth::Accept),
            "on-list IP accepted"
        );

        handler.set_peer(std::net::SocketAddr::from(([10, 9, 9, 7], 0)));
        let auth = handler
            .auth_password("sshexpuser", &listed.plaintext)
            .await
            .unwrap();
        assert!(
            !matches!(auth, russh::server::Auth::Accept),
            "off-list IP rejected at the password stage"
        );
    }
}

/// story: e22s02 — a row minted before the lookup migration (no
/// lookup_hash) is still honored by the legacy argon2 verify scan.
#[sqlx::test(migrations = "./migrations")]
async fn legacy_row_without_lookup_hash_still_authenticates(pool: sqlx::PgPool) {
    sqlx::query("INSERT INTO users (email, password_hash, handle) VALUES ('legacy@example.com', 'x', 'legacy')")
        .execute(&pool)
        .await
        .unwrap();
    let user_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'legacy'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let plaintext = "jjh_".to_string() + &"a".repeat(96);
    // argon2 PHC of the same shape create() stores; params come from
    // the PHC string, so password-hash params verify fine here.
    let token_hash = server::password::hash_password(&plaintext);
    sqlx::query("INSERT INTO tokens (user_id, label, token_hash, prefix) VALUES ($1, 'legacy', $2, 'jjh_aaaaaa')")
        .bind(user_id)
        .bind(token_hash)
        .execute(&pool)
        .await
        .unwrap();

    let auth = server::token::authenticate(&pool, &plaintext, None)
        .await
        .expect("legacy row authenticates via the verify scan");
    assert_eq!(auth.user_id, user_id);

    let none = server::token::authenticate(&pool, "jjh_deadbeef", None).await;
    assert!(none.is_none());
}
