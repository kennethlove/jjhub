// story: e02s02
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use tower::ServiceExt;

mod private_repo_enforcement {
    use super::*;

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
    }

    fn basic(token: &str) -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"))
        )
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir, email: &str) -> String {
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "email={email}&password=correct+horse+battery"
            )))
            .unwrap();
        let response = app(pool.clone(), root).oneshot(request).await.unwrap();
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
        body: &str,
    ) -> axum::response::Response {
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body.to_string()))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        app(pool.clone(), root).oneshot(request).await.unwrap()
    }

    async fn info_refs(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        service: &str,
        auth: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::get(format!(
            "/git/alice/demo.git/info/refs?service=git-{service}"
        ))
        .body(Body::empty())
        .unwrap();
        if let Some(auth) = auth {
            request
                .headers_mut()
                .insert("authorization", auth.parse().unwrap());
        }
        app(pool, root).oneshot(request).await.unwrap()
    }

    async fn setup(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        let cookie = register(pool, root, "alice%40example.com").await;
        create_repo(pool, root, &cookie, "name=demo&visibility=public").await;
        cookie
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn anonymous_push_is_401(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        setup(&pool, &root).await;
        let response = info_refs(pool, &root, "receive-pack", None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn stranger_push_is_403(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        setup(&pool, &root).await;
        let bob = register(&pool, &root, "bob%40example.com").await;
        // bob generates his own token via settings
        let mut request = Request::post("/settings/tokens")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("label=bob"))
            .unwrap();
        request.headers_mut().insert("cookie", bob.parse().unwrap());
        let response = app(pool.clone(), &root).oneshot(request).await.unwrap();
        let bob_token = response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split("new_token=")
            .nth(1)
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();

        let response = info_refs(pool, &root, "receive-pack", Some(&basic(&bob_token))).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn private_clone_requires_token(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = setup(&pool, &root).await;
        // flip to private
        let mut request = Request::post("/alice/demo/settings")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("visibility=private"))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response = app(pool.clone(), &root).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let anon = info_refs(pool.clone(), &root, "upload-pack", None).await;
        assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn public_clone_stays_open(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        setup(&pool, &root).await;
        let response = info_refs(pool, &root, "upload-pack", None).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn owner_push_with_token_passes_auth_gate(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = setup(&pool, &root).await;
        let mut request = Request::post("/settings/tokens")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("label=owner"))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response = app(pool.clone(), &root).oneshot(request).await.unwrap();
        let token = response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split("new_token=")
            .nth(1)
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();

        // 403 would mean authz rejected; any other status (404/400 from empty
        // repo receive-pack handling) means the authz gate passed.
        let response = info_refs(pool, &root, "receive-pack", Some(&basic(&token))).await;
        assert_ne!(response.status(), StatusCode::FORBIDDEN);
        assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
