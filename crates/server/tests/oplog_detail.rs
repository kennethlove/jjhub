// story: e09s02 — the operation detail page: the jj_meta helper reads one
// operation plus its view's ref changes (prior to new commit ids from
// added_ids/removed_ids), the page renders actor, time, description,
// linked parents, and head ids behind the repo read rules, conflicted ref
// targets carry the conflict flag, an unknown id is a 404, and a missing
// .meta store is the list page's "no operation log" empty state. See
// e09s02-oplog-detail.md.
use axum::body::Body;
use axum::http::{Request, StatusCode};
use jj_lib::repo::Repo as _;
use tower::ServiceExt;

fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
    server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
}

async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir, email: &str) -> String {
    std::env::set_var("SESSION_SECRET", "opdetail-suite");
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
    identity.set_value("operation.username", "op-tester").ok();
    identity.set_value("operation.hostname", "op-host").ok();
    config.add_layer(identity);
    jj_lib::settings::UserSettings::from_config(config).expect("jj settings")
}

async fn load_meta_workspace(meta: &std::path::Path) -> jj_lib::workspace::Workspace {
    jj_lib::workspace::Workspace::load(
        &jj_settings(),
        meta,
        &jj_lib::repo::StoreFactories::default(),
        &jj_lib::workspace::default_working_copy_factories(),
    )
    .expect("load meta workspace")
}

// story: e09s01 — record operations directly through jj transactions so
// each one carries a distinct description in its OperationMetadata.
async fn record_operations(meta: &std::path::Path, descriptions: &[&str]) {
    let workspace = load_meta_workspace(meta).await;
    for description in descriptions {
        let repo = workspace
            .repo_loader()
            .load_at_head()
            .await
            .expect("load_at_head");
        let tx = repo.start_transaction();
        tx.commit(format!("jjhub-test: {description}"))
            .await
            .expect("commit operation");
    }
}

// story: e09s02 — one transaction that moves the local `trunk` bookmark
// to the root commit, so the committed view carries a real ref change.
async fn move_trunk(meta: &std::path::Path, description: &str) {
    let workspace = load_meta_workspace(meta).await;
    let repo = workspace
        .repo_loader()
        .load_at_head()
        .await
        .expect("load_at_head");
    let root = repo.store().root_commit_id().clone();
    let mut tx = repo.start_transaction();
    tx.repo_mut()
        .set_local_bookmark_target(jj_lib::ref_name::RefName::new("trunk"), {
            use jj_lib::op_store::RefTarget;
            RefTarget::normal(root)
        });
    tx.commit(format!("jjhub-test: {description}"))
        .await
        .expect("commit operation");
}

// story: e09s02 — a bookmark set to two commits at once is a conflicted
// ref target; the detail page must flag it.
async fn conflict_trunk(meta: &std::path::Path) {
    let workspace = load_meta_workspace(meta).await;
    let repo = workspace
        .repo_loader()
        .load_at_head()
        .await
        .expect("load_at_head");
    let root = repo.store().root_commit_id().clone();
    let wc = repo
        .view()
        .wc_commit_ids()
        .values()
        .next()
        .cloned()
        .expect("working-copy commit id");
    let mut tx = repo.start_transaction();
    tx.repo_mut()
        .set_local_bookmark_target(jj_lib::ref_name::RefName::new("trunk"), {
            use jj_lib::op_store::RefTarget;
            RefTarget::from_legacy_form([], vec![root, wc])
        });
    tx.commit("jjhub-test: conflicted trunk")
        .await
        .expect("commit operation");
}

async fn root_commit_hex(meta: &std::path::Path) -> String {
    use jj_lib::object_id::ObjectId;
    let workspace = load_meta_workspace(meta).await;
    let repo = workspace
        .repo_loader()
        .load_at_head()
        .await
        .expect("load_at_head");
    repo.store().root_commit_id().hex()
}

async fn newest_operation_id(meta: &std::path::Path) -> String {
    server::jj_meta::load_operations(meta, 50)
        .await
        .expect("load_operations")
        .remove(0)
        .id
}

