// story: e15s03 — the MR list and detail pages render from templates,
// not from Rust string literals
mod extract_mr {
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

    /// Seed trunk + two stacked commits directly in the bare repo and
    /// return their ids (bottom first).
    async fn seed_stack(bare: &std::path::Path) -> Vec<String> {
        let seed = bare.join("../seed");
        std::fs::create_dir_all(&seed).unwrap();
        git(&["init", "-b", "trunk", seed.to_str().unwrap()]);
        let mut ids = Vec::new();
        for (i, name) in ["a.txt", "b.txt"].iter().enumerate() {
            std::fs::write(seed.join(name), format!("{name}\n")).unwrap();
            git_in(&seed, &["add", "."]);
            git_in(
                &seed,
                &[
                    "-c",
                    "user.email=a@e.com",
                    "-c",
                    "user.name=A",
                    "commit",
                    "-m",
                    &format!("change {}", i + 1),
                ],
            );
            ids.push(
                String::from_utf8_lossy(
                    &std::process::Command::new("git")
                        .args(["rev-parse", "HEAD"])
                        .current_dir(&seed)
                        .output()
                        .unwrap()
                        .stdout,
                )
                .trim()
                .to_string(),
            );
        }
        git_in(&seed, &["push", bare.to_str().unwrap(), "trunk"]);
        ids
    }

    async fn mount(
        pool: &sqlx::PgPool,
        root: &std::path::Path,
    ) -> (std::path::PathBuf, Vec<String>) {
        let bare = domain::storage_path(root.to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let commits = seed_stack(&bare).await;
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
        for (change, commit) in ["aaa", "bbb"].iter().zip(&commits) {
            let mr_id: uuid::Uuid = sqlx::query_scalar(
                "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, $3) RETURNING id",
            )
            .bind(repo_id)
            .bind(user_id)
            .bind(change)
            .fetch_one(pool)
            .await
            .unwrap();
            sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2)")
                .bind(mr_id)
                .bind(commit)
                .execute(pool)
                .await
                .unwrap();
        }
        (bare, commits)
    }

    async fn get(pool: &sqlx::PgPool, root: &std::path::Path, path: &str) -> String {
        let response = server::app_with_storage(pool.clone(), root.to_str().unwrap().to_string())
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    fn detail_ctx(bare: &std::path::Path, commit: &str, threads: &[serde_json::Value]) -> Context {
        let mut revision = Context::new();
        revision.insert("owner", "alice");
        revision.insert("name", "demo");
        revision.insert("change_id", "aaa");
        revision.insert("csrf", "");
        revision.insert("theme", "system"); // e21s02: base.html declares theme
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
        page.insert("theme", "system"); // e21s02: base.html declares theme
        page.insert("badges", &Vec::<String>::new());
        page.insert(
            "sections",
            &[server::render::render("mr_revision.html", &revision)],
        );
        page.insert("review_href", &None::<String>);
        page
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn merge_requests_page_matches_template(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (bare, _commits) = mount(&pool, root.path()).await;
        let html = get(&pool, root.path(), "/alice/demo/merge-requests").await;

        let repo_id: uuid::Uuid = sqlx::query_scalar(
            "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id \
             WHERE u.handle = 'alice' AND r.name = 'demo'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let mrs = server::stack::open_mrs(&pool, repo_id).await.unwrap();
        let stacks = server::stack::derive_page_model(&bare, &mrs);
        let mut items = Vec::new();
        for (stack_idx, stack) in stacks.iter().enumerate() {
            for (mr_idx, change_id) in stack.iter().enumerate() {
                let mut item = Context::new();
                item.insert("owner", "alice");
                item.insert("name", "demo");
                item.insert("change_id", change_id);
                item.insert("stack_idx", &stack_idx);
                item.insert("mr_idx", &mr_idx);
                item.insert(
                    "review_href",
                    &server::review_proxy::review_link(
                        "",
                        root.path().to_str().unwrap(),
                        "alice",
                        "demo",
                        change_id,
                    ),
                );
                items.push(server::render::render("mr_list_item.html", &item));
            }
        }
        let mut expected = Context::new();
        expected.insert("owner", "alice");
        expected.insert("name", "demo");
        expected.insert("position_html", &items.join("\n"));
        assert_eq!(html, server::render::render("mr_pages.html", &expected));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mr_detail_page_matches_template(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (bare, commits) = mount(&pool, root.path()).await;
        let commit = commits[0].clone();
        let html = get(&pool, root.path(), "/alice/demo/merge-requests/aaa").await;

        let expected = detail_ctx(&bare, &commit, &[]);
        assert_eq!(html, server::render::render("mr_detail.html", &expected));
    }
}
