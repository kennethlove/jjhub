// story: e03s02
mod mr_pages {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
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

    /// Seed trunk + three stacked commits directly in the bare repo and
    /// return their ids (bottom first).
    async fn seed_stack(bare: &std::path::Path) -> Vec<String> {
        let seed = bare.join("../seed");
        std::fs::create_dir_all(&seed).unwrap();
        git(&["init", "-b", "trunk", seed.to_str().unwrap()]);
        let mut ids = Vec::new();
        for (i, name) in ["a.txt", "b.txt", "c.txt"].iter().enumerate() {
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

    async fn seed_repo_with_mrs(pool: &sqlx::PgPool, commits: &[String]) -> (String, String) {
        let handle = "alice";
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
        for (i, (change, commit)) in ["aaa", "bbb", "ccc"].iter().zip(commits).enumerate() {
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
            let _ = i;
        }
        (handle.to_string(), "demo".to_string())
    }

    async fn get(
        pool: sqlx::PgPool,
        root: &std::path::Path,
        path: &str,
    ) -> axum::response::Response {
        server::app_with_storage(pool, root.to_str().unwrap().to_string())
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn pulls_page_lists_stack_bottom_first(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let commits = seed_stack(&bare).await;
        let (owner, name) = seed_repo_with_mrs(&pool, &commits).await;

        let response = get(pool, root.path(), &format!("/{owner}/{name}/pulls")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let html =
            String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
                .into_owned();

        let pos_aaa = html.find("aaa").expect("bottom MR rendered");
        let pos_bbb = html.find("bbb").expect("middle MR rendered");
        let pos_ccc = html.find("ccc").expect("top MR rendered");
        assert!(
            pos_aaa < pos_bbb && pos_bbb < pos_ccc,
            "stack renders bottom-first: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn pulls_page_unknown_repo_404(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let response = get(pool, root.path(), "/nobody/nope/pulls").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
