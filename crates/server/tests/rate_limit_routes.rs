// story: e20s02 — the policy table routes: push, create, and token
// creation 429 past budget; reads and every other route unaffected.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use server::rate_limit::{Budget, RateLimiter};

const SESSION_SECRET: &str = "test-session-secret";
const SECRET: &[u8] = b"test-session-secret";

fn budget(per_window: u32) -> Budget {
    Budget {
        per_window,
        window_secs: 3600,
        burst: per_window,
    }
}

fn limiter(mutate: impl FnOnce(&mut server::rate_limit::RateLimitConfig)) -> RateLimiter {
    let mut cfg = server::rate_limit::RateLimitConfig {
        enabled: true,
        window_secs: 3600,
        login_ip: budget(20),
        push_user: budget(30),
        push_ip: budget(30),
        create_user: budget(10),
        create_ip: budget(10),
        token_user: budget(10),
    };
    mutate(&mut cfg);
    RateLimiter::new(cfg)
}

/// The limiter state lives in the router's AppState, so one app built
/// here is shared across the requests of a single test.
fn app(
    pool: sqlx::PgPool,
    root: &tempfile::TempDir,
    limiter: Option<RateLimiter>,
) -> axum::Router {
    server::app_with_limiter(pool, root.path().to_str().unwrap().to_string(), limiter)
}

async fn register(app: axum::Router) -> String {
    std::env::set_var("SESSION_SECRET", SESSION_SECRET);
    let request = Request::post("/register")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(
            "email=alice%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
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

fn token_for(cookie: &str) -> String {
    let value = cookie.strip_prefix("session=").unwrap_or(cookie);
    server::session::csrf_token(SECRET, value)
}

async fn post_form(app: axum::Router, path: &str, body: &str, cookie: &str) -> StatusCode {
    let mut request = Request::post(path)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body.to_string()))
        .unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    app.oneshot(request).await.unwrap().status()
}

/// story: e20s02 — second push past budget is 429 with Retry-After and
/// nothing reaches a git process: no row is created, ever.
#[sqlx::test(migrations = "./migrations")]
async fn push_past_budget_is_429_and_creates_nothing(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let app = app(
        pool.clone(),
        &root,
        Some(limiter(|cfg| {
            cfg.push_user = budget(1);
            cfg.push_ip = budget(1);
        })),
    );

    let push = || {
        let app = app.clone();
        async move {
            let request = Request::post("/git/alice/fresh.git/git-receive-pack")
                .header("content-type", "application/x-git-receive-pack-request")
                .body(Body::from(vec![0u8; 24]))
                .unwrap();
            let response = app.oneshot(request).await.unwrap();
            (
                response.status(),
                response.headers().get("retry-after").cloned(),
            )
        }
    };

    let (first, _) = push().await;
    assert_ne!(
        first,
        StatusCode::TOO_MANY_REQUESTS,
        "first push passes the budget check"
    );

    let (second, retry_after) = push().await;
    assert_eq!(second, StatusCode::TOO_MANY_REQUESTS);
    assert!(
        retry_after.is_some(),
        "429 must carry Retry-After so the git client can back off"
    );

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM repositories")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0, "budget gate sits before any git spawn or create");
}

/// story: e20s02 — reads are never throttled: upload-pack fetches
/// stream regardless of push budget state.
#[sqlx::test(migrations = "./migrations")]
async fn reads_never_throttle(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let app = app(
        pool,
        &root,
        Some(limiter(|cfg| {
            cfg.push_user = budget(1);
            cfg.push_ip = budget(1);
        })),
    );
    for _ in 0..3 {
        let request = Request::get("/git/alice/any.git/info/refs?service=git-upload-pack")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_ne!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}

/// story: e20s02 — POST /repos consumes the create budget: first
/// succeeds, second is 429.
#[sqlx::test(migrations = "./migrations")]
async fn create_route_past_budget_is_429(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(app(pool.clone(), &root, None)).await;
    let csrf = token_for(&cookie);
    let form = format!("name=fresh&visibility=private&_csrf={csrf}");

    let app = app(
        pool,
        &root,
        Some(limiter(|cfg| {
            cfg.create_user = budget(1);
            cfg.create_ip = budget(1);
        })),
    );
    let first = post_form(app.clone(), "/repos", &form, &cookie).await;
    assert_eq!(first, StatusCode::SEE_OTHER);

    let second = post_form(app, "/repos", &form, &cookie).await;
    assert_eq!(second, StatusCode::TOO_MANY_REQUESTS);
}

/// story: e20s02 — token creation consumes the token budget: first
/// succeeds, second is 429.
#[sqlx::test(migrations = "./migrations")]
async fn token_route_past_budget_is_429(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(app(pool.clone(), &root, None)).await;
    let csrf = token_for(&cookie);

    let app = app(
        pool,
        &root,
        Some(limiter(|cfg| cfg.token_user = budget(1))),
    );
    let first = post_form(
        app.clone(),
        "/settings/tokens",
        &format!("label=one&_csrf={csrf}"),
        &cookie,
    )
    .await;
    assert_eq!(first, StatusCode::SEE_OTHER);

    let second = post_form(
        app,
        "/settings/tokens",
        &format!("label=two&_csrf={csrf}"),
        &cookie,
    )
    .await;
    assert_eq!(second, StatusCode::TOO_MANY_REQUESTS);
}
