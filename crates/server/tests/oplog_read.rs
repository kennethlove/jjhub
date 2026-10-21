// story: e09s01 — the operation-log read path: the jj_meta helper reads
// the colocated .meta store newest-first with OperationMetadata fields,
// the list page renders capped, escaped rows behind the repo read rules,
// and a missing .meta store is a visible empty state. See
// e09s01-oplog-read.md.
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
    std::env::set_var("SESSION_SECRET", "oplog-suite");
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

async fn create_repo(
    pool: &sqlx::PgPool,
    root: &tempfile::TempDir,
    cookie: &str,
    name: &str,
    visibility: &str,
) {
    let csrf = {
        let secret = std::env::var("SESSION_SECRET").unwrap();
        let value = cookie.strip_prefix("session=").unwrap();
        server::session::csrf_token(secret.as_bytes(), value)
    };
    let mut request = Request::post("/repos")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!(
            "name={name}&visibility={visibility}&_csrf={csrf}"
        )))
        .unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    let response = app(pool.clone(), root).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER, "create repo");
}

async fn get(
    pool: &sqlx::PgPool,
    root: &tempfile::TempDir,
    path: &str,
    cookie: Option<&str>,
) -> (StatusCode, String) {
    let mut builder = Request::get(path);
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    let response = app(pool.clone(), root)
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1_048_576)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

fn jj_settings() -> jj_lib::settings::UserSettings {
    use jj_lib::config::{ConfigLayer, ConfigSource, StackedConfig};
    let mut config = StackedConfig::with_defaults();
    let mut identity = ConfigLayer::empty(ConfigSource::Default);
    identity.set_value("user.name", "op-tester").ok();
    identity.set_value("user.email", "op-tester@invalid").ok();
    // jj defaults operation identity to empty; populate it so the audit
    // fields have something to read.
    identity.set_value("operation.username", "op-tester").ok();
    identity.set_value("operation.hostname", "op-host").ok();
    config.add_layer(identity);
    jj_lib::settings::UserSettings::from_config(config).expect("jj settings")
}

// story: e09s01 — record operations directly through jj transactions so
// each one carries a distinct description in its OperationMetadata.
async fn record_operations(meta: &std::path::Path, descriptions: &[&str]) {
    let workspace = jj_lib::workspace::Workspace::load(
        &jj_settings(),
        meta,
        &jj_lib::repo::StoreFactories::default(),
        &jj_lib::workspace::default_working_copy_factories(),
    )
    .expect("load meta workspace");
    for description in descriptions {
        let repo = workspace
            .repo_loader()
            .load_at_head()
            .await
            .expect("load_at_head");
        let mut tx = repo.start_transaction();
        tx.commit(format!("jjhub-test: {description}"))
            .await
            .expect("commit operation");
    }
}

// story: e09s01t1
#[sqlx::test(migrations = "./migrations")]
async fn oplog_read_helper_reads_newest_first_with_metadata(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo", "public").await;
    let meta = root.path().join(".meta").join("alice__demo");
    record_operations(&meta, &["alpha op", "beta op", "gamma op"]).await;

    let entries = server::jj_meta::load_operations(&meta, 50)
        .await
        .expect("load_operations");
    let descriptions: Vec<&str> = entries
        .iter()
        .map(|entry| entry.description.as_str())
        .collect();
    assert!(
        descriptions
            .starts_with(&["jjhub-test: gamma op", "jjhub-test: beta op", "jjhub-test: alpha op"]),
        "newest first: {descriptions:?}"
    );
    let newest = &entries[0];
    assert_eq!(newest.username, "op-tester");
    assert!(!newest.hostname.is_empty(), "hostname populated");
    assert!(!newest.time.is_empty(), "time populated");
    assert!(!newest.id.is_empty(), "operation id populated");
}

// story: e09s01t2
#[sqlx::test(migrations = "./migrations")]
async fn oplog_page_newest_first_capped_and_escaped(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo", "public").await;
    let meta = root.path().join(".meta").join("alice__demo");
    let mut descriptions: Vec<String> = (0..59).map(|i| format!("audit change {i}")).collect();
    descriptions.push("<script>alert(\"owned\")</script>".to_string());
    let borrowed: Vec<&str> = descriptions.iter().map(String::as_str).collect();
    record_operations(&meta, &borrowed).await;

    let (status, body) = get(&pool, &root, "/alice/demo/operations", None).await;
    assert_eq!(status, StatusCode::OK);
    // story: e09s01 — exactly 50 rows, newest first, capped server-side.
    assert_eq!(body.matches("class=\"op-row\"").count(), 50, "row cap");
    let newest = body.find("audit change 58").expect("newest row");
    let older = body.find("audit change 57").expect("second row");
    assert!(newest < older, "newest-first row order");
    assert!(
        !body.contains("audit change 9"),
        "operations past the cap stay off the page"
    );
    // story: e09s01 — descriptions are attacker-influenced; escaped, and
    // the identity column renders.
    assert!(body.contains("&lt;script&gt;"), "escaped description");
    assert!(!body.contains("<script>"), "no raw markup survives");
    assert!(body.contains("op-tester@"), "username@hostname identity");
}

// story: e09s01t2 — repo read rules apply to the log page.
#[sqlx::test(migrations = "./migrations")]
async fn oplog_page_private_repo_404_for_strangers(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let alice = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &alice, "secret", "private").await;
    let bob = register(&pool, &root, "bob%40example.com").await;

    let (status, _) = get(&pool, &root, "/alice/secret/operations", Some(&bob)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "private repo, stranger");
    let (status, _) = get(&pool, &root, "/alice/secret/operations", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "private repo, anonymous");
    let (status, _) = get(&pool, &root, "/alice/secret/operations", Some(&alice)).await;
    assert_eq!(status, StatusCode::OK, "private repo, owner");
}

// story: e09s01t3
#[sqlx::test(migrations = "./migrations")]
async fn oplog_missing_meta_renders_empty_state(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo", "public").await;
    std::fs::remove_dir_all(root.path().join(".meta").join("alice__demo")).unwrap();

    let (status, body) = get(&pool, &root, "/alice/demo/operations", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("no operation log"),
        "explicit empty state, got: {body}"
    );
    assert!(
        !body.contains("workspace load") && !body.contains("No such file"),
        "no raw jj-lib error text"
    );
}
