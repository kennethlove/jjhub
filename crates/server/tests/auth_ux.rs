// story: e01s03
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn app(pool: sqlx::PgPool) -> axum::Router {
    server::app(pool)
}

async fn post(pool: sqlx::PgPool, path: &str, body: &str) -> axum::response::Response {
    app(pool)
        .oneshot(
            Request::post(path)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn body_of(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

async fn register(pool: &sqlx::PgPool) -> String {
    std::env::set_var("SESSION_SECRET", "auth-ux-secret");
    let response = post(
        pool.clone(),
        "/register",
        "email=alice%40example.com&password=correct+horse+battery\
         &password_confirm=correct+horse+battery",
    )
    .await;
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

#[sqlx::test(migrations = "./migrations")]
async fn signup_confirmation_rejects_mismatch(pool: sqlx::PgPool) {
    let response = post(
        pool.clone(),
        "/register",
        "email=alice%40example.com&password=correct+horse+battery\
         &password_confirm=different+entirely",
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_of(response).await;
    assert!(
        body.contains("passwords do not match"),
        "visible error message: {body}"
    );
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(users, 0, "mismatch must create no user");
}

#[sqlx::test(migrations = "./migrations")]
async fn auth_error_page_login_shows_message(pool: sqlx::PgPool) {
    register(&pool).await;
    let response = post(
        pool.clone(),
        "/login",
        "email=alice%40example.com&password=wrong-password",
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = body_of(response).await;
    assert!(
        body.contains("email or password is incorrect"),
        "login failure must render a message: {body}"
    );
    assert!(body.contains("<form"), "must re-render the form: {body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn auth_error_page_register_conflict_shows_form(pool: sqlx::PgPool) {
    register(&pool).await;
    let response = post(
        pool.clone(),
        "/register",
        "email=ALICE%40Example.com&password=another+long+password\
         &password_confirm=another+long+password",
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = body_of(response).await;
    assert!(
        body.contains("email already registered"),
        "conflict must render a message: {body}"
    );
    assert!(
        body.contains(r#"<form method="post" action="/register""#),
        "must re-render the register form: {body}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn logout_control_settings_nav_has_button(pool: sqlx::PgPool) {
    let cookie = register(&pool).await;
    let mut request = Request::get("/settings").body(Body::empty()).unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    let response = app(pool).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_of(response).await;
    assert!(
        body.contains(r#"action="/logout""#),
        "settings nav must offer logout: {body}"
    );
    assert!(
        body.contains(r#"name="_csrf""#),
        "logout needs csrf: {body}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn logout_control_anonymous_nav_has_none(pool: sqlx::PgPool) {
    let response = post(pool.clone(), "/login", "email=x%40y.com&password=nope").await;
    let body = body_of(response).await;
    assert!(
        !body.contains(r#"action="/logout""#),
        "anonymous page must not offer logout: {body}"
    );
}
