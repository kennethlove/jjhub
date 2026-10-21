// story: e08s01 — acceptance (e08s01t2): smart-http upload-pack advertises
// `refs/jj/keep/*`, a clone fetches every object a conflicted commit needs,
// receive-pack accepts a push containing a conflicted commit, and the
// existing identity rejection still fires for the identity case. Conflict
// state alone is never a rejection.
mod conflict_support;

mod conflict_refs {
    use super::conflict_support;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        std::env::set_var("SESSION_SECRET", "conflict-refs-suite");
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
        let secret =
            std::env::var("SESSION_SECRET").unwrap_or_else(|_| "conflict-refs-suite".into());
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

    /// register + create + build the fixture into the server's bare repo;
    /// returns the app root and the built fixture.
    async fn seeded_repo(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
    ) -> conflict_support::Fixture {
        let cookie = register(pool, root).await;
        create_repo(pool, root, &cookie).await;
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        conflict_support::build(&bare)
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

    async fn alice_token(pool: &sqlx::PgPool) -> String {
        let alice: (uuid::Uuid,) = sqlx::query_as("SELECT id FROM users WHERE handle = 'alice'")
            .fetch_one(pool)
            .await
            .expect("alice row");
        server::token::create(pool, alice.0, "e08s01")
            .await
            .plaintext
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn upload_pack_advertises_keep_refs(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let fixture = seeded_repo(&pool, &root).await;

        let body = get(
            pool,
            &root,
            "/git/alice/demo.git/info/refs?service=git-upload-pack",
        )
        .await;
        for commit in [&fixture.parent, &fixture.conflicted] {
            let keep_ref = format!("refs/jj/keep/{commit}");
            assert!(
                body.contains(&keep_ref),
                "upload-pack must advertise {keep_ref}:\n{body}"
            );
        }
        assert!(
            body.contains("refs/heads/trunk"),
            "trunk advertised alongside the keep refs:\n{body}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn clone_fetches_every_conflict_object(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let fixture = seeded_repo(&pool, &root).await;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = app(pool, &root);
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let clone = tempfile::tempdir().unwrap();
        let clone_path = clone.path().to_path_buf();
        let url = format!("http://{addr}/git/alice/demo.git");
        let out = tokio::task::spawn_blocking(move || {
            std::process::Command::new("git")
                .args(["clone", &url])
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .arg(clone_path)
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(
            out.status.success(),
            "git clone failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let fsck = tokio::task::spawn_blocking({
            let clone = clone.path().to_path_buf();
            move || {
                std::process::Command::new("git")
                    .args(["fsck"])
                    .current_dir(clone)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_SYSTEM", "/dev/null")
                    .output()
                    .unwrap()
            }
        })
        .await
        .unwrap();
        assert!(
            fsck.status.success(),
            "clone fsck: {}",
            String::from_utf8_lossy(&fsck.stderr)
        );

        let raw = tokio::task::spawn_blocking({
            let clone = clone.path().to_path_buf();
            move || {
                std::process::Command::new("git")
                    .args(["cat-file", "commit", "HEAD"])
                    .current_dir(clone)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_SYSTEM", "/dev/null")
                    .output()
                    .unwrap()
            }
        })
        .await
        .unwrap();
        let raw = String::from_utf8_lossy(&raw.stdout).to_string();
        assert!(
            raw.contains("jj:trees "),
            "cloned head keeps the jj:trees header:\n{raw}"
        );

        let listing = tokio::task::spawn_blocking({
            let clone = clone.path().to_path_buf();
            move || {
                std::process::Command::new("git")
                    .args(["ls-tree", "HEAD"])
                    .current_dir(clone)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_SYSTEM", "/dev/null")
                    .output()
                    .unwrap()
            }
        })
        .await
        .unwrap();
        let listing = String::from_utf8_lossy(&listing.stdout).to_string();
        assert!(
            listing.contains(".jjconflict-base-0"),
            "clone carries the conflict trees:\n{listing}"
        );

        // the keep refs are fetchable too — GC must never prune them
        let fetch = tokio::task::spawn_blocking({
            let clone = clone.path().to_path_buf();
            move || {
                std::process::Command::new("git")
                    .args(["fetch", "origin", "+refs/jj/keep/*:refs/jj/keep/*"])
                    .current_dir(clone)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_SYSTEM", "/dev/null")
                    .output()
                    .unwrap()
            }
        })
        .await
        .unwrap();
        assert!(
            fetch.status.success(),
            "keep-ref fetch: {}",
            String::from_utf8_lossy(&fetch.stderr)
        );
        let resolved = tokio::task::spawn_blocking({
            let clone = clone.path().to_path_buf();
            let ref_name = format!("refs/jj/keep/{}", fixture.conflicted);
            move || {
                std::process::Command::new("git")
                    .args(["rev-parse", "--verify", &ref_name])
                    .current_dir(clone)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_SYSTEM", "/dev/null")
                    .output()
                    .unwrap()
            }
        })
        .await
        .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&resolved.stdout).trim(),
            fixture.conflicted
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn receive_pack_accepts_conflicted_push(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;
        let token = alice_token(&pool).await;

        // the push source: a standalone bare carrying the conflicted chain
        let source = tempfile::tempdir().unwrap();
        let fixture = conflict_support::build(source.path());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = app(pool, &root);
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let url = format!("http://x-access-token:{token}@{addr}/git/alice/demo.git");
        let source_path = source.path().to_path_buf();
        let out = tokio::task::spawn_blocking(move || {
            std::process::Command::new("git")
                .args([
                    "-C",
                    source_path.to_str().unwrap(),
                    "push",
                    &url,
                    "+refs/heads/trunk:refs/heads/trunk",
                    "+refs/jj/keep/*:refs/jj/keep/*",
                ])
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(
            out.status.success(),
            "conflicted push rejected: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        let trunk = std::process::Command::new("git")
            .args(["rev-parse", "--verify", "refs/heads/trunk"])
            .current_dir(&bare)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&trunk.stdout).trim(),
            fixture.conflicted
        );
        let keep = std::process::Command::new("git")
            .args([
                "rev-parse",
                "--verify",
                &format!("refs/jj/keep/{}", fixture.conflicted),
            ])
            .current_dir(&bare)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert!(
            keep.status.success(),
            "keep ref lands with the push: {}",
            String::from_utf8_lossy(&keep.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&keep.stdout).trim(),
            fixture.conflicted
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn identity_rejection_still_fires_for_conflicted_push(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");

        let (repo_id, owner_id): (uuid::Uuid, uuid::Uuid) = sqlx::query_as(
            "SELECT r.id, u.id FROM repositories r JOIN users u ON u.id = r.owner_id \
             WHERE u.handle = 'alice' AND r.name = 'demo'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, $3)",
        )
        .bind(repo_id)
        .bind(owner_id)
        .bind(conflict_support::CONFLICT_CHANGE_ID)
        .execute(&pool)
        .await
        .unwrap();

        // foreign author rewrites the conflicted change of the open MR
        let fixture = conflict_support::build_as(
            &bare,
            "Bob",
            "bob@example.com",
            conflict_support::CONFLICT_CHANGE_ID,
        );
        let updates = vec![server::receive_guard::PushedRef {
            old: "0".repeat(40),
            new: fixture.conflicted.clone(),
            ref_name: "refs/heads/trunk".to_string(),
        }];
        let rejections = server::receive_guard::admit_push(&pool, repo_id, &updates, &bare)
            .await
            .unwrap();
        assert_eq!(
            rejections.len(),
            1,
            "identity rule fires on the conflicted push: {rejections:?}"
        );
        assert!(
            rejections[0].reason.contains("open merge request"),
            "identity reason: {}",
            rejections[0].reason
        );

        // the same conflicted commit under the owner's identity is admitted —
        // conflict state alone is never a rejection case (e08s01)
        let owner_dir = tempfile::tempdir().unwrap();
        let fixture = conflict_support::build_as(
            owner_dir.path(),
            "Alice",
            "alice@example.com",
            conflict_support::CONFLICT_CHANGE_ID,
        );
        let updates = vec![server::receive_guard::PushedRef {
            old: "0".repeat(40),
            new: fixture.conflicted.clone(),
            ref_name: "refs/heads/trunk".to_string(),
        }];
        let rejections =
            server::receive_guard::admit_push(&pool, repo_id, &updates, owner_dir.path())
                .await
                .unwrap();
        assert!(
            rejections.is_empty(),
            "conflicted push under the owner's identity admits: {rejections:?}"
        );
    }
}
