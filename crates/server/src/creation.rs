// story: e19s01 — the single repo creation sequence. The web form and
// both transports call this so validate -> dup-check -> init -> INSERT
// cannot drift between callers. The service owns sequence, cleanup and
// errors; HTTP response shapes stay with the caller.
use uuid::Uuid;

/// story: e19s01 — what a caller needs to answer the request (the UI
/// handler redirects to `/{owner}/{name}`).
#[derive(Debug)]
pub struct Created {
    pub id: Uuid,
    pub owner: String,
    pub name: String,
}

/// story: e19s01 — failure modes the handler maps to its response
/// shape (422 / 409 / 500); `Storage` and `Db` carry the exact
/// message the handler returns verbatim.
#[derive(Debug)]
pub enum CreateError {
    InvalidName,
    InvalidVisibility,
    Duplicate,
    Storage(String),
    Db(String),
}

// story: e19s01 — two concurrent creates of one name must not init the
// same stores in parallel: jj workspace init on a shared path corrupts
// both callers (`workspace load: There is no Jujutsu repo`). Serializing
// just the init phase keeps the INSERT race below intact for the
// re-lookup; the lock is process-local and held only across init.
static STORE_INIT: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

/// story: e19s01 — run the creation sequence for `owner_id`. `raw_name`
/// and `raw_visibility` are untrusted form input; the service normalizes
/// and validates both.
pub async fn create_repository(
    pool: &sqlx::PgPool,
    storage_root: &str,
    owner_id: Uuid,
    raw_name: &str,
    raw_visibility: &str,
) -> Result<Created, CreateError> {
    let name = raw_name.trim().to_lowercase();
    if !domain::repo_name_valid(&name) {
        return Err(CreateError::InvalidName);
    }
    let visibility = match domain::Visibility::parse(raw_visibility) {
        Some(v) => v,
        None => return Err(CreateError::InvalidVisibility),
    };

    let owner: String = sqlx::query_scalar("SELECT handle FROM users WHERE id = $1")
        .bind(owner_id)
        .fetch_one(pool)
        .await
        .unwrap_or_default();

    // story: e05s01 — the DB row is the authoritative duplicate check, so a
    // known duplicate returns before any store init (init_external_git
    // refuses to re-init an existing .jj, which would otherwise 500). The
    // insert+commit below stays one atomic write for the race the pre-check
    // can't see: no row can point at a missing store.
    let existing: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM repositories WHERE owner_id = $1 AND name = $2")
            .bind(owner_id)
            .bind(&name)
            .fetch_one(pool)
            .await
            .unwrap_or(0);
    if existing > 0 {
        return Err(CreateError::Duplicate);
    }

    // story: e05s01 — create both stores, then commit the row in one
    // transaction. No manual compensation: if either init fails we clean up
    // two dirs we own, and if the transaction aborts the filesystem work is
    // discarded the same way.
    let _init_guard = STORE_INIT.lock().await;
    let bare = domain::storage_path(storage_root, &owner, &name);
    if let Err(err) = crate::repo::init_bare(&bare) {
        let _ = std::fs::remove_dir_all(&bare);
        return Err(CreateError::Storage(err));
    }

    // story: e05s01 — colocate jj metadata beside the bare repo.
    let meta = domain::meta_path(storage_root, &owner, &name);
    if let Err(err) = crate::jj_meta::init(&meta, &bare).await {
        let _ = std::fs::remove_dir_all(&bare);
        let _ = std::fs::remove_dir_all(&meta);
        drop(_init_guard);
        return Err(CreateError::Storage(err));
    }
    drop(_init_guard);

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(err) => {
            let _ = std::fs::remove_dir_all(&bare);
            let _ = std::fs::remove_dir_all(&meta);
            return Err(CreateError::Db(format!("db error: {err}")));
        }
    };
    let inserted: Result<(uuid::Uuid,), _> = sqlx::query_as(
        "INSERT INTO repositories (owner_id, name, visibility) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(owner_id)
    .bind(&name)
    .bind(visibility.as_str())
    .fetch_one(&mut *tx)
    .await;

    match inserted {
        Ok((id,)) => {
            if let Err(err) = tx.commit().await {
                let _ = std::fs::remove_dir_all(&bare);
                let _ = std::fs::remove_dir_all(&meta);
                return Err(CreateError::Db(format!("db error: {err}")));
            }
            Ok(Created { id, owner, name })
        }
        Err(err) => {
            drop(tx);
            match &err {
                sqlx::Error::Database(db)
                    if db.constraint() == Some("repositories_owner_id_name_key") =>
                {
                    // story: e19s01 — the race loser must not clean up: those
                    // stores on disk belong to the winner's committed row.
                    // Re-look-up proves the row exists and answers 409.
                    let winner: Option<uuid::Uuid> = sqlx::query_scalar(
                        "SELECT id FROM repositories WHERE owner_id = $1 AND name = $2",
                    )
                    .bind(owner_id)
                    .bind(&name)
                    .fetch_optional(pool)
                    .await
                    .ok()
                    .flatten();
                    if winner.is_none() {
                        let _ = std::fs::remove_dir_all(&bare);
                        let _ = std::fs::remove_dir_all(&meta);
                    }
                    Err(CreateError::Duplicate)
                }
                _ => {
                    let _ = std::fs::remove_dir_all(&bare);
                    let _ = std::fs::remove_dir_all(&meta);
                    Err(CreateError::Db(format!("db error: {err}")))
                }
            }
        }
    }
}
