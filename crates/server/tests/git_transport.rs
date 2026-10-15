// story: e02s01
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

mod git_transport_public {
    use super::*;

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "email=alice%40example.com&password=correct+horse+battery",
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

    async fn create_repo(pool: &sqlx::PgPool, root: &tempfile::TempDir, cookie: &str) {
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("name=demo&visibility=public"))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response = app(pool.clone(), root).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    async fn get(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        path: &str,
    ) -> axum::response::Response {
        app(pool, root)
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn info_refs_advertises_for_public_repo(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;

        let response = get(
            pool,
            &root,
            "/git/alice/demo.git/info/refs?service=git-upload-pack",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            content_type.contains("application/x-git-upload-pack-advertisement"),
            "smart HTTP advertisement: {content_type}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unknown_repo_is_404(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let response = get(
            pool,
            &root,
            "/git/nobody/nope.git/info/refs?service=git-upload-pack",
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn real_git_clone_of_pushed_content(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;

        // seed the bare repo with a commit using real git (simpler than driving
        // receive-pack by hand in the test)
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        let seed = tempfile::tempdir().unwrap();
        let out = std::process::Command::new("git")
            .args(["init", "-b", "trunk"])
            .arg(seed.path())
            .output()
            .unwrap();
        assert!(out.status.success());
        std::fs::write(seed.path().join("hello.txt"), "hello jjhub\n").unwrap();
        for args in [
            ["add", "."].as_slice(),
            ["-c", "user.email=alice@example.com", "-c", "user.name=Alice", "commit", "-m", "init"]
                .as_slice(),
        ] {
            let out = std::process::Command::new("git")
                .arg(args)
                .current_dir(seed.path())
                .output()
                .unwrap();
            assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
        }

        // serve via oneshot? real git needs a listener — spawn the app on an ephemeral port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = app(pool, &root);
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let clone = tempfile::tempdir().unwrap();
        let out = std::process::Command::new("git")
            .args(["clone", &format!("http://{addr}/git/alice/demo.git")])
            .arg(clone.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git clone failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(clone.path().join("hello.txt")).unwrap(),
            "hello jjhub\n"
        );
    }
}
