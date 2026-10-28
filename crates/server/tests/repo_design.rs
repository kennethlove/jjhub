//! story: e26s01 — the approved repo landing shell from
//! specs/designs/repo.html renders with its header, tabs, and split
//! grid; pinned behaviours (clone snippet, token form) live inside it.

#[cfg(test)]
mod repo_design {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app(pool: sqlx::PgPool, root: &tempfile::TempDir) -> axum::Router {
        server::app_with_storage(pool, root.path().to_str().unwrap().to_string())
    }

    fn csrf(cookie: &str) -> String {
        let secret =
            std::env::var("SESSION_SECRET").unwrap_or_else(|_| "repo-design-secret".into());
        let value = cookie.strip_prefix("session=").unwrap_or(cookie);
        server::session::csrf_token(secret.as_bytes(), value)
    }

    async fn body_of(response: axum::response::Response) -> String {
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    async fn register(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        std::env::set_var("SESSION_SECRET", "repo-design-secret");
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

    async fn repo_page(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> String {
        let request = Request::get("/alice/demo")
            .header("host", "example.test:3210")
            .body(Body::empty())
            .unwrap();
        let response = app(pool.clone(), root).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        body_of(response).await
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn renders_the_design_shell(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;

        let body = repo_page(&pool, &root).await;

        // header: wordmark eyebrow, repo title, status pill,
        // visibility note, clone command
        assert!(body.contains("class=\"wordmark\""), "wordmark eyebrow");
        assert!(body.contains("class=\"repo-title\""), "repo title");
        assert!(body.contains("class=\"status-pill\""), "status pill");
        assert!(body.contains("class=\"visibility-def\""), "visibility note");
        assert!(body.contains("jj git clone"), "clone command");

        // tabs: Browse active
        assert!(body.contains("class=\"section-tabs\""), "section tabs");
        assert!(body.contains("aria-current=\"page\""), "active tab");

        // split grid: about rail + files panel
        assert!(body.contains("class=\"split-grid\""), "split grid");
        assert!(body.contains("class=\"about-panel\""), "about rail");
        assert!(body.contains("class=\"files-panel\""), "files panel");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn clone_snippet_and_token_form_survive(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = register(&pool, &root).await;
        create_repo(&pool, &root, &cookie).await;

        let body = repo_page(&pool, &root).await;

        // the snippet stays (pinned by clone_snippet_token)
        assert!(body.contains("jj git remote add origin"), "clone snippet");
        assert!(body.contains("class=\"code-block\""), "code block");

        // the token form posts to the scoped mint endpoint
        assert!(body.contains("action=\"/alice/demo/tokens\""), "token form");
        assert!(body.contains("name=\"label\""), "token label input");

        // logged-out placeholder in the snippet
        assert!(
            body.contains("x-access-token:&lt;TOKEN&gt;"),
            "placeholder"
        );
    }
}
