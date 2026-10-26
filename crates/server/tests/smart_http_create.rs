// story: e19s02 — smart-HTTP receive on a fresh owned path creates the
// repo (PRIVATE) and lands the pushed refs end to end: no row, no
// form — the push itself is the create.
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

mod smart_http_create {
    use super::*;

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        std::env::set_var("SESSION_SECRET", "smart-http-create-suite");
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "email=alice%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
            ))
            .unwrap();
        let response = app(pool.clone(), root).oneshot(request).await.unwrap();
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

    #[sqlx::test(migrations = "./migrations")]
    async fn push_to_fresh_path_creates_private_repo(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        assert!(!cookie.is_empty(), "registration fixture works");

        let alice: uuid::Uuid =
            sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let token = server::token::create(&pool, alice, "push").await;

        // Seed a local commit to push (pattern: e02s01 clone test).
        let seed = tempfile::tempdir().unwrap();
        let out = std::process::Command::new("git")
            .args(["init", "-b", "trunk"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .arg(seed.path())
            .output()
            .unwrap();
        assert!(out.status.success());
        std::fs::write(seed.path().join("hello.txt"), "first push\n").unwrap();
        for args in [
            &["add", "."][..],
            &[
                "-c",
                "user.email=alice@example.com",
                "-c",
                "user.name=Alice",
                "commit",
                "-m",
                "init",
            ][..],
        ] {
            let out = std::process::Command::new("git")
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .current_dir(seed.path())
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {:?}: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        }

        // Real listener: git's blocking HTTP needs one.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = app(pool.clone(), &root);
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        use base64::Engine;
        let auth = base64::engine::general_purpose::STANDARD
            .encode(format!("x-access-token:{}", token.plaintext));
        let seed_path = seed.path().to_path_buf();
        let out = tokio::task::spawn_blocking(move || {
            std::process::Command::new("git")
                .args([
                    "-c",
                    &format!("http.extraHeader=Authorization: Basic {auth}"),
                    "push",
                    &format!("http://{addr}/git/alice/newproj.git"),
                    "trunk",
                ])
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .current_dir(seed_path)
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(
            out.status.success(),
            "push must create and land: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        // Row exists and is PRIVATE — before http-backend could have run
        // the guard would reject a missing row, so a landed push already
        // proves the ordering; assert the state itself.
        let row: (uuid::Uuid, String) = sqlx::query_as(
            "SELECT id, visibility FROM repositories WHERE owner_id = $1 AND name = 'newproj'",
        )
        .bind(alice)
        .fetch_one(&pool)
        .await
        .expect("push-to-create inserts the row");
        assert_eq!(row.1, "private");

        // Pushed refs land in the created bare store.
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "newproj");
        let out = std::process::Command::new("git")
            .args(["--git-dir", bare.to_str().unwrap(), "show-ref"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        let refs = String::from_utf8_lossy(&out.stdout);
        assert!(
            refs.contains("refs/heads/trunk"),
            "pushed ref lands: {refs}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn upload_pack_and_foreign_namespace_stay_404(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        assert!(!cookie.is_empty());

        let alice: uuid::Uuid =
            sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let token = server::token::create(&pool, alice, "push").await;
        use base64::Engine;
        let auth = base64::engine::general_purpose::STANDARD
            .encode(format!("x-access-token:{}", token.plaintext));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = app(pool.clone(), &root);
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let auth_head = format!("http.extraHeader=Authorization: Basic {auth}");
        let url = format!("http://{addr}");
        let (ls_nothing, ls_squat) = {
            let auth_head = auth_head.clone();
            let url = url.clone();
            tokio::task::spawn_blocking(move || {
                let run = |path: &str| {
                    std::process::Command::new("git")
                        .args(["-c", &auth_head, "ls-remote", &format!("{url}{path}")])
                        .env("GIT_CONFIG_GLOBAL", "/dev/null")
                        .env("GIT_CONFIG_SYSTEM", "/dev/null")
                        .output()
                        .unwrap()
                };
                (
                    run("/git/alice/nothing.git"),
                    run("/git/bob/squat.git"),
                )
            })
            .await
            .unwrap()
        };
        assert!(
            !ls_nothing.status.success(),
            "clone of nothing must 404: {}",
            String::from_utf8_lossy(&ls_nothing.stderr)
        );
        assert!(
            !ls_squat.status.success(),
            "foreign namespace must 404: {}",
            String::from_utf8_lossy(&ls_squat.stderr)
        );

        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM repositories")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 0, "neither probe may create a row");

        // No token at all: same 404, still no create.
        let response = app(pool, &root)
            .oneshot(
                Request::get("/git/alice/nothing.git/info/refs?service=git-receive-pack")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
