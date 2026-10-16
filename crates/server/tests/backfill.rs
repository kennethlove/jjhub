// story: e05s02
mod backfill_meta {
    fn git(args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Simulate a pre-e05 repo row + bare store with NO metadata dir.
    async fn seed_legacy_repo(
        pool: &sqlx::PgPool,
        root: &std::path::Path,
        owner: &str,
        name: &str,
    ) {
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, handle) VALUES ($1, 'x', $2) RETURNING id",
        )
        .bind(format!("{owner}@example.com"))
        .bind(owner)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO repositories (owner_id, name) VALUES ($1, $2)")
            .bind(user_id)
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
        let bare = domain::storage_path(root.to_str().unwrap(), owner, name);
        std::fs::create_dir_all(&bare).unwrap();
        git(&["init", "--bare", bare.to_str().unwrap()]);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn backfill_creates_missing_metadata(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let storage = root.path().to_str().unwrap();

        // pre-e05 repo: row + bare store, no metadata
        seed_legacy_repo(&pool, root.path(), "alice", "legacy").await;

        // repo that is already colocated (as e05s01 creates them) — must stay untouched
        seed_legacy_repo(&pool, root.path(), "bob", "modern").await;
        let already_meta = domain::meta_path(storage, "bob", "modern");
        server::jj_meta::init(
            &already_meta,
            &domain::storage_path(storage, "bob", "modern"),
        )
        .await
        .expect("seed colocated repo");
        let before = std::fs::read_dir(&already_meta).unwrap().count();

        let outcome = server::backfill::run(&pool, storage)
            .await
            .expect("backfill");

        assert_eq!(outcome.created, vec!["alice/legacy".to_string()]);
        assert!(
            outcome.failed.is_empty(),
            "no failures: {:?}",
            outcome.failed
        );
        assert!(
            domain::meta_path(storage, "alice", "legacy")
                .join(".jj")
                .exists(),
            "legacy repo got metadata"
        );

        // idempotent: second run creates nothing and leaves existing metadata alone
        let again = server::backfill::run(&pool, storage).await.unwrap();
        assert!(again.created.is_empty(), "second run is a no-op");
        assert_eq!(again.already_present.len(), 2);
        assert_eq!(
            std::fs::read_dir(&already_meta).unwrap().count(),
            before,
            "existing metadata untouched"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn backfill_reports_missing_bare_repo(pool: sqlx::PgPool) {
        let root = tempfile::tempdir().unwrap();
        let storage = root.path().to_str().unwrap();
        // repo row with no bare store on disk — must fail loudly, not crash
        seed_legacy_repo(&pool, root.path(), "carol", "ghost").await;
        std::fs::remove_dir_all(domain::storage_path(storage, "carol", "ghost")).unwrap();

        let outcome = server::backfill::run(&pool, storage)
            .await
            .expect("backfill");
        assert_eq!(outcome.created.len(), 0);
        assert_eq!(outcome.failed.len(), 1);
        assert_eq!(outcome.failed[0].0, "carol/ghost");
        assert!(
            outcome.failed[0].1.contains("missing bare repo"),
            "reason: {}",
            outcome.failed[0].1
        );
    }
}
