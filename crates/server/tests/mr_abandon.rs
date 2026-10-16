// story: e03s04
mod mr_abandon_reopen {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    /// reuse the session()-registered alice (email is unique)
    async fn seed(pool: &sqlx::PgPool) -> (uuid::Uuid, uuid::Uuid) {
        let user_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
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
        (repo_id, user_id)
    }

    async fn open_mr(pool: &sqlx::PgPool, repo: uuid::Uuid, owner: uuid::Uuid) -> uuid::Uuid {
        let mr_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, 'aaaa') RETURNING id",
        )
        .bind(repo)
        .bind(owner)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, 'c1')")
            .bind(mr_id)
            .execute(pool)
            .await
            .unwrap();
        mr_id
    }

    async fn session(pool: &sqlx::PgPool, root: &std::path::Path, email: &str) -> String {
        let request = Request::post("/register")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "email={email}&password=correct+horse+battery"
            )))
            .unwrap();
        let response = server::app_with_storage(pool.clone(), root.to_str().unwrap().to_string())
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

    async fn abandon_post(
        pool: sqlx::PgPool,
        root: &std::path::Path,
        cookie: &str,
        path: &str,
    ) -> axum::response::Response {
        let mut request = Request::post(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(""))
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        server::app_with_storage(pool, root.to_str().unwrap().to_string())
            .oneshot(request)
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn owner_abandons_and_push_reopens(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = session(&pool, root.path(), "alice@example.com").await;
        let (repo_id, alice) = seed(&pool).await;
        open_mr(&pool, repo_id, alice).await;

        let response = abandon_post(
            pool.clone(),
            root.path(),
            &cookie,
            "/alice/demo/pulls/aaaa/abandon",
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let state: String =
            sqlx::query_scalar("SELECT state FROM merge_requests WHERE change_id = 'aaaa'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, "abandoned");

        // reopen on re-push is the upsert's job (e03s01) — state flips back
        let summary = server::mr::upsert_after_push(
            &pool,
            repo_id,
            alice,
            vec![server::mr::PushedChange {
                change_id: "aaaa".into(),
                commit_id: "c2".into(),
                author_email: "alice@example.com".into(),
            }],
        )
        .await
        .unwrap();
        assert_eq!(summary.reopened, 1);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn non_owner_cannot_abandon(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let _alice_cookie = session(&pool, root.path(), "alice@example.com").await;
        let cookie = session(&pool, root.path(), "bob@example.com").await;
        let (repo_id, alice) = seed(&pool).await;
        open_mr(&pool, repo_id, alice).await;

        let response =
            abandon_post(pool, root.path(), &cookie, "/alice/demo/pulls/aaaa/abandon").await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn merged_mr_is_not_abandonable(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let cookie = session(&pool, root.path(), "alice@example.com").await;
        let (repo_id, alice) = seed(&pool).await;
        let mr_id = open_mr(&pool, repo_id, alice).await;
        sqlx::query("UPDATE merge_requests SET state = 'merged' WHERE id = $1")
            .bind(mr_id)
            .execute(&pool)
            .await
            .unwrap();

        let response =
            abandon_post(pool, root.path(), &cookie, "/alice/demo/pulls/aaaa/abandon").await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    // story: e03s04 — bookmark deletion abandons the matching open MR
    // (latest revision commit was the deleted ref's target).
    #[sqlx::test(migrations = "./migrations")]
    async fn deleted_ref_abandons_matching_open_mr(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let _cookie = session(&pool, root.path(), "alice@example.com").await;
        let (repo_id, alice) = seed(&pool).await;
        open_mr(&pool, repo_id, alice).await;

        let abandoned = server::mr::abandon_for_deleted_ref(&pool, repo_id, "c1")
            .await
            .unwrap();
        assert_eq!(abandoned, 1, "open MR at the deleted commit abandoned");

        let state: String =
            sqlx::query_scalar("SELECT state FROM merge_requests WHERE change_id = 'aaaa'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, "abandoned");
    }
}
