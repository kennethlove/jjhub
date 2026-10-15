// story: e02s01
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

mod repo_create {
    use super::*;

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
    }

    async fn post_form(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        path: &str,
        body: &str,
        cookie: &str,
    ) -> axum::response::Response {
        let mut request = Request::post(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body.to_string()))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        app(pool, root).oneshot(request).await.unwrap()
    }

    async fn get(
        pool: sqlx::PgPool,
        root: &tempfile::TempDir,
        path: &str,
    ) -> axum::response::Response {
        app(pool, root)
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "email=alice%40example.com&password=correct+horse+battery",
            ))
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

    #[sqlx::test(migrations = "./migrations")]
    async fn create_repo_redirects_and_creates_bare(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;

        let response = post_form(
            pool.clone(),
            &root,
            "/repos",
            "name=demo&visibility=public",
            &cookie,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(location, "/alice/demo");

        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        assert!(
            bare.join("HEAD").exists(),
            "bare repo initialized at {bare:?}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn duplicate_name_conflicts(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;

        post_form(
            pool.clone(),
            &root,
            "/repos",
            "name=demo&visibility=public",
            &cookie,
        )
        .await;
        let response = post_form(
            pool,
            &root,
            "/repos",
            "name=demo&visibility=public",
            &cookie,
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn invalid_name_rejected(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        let response = post_form(
            pool,
            &root,
            "/repos",
            "name=Bad+Name&visibility=public",
            &cookie,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn repo_page_is_public(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        post_form(
            pool.clone(),
            &root,
            "/repos",
            "name=demo&visibility=public",
            &cookie,
        )
        .await;

        let response = get(pool, &root, "/alice/demo").await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(
            html.contains("alice/demo"),
            "repo page shows owner/name: {html}"
        );
        assert!(
            html.contains("/git/alice/demo.git"),
            "clone URL shown: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn create_requires_session(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let response = app(pool, &root)
            .oneshot(
                Request::post("/repos")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from("name=demo&visibility=public"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(location, "/login");
    }
}
