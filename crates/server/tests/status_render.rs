// story: e04s02
mod status_render {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use base64::Engine;
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

    fn basic(token: &str) -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"))
        )
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir, email: &str) -> String {
        std::env::set_var("SESSION_SECRET", "csrf-suite-secret");
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "email={email}&password=correct+horse+battery"
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

    async fn token(pool: &sqlx::PgPool, root: &tempfile::TempDir, cookie: &str) -> String {
        let mut request = Request::post("/settings/tokens")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!("label=ci&_csrf={}", csrf(cookie))))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response = app(pool.clone(), root).oneshot(request).await.unwrap();
        response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split("new_token=")
            .nth(1)
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
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

    /// alice's public repo with an MR holding two real revisions
    /// (trunk older, head latest).
    async fn seed(pool: &sqlx::PgPool, root: &tempfile::TempDir, cookie: &str) -> (String, String) {
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "name=demo&visibility=public&_csrf={}",
                csrf(cookie)
            )))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        app(pool.clone(), root).oneshot(request).await.unwrap();

        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let seed_dir = bare.join("../seed");
        std::fs::create_dir_all(&seed_dir).unwrap();
        git_in(&seed_dir, &["init", "-b", "trunk"]);
        std::fs::write(seed_dir.join("hello.txt"), "first line\n").unwrap();
        git_in(&seed_dir, &["add", "."]);
        git_in(
            &seed_dir,
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
        let trunk_commit = rev_parse(&seed_dir);
        std::fs::write(seed_dir.join("second.txt"), "second line\n").unwrap();
        git_in(&seed_dir, &["add", "."]);
        git_in(
            &seed_dir,
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
        let head_commit = rev_parse(&seed_dir);
        git_in(&seed_dir, &["push", bare.to_str().unwrap(), "trunk"]);

        let repo_id: uuid::Uuid = sqlx::query_scalar(
            "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id \
             WHERE u.handle = 'alice' AND r.name = 'demo'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let mr_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) \
             VALUES ($1, (SELECT id FROM users WHERE handle = 'alice'), 'aaaa') RETURNING id",
        )
        .bind(repo_id)
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
        (trunk_commit, head_commit)
    }

    async fn post_status(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        auth: &str,
        body: String,
    ) -> StatusCode {
        let mut request = Request::post("/alice/demo/merge-requests/aaaa/statuses")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();
        request
            .headers_mut()
            .insert("authorization", auth.parse().unwrap());
        app(pool, root).oneshot(request).await.unwrap().status()
    }

    async fn detail(pool: sqlx::PgPool, root: &tempfile::TempDir) -> String {
        let response = app(pool, root)
            .oneshot(
                Request::get("/alice/demo/merge-requests/aaaa")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn badges_render_for_latest_revision_only(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice = register(&pool, &root, "alice@example.com").await;
        let (trunk, head) = seed(&pool, &root, &alice).await;
        let tok = token(&pool, &root, &alice).await;

        let status = post_status(
            pool.clone(),
            &root,
            &basic(&tok),
            format!(
                "revision={head}&context=ci%2Fbuild&state=success&target_url=https%3A%2F%2Fci%2F1"
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let status = post_status(
            pool.clone(),
            &root,
            &basic(&tok),
            format!("revision={trunk}&context=ci%2Fbuild&state=failure"),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let html = detail(pool, &root).await;
        assert!(
            html.contains("<span data-status=\"ci/build\" data-state=\"success\">"),
            "latest revision badge rendered: {html}"
        );
        assert!(
            html.contains("href=\"https://ci/1\""),
            "target url linked: {html}"
        );
        assert!(
            !html.contains("data-state=\"failure\""),
            "older revision status must not render: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unsafe_target_urls_never_become_links(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let alice = register(&pool, &root, "alice@example.com").await;
        let (_, head) = seed(&pool, &root, &alice).await;
        let tok = token(&pool, &root, &alice).await;

        let status = post_status(
            pool.clone(),
            &root,
            &basic(&tok),
            format!(
                "revision={head}&context=ci%2Fx&state=pending&target_url=javascript%3Aalert(1)"
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let html = detail(pool, &root).await;
        assert!(
            !html.contains("href=\"javascript"),
            "javascript: url must not become a link: {html}"
        );
        assert!(
            html.contains("javascript:alert(1)"),
            "status still visible as text: {html}"
        );
    }
}
