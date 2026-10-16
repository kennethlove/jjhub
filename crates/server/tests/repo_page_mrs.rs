// story: repo dashboard — the repo page must surface open MRs, not just
// clone instructions.
mod repo_page_mrs {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        std::env::set_var("SESSION_SECRET", "repo-page-suite");
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "email=alice%40example.com&password=correct+horse+battery",
            ))
            .unwrap();
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
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
        let secret = std::env::var("SESSION_SECRET").unwrap();
        let value = cookie.strip_prefix("session=").unwrap();
        let token = server::session::csrf_token(secret.as_bytes(), value);
        let mut request = Request::post("/repos")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "name=demo&visibility=public&_csrf={token}"
            )))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    async fn get(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        path: &str,
        cookie: &str,
    ) -> String {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        let response =
            server::app_with_storage(pool.clone(), root.path().to_str().unwrap().to_string())
                .oneshot(request)
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn repo_page_lists_open_mrs(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;

        // a push already ran: seed an open MR directly
        let repo_id: uuid::Uuid = sqlx::query_scalar(
            "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = 'alice' AND r.name = 'demo'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let owner_id: uuid::Uuid =
            sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let mr_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, 'aaaa') RETURNING id",
        )
        .bind(repo_id)
        .bind(owner_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, 'c1')")
            .bind(mr_id)
            .execute(&pool)
            .await
            .unwrap();

        let html = get(&pool, &root, "/alice/demo", &cookie).await;
        assert!(
            html.contains("/alice/demo/merge-requests/aaaa"),
            "open MR linked from the repo page: {html}"
        );
        assert!(
            html.contains("merge-requests\""), // list link present
            "link to the full list: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn repo_page_without_mrs_still_links_to_list(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;
        let html = get(&pool, &root, "/alice/demo", &cookie).await;
        assert!(
            html.contains("/alice/demo/merge-requests"),
            "list link even when empty: {html}"
        );
    }
}
