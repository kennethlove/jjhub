// story: e03s01
mod mr_open_on_push {
    #[sqlx::test(migrations = "./migrations")]
    async fn schema_has_invariant_one_partial_unique(pool: sqlx::PgPool) {
        let repo_id = seed_repo(&pool, "alice").await;
        let owner_id: uuid::Uuid =
            sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
                .fetch_one(&pool)
                .await
                .unwrap();

        sqlx::query(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, 'kpqr')",
        )
        .bind(repo_id)
        .bind(owner_id)
        .execute(&pool)
        .await
        .unwrap();

        // invariant 1: second OPEN row for same (repo, change-id) rejected
        let dup = sqlx::query(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, 'kpqr')",
        )
        .bind(repo_id)
        .bind(owner_id)
        .execute(&pool)
        .await;
        assert!(
            dup.is_err(),
            "partial unique index must reject open duplicate"
        );

        // abandoned row does not block a new open MR
        sqlx::query("UPDATE merge_requests SET state = 'abandoned'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, 'kpqr')",
        )
        .bind(repo_id)
        .bind(owner_id)
        .execute(&pool)
        .await
        .expect("abandoned must not block a new open MR");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn revisions_are_unique_per_mr_and_commit(pool: sqlx::PgPool) {
        let repo_id = seed_repo(&pool, "alice").await;
        let owner_id: uuid::Uuid =
            sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let mr_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO merge_requests (repo_id, owner_id, change_id) VALUES ($1, $2, 'abcd') RETURNING id",
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
            .expect("first revision inserts");

        // schema rejects a duplicate revision; idempotent re-push is the
        // upsert function's job (see t2), not a raw INSERT's
        let dup = sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, 'c1')")
            .bind(mr_id)
            .execute(&pool)
            .await;
        assert!(dup.is_err(), "unique(mr_id, commit_id) rejects duplicates");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM revisions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);

        sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, 'c2')")
            .bind(mr_id)
            .execute(&pool)
            .await
            .expect("new commit appends a revision");
    }

    async fn seed_repo(pool: &sqlx::PgPool, handle: &str) -> uuid::Uuid {
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ($1, 'x', $2) RETURNING id",
        )
        .bind(format!("{handle}@example.com"))
        .bind(handle)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query_scalar(
            "INSERT INTO repositories (owner_id, name) VALUES ($1, 'demo') RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }
}

#[allow(dead_code)]
mod upsert_behavior {
    /// one pushed change handed to the upsert
    #[derive(Debug, Clone)]
    pub struct PushedChange {
        pub change_id: String,
        pub commit_id: String,
        pub author_email: String,
    }

    async fn seed_repo(pool: &sqlx::PgPool, handle: &str) -> (uuid::Uuid, uuid::Uuid) {
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ($1, 'x', $2) RETURNING id",
        )
        .bind(format!("{handle}@example.com"))
        .bind(handle)
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

    fn change(change_id: &str, commit: &str) -> PushedChange {
        PushedChange {
            change_id: change_id.to_string(),
            commit_id: commit.to_string(),
            author_email: "alice@example.com".to_string(),
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn push_opens_mrs_and_appends_revisions(pool: sqlx::PgPool) {
        let (repo_id, alice) = seed_repo(&pool, "alice").await;

        // first push: two changes -> two open MRs with one revision each
        let summary = server::mr::upsert_after_push(
            &pool,
            repo_id,
            alice,
            vec![change("aaaa", "c1"), change("bbbb", "c2")],
        )
        .await
        .expect("upsert");
        assert_eq!(summary.opened, 2);
        assert_eq!(summary.reopened, 0);
        assert_eq!(summary.appended, 0);

        // rewrite of aaaa: same MR, second revision
        let summary = server::mr::upsert_after_push(
            &pool,
            repo_id,
            alice,
            vec![change("aaaa", "c3")],
        )
        .await
        .expect("upsert");
        assert_eq!(summary.opened, 0, "no duplicate MR on rewrite");
        assert_eq!(summary.appended, 1, "rewrite appends a revision");

        // re-push same commit: idempotent, no new revision
        let summary = server::mr::upsert_after_push(
            &pool,
            repo_id,
            alice,
            vec![change("aaaa", "c3")],
        )
        .await
        .expect("upsert");
        assert_eq!(summary.appended, 0, "same commit is a no-op");

        let revisions: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM revisions WHERE commit_id = 'c3'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(revisions, 1);
        let mrs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM merge_requests")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mrs, 2, "still exactly two MRs");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn abandoned_mr_reopens_on_push(pool: sqlx::PgPool) {
        let (repo_id, alice) = seed_repo(&pool, "alice").await;
        server::mr::upsert_after_push(&pool, repo_id, alice, vec![change("aaaa", "c1")])
            .await
            .unwrap();
        sqlx::query("UPDATE merge_requests SET state = 'abandoned'")
            .execute(&pool)
            .await
            .unwrap();

        let summary = server::mr::upsert_after_push(
            &pool,
            repo_id,
            alice,
            vec![change("aaaa", "c2")],
        )
        .await
        .unwrap();
        assert_eq!(summary.reopened, 1, "abandoned flips back to open");
        let state: String =
            sqlx::query_scalar("SELECT state FROM merge_requests WHERE change_id = 'aaaa'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, "open");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn merged_mr_is_terminal(pool: sqlx::PgPool) {
        let (repo_id, alice) = seed_repo(&pool, "alice").await;
        server::mr::upsert_after_push(&pool, repo_id, alice, vec![change("aaaa", "c1")])
            .await
            .unwrap();
        sqlx::query("UPDATE merge_requests SET state = 'merged'")
            .execute(&pool)
            .await
            .unwrap();

        let summary = server::mr::upsert_after_push(
            &pool,
            repo_id,
            alice,
            vec![change("aaaa", "c2")],
        )
        .await
        .unwrap();
        assert_eq!(summary.opened, 0);
        assert_eq!(summary.reopened, 0, "merged never reopens");
        assert_eq!(summary.appended, 0, "merged gains no revisions");
        let state: String =
            sqlx::query_scalar("SELECT state FROM merge_requests WHERE change_id = 'aaaa'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, "merged");
    }
}
