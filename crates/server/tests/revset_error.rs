//! story: e11s01 — invalid revset -> 400 with the parser's message;
//! unknown/unauthorized/missing-q -> 404. See e11s01-revset-endpoint.md.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

const SUITE_SECRET: &str = "revset-error-suite";

fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
    std::env::set_var("SESSION_SECRET", SUITE_SECRET);
    server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
}

fn csrf(cookie: &str) -> String {
    let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| SUITE_SECRET.into());
    let value = cookie.strip_prefix("session=").unwrap_or(cookie);
    server::session::csrf_token(secret.as_bytes(), value)
}

async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir, email: &str) -> String {
    std::env::set_var("SESSION_SECRET", SUITE_SECRET);
    let request = Request::post("/register")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!(
            "email={email}&password=correct+horse+battery&password_confirm=correct+horse+battery"
        )))
        .unwrap();
    let response = app(pool.clone(), root).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
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
    let mut request = Request::post("/repos")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("cookie", cookie)
        .body(Body::from(format!(
            "name={name}&visibility={visibility}&_csrf={}",
            csrf(cookie)
        )))
        .unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    let response = app(pool.clone(), root).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
}

async fn revsets(
    pool: &sqlx::PgPool,
    root: &tempfile::TempDir,
    target: &str,
    cookie: Option<&str>,
    query: Option<&str>,
) -> (StatusCode, String) {
    let path = query.map_or_else(|| target.to_string(), |q| format!("{target}?q={q}"));
    let mut builder = Request::get(path);
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    let request = builder.body(Body::empty()).unwrap();
    let response = app(pool.clone(), root).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1_048_576)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

// story: e11s01
#[sqlx::test(migrations = "./migrations")]
async fn revset_error_invalid_query_returns_400_with_parser_message(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let alice = register(&pool, &root, "alice@revsets.test").await;
    create_repo(&pool, &root, &alice, "demo", "public").await;

    let (status, body) = revsets(&pool, &root, "/alice/demo/revsets", None, Some("::::")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!body.is_empty(), "400 must carry a parser message");
}

// story: e11s01
#[sqlx::test(migrations = "./migrations")]
async fn revset_error_unknown_repo_is_404(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();

    let (status, _) = revsets(&pool, &root, "/nobody/demo/revsets", None, Some("all()")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// story: e11s01
#[sqlx::test(migrations = "./migrations")]
async fn revset_error_missing_query_is_404(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let alice = register(&pool, &root, "alice@revsets.test").await;
    create_repo(&pool, &root, &alice, "demo", "public").await;

    let (status, _) = revsets(&pool, &root, "/alice/demo/revsets", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// story: e11s01
#[sqlx::test(migrations = "./migrations")]
async fn revset_error_private_repo_stranger_is_404(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let alice = register(&pool, &root, "alice@revsets.test").await;
    create_repo(&pool, &root, &alice, "vault", "private").await;

    let stranger = register(&pool, &root, "mallory@revsets.test").await;
    let (status, body) = revsets(
        &pool,
        &root,
        "/alice/vault/revsets",
        Some(&stranger),
        Some("all()"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        !body.contains("is_open") && !body.contains("change_id"),
        "404 must not leak repository data"
    );
}
