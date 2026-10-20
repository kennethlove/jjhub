// story: e11s02 — the repository-scoped search page: query box, result
// rows linking open MRs, an escaped reflected query, the e11s01 authz, and
// the e11s01 result cap. See e11s02-search-ui.md.
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
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
    std::env::set_var("SESSION_SECRET", "revset-ui-suite");
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

async fn get(
    pool: sqlx::PgPool,
    root: &tempfile::TempDir,
    path: &str,
    cookie: Option<&str>,
) -> (StatusCode, String) {
    let mut builder = Request::get(path);
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    let response = app(pool, root)
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1_048_576)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

// story: e11s02t1
#[sqlx::test(migrations = "./migrations")]
async fn revset_ui_search_page_renders_results_and_mr_links(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo").await;
    seed_trunk(
        root.path(),
        "alice",
        "demo",
        &["first change", "second change"],
    )
    .await;

    // a push already opened an MR on the second change; find its change id
    // through the same query path the page consumes
    let meta = root.path().join(".meta").join("alice__demo");
    let hits = server::jj_meta::revset_query(&meta, "trunk", "all()", 50)
        .await
        .expect("query hits");
    let linked = hits
        .iter()
        .find(|hit| hit.description == "second change")
        .expect("second change hit");
    let repo_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = 'alice' AND r.name = 'demo'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let owner_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, $3)")
        .bind(repo_id)
        .bind(owner_id)
        .bind(&linked.change_id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, body) = get(pool.clone(), &root, "/alice/demo/search?q=all()", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("name=\"q\""), "query box: {body}");
    assert!(body.contains("value=\"all()\""), "box prefilled: {body}");
    assert!(body.contains("first change"), "row: {body}");
    assert!(body.contains("second change"), "row: {body}");
    assert!(
        body.contains(&format!("/alice/demo/merge-requests/{}", linked.change_id)),
        "row links the open MR: {body}"
    );
}

// story: e11s02t1
#[sqlx::test(migrations = "./migrations")]
async fn revset_ui_search_page_escapes_query(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo").await;

    let (status, body) = get(pool.clone(), &root, "/alice/demo/search?q=%3Cscript%3E", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("<script>"), "no raw tag: {body}");
    assert!(body.contains("&lt;script&gt;"), "escaped query: {body}");
}

// story: e11s02t1
#[sqlx::test(migrations = "./migrations")]
async fn revset_ui_repo_page_links_to_search(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo").await;

    let (status, body) = get(pool.clone(), &root, "/alice/demo", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("/alice/demo/search"),
        "repo page links Search: {body}"
    );
}

// story: e11s02t1
#[sqlx::test(migrations = "./migrations")]
async fn revset_ui_private_repo_stranger_is_404(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    std::env::set_var("SESSION_SECRET", "revset-ui-suite");
    let request = Request::post("/register")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(
            "email=alice%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
        ))
        .unwrap();
    let response = app(pool.clone(), &root).oneshot(request).await.unwrap();
    let cookie = response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let csrf = server::session::csrf_token(
        std::env::var("SESSION_SECRET").unwrap().as_bytes(),
        cookie.strip_prefix("session=").unwrap(),
    );
    let mut request = Request::post("/repos")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from("name=vault&visibility=private&_csrf=".to_string() + &csrf))
        .unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    let response = app(pool.clone(), &root).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER, "create private");

    let stranger = register(&pool, &root, "mallory%40example.com").await;
    let (status, body) = get(
        pool.clone(),
        &root,
        "/alice/vault/search?q=all()",
        Some(&stranger),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(!body.contains("name=\"q\""), "no search box: {body}");
}

// story: e11s02t1
#[sqlx::test(migrations = "./migrations")]
async fn revset_ui_unknown_repo_is_404(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let (status, _) = get(pool.clone(), &root, "/nobody/demo/search?q=all()", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// story: e11s02t1 — the e11s01 cap applies to the page too
#[sqlx::test(migrations = "./migrations")]
async fn revset_ui_states_result_cap_when_truncated(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "wide").await;
    let descriptions: Vec<String> = (0..55).map(|i| format!("change {i}")).collect();
    let refs: Vec<&str> = descriptions.iter().map(String::as_str).collect();
    seed_trunk(root.path(), "alice", "wide", &refs).await;

    let (status, body) = get(pool.clone(), &root, "/alice/wide/search?q=all()", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("first 50"), "cap stated: {body}");
}
