// story: e08s03 — acceptance (e08s03t2): conflict flags are PRESENTATION
// only — the landing page flags a conflicted trunk, MR detail flags a
// conflicted revision commit, and both pages still return 200; a clean
// repo and a clean revision carry no flag.
mod conflict_support;

mod conflict_ui {
    use super::conflict_support;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::path::Path;
    use std::process::Command;
    use tower::ServiceExt;

    const FLAG: &str = "conflict-flag";

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-c")
            .arg("safe.directory=*")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git ran");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        std::env::set_var("SESSION_SECRET", "conflict-ui-suite");
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "email=alice%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
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

    fn csrf(cookie: &str) -> String {
        let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "conflict-ui-suite".into());
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(secret.as_bytes(), value)
    }

    async fn create_repo(pool: &sqlx::PgPool, root: &tempfile::TempDir, cookie: &str) {
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
        let response = app(pool.clone(), root).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    async fn get(pool: sqlx::PgPool, root: &tempfile::TempDir, path: &str) -> String {
        let response = app(pool, root)
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        String::from_utf8_lossy(&body).to_string()
    }

    /// Push one clean commit to trunk in the server's bare repo; returns its
    /// id (what the MR revision row will point at).
    async fn seed_clean_commit(root: &tempfile::TempDir) -> String {
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        let seed = root.path().join("clean-seed");
        git(
            root.path(),
            &["clone", bare.to_str().unwrap(), seed.to_str().unwrap()],
        );
        git(&seed, &["checkout", "-b", "trunk"]);
        std::fs::write(seed.join("clean.txt"), "clean\n").unwrap();
        git(&seed, &["add", "."]);
        git(
            &seed,
            &[
                "-c",
                "user.email=a@e.com",
                "-c",
                "user.name=A",
                "commit",
                "-m",
                "clean",
            ],
        );
        let sha = git(&seed, &["rev-parse", "HEAD"]);
        git(&seed, &["push", bare.to_str().unwrap(), "trunk"]);
        sha
    }

    async fn seed_mr_revision(pool: &sqlx::PgPool, change_id: &str, commit: &str) {
        let (user_id, repo_id): (uuid::Uuid, uuid::Uuid) = sqlx::query_as(
            "SELECT u.id, r.id FROM users u JOIN repositories r ON r.owner_id = u.id \
             WHERE u.handle = 'alice' AND r.name = 'demo'",
        )
        .fetch_one(pool)
        .await
        .expect("alice/demo rows");
        let mr_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(repo_id)
        .bind(user_id)
        .bind(change_id)
        .fetch_one(pool)
        .await
        .expect("mr row");
        sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2)")
            .bind(mr_id)
            .bind(commit)
            .execute(pool)
            .await
            .expect("revision row");
    }

    // AC: the landing page flags a conflicted trunk commit and still 200s —
    // no error string, no 500.
    #[sqlx::test(migrations = "./migrations")]
    async fn conflicted_trunk_flags_landing_page(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        conflict_support::build(&bare);

        let body = get(pool, &root, "/alice/demo").await;
        assert!(
            body.contains(FLAG),
            "landing page carries the conflict flag for a conflicted trunk"
        );
    }

    // AC: a clean repo carries no conflict flag.
    #[sqlx::test(migrations = "./migrations")]
    async fn clean_landing_carries_no_flag(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;
        seed_clean_commit(&root).await;

        let body = get(pool, &root, "/alice/demo").await;
        assert!(
            !body.contains(FLAG),
            "clean landing page carries no conflict flag"
        );
    }

    // AC: a conflicted revision commit is flagged on the MR detail page and
    // the page still 200s.
    #[sqlx::test(migrations = "./migrations")]
    async fn conflicted_revision_flags_mr_detail(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        let fixture = conflict_support::build(&bare);
        seed_mr_revision(&pool, "cccc", &fixture.conflicted).await;

        let body = get(pool, &root, "/alice/demo/merge-requests/cccc").await;
        assert!(
            body.contains(FLAG),
            "MR detail flags the conflicted revision commit"
        );
    }

    // AC: a clean revision carries no flag.
    #[sqlx::test(migrations = "./migrations")]
    async fn clean_revision_carries_no_flag(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;
        let clean = seed_clean_commit(&root).await;
        seed_mr_revision(&pool, "dddd", &clean).await;

        let body = get(pool, &root, "/alice/demo/merge-requests/dddd").await;
        assert!(
            !body.contains(FLAG),
            "clean revision carries no conflict flag"
        );
    }
}
