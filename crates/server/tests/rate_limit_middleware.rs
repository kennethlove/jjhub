// story: e20s01
//! Rate-limit middleware on POST /login: None limiter bypasses,
//! under-budget passes through unchanged, over budget 429 +
//! Retry-After plain text; GET /login never throttled.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use server::rate_limit::{Budget, RateLimitConfig, RateLimiter};
use tower::ServiceExt;

fn limiter(per_window: u32, burst: u32) -> RateLimiter {
    RateLimiter::new(RateLimitConfig {
        enabled: true,
        window_secs: 3600,
        login_ip: Budget {
            per_window,
            window_secs: 3600,
            burst,
        },
        push_user: Budget {
            per_window: 30,
            window_secs: 3600,
            burst: 10,
        },
        push_ip: Budget {
            per_window: 30,
            window_secs: 3600,
            burst: 10,
        },
        create_user: Budget {
            per_window: 10,
            window_secs: 3600,
            burst: 3,
        },
        create_ip: Budget {
            per_window: 10,
            window_secs: 3600,
            burst: 3,
        },
        token_user: Budget {
            per_window: 10,
            window_secs: 3600,
            burst: 5,
        },
    })
}

fn login_request(ip: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/login")
        .header("content-type", "application/x-www-form-urlencoded")
        .extension(axum::extract::ConnectInfo(
            format!("{ip}:9999")
                .parse::<std::net::SocketAddr>()
                .unwrap(),
        ))
        .body(Body::from("email=nobody%40example.com&password=wrong"))
        .unwrap()
}

/// story: e20s01 — AppState.limiter is None in every test constructor:
/// the wrapped handler's behavior is unchanged.
#[sqlx::test(migrations = "./migrations")]
async fn none_limiter_bypasses(pool: sqlx::PgPool) {
    let root = std::env::temp_dir().join(format!("rate-mw-none-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let app = server::app_with_limiter(pool, root.to_string_lossy(), None);
    let resp = app.oneshot(login_request("10.0.0.1")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// story: e20s01 — under budget the login row behaves exactly as today.
#[sqlx::test(migrations = "./migrations")]
async fn under_budget_passes_through(pool: sqlx::PgPool) {
    let root = std::env::temp_dir().join(format!("rate-mw-under-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let app = server::app_with_limiter(pool, root.to_string_lossy(), Some(limiter(5, 5)));
    let resp = app.oneshot(login_request("10.0.0.2")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// story: e20s01 — past the budget the same POST is refused with a
/// plain-text 429 carrying Retry-After, before any handler runs.
#[sqlx::test(migrations = "./migrations")]
async fn over_budget_gets_429_with_retry_after(pool: sqlx::PgPool) {
    let root = std::env::temp_dir().join(format!("rate-mw-over-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let app = server::app_with_limiter(pool, root.to_string_lossy(), Some(limiter(1, 1)));

    let first = app
        .clone()
        .oneshot(login_request("10.0.0.3"))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::UNAUTHORIZED);

    let second = app.oneshot(login_request("10.0.0.3")).await.unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after = second
        .headers()
        .get("retry-after")
        .expect("429 must carry Retry-After")
        .to_str()
        .unwrap()
        .parse::<u64>()
        .expect("Retry-After is whole seconds");
    assert!(retry_after >= 1);
    let body = axum::body::to_bytes(second.into_body(), 4096)
        .await
        .unwrap();
    assert_eq!(&body[..], b"too many requests");
}

/// story: e20s01 — only POST is charged; the login page stays reachable.
#[sqlx::test(migrations = "./migrations")]
async fn get_login_never_throttled(pool: sqlx::PgPool) {
    let root = std::env::temp_dir().join(format!("rate-mw-get-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let app = server::app_with_limiter(pool, root.to_string_lossy(), Some(limiter(1, 1)));

    let _ = app
        .clone()
        .oneshot(login_request("10.0.0.4"))
        .await
        .unwrap();
    let _ = app
        .clone()
        .oneshot(login_request("10.0.0.4"))
        .await
        .unwrap();

    let get = Request::builder()
        .method("GET")
        .uri("/login")
        .extension(axum::extract::ConnectInfo(
            "10.0.0.4:9999".parse::<std::net::SocketAddr>().unwrap(),
        ))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(get).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
