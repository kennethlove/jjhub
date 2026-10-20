// story: e15s03 — CI statuses, comment threads, and the review link on
// the MR detail page render from templates, not from Rust string literals
mod extract_review {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tera::Context;
    use tower::ServiceExt;

    fn git(args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn git_in(cwd: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// alice's public repo with one MR holding one real revision; returns
    /// the bare path and the revision commit id.
    async fn mount(pool: &sqlx::PgPool, root: &std::path::Path) -> (std::path::PathBuf, String) {
        let bare = domain::storage_path(root.to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let seed = bare.join("../seed");
        std::fs::create_dir_all(&seed).unwrap();
        git(&["init", "-b", "trunk", seed.to_str().unwrap()]);
        std::fs::write(seed.join("a.txt"), "a.txt\n").unwrap();
        git_in(&seed, &["add", "."]);
        git_in(&seed, &["-c", "user.email=a@e.com", "-c", "user.name=A", "commit", "-m", "one"]);
        git_in(&seed, &["push", bare.to_str().unwrap(), "trunk"]);
        let commit = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&seed)
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let repo_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo') RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        let mr_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, 'aaa') RETURNING id",
        )
        .bind(repo_id)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2)")
            .bind(mr_id)
            .bind(&commit)
            .execute(pool)
            .await
            .unwrap();
        (bare, commit)
    }

    async fn get(pool: &sqlx::PgPool, root: &std::path::Path, path: &str) -> String {
        let response =
            server::app_with_storage(pool.clone(), root.to_str().unwrap().to_string())
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    /// The expected detail page for the seeded MR: every context the page
    /// renders comes from the template model.
    fn detail_ctx(
        bare: &std::path::Path,
        commit: &str,
        badges: &[serde_json::Value],
        threads: &[serde_json::Value],
        review_href: &Option<String>,
    ) -> Context {
        let mut revision = Context::new();
        revision.insert("owner", "alice");
        revision.insert("name", "demo");
        revision.insert("change_id", "aaa");
        revision.insert("csrf", "");
        revision.insert("commit", commit);
        revision.insert(
            "diff_html",
            &server::stack::html_escape(&server::stack::unified_diff(bare, commit)),
        );
        revision.insert("threads", threads);
        let mut page = Context::new();
        page.insert("owner", "alice");
        page.insert("name", "demo");
        page.insert("change_id", "aaa");
        page.insert("mr_state", "open");
        page.insert("csrf", "");
        page.insert("badges", badges);
        page.insert(
            "sections",
            &[server::render::render("mr_revision.html", &revision)],
        );
        page.insert("review_href", review_href);
        page
    }

    /// One expected status badge, following the page's link rules.
    fn badge(context: &str, state: &str, url: Option<&str>) -> serde_json::Value {
        let label = server::stack::html_escape(&format!("{context}: {state}"));
        let url = url.map(server::stack::html_escape);
        let href = match &url {
            Some(u) if u.starts_with("http://") || u.starts_with("https://") => Some(u.clone()),
            _ => None,
        };
        serde_json::json!({
            "context": server::stack::html_escape(context),
            "state": server::stack::html_escape(state),
            "label": label,
            "url": url,
            "href": href,
        })
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn ci_status_badges_match_template(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (bare, commit) = mount(&pool, root.path()).await;
        let revision_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM revisions")
            .fetch_one(&pool)
            .await
            .unwrap();
        for (context, state, url) in [
            ("ci/build", "success", Some("https://ci.example/1")),
            ("ci/test", "failure", Some("javascript:alert(1)")),
            ("ci/docs", "pending", None),
        ] {
            sqlx::query(
                "INSERT INTO statuses (revision_id, context, state, target_url) \
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(revision_id)
            .bind(context)
            .bind(state)
            .bind(url)
            .execute(&pool)
            .await
            .unwrap();
        }
        let html = get(&pool, root.path(), "/alice/demo/merge-requests/aaa").await;

        let badges = vec![
            badge("ci/build", "success", Some("https://ci.example/1")),
            badge("ci/docs", "pending", None),
            badge("ci/test", "failure", Some("javascript:alert(1)")),
        ];
        let expected = detail_ctx(&bare, &commit, &badges, &[], &None);
        assert_eq!(html, server::render::render("mr_detail.html", &expected));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn comment_threads_match_template(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (bare, commit) = mount(&pool, root.path()).await;
        let (mr_id, revision_id): (uuid::Uuid, uuid::Uuid) =
            sqlx::query_as("SELECT mr_id, id FROM revisions")
                .fetch_one(&pool)
                .await
                .unwrap();
        let author_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO comments (mr_id, revision_id, author_id, body) VALUES ($1, $2, $3, $4)",
        )
        .bind(mr_id)
        .bind(revision_id)
        .bind(author_id)
        .bind("<script>alert(1)</script>")
        .execute(&pool)
        .await
        .unwrap();
        let html = get(&pool, root.path(), "/alice/demo/merge-requests/aaa").await;

        let threads = vec![serde_json::json!({
            "handle_html": server::stack::html_escape("alice"),
            "body_html": server::stack::html_escape("<script>alert(1)</script>"),
        })];
        let expected = detail_ctx(&bare, &commit, &[], &threads, &None);
        assert_eq!(html, server::render::render("mr_detail.html", &expected));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn review_link_matches_template(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (bare, commit) = mount(&pool, root.path()).await;
        let meta = domain::meta_path(root.path().to_str().unwrap(), "alice", "demo");
        std::fs::create_dir_all(meta.join(".jj")).unwrap();
        let html = get(&pool, root.path(), "/alice/demo/merge-requests/aaa").await;

        let review_href = server::review_proxy::review_link(
            "",
            root.path().to_str().unwrap(),
            "alice",
            "demo",
            "aaa",
        );
        let expected = detail_ctx(&bare, &commit, &[], &[], &review_href);
        assert_eq!(html, server::render::render("mr_detail.html", &expected));
    }
}
