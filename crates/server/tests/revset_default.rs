// story: e11s02 — the default query and the empty state: absent or empty
// `q` runs `open()`, and a valid query with zero matches renders a
// dedicated empty state, never a bare table. See e11s02-search-ui.md.
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

async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir, email: &str) -> String {
    std::env::set_var("SESSION_SECRET", "revset-default-suite");
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
    let csrf = {
        let secret = std::env::var("SESSION_SECRET").unwrap();
        let value = cookie.strip_prefix("session=").unwrap();
        server::session::csrf_token(secret.as_bytes(), value)
    };
    let mut request = Request::post("/repos")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!(
            "name={name}&visibility=public&_csrf={csrf}"
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
    git(
        &["clone", bare.to_str().unwrap(), scratch.to_str().unwrap()],
        None,
    );
    for (i, description) in descriptions.iter().enumerate() {
        std::fs::write(scratch.join(format!("file-{i}.txt")), format!("{i}\n")).unwrap();
        git(&["add", "."], Some(&scratch));
        git(&["commit", "-m", description], Some(&scratch));
    }
    git(&["push", "origin", "HEAD:trunk"], Some(&scratch));
}

// story: e11s02 — an open commit lives in the meta workspace, reachable
// from no bookmark; same seed as the e11s01 open-dialect test.
async fn seed_open(root: &std::path::Path, owner: &str, name: &str) {
    use jj_lib::repo::{Repo as _, StoreFactories};
    use jj_lib::workspace::default_working_copy_factories;

    let meta = root.join(".meta").join(format!("{owner}__{name}"));
    server::jj_meta::sync_refs(&meta).await.expect("sync refs");
    let mut config = jj_lib::config::StackedConfig::with_defaults();
    let mut identity = jj_lib::config::ConfigLayer::empty(jj_lib::config::ConfigSource::Default);
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
    tx.repo_mut()
        .new_commit(vec![trunk_id], trunk_commit.tree())
        .set_description("open work")
        .write()
        .await
        .unwrap();
    tx.commit("seed open commit").await.unwrap();
    drop(workspace);
}

async fn get(pool: sqlx::PgPool, root: &tempfile::TempDir, path: &str) -> (StatusCode, String) {
    let response = app(pool, root)
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1_048_576)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

// story: e11s02t2
#[sqlx::test(migrations = "./migrations")]
async fn revset_default_absent_or_empty_q_runs_open(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo").await;
    seed_trunk(root.path(), "alice", "demo", &["trunk work"]).await;
    seed_open(root.path(), "alice", "demo").await;

    let (status, body) = get(pool.clone(), &root, "/alice/demo/search").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("value=\"open()\""), "box prefilled: {body}");
    assert!(body.contains("open work"), "open change listed: {body}");

    let (status, body) = get(pool.clone(), &root, "/alice/demo/search?q=").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("open work"), "empty q too: {body}");
}

// story: e11s02t2
#[sqlx::test(migrations = "./migrations")]
async fn revset_default_zero_matches_renders_empty_state(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo").await;
    seed_trunk(root.path(), "alice", "demo", &["first change"]).await;

    let (status, body) = get(
        pool.clone(),
        &root,
        "/alice/demo/search?q=description(nope-nothing)",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("No revisions match"), "empty state: {body}");
    assert!(
        body.contains("nope-nothing"),
        "empty state names the query: {body}"
    );
    assert!(!body.contains("<ul>"), "no bare results table: {body}");
}
