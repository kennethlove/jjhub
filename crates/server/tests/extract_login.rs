// story: e15s01 — GET /login is served from login.html; the handler
// passes context only and the pre-extraction markup contracts hold.
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tera::Context;
use tower::ServiceExt;

#[sqlx::test(migrations = "./migrations")]
async fn extract_login_serves_login_template(pool: sqlx::PgPool) {
    let response = server::app(pool)
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .expect("content-type header")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        content_type.starts_with("text/html"),
        "content type: {content_type}"
    );
    let body = String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
        .into_owned();

    // the page body is the rendered login.html, not a string literal
    let mut context = Context::new();
    context.insert("error", "");
    context.insert("theme", "system"); // e21s02: base.html declares theme
    let expected = server::render::render("login.html", &context);
    assert_eq!(body, expected, "GET /login must render login.html");

    // pre-extraction contracts, unchanged
    assert!(body.contains("<h1>Log in</h1>"), "heading: {body}");
    assert!(
        body.contains(r#"<form method="post" action="/login">"#),
        "form: {body}"
    );
    assert!(body.contains(r#"name="email""#), "email field: {body}");
    assert!(
        body.contains(r#"name="password""#),
        "password field: {body}"
    );
}
