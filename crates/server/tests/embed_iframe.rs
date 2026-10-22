// story: e14s01 — the MR detail page embeds the change's kata view in a
// same-origin iframe, and the embed route answers with a frame policy.
mod embed_iframe {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    /// Deliberately unroutable upstream host: it must never appear in any
    /// jjhub HTML, only in the server's own proxy configuration.
    const UPSTREAM: &str = "http://kata-upstream-sidecar.invalid:7778";

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

    /// alice/demo with one open MR for change aaaa and colocated jj meta.
    async fn setup(pool: &sqlx::PgPool, root: &std::path::Path) {
        let bare = domain::storage_path(root.to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let meta = domain::meta_path(root.to_str().unwrap(), "alice", "demo");
        tokio::fs::create_dir_all(meta.join(".jj")).await.unwrap();
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git_in(&seed, &["init", "-b", "trunk"]);
        std::fs::write(seed.join("f.txt"), "x\n").unwrap();
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
        git_in(&seed, &["push", bare.to_str().unwrap(), "trunk"]);

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
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, 'aaaa') RETURNING id",
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
    }

    fn app(pool: sqlx::PgPool, root: &std::path::Path, upstream: &str) -> axum::Router {
        server::app_with_kata(
            pool,
            root.to_str().unwrap().to_string(),
            upstream.to_string(),
            String::new(),
        )
    }

    async fn body_of(response: axum::response::Response) -> String {
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    async fn spawn_mock_kata() -> (String, Arc<Mutex<bool>>) {
        let hit = Arc::new(Mutex::new(false));
        let hit2 = hit.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = axum::Router::new().fallback(move |_req: axum::extract::Request| {
            let hit = hit2.clone();
            async move {
                *hit.lock().unwrap() = true;
                (StatusCode::OK, "kata-ok").into_response()
            }
        });
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{addr}"), hit)
    }

    async fn register(pool: &sqlx::PgPool, root: &std::path::Path, upstream: &str) -> String {
        std::env::set_var("SESSION_SECRET", "e14s01-suite");
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "email=alice@example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
            ))
            .unwrap();
        let response = app(pool.clone(), root, upstream)
            .oneshot(request)
            .await
            .unwrap();
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

    // story: e14s01 (e14s01t1) — the MR page embeds the change's kata view
    // through the same-origin forwarder; the sidecar host never leaks; the
    // e05s04 Review link stays as the no-frame fallback.
    #[sqlx::test(migrations = "./migrations")]
    async fn mr_page_embeds_change_iframe(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        setup(&pool, root.path()).await;
        let response = app(pool.clone(), root.path(), UPSTREAM)
            .oneshot(
                Request::get("/alice/demo/merge-requests/aaaa")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_of(response).await;

        let target = "/r/alice__demo/browse?change=aaaa";
        let frame = body
            .split("<iframe")
            .nth(1)
            .and_then(|rest| rest.split("</iframe>").next())
            .unwrap_or_default();
        assert!(
            frame.contains(&format!("src=\"{target}\"")),
            "iframe embeds the change same-origin: {body}"
        );
        assert!(
            body.contains(&format!("href=\"{target}\"")),
            "e05s04 Review link stays as fallback: {body}"
        );
        assert!(
            !body.contains("kata-upstream-sidecar.invalid"),
            "no sidecar host anywhere in the body: {body}"
        );
    }

    // story: e14s01 (e14s01t2) — the embed route answers with a frame
    // policy; direct navigation still works; ordinary jjhub routes are
    // unchanged.
    #[sqlx::test(migrations = "./migrations")]
    async fn embed_csp_frame_ancestors(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let (upstream, hit) = spawn_mock_kata().await;
        let cookie = register(&pool, root.path(), &upstream).await;
        sqlx::query(
            "INSERT INTO repositories (owner_id, name) \
             SELECT id, 'demo' FROM users WHERE email = 'alice@example.com'",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut request = Request::get("/r/alice__demo/browse")
            .body(Body::empty())
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response = app(pool.clone(), root.path(), &upstream)
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "direct navigation ok");
        assert!(*hit.lock().unwrap(), "forwarder reached the sidecar");
        let csp = response.headers().get(header::CONTENT_SECURITY_POLICY);
        assert_eq!(
            csp.and_then(|v| v.to_str().ok()),
            Some("frame-ancestors 'self'"),
            "embed route carries the frame policy"
        );

        // the forwarder's asset path is embed surface too
        let response = app(pool.clone(), root.path(), &upstream)
            .oneshot(
                Request::get("/assets/app-ABC.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let csp = response.headers().get(header::CONTENT_SECURITY_POLICY);
        assert_eq!(
            csp.and_then(|v| v.to_str().ok()),
            Some("frame-ancestors 'self'")
        );

        // ordinary jjhub pages are unchanged
        let response = app(pool.clone(), root.path(), &upstream)
            .oneshot(Request::get("/login").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(
            !response
                .headers()
                .contains_key(header::CONTENT_SECURITY_POLICY),
            "ordinary routes carry no frame policy"
        );
    }
}