// story: e09s02t1 — the detail page shows the operation's metadata, its
// linked parents, head ids, and the ref-change table (prior to new ids).
#[sqlx::test(migrations = "./migrations")]
async fn oplog_detail_page_renders_operation_and_ref_changes(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo", "public").await;
    let meta = root.path().join(".meta").join("alice__demo");
    // description is attacker-influenced: it must arrive escaped
    record_operations(&meta, &["alpha <b>op</b>"]).await;
    move_trunk(&meta, "move trunk").await;

    let entries = server::jj_meta::load_operations(&meta, 50)
        .await
        .expect("load_operations");
    let newest = &entries[0];
    assert_eq!(newest.description, "jjhub-test: move trunk");
    let parent_id = &entries[1].id;

    // story: e09s02 — every list row links to its detail page
    let (status, list) = get(&pool, &root, "/alice/demo/operations", None).await;
    assert_eq!(status, StatusCode::OK, "list page: {list}");
    assert!(
        list.contains(&format!("href=\"/alice/demo/operations/{}\"", newest.id)),
        "list row links to detail: {list}"
    );

    let (status, body) = get(
        &pool,
        &root,
        &format!("/alice/demo/operations/{}", newest.id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "detail page: {body}");
    // actor identity, timestamp, description
    assert!(body.contains("op-tester@op-host"), "actor: {body}");
    assert!(!newest.time.is_empty(), "fixture time populated");
    assert!(body.contains(&newest.time), "timestamp: {body}");
    assert!(body.contains("jjhub-test: move trunk"), "description");
    // parent operations linked to their own detail pages
    assert!(
        body.contains(&format!("href=\"/alice/demo/operations/{parent_id}\"")),
        "linked parent: {body}"
    );
    // the view's head ids are listed
    assert!(body.contains("class=\"op-heads\""), "head id list: {body}");
    // ref-change table: trunk moved to the root commit, prior to new
    let root_hex = root_commit_hex(&meta).await;
    assert!(body.contains("local: trunk"), "ref name: {body}");
    assert!(body.contains(&root_hex), "new commit id: {body}");
    assert!(body.contains("Prior") && body.contains("New"), "columns");
    // unchanged refs (absent git head, untouched bookmarks) stay omitted
    assert_eq!(
        body.matches("ref-row").count(),
        1,
        "only changed refs listed: {body}"
    );

    // story: e09s02 — a description is attacker-influenced: the parent
    // operation's own detail page must render it escaped, not as markup
    let (status, parent_page) = get(
        &pool,
        &root,
        &format!("/alice/demo/operations/{parent_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "parent detail: {parent_page}");
    assert!(
        parent_page.contains("jjhub-test: alpha &lt;b&gt;op&lt;&#x2F;b&gt;")
            && !parent_page.contains("<b>op</b>"),
        "description HTML-escaped: {parent_page}"
    );
}

// story: e09s02t1 — a conflicted ref target carries the conflict flag.
#[sqlx::test(migrations = "./migrations")]
async fn oplog_detail_flags_conflicted_ref(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo", "public").await;
    let meta = root.path().join(".meta").join("alice__demo");
    conflict_trunk(&meta).await;
    let op_id = newest_operation_id(&meta).await;

    let (status, body) = get(
        &pool,
        &root,
        &format!("/alice/demo/operations/{op_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "detail page: {body}");
    assert!(
        body.contains("conflict-flag") && body.contains("conflicted"),
        "conflict flag on the conflicted ref: {body}"
    );
    // both sides of the conflict show as the new ids
    assert!(
        body.contains(&root_commit_hex(&meta).await),
        "root id: {body}"
    );
    assert_eq!(body.matches("ref-row").count(), 1, "one ref row: {body}");
}

// story: e09s02t1 — an id that does not resolve in the .meta store is a 404.
#[sqlx::test(migrations = "./migrations")]
async fn oplog_detail_unknown_operation_404(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo", "public").await;
    let meta = root.path().join(".meta").join("alice__demo");
    record_operations(&meta, &["alpha op"]).await;

    // well-formed hex that no operation carries
    let unknown = "00112233445566778899aabbccddeeff00112233";
    let (status, _) = get(
        &pool,
        &root,
        &format!("/alice/demo/operations/{unknown}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown operation id");
    // malformed id is equally unresolvable
    let (status, _) = get(&pool, &root, "/alice/demo/operations/not-a-hex-id", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "malformed operation id");
}

// story: e09s02t1 — a missing .meta store renders the list page's empty
// state, never a 500.
#[sqlx::test(migrations = "./migrations")]
async fn oplog_detail_missing_meta_renders_empty_state(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &cookie, "demo", "public").await;
    let meta = root.path().join(".meta").join("alice__demo");
    std::fs::remove_dir_all(&meta).unwrap();

    let (status, body) = get(
        &pool,
        &root,
        "/alice/demo/operations/00112233445566778899aabbccddeeff00112233",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "empty state page: {body}");
    assert!(
        body.contains("no operation log"),
        "same empty state as the list page: {body}"
    );
    assert!(
        !body.contains("workspace load") && !body.contains("No such file"),
        "no raw jj-lib error text: {body}"
    );
}

// story: e09s02t1 — repo read rules apply to the detail page.
#[sqlx::test(migrations = "./migrations")]
async fn oplog_detail_private_repo_404_for_strangers(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let alice = register(&pool, &root, "alice%40example.com").await;
    create_repo(&pool, &root, &alice, "secret", "private").await;
    let meta = root.path().join(".meta").join("alice__secret");
    record_operations(&meta, &["alpha op"]).await;
    let op_id = newest_operation_id(&meta).await;
    let bob = register(&pool, &root, "bob%40example.com").await;

    let path = format!("/alice/secret/operations/{op_id}");
    let (status, _) = get(&pool, &root, &path, Some(&bob)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "private repo, stranger");
    let (status, _) = get(&pool, &root, &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "private repo, anonymous");
    let (status, _) = get(&pool, &root, &path, Some(&alice)).await;
    assert_eq!(status, StatusCode::OK, "private repo, owner");
}
