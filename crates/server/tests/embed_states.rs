// story: e14s02 — the kata embed degrades honestly: an announced
// loading placeholder inside the embed container, the e05s04 Review
// link always visible beside it, a <noscript> copy for frame-less
// clients, and reduced motion renders the embed statically.
mod embed_states {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn git_in(cwd: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// alice/demo with one open MR whose revision is a real commit and
    /// colocated jj meta, so the e05s04 review link exists.
    async fn setup(pool: &sqlx::PgPool, root: &std::path::Path) {
        let bare = domain::storage_path(root.to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let meta = domain::meta_path(root.to_str().unwrap(), "alice", "demo");
        tokio::fs::create_dir_all(meta.join(".jj")).await.unwrap();
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git_in(&seed, &["init", "-b", "trunk"]);
        std::fs::write(seed.join("f.txt"), "x\n").unwrap();
        git_in(&seed, &["add", "."]);
        git_in(
            &seed,
            &[
                "-c",
                "user.email=a@e.com",
                "-c",
                "user.name=A",
                "commit",
                "-m",
                "one",
            ],
        );
        let commit = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&seed)
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();
        git_in(&seed, &["push", bare.to_str().unwrap(), "trunk"]);

        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ('alice@example.com', 'x', 'alice') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let repo_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo') RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        let mr_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, 'aaaa') RETURNING id",
        )
        .bind(repo_id)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2)")
            .bind(mr_id)
            .bind(&commit)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn get(pool: sqlx::PgPool, root: &std::path::Path, path: &str) -> String {
        let response = server::app_with_storage(pool, root.to_str().unwrap().to_string())
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    // story: e14s02 (e14s02t1) — the MR page renders the loading
    // placeholder inside the embed container, keeps the e05s04 Review
    // link visible beside the container, and exposes the noscript
    // fallback link.
    #[sqlx::test(migrations = "./migrations")]
    async fn mr_page_renders_embed_states(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        setup(&pool, root.path()).await;
        let html = get(pool, root.path(), "/alice/demo/merge-requests/aaaa").await;
        let target = "/r/alice__demo/browse?change=aaaa";

        // the embed container carries its state markup
        let container = html
            .split("class=\"embed\"")
            .nth(1)
            .and_then(|rest| rest.split("</noscript>").next())
            .unwrap_or_default();
        assert!(!container.is_empty(), "embed container rendered: {html}");

        // loading placeholder inside the container, announced
        assert!(
            container.contains("embed__placeholder"),
            "loading placeholder inside the container: {html}"
        );
        assert!(
            container.contains("role=\"status\""),
            "placeholder announces the pending embed: {html}"
        );

        // the frame itself lives inside the same container
        let frame = container
            .split("<iframe")
            .nth(1)
            .and_then(|rest| rest.split('>').next())
            .unwrap_or_default();
        assert!(
            frame.contains(&format!("src=\"{target}\"")),
            "iframe inside the container embeds the change: {html}"
        );

        // noscript exposes the fallback link inside the container
        let noscript = container
            .split("<noscript>")
            .nth(1)
            .and_then(|rest| rest.split("</noscript>").next())
            .unwrap_or_default();
        assert!(
            noscript.contains(&format!("href=\"{target}\"")),
            "noscript fallback link: {html}"
        );

        // the visible Review link sits beside the container — outside
        // any noscript, so it ships with JavaScript enabled too
        let beside = html.rsplit("</noscript>").next().unwrap_or_default();
        assert!(
            beside.contains(&format!("href=\"{target}\"")),
            "e05s04 Review link visible beside the container: {html}"
        );
        assert!(
            beside.contains("Review in kata"),
            "fallback link keeps the kata wording: {html}"
        );
    }

    // story: e14s02 (e14s02t2) — the stylesheet carries the embed
    // transition rule and a prefers-reduced-motion rule that disables
    // it, so the embed renders statically.
    #[sqlx::test(migrations = "./migrations")]
    async fn stylesheet_disables_embed_motion_for_reduced_motion(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        setup(&pool, root.path()).await;
        let css = get(pool, root.path(), "/assets/app.css").await;

        // the embed reveal transition exists on the container's frame
        assert!(
            css.contains(".embed__frame iframe") && css.contains("animation:"),
            "embed transition rule present: {css}"
        );

        // and the reduced-motion block turns it off
        let reduced: Vec<&str> = css
            .split("@media (prefers-reduced-motion: reduce)")
            .skip(1)
            .collect();
        assert!(!reduced.is_empty(), "reduced-motion rule present: {css}");
        let disables = reduced.iter().any(|block| {
            let rule = block.split('}').next().unwrap_or_default();
            rule.contains("embed") && rule.contains("animation: none")
        });
        assert!(
            disables,
            "prefers-reduced-motion disables the embed animation: {css}"
        );
    }
}
