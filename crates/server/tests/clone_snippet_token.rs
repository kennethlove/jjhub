// story: e02s03
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
    server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
}

fn csrf(cookie: &str) -> String {
    let secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| "snippet-secret".into());
    let value = cookie.strip_prefix("session=").unwrap_or(cookie);
    server::session::csrf_token(secret.as_bytes(), value)
}

async fn body_of(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
    std::env::set_var("SESSION_SECRET", "snippet-secret");
    let response = app(pool.clone(), root)
        .oneshot(
            Request::post("/register")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(
                    "email=alice%40example.com&password=correct+horse+battery\
                     &password_confirm=correct+horse+battery",
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

async fn create_repo(pool: &sqlx::PgPool, root: &tempfile::TempDir, cookie: &str) {
    let mut request = Request::post("/repos")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!(
            "name=demo&visibility=public&_csrf={}",
            csrf(cookie)
        )))
        .unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    app(pool.clone(), root).oneshot(request).await.unwrap();
}

async fn cookie_value(response: &axum::response::Response, name: &str) -> String {
    let raw = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .unwrap_or_else(|| panic!("cookie {name} missing"));
    raw.split(';')
        .next()
        .unwrap()
        .trim_start_matches(&format!("{name}="))
        .to_string()
}

#[sqlx::test(migrations = "./migrations")]
async fn generate_token_lands_it_in_the_snippet(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root).await;
    create_repo(&pool, &root, &cookie).await;

    let mut request = Request::post("/alice/demo/tokens")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!("label=clone&_csrf={}", csrf(&cookie))))
        .unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    let response = app(pool.clone(), &root).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers().get("location").unwrap(),
        "/alice/demo",
        "must land back on the repo page"
    );
    let token = cookie_value(&response, "new_token").await;
    assert!(token.starts_with("jjh_"), "fresh token: {token}");

    let mut request = Request::get("/alice/demo").body(Body::empty()).unwrap();
    let cookies = format!("{cookie}; new_token={token}");
    request
        .headers_mut()
        .insert("cookie", cookies.parse().unwrap());
    let response = app(pool, &root).oneshot(request).await.unwrap();
    let body = body_of(response).await;
    assert!(
        body.contains(&format!("x-access-token:{token}@")),
        "snippet must embed the viewer's token: {body}"
    );
    assert!(
        body.contains("jj git remote add origin"),
        "snippet stays copy-pasteable: {body}"
    );
    // story: e02s03 — the button's own form must carry every field the
    // handler needs, or a real browser gets a deserialization error
    let form_start = body.find(r#"action="/alice/demo/tokens""#).expect("generate form");
    let form_end = body[form_start..].find("</form>").expect("form close") + form_start;
    let form = &body[form_start..form_end];
    assert!(form.contains(r#"name="label""#), "form must include label: {form}");
    assert!(form.contains(r#"name="_csrf""#), "form must include csrf: {form}");
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_snippet_keeps_placeholder(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let cookie = register(&pool, &root).await;
    create_repo(&pool, &root, &cookie).await;

    let response = app(pool, &root)
        .oneshot(Request::get("/alice/demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = body_of(response).await;
    assert!(
        body.contains("&lt;TOKEN&gt;"),
        "anonymous viewers keep the placeholder: {body}"
    );
    assert!(
        !body.contains(r#"action="/alice/demo/tokens""#),
        "anonymous viewers get no generate button: {body}"
    );
}
