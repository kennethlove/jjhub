// story: e20s01
//! Login POST per-IP policy row: budgets are keyed by client IP (two
//! clients never share a bucket) and BOTH success and failure charge
//! the row.

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

fn login_request(ip: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/login")
        .header("content-type", "application/x-www-form-urlencoded")
        .extension(axum::extract::ConnectInfo(
            format!("{ip}:9999")
                .parse::<std::net::SocketAddr>()
                .unwrap(),
        ))
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn create_user(pool: &sqlx::PgPool, handle: &str) {
    let hash = server::password::hash_password("s3cret!");
    sqlx::query("INSERT INTO users (handle, email, password_hash) VALUES ($1, $2, $3)")
        .bind(handle)
        .bind(format!("{handle}@rate.example"))
        .bind(&hash)
        .execute(pool)
        .await
        .unwrap();
}

/// story: e20s01 — the policy row is keyed by client IP: exhausting
/// one client's budget leaves another client unaffected.
#[sqlx::test(migrations = "./migrations")]
async fn budgets_isolated_per_client_ip(pool: sqlx::PgPool) {
    let root = std::env::temp_dir().join(format!("rate-login-ip-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let app = server::app_with_limiter(pool, root.to_string_lossy(), Some(limiter(1, 1)));

    let first = app
        .clone()
        .oneshot(login_request("10.1.0.1", "email=a%40b.com&password=wrong"))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::UNAUTHORIZED);

    let second = app
        .clone()
        .oneshot(login_request("10.1.0.1", "email=a%40b.com&password=wrong"))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);

    let other_client = app
        .clone()
        .oneshot(login_request("10.2.0.2", "email=a%40b.com&password=wrong"))
        .await
        .unwrap();
    assert_eq!(
        other_client.status(),
        StatusCode::UNAUTHORIZED,
        "a different client IP must not inherit the exhausted budget"
    );
}

/// story: e20s01 — every POST /login charges the row: a successful
/// login spends a token exactly like a failed attempt.
#[sqlx::test(migrations = "./migrations")]
async fn successful_login_charges_the_row(pool: sqlx::PgPool) {
    let root = std::env::temp_dir().join(format!("rate-login-ok-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let handle = format!("rateloginok{}", uuid::Uuid::new_v4().simple());
    create_user(&pool, &handle).await;
    let body = format!("email={handle}%40rate.example&password=s3cret%21");

    let app = server::app_with_limiter(pool, root.to_string_lossy(), Some(limiter(1, 1)));

    let success = app
        .clone()
        .oneshot(login_request("10.3.0.1", &body))
        .await
        .unwrap();
    assert_ne!(success.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        success
            .headers()
            .get("set-cookie")
            .map(|v| v.to_str().unwrap_or("").contains("session="))
            .unwrap_or(false),
        "successful login sets the session cookie"
    );

    let after = app
        .oneshot(login_request("10.3.0.1", "email=a%40b.com&password=wrong"))
        .await
        .unwrap();
    assert_eq!(
        after.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "the successful login already spent the only token"
    );
}
