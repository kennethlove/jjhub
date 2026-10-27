// story: e19s02 — the admission decision for a missing row: create
// only on an authenticated ReceivePack into the presenter's own
// namespace; everything else falls through to 404.
use server::transport::{admit_create, GitOp};

async fn user(pool: &sqlx::PgPool, handle: &str, email: &str) -> uuid::Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, handle) VALUES ($1, 'x', $2) RETURNING id",
    )
    .bind(email)
    .bind(handle)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn rows_for(pool: &sqlx::PgPool, owner_id: uuid::Uuid, name: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM repositories WHERE owner_id = $1 AND name = $2")
        .bind(owner_id)
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn owned_receive_pack_creates_private(pool: sqlx::PgPool) {
    let alice = user(&pool, "alice", "alice@example.com").await;
    let root = tempfile::tempdir().unwrap();
    let root_str = root.path().to_str().unwrap();

    let admitted = admit_create(
        &pool,
        root_str,
        "alice",
        "newproj",
        GitOp::ReceivePack,
        Some(alice),
        "unknown",
        None,
    )
    .await
    .expect("create failures are 500s, not silent falls");
    let (repo_id, owner_id, visibility) = admitted.expect("owned ReceivePack must admit-create");

    assert_eq!(owner_id, alice, "row owner is the pushing user");
    assert_eq!(visibility, "private", "push-created repos start private");
    let row_vis: String = sqlx::query_scalar("SELECT visibility FROM repositories WHERE id = $1")
        .bind(repo_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row_vis, "private");
    assert!(
        domain::storage_path(root_str, "alice", "newproj").exists(),
        "bare store exists before http-backend would run"
    );
    assert!(
        domain::meta_path(root_str, "alice", "newproj")
            .join(".jj")
            .exists(),
        "jj store colocated"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn upload_pack_missing_stays_404(pool: sqlx::PgPool) {
    let alice = user(&pool, "alice", "alice@example.com").await;
    let root = tempfile::tempdir().unwrap();

    let admitted = admit_create(
        &pool,
        root.path().to_str().unwrap(),
        "alice",
        "nothing",
        GitOp::UploadPack,
        Some(alice),
        "unknown",
        None,
    )
    .await
    .unwrap();
    assert!(admitted.is_none(), "clone of nothing is not a create");
    assert_eq!(rows_for(&pool, alice, "nothing").await, 0, "no row");
}

#[sqlx::test(migrations = "./migrations")]
async fn foreign_namespace_stays_404(pool: sqlx::PgPool) {
    let alice = user(&pool, "alice", "alice@example.com").await;
    let bob = user(&pool, "bob", "bob@example.com").await;
    let root = tempfile::tempdir().unwrap();

    let admitted = admit_create(
        &pool,
        root.path().to_str().unwrap(),
        "bob",
        "squat",
        GitOp::ReceivePack,
        Some(alice),
        "unknown",
        None,
    )
    .await
    .unwrap();
    assert!(
        admitted.is_none(),
        "foreign namespace must not admit-create"
    );
    assert_eq!(rows_for(&pool, bob, "squat").await, 0, "no row for bob");
    assert_eq!(rows_for(&pool, alice, "squat").await, 0, "no row for alice");
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_receive_pack_stays_404(pool: sqlx::PgPool) {
    let alice = user(&pool, "alice", "alice@example.com").await;
    let root = tempfile::tempdir().unwrap();

    let admitted = admit_create(
        &pool,
        root.path().to_str().unwrap(),
        "alice",
        "anon",
        GitOp::ReceivePack,
        None,
        "unknown",
        None,
    )
    .await
    .unwrap();
    assert!(admitted.is_none(), "no token, no create");
    assert_eq!(rows_for(&pool, alice, "anon").await, 0, "no row");
}
