// story: e04s01
mod comment_flow {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
    }

    fn csrf(cookie: &str) -> String {
        let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "csrf-suite-secret".into());
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(secret.as_bytes(), value)
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir, email: &str) -> String {
        std::env::set_var("SESSION_SECRET", "csrf-suite-secret");
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "email={email}&password=correct+horse+battery&password_confirm=correct+horse+battery"
            )))
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

    async fn post_form(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        path: &str,
        cookie: Option<&str>,
        body: String,
    ) -> axum::response::Response {
        let mut request = Request::post(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", cookie.parse().unwrap());
        }
        app(pool, root).oneshot(request).await.unwrap()
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

    /// Seed trunk + a second commit and push both into the bare repo.
    async fn seed(bare: &std::path::Path) -> (String, String) {
        let seed = bare.join("../seed");
        std::fs::create_dir_all(&seed).unwrap();
        git_in(&seed, &["init", "-b", "trunk"]);
        std::fs::write(seed.join("hello.txt"), "first line\n").unwrap();
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
                "one",
            ],
        );
        let trunk_commit = rev_parse(&seed);
        std::fs::write(seed.join("second.txt"), "second line\n").unwrap();
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
                "two",
            ],
        );
        let head_commit = rev_parse(&seed);
        git_in(&seed, &["push", bare.to_str().unwrap(), "trunk"]);
        (trunk_commit, head_commit)
    }

    /// Create alice's public repo via the real form, then seed an MR with
    /// one revision per commit. Returns (repo_path_change, trunk, head).
    async fn seed_public_mr(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        alice: &str,
    ) -> (String, String, String) {
        let response = post_form(
            pool.clone(),
            root,
            "/repos",
            Some(alice),
            format!("name=demo&visibility=public&_csrf={}", csrf(alice)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let (trunk_commit, head_commit) = seed(&bare).await;

        let repo_id: uuid::Uuid = sqlx::query_scalar(
            "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id \
             WHERE u.handle = 'alice' AND r.name = 'demo'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let owner_id: uuid::Uuid =
            sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
                .fetch_one(pool)
                .await
                .unwrap();
        let mr_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) \
             VALUES ($1, $2, 'aaaa') RETURNING id",
        )
        .bind(repo_id)
        .bind(owner_id)
        .fetch_one(pool)
        .await
        .unwrap();
        for commit in [&trunk_commit, &head_commit] {
            sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2)")
                .bind(mr_id)
                .bind(commit)
                .execute(pool)
                .await
                .unwrap();
        }
        ("aaaa".to_string(), trunk_commit, head_commit)
    }

    async fn body_of(response: axum::response::Response) -> String {
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn comment_renders_pinned_to_its_revision(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice = register(&pool, &root, "alice@example.com").await;
        let (change, trunk, head) = seed_public_mr(&pool, &root, &alice).await;

        // comment on the trunk revision
        let response = post_form(
            pool.clone(),
            &root,
            &format!("/alice/demo/merge-requests/{change}/comments"),
            Some(&alice),
            format!(
                "revision={trunk}&body=Trunk+looks+wrong&_csrf={}",
                csrf(&alice)
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        // comment on the head revision
        let response = post_form(
            pool.clone(),
            &root,
            &format!("/alice/demo/merge-requests/{change}/comments"),
            Some(&alice),
            format!(
                "revision={head}&body=Second+line+fine&_csrf={}",
                csrf(&alice)
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let response = post_form(
            pool.clone(),
            &root,
            &format!("/alice/demo/merge-requests/{change}/comments"),
            Some(&alice),
            format!(
                "revision={head}&body=%3Cscript%3Ealert(1)%3C%2Fscript%3E&_csrf={}",
                csrf(&alice)
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let html = body_of(
            app(pool.clone(), &root)
                .oneshot(
                    Request::get(format!("/alice/demo/merge-requests/{change}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;

        // sections render newest-first: head section opens before trunk section
        let head_pos = html
            .find(&format!("<section data-revision=\"{head}\">"))
            .expect("head section rendered");
        let trunk_pos = html
            .find(&format!("<section data-revision=\"{trunk}\">"))
            .expect("trunk section rendered");
        assert!(head_pos < trunk_pos, "newest first: {html}");

        // each comment sits inside its own revision's section (pinned, never re-anchored)
        let trunk_comment = html
            .find("Trunk looks wrong")
            .expect("trunk comment rendered");
        assert!(
            trunk_comment > trunk_pos,
            "trunk comment pinned to trunk section: {html}"
        );
        let head_comment = html
            .find("Second line fine")
            .expect("head comment rendered");
        assert!(
            head_comment > head_pos && head_comment < trunk_pos,
            "head comment pinned to head section: {html}"
        );

        // comment bodies are escaped, never live html
        assert!(!html.contains("<script>alert(1)</script>"), "XSS: {html}");
        assert!(html.contains("&lt;script&gt;"), "escaped body: {html}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn private_repo_comments_gated(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice = register(&pool, &root, "alice@example.com").await;
        let bob = register(&pool, &root, "bob@example.com").await;

        // owner creates the repo, then flips it private
        let response = post_form(
            pool.clone(),
            &root,
            "/repos",
            Some(&alice),
            format!("name=demo&visibility=public&_csrf={}", csrf(&alice)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let response = post_form(
            pool.clone(),
            &root,
            "/alice/demo/settings",
            Some(&alice),
            format!("visibility=private&_csrf={}", csrf(&alice)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let repo_id: uuid::Uuid = sqlx::query_scalar(
            "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id \
             WHERE u.handle = 'alice' AND r.name = 'demo'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let mr_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) \
             VALUES ($1, (SELECT id FROM users WHERE handle = 'alice'), 'aaaa') RETURNING id",
        )
        .bind(repo_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, 'caaaa1')")
            .bind(mr_id)
            .execute(&pool)
            .await
            .unwrap();

        // logged-in stranger cannot comment on a private MR
        let response = post_form(
            pool.clone(),
            &root,
            "/alice/demo/merge-requests/aaaa/comments",
            Some(&bob),
            format!("revision=caaaa1&body=hi&_csrf={}", csrf(&bob)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // owner can comment
        let response = post_form(
            pool.clone(),
            &root,
            "/alice/demo/merge-requests/aaaa/comments",
            Some(&alice),
            format!("revision=caaaa1&body=owner+here&_csrf={}", csrf(&alice)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        // add bob as collaborator: now he can comment
        let response = post_form(
            pool.clone(),
            &root,
            "/alice/demo/settings/collaborators",
            Some(&alice),
            format!("email=bob%40example.com&_csrf={}", csrf(&alice)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let response = post_form(
            pool,
            &root,
            "/alice/demo/merge-requests/aaaa/comments",
            Some(&bob),
            format!("revision=caaaa1&body=now+mine&_csrf={}", csrf(&bob)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn anonymous_comment_redirects_to_login(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice = register(&pool, &root, "alice@example.com").await;
        let (change, trunk, _) = seed_public_mr(&pool, &root, &alice).await;

        let response = post_form(
            pool,
            &root,
            &format!("/alice/demo/merge-requests/{change}/comments"),
            None,
            format!("revision={trunk}&body=anon"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(location, "/login");
    }
}
