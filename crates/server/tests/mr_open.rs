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
        assert!(dup.is_err(), "partial unique index must reject open duplicate");

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

        for _ in 0..2 {
            sqlx::query("INSERT INTO revisions (mr_id, commit_id) VALUES ($1, 'c1')")
                .bind(mr_id)
                .execute(&pool)
                .await
                .expect("same commit must not double-insert");
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM revisions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "revision upsert is idempotent per commit");

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
