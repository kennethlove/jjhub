// story: e16s02 — MR detail says so when the change diverged: a notice
// above the revisions names both sibling heads and links each to its
// revision section; ordinary MRs render no notice.
mod divergence_mr_detail {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

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

    fn rev_parse(dir: &std::path::Path) -> String {
        String::from_utf8_lossy(
            &std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(dir)
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string()
    }

    struct Siblings {
        trunk: String,
        head_a: String,
        head_b: String,
    }

    /// Seed trunk base + two sibling commits (one child per branch) and
    /// push both refs — the history-rewrite-across-clones state.
    async fn seed_siblings(bare: &std::path::Path) -> Siblings {
        let seed = bare.join("../seed");
        std::fs::create_dir_all(&seed).unwrap();
        git_in(&seed, &["init", "-b", "trunk"]);
        std::fs::write(seed.join("base.txt"), "base\n").unwrap();
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
                "base",
            ],
        );
        let trunk = rev_parse(&seed);
        git_in(&seed, &["branch", "sibling"]);
        std::fs::write(seed.join("a.txt"), "change a\n").unwrap();
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
                "change a",
            ],
        );
        let head_a = rev_parse(&seed);
        git_in(&seed, &["checkout", "sibling"]);
        std::fs::write(seed.join("b.txt"), "change b\n").unwrap();
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
                "change b",
            ],
        );
        let head_b = rev_parse(&seed);
        git_in(&seed, &["push", bare.to_str().unwrap(), "trunk", "sibling"]);
        Siblings {
            trunk,
            head_a,
            head_b,
        }
    }

    async fn seed_repo(pool: &sqlx::PgPool, mrs: &[(&str, &[String])]) {
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
        for (change, commits) in mrs {
            let mr_id: uuid::Uuid = sqlx::query_scalar(
                "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, $3) RETURNING id",
            )
            .bind(repo_id)
            .bind(user_id)
            .bind(change)
            .fetch_one(pool)
            .await
            .unwrap();
            for commit in commits.iter() {
                sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2)")
                    .bind(mr_id)
                    .bind(commit)
                    .execute(pool)
                    .await
                    .unwrap();
            }
        }
    }

    async fn get_html(pool: sqlx::PgPool, root: &std::path::Path, path: &str) -> String {
        let response = server::app_with_storage(pool, root.to_str().unwrap().to_string())
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn divergent_mr_detail_names_both_heads(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let s = seed_siblings(&bare).await;
        seed_repo(&pool, &[("aaaa", &[s.head_a.clone(), s.head_b.clone()])]).await;

        let html = get_html(pool, root.path(), "/alice/demo/merge-requests/aaaa").await;

        // the notice renders above the revision list
        let flag = html
            .find("divergence-flag")
            .expect("divergence notice rendered");
        let first_revision = html.find("data-revision").expect("revisions rendered");
        assert!(flag < first_revision, "notice above revisions: {html}");
        assert!(html.contains("has diverged"), "notice says so: {html}");

        // both heads named by short id
        assert!(html.contains(&s.head_a[..12]), "head a short id: {html}");
        assert!(html.contains(&s.head_b[..12]), "head b short id: {html}");

        // each head links to its own revision section
        assert!(
            html.contains(&format!("href=\"#rev-{}\"", s.head_a)),
            "head a links its section: {html}"
        );
        assert!(
            html.contains(&format!("href=\"#rev-{}\"", s.head_b)),
            "head b links its section: {html}"
        );
        assert!(
            html.contains(&format!("id=\"rev-{}\"", s.head_a)),
            "head a section anchor exists: {html}"
        );
        assert!(
            html.contains(&format!("id=\"rev-{}\"", s.head_b)),
            "head b section anchor exists: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn ordinary_mr_detail_has_no_notice(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let s = seed_siblings(&bare).await;
        // linear revisions: base + its child, one line of history
        seed_repo(&pool, &[("bbbb", &[s.trunk.clone(), s.head_a.clone()])]).await;

        let html = get_html(pool, root.path(), "/alice/demo/merge-requests/bbbb").await;

        assert!(
            !html.contains("divergence-flag"),
            "no notice for ordinary MR: {html}"
        );
        assert!(!html.contains("diverged"), "no divergence text: {html}");
    }
}
