//! story: e16s03 — divergence mark on the repo page's Open MRs list.
//!
//! Seeds a bare repo with a divergent pair (two sibling heads sharing
//! one jj change-id) and a linear MR commit, plus the matching DB
//! rows, and asserts the repo page marks only the divergent change
//! with the shared `.divergence-flag` span — linear and quiet lists
//! stay byte-identical to before.

#[cfg(test)]
mod divergence_repo_page {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use std::io::Write;
    use tower::ServiceExt;

    fn git_in(cwd: &std::path::Path, args: &[&str]) -> String {
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
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Build a jj-style commit object (with a `change-id` header) on
    /// top of `parent` with the staged tree of `dir`.
    fn mkcommit(dir: &std::path::Path, parent: &str, change_id: &str, msg: &str) -> String {
        let tree = git_in(dir, &["write-tree"]);
        let parent_line = if parent.chars().all(|c| c == '0') {
            String::new()
        } else {
            format!("parent {parent}\n")
        };
        let object = format!(
            "tree {tree}\n{parent_line}author A <a@e.com> 1700000000 +0000\ncommitter A <a@e.com> 1700000000 +0000\nchange-id {change_id}\n\n{msg}\n"
        );
        let mut child = std::process::Command::new("git")
            .args(["hash-object", "-t", "commit", "-w", "--stdin"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .current_dir(dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(object.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "mkcommit: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Seed the bare repo with trunk T, a divergent pair S1/S2 sharing
    /// one change-id, and a linear commit L on its own change-id.
    /// Returns (s1, s2, l) as pushed commit ids.
    fn seed_bare(bare: &std::path::Path) -> (String, String, String) {
        let seed = bare.join("../seed");
        std::fs::create_dir_all(&seed).unwrap();
        git_in(&seed, &["init", "-b", "trunk"]);
        std::fs::write(seed.join("base.txt"), "base\n").unwrap();
        git_in(&seed, &["add", "."]);
        let t = mkcommit(
            &seed,
            "0000000000000000000000000000000000000000",
            "0000trunk0000",
            "base",
        );
        // sibling one of the diverging pair
        std::fs::write(seed.join("s1.txt"), "one\n").unwrap();
        git_in(&seed, &["add", "."]);
        let s1 = mkcommit(&seed, &t, "kkkkktaaaaa", "sibling one");
        // sibling two: same parent, same change-id, different content
        git_in(&seed, &["reset", "--mixed", &t]);
        std::fs::write(seed.join("s2.txt"), "two\n").unwrap();
        git_in(&seed, &["add", "."]);
        let s2 = mkcommit(&seed, &t, "kkkkktaaaaa", "sibling two");
        // linear commit on its own change-id
        git_in(&seed, &["reset", "--mixed", &t]);
        std::fs::write(seed.join("linear.txt"), "linear\n").unwrap();
        git_in(&seed, &["add", "."]);
        let l = mkcommit(&seed, &t, "bbbbbraaaaas", "linear work");
        // push everything into the bare repo's object graph
        let bare_str = bare.to_str().unwrap();
        git_in(&seed, &["push", bare_str, &format!("{t}:refs/heads/trunk")]);
        git_in(
            &seed,
            &["push", bare_str, &format!("{s1}:refs/heads/mr-kkkk-1")],
        );
        git_in(
            &seed,
            &["push", bare_str, &format!("{s2}:refs/heads/mr-kkkk-2")],
        );
        git_in(
            &seed,
            &["push", bare_str, &format!("{l}:refs/heads/mr-bbbb-1")],
        );
        (s1, s2, l)
    }

    /// Seed alice/demo plus one open MR per (change_id, commits) pair.
    async fn seed_mrs(pool: &sqlx::PgPool, mrs: &[(&str, &[String])]) {
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
        for (change_id, commits) in mrs {
            let mr_id: uuid::Uuid = sqlx::query_scalar(
                "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, $3) RETURNING id",
            )
            .bind(repo_id)
            .bind(user_id)
            .bind(change_id)
            .fetch_one(pool)
            .await
            .unwrap();
            for commit in *commits {
                sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2)")
                    .bind(mr_id)
                    .bind(commit)
                    .execute(pool)
                    .await
                    .unwrap();
            }
        }
    }

    async fn get_body(pool: sqlx::PgPool, root: &std::path::Path) -> String {
        let response = server::app_with_storage(pool, root.to_str().unwrap().to_string())
            .oneshot(Request::get("/alice/demo").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned()
    }

    /// story: e16s03 — the divergent open MR carries the shared mark;
    /// the linear MR's row is byte-identical to the unmarked markup.
    #[sqlx::test(migrations = "./migrations")]
    async fn repo_page_marks_only_divergent_open_mr(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let (s1, s2, l) = seed_bare(&bare);
        seed_mrs(&pool, &[("kkkk", &[s1, s2]), ("bbbb", &[l])]).await;

        let html = get_body(pool.clone(), root.path()).await;

        // the divergent change carries the shared divergence mark
        assert!(
            html.contains(
                "<a href=\"/alice/demo/merge-requests/kkkk\">kkkk</a> <span class=\"divergence-flag\" title=\"Divergent change: two heads exist\">diverged</span>"
            ),
            "divergent MR row missing the divergence-flag mark"
        );
        // the linear change renders exactly as before the mark existed
        assert!(
            html.contains("<li><a href=\"/alice/demo/merge-requests/bbbb\">bbbb</a></li>"),
            "linear MR row changed"
        );
    }

    /// story: e16s03 — a repo whose only open MR is linear renders no
    /// mark anywhere (byte-identical list context).
    #[sqlx::test(migrations = "./migrations")]
    async fn linear_only_repo_renders_no_mark(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        let (_s1, _s2, l) = seed_bare(&bare);
        seed_mrs(&pool, &[("bbbb", &[l])]).await;

        let html = get_body(pool.clone(), root.path()).await;

        assert!(!html.contains("divergence-flag"), "unexpected mark");
    }

    /// story: e16s03 — no open MRs keeps the quiet placeholder and
    /// never renders a mark.
    #[sqlx::test(migrations = "./migrations")]
    async fn quiet_repo_renders_placeholder_without_mark(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let bare = domain::storage_path(root.path().to_str().unwrap(), "alice", "demo");
        server::repo::init_bare(&bare).unwrap();
        seed_bare(&bare);
        seed_mrs(&pool, &[]).await;

        let html = get_body(pool.clone(), root.path()).await;

        assert!(html.contains("No open merge requests — push a change to open one."));
        assert!(!html.contains("divergence-flag"), "unexpected mark");
    }
}
