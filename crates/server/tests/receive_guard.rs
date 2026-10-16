// story: e03s01
mod receive_guard {
    use server::receive_guard::check_push;

    #[test]
    fn parses_change_id_and_author_email_from_commit_object() {
        let raw = "tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904\n\
                   parent 1111111111111111111111111111111111111111\n\
                   author Alice <alice@example.com> 1700000000 +0000\n\
                   committer Alice <alice@example.com> 1700000000 +0000\n\
                   change-id kpqrvtvxsvww\n\
                   \n\
                   subject line\n";
        let identity = server::receive_guard::parse_commit_identity(raw).expect("identity parsed");
        assert_eq!(identity.change_id, "kpqrvtvxsvww");
        assert_eq!(identity.author_email, "alice@example.com");
    }

    #[test]
    fn commits_without_change_id_parse_to_none() {
        let raw = "tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904\n\
                   author Bob <bob@example.com> 1700000000 +0000\n\
                   committer Bob <bob@example.com> 1700000000 +0000\n\
                   \n\
                   plain git commit\n";
        assert!(server::receive_guard::parse_commit_identity(raw).is_none());
    }

    #[test]
    fn parses_ref_update_lines() {
        let stdin = "0000000000000000000000000000000000000000 aaaa1111 refs/heads/trunk\n\
                     bbbb2222 cccc3333 refs/heads/feature\n";
        let updates = server::receive_guard::parse_ref_updates(stdin.as_bytes());
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].new, "aaaa1111");
        assert!(updates[0].is_create());
        assert_eq!(updates[1].old, "bbbb2222");
        assert!(!updates[1].is_create());
    }

    async fn seed(pool: &sqlx::PgPool) -> (uuid::Uuid, uuid::Uuid) {
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
        (repo_id, user_id)
    }

    fn pushed(
        new: &str,
        change_id: &str,
        author_email: &str,
    ) -> server::receive_guard::PushedCommit {
        server::receive_guard::PushedCommit {
            ref_name: "refs/heads/trunk".to_string(),
            old: "0".repeat(40),
            new: new.to_string(),
            change_id: Some(change_id.to_string()),
            author_email: author_email.to_string(),
            // jj preserves author and stamps the rewriting identity as
            // committer, so a "same author" push also carries it
            committer_email: author_email.to_string(),
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn same_author_passes(pool: sqlx::PgPool) {
        let (repo_id, alice) = seed(&pool).await;
        // open MR owned by alice via the real upsert
        server::mr::upsert_after_push(
            &pool,
            repo_id,
            alice,
            vec![server::mr::PushedChange {
                change_id: "aaaa".into(),
                commit_id: "c1".into(),
                author_email: "alice@example.com".into(),
            }],
        )
        .await
        .unwrap();

        let rejections = check_push(
            &pool,
            repo_id,
            vec![pushed("c2", "aaaa", "alice@example.com")],
        )
        .await
        .unwrap();
        assert!(
            rejections.is_empty(),
            "same author must pass: {rejections:?}"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn different_author_is_rejected(pool: sqlx::PgPool) {
        let (repo_id, alice) = seed(&pool).await;
        server::mr::upsert_after_push(
            &pool,
            repo_id,
            alice,
            vec![server::mr::PushedChange {
                change_id: "aaaa".into(),
                commit_id: "c1".into(),
                author_email: "alice@example.com".into(),
            }],
        )
        .await
        .unwrap();

        let rejections = check_push(
            &pool,
            repo_id,
            vec![pushed("c2", "aaaa", "mallory@example.com")],
        )
        .await
        .unwrap();
        assert_eq!(rejections.len(), 1, "author mismatch rejected");
        assert!(
            rejections[0].contains("aaaa"),
            "names the change: {}",
            rejections[0]
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn foreign_committer_is_rejected_even_with_preserved_author(pool: sqlx::PgPool) {
        let (repo_id, alice) = seed(&pool).await;
        server::mr::upsert_after_push(
            &pool,
            repo_id,
            alice,
            vec![server::mr::PushedChange {
                change_id: "aaaa".into(),
                commit_id: "c1".into(),
                author_email: "alice@example.com".into(),
            }],
        )
        .await
        .unwrap();

        // jj-shaped rewrite: author stays alice, committer becomes mallory
        let mut commit = pushed("c2", "aaaa", "alice@example.com");
        commit.committer_email = "mallory@example.com".into();
        let rejections = check_push(&pool, repo_id, vec![commit]).await.unwrap();
        assert_eq!(rejections.len(), 1, "foreign committer rejected");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unknown_change_ids_pass(pool: sqlx::PgPool) {
        let (repo_id, _alice) = seed(&pool).await;
        let rejections = check_push(
            &pool,
            repo_id,
            vec![pushed("c9", "zzzz", "mallory@example.com")],
        )
        .await
        .unwrap();
        assert!(rejections.is_empty(), "new change-ids are always allowed");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn merged_and_abandoned_do_not_block(pool: sqlx::PgPool) {
        let (repo_id, alice) = seed(&pool).await;
        server::mr::upsert_after_push(
            &pool,
            repo_id,
            alice,
            vec![server::mr::PushedChange {
                change_id: "aaaa".into(),
                commit_id: "c1".into(),
                author_email: "alice@example.com".into(),
            }],
        )
        .await
        .unwrap();
        for state in ["merged", "abandoned"] {
            sqlx::query("UPDATE merge_requests SET state = $1 WHERE repo_id = $2")
                .bind(state)
                .bind(repo_id)
                .execute(&pool)
                .await
                .unwrap();
            let rejections = check_push(
                &pool,
                repo_id,
                vec![pushed("c9", "aaaa", "mallory@example.com")],
            )
            .await
            .unwrap();
            assert!(rejections.is_empty(), "{state} MRs must not block pushes");
        }
    }
}
