// story: e21s03 — regenerate counts as token creation for the e20
// token row: past budget it is 429, and it shares that budget with
// the mint routes.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

const SESSION_SECRET: &str = "test-session-secret";

fn budget(per_window: u32) -> server::rate_limit::Budget {
    server::rate_limit::Budget {
        per_window,
        window_secs: 3600,
        burst: per_window,
    }
}

fn limiter(
    mutate: impl FnOnce(&mut server::rate_limit::RateLimitConfig),
) -> server::rate_limit::RateLimiter {
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
    server::rate_limit::RateLimiter::new(cfg)
}

fn app(
    pool: sqlx::PgPool,
    root: &tempfile::TempDir,
    limiter: Option<server::rate_limit::RateLimiter>,
) -> axum::Router {
    server::app_with_limiter(pool, root.path().to_str().unwrap().to_string(), limiter)
}

async fn register(app: axum::Router) -> String {
    let response = app
        .oneshot(
            Request::post("/register")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(
                    "email=alice%40example.com&password=correct+horse+battery&password_confirm=correct+horse+battery",
                ))
                .unwrap(),
        )
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

fn csrf_of(cookie: &str) -> String {
    let value = cookie.strip_prefix("session=").unwrap_or(cookie);
    server::session::csrf_token(SESSION_SECRET.as_bytes(), value)
}

async fn post_form(app: axum::Router, path: &str, cookie: &str) -> StatusCode {
    let mut request = Request::post(path)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("cookie", cookie.parse::<axum::http::HeaderValue>().unwrap())
        .body(Body::from(format!("_csrf={}", csrf_of(cookie))))
        .unwrap();
    request
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            0,
        ))));
    app.oneshot(request).await.unwrap().status()
}

/// e21s03t3 — a token budget of 1: first regenerate rotates, second
/// is 429 with Retry-After.
#[sqlx::test(migrations = "./migrations")]
async fn regenerate_past_budget_is_429(pool: sqlx::PgPool) {
    std::env::set_var("SESSION_SECRET", SESSION_SECRET);
    let root = tempfile::tempdir().unwrap();
    let cookie = register(app(pool.clone(), &root, None)).await;

    let app = app(pool, &root, Some(limiter(|cfg| cfg.token_user = budget(1))));
    let first = post_form(app.clone(), "/settings/regenerate", &cookie).await;
    assert_eq!(first, StatusCode::SEE_OTHER);

    let second = post_form(app, "/settings/regenerate", &cookie).await;
    assert_eq!(second, StatusCode::TOO_MANY_REQUESTS);
}

/// e21s03t3 — regenerate and the mint routes draw on ONE budget row:
/// minting spends it, regenerate then 429s (and vice versa).
#[sqlx::test(migrations = "./migrations")]
async fn regenerate_shares_the_token_budget(pool: sqlx::PgPool) {
    std::env::set_var("SESSION_SECRET", SESSION_SECRET);
    let root = tempfile::tempdir().unwrap();
    let cookie = register(app(pool.clone(), &root, None)).await;

    let app = app(pool, &root, Some(limiter(|cfg| cfg.token_user = budget(1))));

    // mint first (spends the single token), then regenerate must 429
    let mint = {
        let mut r = Request::post("/settings/tokens")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("cookie", cookie.parse::<axum::http::HeaderValue>().unwrap())
            .body(Body::from(format!("label=one&_csrf={}", csrf_of(&cookie))))
            .unwrap();
        r.extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                0,
            ))));
        r
    };
    let minted = app.clone().oneshot(mint).await.unwrap().status();
    assert_eq!(minted, StatusCode::SEE_OTHER);

    let rotated = post_form(app, "/settings/regenerate", &cookie).await;
    assert_eq!(
        rotated,
        StatusCode::TOO_MANY_REQUESTS,
        "regenerate draws from the same token row as minting"
    );
}
