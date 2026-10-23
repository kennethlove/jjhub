// story: e13s02 — landing-tree entries navigate into e07's pages: dirs
// to the tree route at trunk, files to the blob route, names escaped in
// link text and hrefs.
mod landing_links {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// seed a bare repo on `branch` with docs/guide.md + README.md plus
    /// an attacker-named file when `extra` is given; returns bare path
    fn seed(root: &std::path::Path, branch: &str, extra: Option<&str>) -> std::path::PathBuf {
        let bare = root.join("bare.git");
        let seed = root.join("seed");
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
        git(&["init", "--bare", bare.to_str().unwrap()]);
        git(&["init", "-b", branch]);
        std::fs::create_dir_all(seed.join("docs")).unwrap();
        std::fs::write(seed.join("README.md"), "# hello\n").unwrap();
        std::fs::write(seed.join("docs/guide.md"), "guide\n").unwrap();
        if let Some(name) = extra {
            std::fs::write(seed.join(name), "payload\n").unwrap();
        }
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
        git(&["push", bare.to_str().unwrap(), branch]);
        bare
    }

    async fn mount(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        branch: &str,
        extra: Option<&str>,
    ) {
        let bare = seed(root.path(), branch, extra);
        let target = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::rename(&bare, &target).unwrap();
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        ).fetch_one(pool).await.unwrap();
        sqlx::query("INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo')")
            .bind(user_id)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn get(pool: &sqlx::PgPool, root: &std::path::Path) -> (StatusCode, String) {
        let response = server::app_with_storage(pool.clone(), root.to_str().unwrap().to_string())
            .oneshot(Request::get("/alice/demo").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body =
            String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
                .into_owned();
        (status, body)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn landing_entries_link_to_tree_and_blob(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root, "trunk", None).await;

        let (status, html) = get(&pool, root.path()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            html.contains(r#"href="/alice/demo/tree/trunk/docs""#),
            "dir entry links to the e07 tree route at trunk: {html}"
        );
        assert!(
            html.contains(r#"<a href="/alice/demo/tree/trunk/docs">docs</a>"#),
            "dir link text is the entry name: {html}"
        );
        assert!(
            html.contains(r#"href="/alice/demo/blob/trunk/README.md""#),
            "file entry links to the e07 blob route: {html}"
        );
        assert!(
            html.contains(r#"href="/alice/demo/blob/trunk/docs/guide.md""#),
            "nested file links to the e07 blob route: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn landing_links_escape_names(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root, "trunk", Some("<script>.md")).await;

        let (status, html) = get(&pool, root.path()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            !html.contains("<script>"),
            "raw name never reaches link text or href: {html}"
        );
        assert!(
            html.contains(r#"href="/alice/demo/blob/trunk/&lt;script&gt;.md""#),
            "href escaped: {html}"
        );
        assert!(
            html.contains(r#">&lt;script&gt;.md</a>"#),
            "link text escaped: {html}"
        );
    }
}
