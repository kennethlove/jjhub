// story: e20s02 — push-to-create consumes the SAME create budget as
// the form path: one action, one row. Form + push-to-create spend the
// budget; the third create attempt is 429, and the token budget is a
// separate row.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use server::rate_limit::{Budget, RateLimitConfig, RateLimiter};

const SESSION_SECRET: &str = "test-session-secret";
const SECRET: &[u8] = b"test-session-secret";

fn budget(per_window: u32) -> Budget {
    Budget {
        per_window,
        window_secs: 3600,
        burst: per_window,
    }
}

fn create_limiter(per_window: u32) -> RateLimiter {
    RateLimiter::new(RateLimitConfig {
        enabled: true,
        window_secs: 3600,
        login_ip: budget(20),
        push_user: budget(30),
        push_ip: budget(30),
        create_user: budget(per_window),
        create_ip: budget(per_window),
        token_user: budget(10),
    })
}

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
            "email=budgetuser%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
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

/// story: e20s02 — form create, then push-to-create over smart HTTP,
/// then the third create attempt hits the shared create budget. The
/// limiter state lives in the router's AppState, so one limiter-wrapped
/// app serves all three creates.
#[sqlx::test(migrations = "./migrations")]
async fn form_and_push_share_create_budget(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(app(pool.clone(), &root, None)).await;
    let csrf = token_for(&cookie);
    let appw = app(pool.clone(), &root, Some(create_limiter(2)));

    // 1st create: the form — succeeds (302), spends 1 of the create budget.
    let first = appw
        .clone()
        .oneshot(
            Request::post("/repos")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", cookie.clone())
                .body(Body::from(format!(
                    "name=formrepo&visibility=private&_csrf={csrf}"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::SEE_OTHER);

    // 2nd create: push-to-create with a valid transport token — spends 2.
    let user: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'budgetuser'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let token = server::token::create(&pool, user, "push").await;
    let auth = format!(
        "Basic {}",
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            format!("x-access-token:{}", token.plaintext)
        )
    );
    let second = appw
        .clone()
        .oneshot(
            Request::post("/git/budgetuser/pushrepo.git/git-receive-pack")
                .header("content-type", "application/x-git-receive-pack-request")
                .header("authorization", auth)
                .body(Body::from(vec![0u8; 24]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(
        second.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "push-to-create must pass the create budget it is about to charge"
    );

    let pushed: Uuid = sqlx::query_scalar("SELECT id FROM repositories WHERE name = 'pushrepo'")
        .fetch_one(&pool)
        .await
        .expect("push-to-create admitted the repo before the budget ran out");
    let visibility: String = sqlx::query_scalar("SELECT visibility FROM repositories WHERE id = $1")
        .bind(pushed)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(visibility, "private", "push-to-create still creates PRIVATE");

    // 3rd create: budget spent — 429 + Retry-After.
    let third = appw
        .clone()
        .oneshot(
            Request::post("/repos")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", cookie.clone())
                .body(Body::from(format!(
                    "name=thirdrepo&visibility=private&_csrf={csrf}"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(third.status(), StatusCode::TOO_MANY_REQUESTS);

    // The token budget is a different row: token creation still works.
    let token_route = app(pool, &root, None)
        .oneshot(
            Request::post("/settings/tokens")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", cookie)
                .body(Body::from(format!("label=fine&_csrf={csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(token_route.status(), StatusCode::SEE_OTHER);
}
