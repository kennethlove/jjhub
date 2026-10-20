// story: e11s01
//! t2 — the revset endpoint returns matching commits.
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn git(args: &[&str], cwd: Option<&std::path::Path>) {
    let mut cmd = std::process::Command::new("git");
    cmd.args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com");
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
    server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
}

fn csrf(cookie: &str) -> String {
    let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "revset-suite".into());
    let value = cookie.strip_prefix("session=").unwrap_or(cookie);
    server::session::csrf_token(secret.as_bytes(), value)
}

async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir, email: &str) -> String {
    std::env::set_var("SESSION_SECRET", "revset-suite");
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

async fn create_repo(pool: &sqlx::PgPool, root: &tempfile::TempDir, cookie: &str, name: &str) {
    let mut request = Request::post("/repos")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!(
            "name={name}&visibility=public&_csrf={}",
            csrf(cookie)
        )))
        .unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    let response = app(pool.clone(), root).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER, "create repo");
}

// story: e11s01 — push `descriptions` as sequential commits on trunk.
async fn seed_trunk(root: &std::path::Path, owner: &str, name: &str, descriptions: &[&str]) {
    let bare = root.join(owner).join(format!("{name}.git"));
    let scratch = root.join(format!("seed-{name}"));
    git(&["clone", bare.to_str().unwrap(), scratch.to_str().unwrap()], None);
    for (i, description) in descriptions.iter().enumerate() {
        std::fs::write(scratch.join(format!("file-{i}.txt")), format!("{i}\n")).unwrap();
        git(&["add", "."], Some(&scratch));
        git(&["commit", "-m", description], Some(&scratch));
    }
    git(&["push", "origin", "HEAD:trunk"], Some(&scratch));
}

async fn revsets(
    pool: sqlx::PgPool,
    root: &tempfile::TempDir,
    owner: &str,
    name: &str,
    query: &str,
) -> (StatusCode, serde_json::Value) {
    let request = Request::get(format!("/{owner}/{name}/revsets?q={query}"))
        .body(Body::empty())
        .unwrap();
    let response = app(pool, root).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, value)
}

#[sqlx::test(migrations = "./migrations")]
async fn revset_query_returns_matching_commits(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo").await;
    seed_trunk(root.path(), "alice", "demo", &["first change", "second change"]).await;

    let (status, value) = revsets(pool.clone(), &root, "alice", "demo", "all()").await;
    assert_eq!(status, StatusCode::OK, "body: {value}");
    let commits = value.as_array().expect("JSON array");
    let descriptions: Vec<&str> = commits
        .iter()
        .map(|c| c["description"].as_str().unwrap_or_default())
        .collect();
    assert!(descriptions.contains(&"first change"), "{descriptions:?}");
    assert!(descriptions.contains(&"second change"), "{descriptions:?}");
    for commit in commits {
        assert!(commit["change_id"].is_string(), "{commit}");
        assert!(commit["commit_id"].is_string(), "{commit}");
        assert!(commit["is_open"].is_boolean(), "{commit}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn revset_query_filters_by_description(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo").await;
    seed_trunk(root.path(), "alice", "demo", &["first change", "second change"]).await;

    let (status, value) =
        revsets(pool.clone(), &root, "alice", "demo", "description(%22second%22)").await;
    assert_eq!(status, StatusCode::OK, "body: {value}");
    let commits = value.as_array().expect("JSON array");
    assert_eq!(commits.len(), 1, "body: {value}");
    assert_eq!(commits[0]["description"], "second change");
}

#[sqlx::test(migrations = "./migrations")]
async fn revset_query_caps_at_fifty(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "wide").await;
    let descriptions: Vec<String> = (0..55).map(|i| format!("change {i}")).collect();
    let refs: Vec<&str> = descriptions.iter().map(String::as_str).collect();
    seed_trunk(root.path(), "alice", "wide", &refs).await;

    let (status, value) = revsets(pool.clone(), &root, "alice", "wide", "all()").await;
    assert_eq!(status, StatusCode::OK, "body: {value}");
    let commits = value.as_array().expect("JSON array");
    assert_eq!(commits.len(), 50, "server-side cap");
}

#[sqlx::test(migrations = "./migrations")]
async fn revset_query_open_dialect_finds_unreachable_commit(pool: sqlx::PgPool) {
    use jj_lib::object_id::ObjectId;
    use jj_lib::repo::{Repo as _, StoreFactories};
    use jj_lib::workspace::default_working_copy_factories;

    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo").await;
    seed_trunk(root.path(), "alice", "demo", &["first change"]).await;

    // story: e11s01 — an open commit: visible in the view, reachable from no
    // bookmark. Created directly in the meta workspace.
    let meta = root.path().join(".meta").join("alice__demo");
    // the push happened after create_repo; import the new refs first
    server::jj_meta::sync_refs(&meta).await.expect("sync refs");
    let mut config = jj_lib::config::StackedConfig::with_defaults();
    let mut identity =
        jj_lib::config::ConfigLayer::empty(jj_lib::config::ConfigSource::Default);
    identity.set_value("user.name", "Test").ok();
    identity.set_value("user.email", "test@example.com").ok();
    config.add_layer(identity);
    let settings = jj_lib::settings::UserSettings::from_config(config).unwrap();
    let workspace = jj_lib::workspace::Workspace::load(
        &settings,
        &meta,
        &StoreFactories::default(),
        &default_working_copy_factories(),
    )
    .unwrap();
    let repo = workspace.repo_loader().load_at_head().await.unwrap();
    let trunk_id = repo
        .view()
        .local_bookmarks()
        .find_map(|(name, target)| {
            (name.as_str() == "trunk")
                .then(|| target.as_normal().cloned())
                .flatten()
        })
        .expect("trunk bookmark");
    let trunk_commit = repo.store().get_commit_async(&trunk_id).await.unwrap();
    let mut tx = repo.start_transaction();
    let open = tx
        .repo_mut()
        .new_commit(vec![trunk_id], trunk_commit.tree())
        .set_description("open work")
        .write()
        .await
        .unwrap();
    tx.commit("seed open commit").await.unwrap();
    drop(workspace);

    let (status, value) = revsets(pool.clone(), &root, "alice", "demo", "open()").await;
    assert_eq!(status, StatusCode::OK, "body: {value}");
    let commits = value.as_array().expect("JSON array");
    assert_eq!(commits.len(), 1, "body: {value}");
    assert_eq!(commits[0]["description"], "open work");
    assert_eq!(commits[0]["is_open"], true);
    assert_eq!(commits[0]["commit_id"], open.id().hex());

    // the pushed trunk commit stays closed
    let (status, value) = revsets(pool.clone(), &root, "alice", "demo", "all()").await;
    assert_eq!(status, StatusCode::OK, "body: {value}");
    let commits = value.as_array().expect("JSON array");
    let first = commits
        .iter()
        .find(|c| c["description"] == "first change")
        .expect("first change present");
    assert_eq!(first["is_open"], false);
}
