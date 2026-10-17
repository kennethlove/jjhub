// story: Browse files — link the repo page at kata's browse view through
// the auth proxy (embed waits on upstream base-path support).
mod browse_link {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn setup(pool: &sqlx::PgPool, root: &tempfile::TempDir, with_meta: bool) -> String {
        std::env::set_var("SESSION_SECRET", "browse-suite");
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
        let cookie = response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        let secret = std::env::var("SESSION_SECRET").unwrap();
        let value = cookie.strip_prefix("session=").unwrap();
        let token = server::session::csrf_token(secret.as_bytes(), value);
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "name=demo&visibility=public&_csrf={token}"
            )))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap();
        if with_meta {
            let meta = domain::meta_path(root.path().to_str().unwrap(), "alice", "demo");
            tokio::fs::create_dir_all(meta.join(".jj")).await.unwrap();
        }
        cookie
    }

    async fn get(pool: &sqlx::PgPool, root: &tempfile::TempDir, cookie: &str) -> String {
        let mut request = Request::get("/alice/demo").body(Body::empty()).unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn browse_link_with_meta(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = setup(&pool, &root, true).await;
        let html = get(&pool, &root, &cookie).await;
        assert!(
            html.contains("href=\"/r/alice__demo/browse\""),
            "browse link rendered: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn browse_link_pins_trunk_head(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = setup(&pool, &root, true).await;
        // seed a commit on trunk in the bare repo
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        let seed = root.path().join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .current_dir(&seed)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-b", "trunk"]);
        std::fs::write(seed.join("f.txt"), "x\n").unwrap();
        git(&["add", "."]);
        git(&[
            "-c",
            "user.email=a@e.com",
            "-c",
            "user.name=A",
            "commit",
            "-m",
            "one",
        ]);
        git(&["push", bare.to_str().unwrap(), "trunk"]);

        let html = get(&pool, &root, &cookie).await;
        assert!(
            html.contains("/alice/demo/tree"),
            "native Files link when commits exist: {html}"
        );
        assert!(
            html.contains("/browse?commit=") && !html.contains("browse?change="),
            "browse link pinned to trunk head: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn no_browse_link_without_meta(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        // pre-backfill shape: repo exists, colocated metadata does not
        let cookie = setup(&pool, &root, false).await;
        let meta = domain::meta_path(root.path().to_str().unwrap(), "alice", "demo");
        tokio::fs::remove_dir_all(&meta).await.unwrap();
        let html = get(&pool, &root, &cookie).await;
        assert!(!html.contains("127.0.0.1"), "no dead browse link: {html}");
    }
}
