// bug: jjhub-m9e — the web UI leaks private repositories. The transport
// and revset/operations surfaces enforce "private = 404 to strangers";
// repo, tree, blob, and merge-request pages did not. These asserts pin
// the whole read surface: stranger 404, owner/collaborator 200, public
// unchanged.
mod private_repo_visibility {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
    }

    fn csrf(cookie: &str) -> String {
        let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "private-vis-suite".into());
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(secret.as_bytes(), value)
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir, email: &str) -> String {
        std::env::set_var("SESSION_SECRET", "private-vis-suite");
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

    fn cmd(dir: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A bare store at `bare` with `file` committed on trunk.
    fn git_fixture(bare: &std::path::Path, file: &str, content: &str) {
        std::fs::create_dir_all(bare).unwrap();
        cmd(bare, &["init", "-q", "--bare"]);
        let work = tempfile::tempdir().unwrap();
        cmd(work.path(), &["init", "-q"]);
        cmd(work.path(), &["config", "user.email", "test@example.com"]);
        cmd(work.path(), &["config", "user.name", "test"]);
        let target = work.path().join(file);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, content).unwrap();
        cmd(work.path(), &["add", "."]);
        cmd(work.path(), &["commit", "-q", "-m", "fixture"]);
        cmd(work.path(), &["branch", "-M", "trunk"]);
        cmd(
            work.path(),
            &["push", "-q", bare.to_str().unwrap(), "trunk"],
        );
    }

    /// Anonymous GET: no session cookie at all.
    async fn anon(pool: sqlx::PgPool, root: &tempfile::TempDir, path: &str) -> StatusCode {
        let request = Request::get(path).body(Body::empty()).unwrap();
        app(pool, root).oneshot(request).await.unwrap().status()
    }

    /// GET with a session cookie.
    async fn as_user(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        cookie: &str,
        path: &str,
    ) -> StatusCode {
        as_user_body(pool, root, cookie, path).await.0
    }

    async fn as_user_body(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        cookie: &str,
        path: &str,
    ) -> (StatusCode, String) {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response = app(pool, root).oneshot(request).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    async fn anon_body(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        path: &str,
    ) -> (StatusCode, String) {
        let request = Request::get(path).body(Body::empty()).unwrap();
        let response = app(pool, root).oneshot(request).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    /// POST a form as the session user; returns the status.
    async fn post_as_user(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        cookie: &str,
        path: &str,
        body: String,
    ) -> StatusCode {
        let mut request = Request::post(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        app(pool, root).oneshot(request).await.unwrap().status()
    }

    async fn user_id(pool: &sqlx::PgPool, email: &str) -> uuid::Uuid {
        sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
            .bind(email)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// alice/demo = private with a real commit, alice/open = public with
    /// a real commit, bob = collaborator on demo, one open MR on demo.
    async fn setup(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> (String, String) {
        let alice = register(pool, root, "alice%40example.com").await;
        let bob = register(pool, root, "bob%40example.com").await;
        let alice_id = user_id(pool, "alice@example.com").await;
        let bob_id = user_id(pool, "bob@example.com").await;
        for (name, visibility) in [("demo", "private"), ("open", "public")] {
            sqlx::query(
                "INSERT INTO repositories (owner_id, name, visibility) VALUES ($1, $2, $3)",
            )
            .bind(alice_id)
            .bind(name)
            .bind(visibility)
            .execute(pool)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO collaborators (repo_id, user_id) SELECT id, $1 FROM repositories WHERE name = 'demo'")
            .bind(bob_id)
            .execute(pool)
            .await
            .unwrap();
        let demo_id: uuid::Uuid =
            sqlx::query_scalar("SELECT id FROM repositories WHERE name = 'demo'")
                .fetch_one(pool)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, 'zzzzz')",
        )
        .bind(demo_id)
        .bind(alice_id)
        .execute(pool)
        .await
        .unwrap();
        let storage = root.path();
        git_fixture(
            &domain::storage_path(storage.to_str().unwrap(), "alice", "demo"),
            "dir/secret.txt",
            "SESSION_SECRET=hunter2\n",
        );
        git_fixture(
            &domain::storage_path(storage.to_str().unwrap(), "alice", "open"),
            "dir/notes.txt",
            "public notes\n",
        );
        (alice, bob)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn stranger_gets_404_on_every_private_surface(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        setup(&pool, &root).await;
        for path in [
            "/alice/demo",
            "/alice/demo/tree",
            "/alice/demo/tree/trunk",
            "/alice/demo/tree/trunk/dir",
            "/alice/demo/blob/trunk/dir/secret.txt",
            "/alice/demo/merge-requests",
            "/alice/demo/merge-requests/zzzzz",
            "/alice/demo/search?q=x",
            "/alice/demo/operations",
        ] {
            assert_eq!(
                anon(pool.clone(), &root, path).await,
                StatusCode::NOT_FOUND,
                "stranger must not read {path}"
            );
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn owner_and_collaborator_still_read_private(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (alice, bob) = setup(&pool, &root).await;
        assert_eq!(
            as_user(pool.clone(), &root, &alice, "/alice/demo").await,
            StatusCode::OK,
            "owner reads the repo page"
        );
        assert_eq!(
            as_user(pool.clone(), &root, &bob, "/alice/demo/tree/trunk").await,
            StatusCode::OK,
            "collaborator reads the tree"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn public_repo_stays_open_to_strangers(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        setup(&pool, &root).await;
        assert_eq!(
            anon(pool.clone(), &root, "/alice/open").await,
            StatusCode::OK
        );
        assert_eq!(
            anon(pool.clone(), &root, "/alice/open/tree/trunk").await,
            StatusCode::OK
        );
        assert_eq!(
            anon(pool.clone(), &root, "/alice/open/blob/trunk/dir/notes.txt").await,
            StatusCode::OK
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unknown_repo_stays_404(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            anon(pool.clone(), &root, "/nobody/nope").await,
            StatusCode::NOT_FOUND
        );
    }

    // The visibility toggle must exist IN THE UI (owner-only form) and
    // the round trip private -> public -> private must move stranger
    // access with it.
    #[sqlx::test(migrations = "./migrations")]
    async fn owner_sees_flip_form_and_round_trip_moves_stranger_access(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (alice, _bob) = setup(&pool, &root).await;

        let (status, page) = as_user_body(pool.clone(), &root, &alice, "/alice/demo").await;
        assert_eq!(status, StatusCode::OK, "owner views the private repo page");
        assert!(
            page.contains("action=\"/alice/demo/settings\""),
            "owner gets the visibility form: {page}"
        );
        assert!(
            page.contains("Make public"),
            "the action names the direction it will take: {page}"
        );

        let flip = |to: &str| format!("visibility={to}&_csrf={}", csrf(&alice));
        assert_eq!(
            post_as_user(
                pool.clone(),
                &root,
                &alice,
                "/alice/demo/settings",
                flip("public")
            )
            .await,
            StatusCode::SEE_OTHER,
            "private -> public"
        );
        assert_eq!(
            anon(pool.clone(), &root, "/alice/demo").await,
            StatusCode::OK,
            "public repos are open to strangers"
        );
        assert_eq!(
            post_as_user(
                pool.clone(),
                &root,
                &alice,
                "/alice/demo/settings",
                flip("private")
            )
            .await,
            StatusCode::SEE_OTHER,
            "public -> private"
        );
        assert_eq!(
            anon(pool.clone(), &root, "/alice/demo").await,
            StatusCode::NOT_FOUND,
            "stranger access goes back behind the wall"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn non_owners_never_see_the_flip_form(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (_alice, bob) = setup(&pool, &root).await;

        let (status, page) = anon_body(pool.clone(), &root, "/alice/open").await;
        assert_eq!(status, StatusCode::OK, "public repo reads anonymously");
        assert!(
            !page.contains("/alice/open/settings"),
            "a stranger never sees the owner form: {page}"
        );

        let (status, page) = as_user_body(pool.clone(), &root, &bob, "/alice/demo").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "collaborator reads the private repo"
        );
        assert!(
            !page.contains("/alice/demo/settings"),
            "a collaborator never sees the owner form: {page}"
        );
    }
}
