// story: e08s02 — acceptance (e08s02t2): a tree listing groups jj's
// `.jjconflict-base-N` / `.jjconflict-side-N` directories into one conflict
// entry instead of linking them as ordinary directories; ordinary entries
// and clean trees keep the e07 rendering.
mod conflict_support;

mod conflict_tree {
    use super::conflict_support;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// Mount a public alice/demo repo whose bare store carries the fixture.
    async fn mount(pool: &sqlx::PgPool, root: &tempfile::TempDir) -> conflict_support::Fixture {
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo')")
            .bind(user_id)
            .execute(pool)
            .await
            .unwrap();
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        std::fs::create_dir_all(&bare).unwrap();
        conflict_support::build(&bare)
    }

    async fn get(pool: &sqlx::PgPool, root: &std::path::Path, path: &str) -> String {
        let response = server::app_with_storage(pool.clone(), root.to_str().unwrap().to_string())
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "GET {path}");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8_lossy(&body).into_owned()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn conflict_dirs_render_as_one_entry(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root).await;

        let html = get(&pool, root.path(), "/alice/demo/tree/trunk").await;
        assert!(
            html.contains("conflict (1 base + 2 sides)"),
            "one conflict entry with side count: {html}"
        );
        assert_eq!(
            html.matches("conflict (").count(),
            1,
            "exactly one conflict entry: {html}"
        );
        assert!(
            !html.contains(".jjconflict"),
            "raw conflict dir names never linked: {html}"
        );
        // ordinary entries unchanged
        assert!(
            html.contains("/alice/demo/blob/trunk/conflict.txt"),
            "conflicted file still linked: {html}"
        );
        assert!(html.contains("same.txt"), "sibling listed: {html}");
        assert!(html.contains("JJ-CONFLICT-README"), "readme listed: {html}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn clean_tree_renders_unchanged(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let fixture = mount(&pool, &root).await;

        let html = get(
            &pool,
            root.path(),
            &format!("/alice/demo/tree/{}", fixture.parent),
        )
        .await;
        assert!(!html.contains(".jjconflict"), "no conflict dirs: {html}");
        assert!(!html.contains("conflict ("), "no conflict entry: {html}");
        assert!(
            html.contains(&format!("/alice/demo/blob/{}/conflict.txt", fixture.parent)),
            "ordinary file linked: {html}"
        );
        assert!(html.contains("same.txt"), "sibling listed: {html}");
    }
}
