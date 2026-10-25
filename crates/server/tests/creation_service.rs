// story: e19s01 — the creation service sequence: one path, cleanup on
// failure, unique-race re-lookup. Unit tests drive the extraction out
// of routes::create_repo.
use server::creation::{create_repository, CreateError};

#[sqlx::test(migrations = "./migrations")]
async fn store_init_failure_leaves_no_dirs(pool: sqlx::PgPool) {
    let owner: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, handle) \
         VALUES ('svc@example.com', 'x', 'svcuser') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let root_str = root.path().to_str().unwrap();

    // Block jj_meta::init: a regular file at the meta path makes
    // create_dir_all fail after init_bare has already run.
    let meta = domain::meta_path(root_str, "svcuser", "svc-repo");
    std::fs::create_dir_all(meta.parent().unwrap()).unwrap();
    std::fs::write(&meta, b"not a directory").unwrap();

    let result = create_repository(&pool, root_str, owner, "Svc-Repo ", "private").await;
    assert!(
        matches!(result, Err(CreateError::Storage(_))),
        "expected Storage error, got {result:?}"
    );

    // Cleanup contract: the bare store initialized before the failure
    // must not survive, and no row may exist.
    let bare = domain::storage_path(root_str, "svcuser", "svc-repo");
    assert!(!bare.exists(), "bare store must be cleaned up");
    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM repositories WHERE owner_id = $1 AND name = 'svc-repo'",
    )
    .bind(owner)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rows, 0, "no repository row may exist");
}

// story: e19s01 — Gherkin: two concurrent creates of one name; one
// wins the unique constraint, the loser returns the existing row (not
// a crash) and never deletes the winner's stores.
#[sqlx::test(migrations = "./migrations")]
async fn duplicate_race_keeps_winner_stores(pool: sqlx::PgPool) {
    let owner: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, handle) \
         VALUES ('race@example.com', 'x', 'raceuser') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let root_str = root.path().to_str().unwrap();

    let (a, b) = tokio::join!(
        create_repository(&pool, root_str, owner, "race-repo", "private"),
        create_repository(&pool, root_str, owner, "race-repo", "private"),
    );
    let results = [a, b];
    let oks: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    let errs: Vec<_> = results.iter().filter_map(|r| r.as_ref().err()).collect();
    assert_eq!(oks.len(), 1, "exactly one winner, got {results:?}");
    assert_eq!(errs.len(), 1, "exactly one loser, got {results:?}");
    assert!(
        matches!(errs[0], CreateError::Duplicate),
        "loser must be Duplicate, got {results:?}"
    );

    let ids: Vec<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT id FROM repositories WHERE owner_id = $1 AND name = 'race-repo'",
    )
    .bind(owner)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(ids.len(), 1, "one row");
    assert_eq!(ids[0].0, oks[0].id, "row is the winner's");

    let bare = domain::storage_path(root_str, "raceuser", "race-repo");
    let meta = domain::meta_path(root_str, "raceuser", "race-repo");
    assert!(bare.exists(), "winner's bare store must survive");
    assert!(meta.join(".jj").exists(), "winner's jj store must survive");
}
