// story: e19s03
//! SSH receive-pack on a missing owned path runs THE SAME admission
//! helper as smart HTTP (transport::admit_create): valid token
//! presenter + ReceivePack + owner == handle creates the repo
//! PRIVATE before git-receive-pack spawns. Upload-pack on a missing
//! repo and a foreign namespace never reach create — they keep the
//! same not-found shape as an anonymous probe.

use server::ssh::SshHandler;

mod ssh_push_to_create {
    use super::*;
    use russh::server::Handler;

    async fn tokentest_user(pool: &sqlx::PgPool) -> uuid::Uuid {
        sqlx::query_scalar("SELECT id FROM users WHERE handle = 'tokentest'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn handler(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> SshHandler {
        SshHandler::with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
    }

    /// story: e19s03 — AC t1: the push-admitted presenter pushing into
    /// their own namespace creates the repo (PRIVATE) and reaches
    /// receive-pack over SSH.
    #[sqlx::test(migrations = "./migrations")]
    async fn owned_receive_pack_creates_private_repo(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (token, _tok) = server::token::insert_test_token(&pool).await;
        let owner = tokentest_user(&pool).await;

        let mut h = handler(&pool, &root).await;
        let auth = h.auth_password("tokentest", &token).await.unwrap();
        assert!(matches!(auth, russh::server::Auth::Accept));

        let (child, _stdin) = h
            .run_git_exec("git-receive-pack '/tokentest/newrepo.git'")
            .await
            .expect("owned push on a fresh path must create the repo and be admitted");
        drop(child); // kill_on_drop

        let visibility: Option<String> = sqlx::query_scalar(
            "SELECT r.visibility FROM repositories r JOIN users u ON u.id = r.owner_id \
             WHERE u.handle = 'tokentest' AND r.name = 'newrepo'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(
            visibility.as_deref(),
            Some("private"),
            "push-created repo must be PRIVATE"
        );
        let bare = domain::storage_path(root.path().to_str().unwrap(), "tokentest", "newrepo");
        assert!(
            std::path::Path::new(&bare).is_dir(),
            "creation service must have created the bare store"
        );
    }

    /// story: e19s03 — upload-pack on a missing repo never creates:
    /// same not-found shape as an anonymous probe.
    #[sqlx::test(migrations = "./migrations")]
    async fn upload_pack_missing_stays_not_found(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (token, _tok) = server::token::insert_test_token(&pool).await;

        let mut h = handler(&pool, &root).await;
        let auth = h.auth_password("tokentest", &token).await.unwrap();
        assert!(matches!(auth, russh::server::Auth::Accept));

        let err = h
            .run_git_exec("git-upload-pack '/tokentest/fresh-clone.git'")
            .await
            .expect_err("clone of nothing must stay refused");
        assert_eq!(err, "repository not found");

        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM repositories WHERE name = 'fresh-clone'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 0, "upload-pack must not create a row");
    }

    /// story: e19s03 — foreign namespace on a missing path stays 404,
    /// never the presenter's namespace.
    #[sqlx::test(migrations = "./migrations")]
    async fn foreign_namespace_stays_not_found(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (token, _tok) = server::token::insert_test_token(&pool).await;

        let mut h = handler(&pool, &root).await;
        let auth = h.auth_password("tokentest", &token).await.unwrap();
        assert!(matches!(auth, russh::server::Auth::Accept));

        let err = h
            .run_git_exec("git-receive-pack '/alice/squat.git'")
            .await
            .expect_err("foreign-namespace push must stay refused");
        assert_eq!(err, "repository not found");

        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM repositories WHERE name = 'squat'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 0, "foreign namespace must not create a row");
    }
}
