// story: e15s02 — the repo, tree, and blob pages render from templates,
// not from Rust string literals
mod extract_browse {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tera::Context;
    use tower::ServiceExt;

    fn ctx() -> Context {
        let mut ctx = Context::new();
        ctx.insert("user_handle", "");
        ctx.insert("csrf", "");
        ctx.insert("theme", "system"); // e21s02: base.html declares theme
        ctx
    }

    fn seed(root: &std::path::Path) -> std::path::PathBuf {
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
        git(&["init", "-b", "trunk"]);
        std::fs::create_dir_all(seed.join("docs")).unwrap();
        std::fs::write(
            seed.join("README.md"),
            "# hello\n<script>alert(1)</script>\n",
        )
        .unwrap();
        std::fs::write(seed.join("docs/guide.md"), "guide\n").unwrap();
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
        bare
    }

    async fn mount(pool: &sqlx::PgPool, root: &tempfile::TempDir) {
        let bare = seed(root.path());
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

    async fn get(pool: &sqlx::PgPool, root: &tempfile::TempDir, path: &str) -> String {
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn repo_page_matches_template(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root).await;
        let html = get(&pool, &root, "/alice/demo").await;

        let mut expected = ctx();
        expected.insert("owner", "alice");
        expected.insert("name", "demo");
        expected.insert("visibility", "public");
        expected.insert("trunk", "trunk");
        // story: e10s02 — settings block lists the push gate's protected set
        expected.insert(
            "protected_refs",
            &["refs/heads/trunk".to_string(), "refs/tags/*".to_string()],
        );
        expected.insert("browse_href", "");
        expected.insert("files", &true);
        // story: e13s01 — the landing page carries the root file tree
        expected.insert(
            "tree",
            &vec![
                serde_json::json!({"is_dir": false, "label": "README.md"}),
                serde_json::json!({"is_dir": true, "label": "docs"}),
            ],
        );
        expected.insert("mrs", &Vec::<String>::new());
        expected.insert("host", "localhost:3000");
        expected.insert("clone_token", "<TOKEN>");
        expected.insert("generate", &false);
        // story: e17s02 — the clone block carries the gated ssh:// line
        expected.insert("ssh_enabled", &true);
        expected.insert("ssh_host", "localhost");
        expected.insert("ssh_port", &server::ssh::DEFAULT_SSH_PORT);
        assert_eq!(html, server::render::render("repo.html", &expected));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn tree_page_matches_template(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root).await;
        let html = get(&pool, &root, "/alice/demo/tree").await;

        let crumbs = vec![serde_json::json!({
            "href": "/alice/demo/tree",
            "label": "alice/demo"
        })];
        let items = vec![
            serde_json::json!({
                "is_dir": false,
                "href": "/alice/demo/blob/trunk/README.md",
                "label": "README.md"
            }),
            serde_json::json!({
                "is_dir": true,
                "href": "/alice/demo/tree/trunk/docs",
                "label": "docs"
            }),
        ];
        let mut expected = ctx();
        expected.insert("owner", "alice");
        expected.insert("name", "demo");
        expected.insert("path_label", "/");
        expected.insert("crumbs", &crumbs);
        expected.insert("items", &items);
        assert_eq!(html, server::render::render("tree.html", &expected));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn blob_page_matches_template(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root).await;
        let html = get(&pool, &root, "/alice/demo/blob/trunk/README.md").await;

        let lines = vec![
            serde_json::json!({ "number": "   1", "text": "# hello" }),
            serde_json::json!({
                "number": "   2",
                "text": "<script>alert(1)</script>"
            }),
        ];
        let mut expected = ctx();
        expected.insert("owner", "alice");
        expected.insert("name", "demo");
        expected.insert("path", "README.md");
        expected.insert("back", "/alice/demo/tree/trunk");
        expected.insert("binary", &false);
        expected.insert("len", &0);
        expected.insert("lines", &lines);
        assert_eq!(html, server::render::render("blob.html", &expected));
    }
}
