// story: e01s01
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn form(body: &str) -> Body {
    Body::from(body.to_string())
}

async fn post(pool: sqlx::PgPool, path: &str, body: &str) -> axum::response::Response {
    server::app(pool)
        .oneshot(
            Request::post(path)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(form(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn get(pool: sqlx::PgPool, path: &str) -> axum::response::Response {
    server::app(pool)
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn register_creates_user_and_redirects_to_settings(pool: sqlx::PgPool) {
    let response = post(
        pool.clone(),
        "/register",
        "email=Alice%40Example.COM&password=correct+horse+battery",
    )
    .await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(location, "/settings");
    assert!(response.headers().contains_key("set-cookie"), "session cookie missing");

    let email: String = sqlx::query_scalar("SELECT email FROM users LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(email, "alice@example.com", "email must be stored normalized");
}

#[sqlx::test(migrations = "./migrations")]
async fn register_rejects_short_password(pool: sqlx::PgPool) {
    let response = post(pool, "/register", "email=alice%40example.com&password=short").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrations = "./migrations")]
async fn register_rejects_duplicate_email(pool: sqlx::PgPool) {
    post(
        pool.clone(),
        "/register",
        "email=alice%40example.com&password=correct+horse+battery",
    )
    .await;
    let response = post(
        pool,
        "/register",
        "email=ALICE%40example.com&password=another+long+password",
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "./migrations")]
async fn settings_redirects_to_login_without_session(pool: sqlx::PgPool) {
    let response = get(pool, "/settings").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(location, "/login");
}

#[sqlx::test(migrations = "./migrations")]
async fn settings_renders_with_session_cookie(pool: sqlx::PgPool) {
    let response = post(
        pool.clone(),
        "/register",
        "email=bob%40example.com&password=correct+horse+battery",
    )
    .await;
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

    let mut request = Request::get("/settings").body(Body::empty()).unwrap();
    request.headers_mut().insert(
        "cookie",
        cookie.parse().unwrap(),
    );
    let response = server::app(pool).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
