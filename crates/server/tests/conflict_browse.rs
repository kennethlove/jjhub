// story: e08s02 — acceptance (e08s02t1): a blob page on a conflicted path
// renders jj's conflict with `<<<<<<<` / `=======` / `>>>>>>>` markers, one
// part per side, every side HTML-escaped; non-conflicted paths and clean
// commits keep the e07 rendering; binary sides show the binary note per
// side.
mod conflict_support;

mod conflict_browse {
    use super::conflict_support;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// Mount a public alice/demo repo whose bare store carries the fixture;
    /// `terms` picks the conflicted file's base/side0/side1 contents.
    async fn mount(
        pool: &sqlx::PgPool,
        root: &tempfile::TempDir,
        terms: Option<(&str, &str, &str)>,
    ) -> conflict_support::Fixture {
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
        match terms {
            Some((base, side0, side1)) => conflict_support::build_terms(
                &bare,
                "Alice",
                "alice@example.com",
                conflict_support::CONFLICT_CHANGE_ID,
                base,
                side0,
                side1,
            ),
            None => conflict_support::build(&bare),
        }
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
    async fn conflicted_blob_shows_markers_with_escaped_sides(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(
            &pool,
            &root,
            Some(("<b>&base", "<script>alert(\"x\")</script>", "a & b")),
        )
        .await;

        let html = get(&pool, root.path(), "/alice/demo/blob/trunk/conflict.txt").await;
        assert!(html.contains("<<<<<<< side-0"), "side-0 marker: {html}");
        assert!(html.contains("||||||| base"), "base marker: {html}");
        assert!(html.contains("======="), "separator marker: {html}");
        assert!(html.contains(">>>>>>> side-1"), "side-1 marker: {html}");
        // every side escaped
        assert!(html.contains("&lt;b&gt;&amp;base"), "base escaped: {html}");
        assert!(html.contains("&lt;script&gt;"), "side-0 escaped: {html}");
        assert!(html.contains("a &amp; b"), "side-1 escaped: {html}");
        assert!(!html.contains("<script>"), "no raw markup: {html}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn non_conflicted_paths_render_unchanged(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let fixture = mount(&pool, &root, None).await;

        // clean commit: no jj:trees header — the plain e07 rendering
        let html = get(
            &pool,
            root.path(),
            &format!("/alice/demo/blob/{}/conflict.txt", fixture.parent),
        )
        .await;
        assert!(html.contains("base"), "clean content: {html}");
        assert!(!html.contains("<<<<<<<"), "no markers on clean commit: {html}");

        // conflicted commit, path identical in every term: plain rendering
        let html = get(&pool, root.path(), "/alice/demo/blob/trunk/same.txt").await;
        assert!(html.contains("same"), "identical-path content: {html}");
        assert!(
            !html.contains("<<<<<<<"),
            "no markers on identical path: {html}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn binary_side_shows_note_per_side(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        mount(&pool, &root, Some(("base\n", "side0\n", "\0\x01binary"))).await;

        let html = get(&pool, root.path(), "/alice/demo/blob/trunk/conflict.txt").await;
        assert!(html.contains("<<<<<<< side-0"), "conflict shown: {html}");
        assert!(html.contains("binary file"), "binary note: {html}");
        assert!(html.contains(">>>>>>> side-1"), "closing marker: {html}");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn oversized_side_is_capped_per_side(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let big = "x".repeat(1024 * 1024 + 64);
        mount(&pool, &root, Some(("base\n", "side0\n", &big))).await;

        let html = get(&pool, root.path(), "/alice/demo/blob/trunk/conflict.txt").await;
        assert!(html.contains("<<<<<<< side-0"), "conflict shown: {html}");
        assert!(html.contains("truncat"), "cap note on the big side: {html}");
    }
}
