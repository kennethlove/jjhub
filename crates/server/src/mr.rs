// story: e03s01
//! Merge request upsert on push: the post-receive path that turns pushed
//! change-ids into open MRs and appends immutable revisions.
//!
//! Invariant 1 (one open MR per repo+change-id) is a partial unique index;
//! this upsert transitions ONE row per change: open → append revision,
//! abandoned → reopen (+ append), merged → terminal (never reopens).

use sqlx::PgPool;
use uuid::Uuid;

/// story: e03s01 — one pushed change handed to the upsert.
#[derive(Debug, Clone)]
pub struct PushedChange {
    pub change_id: String,
    pub commit_id: String,
    pub author_email: String,
}

/// story: e03s01 — what one push did, for tests and logging.
#[derive(Debug, Default, sqlx::FromRow)]
pub struct UpsertSummary {
    pub opened: i64,
    pub reopened: i64,
    pub appended: i64,
    pub skipped_merged: i64,
}

/// story: e03s01 — apply a push to the MR table. Returns counts for
/// observability. Never rewrites existing rows beyond state transitions.
pub async fn upsert_after_push(
    pool: &PgPool,
    repo_id: Uuid,
    pusher_id: Uuid,
    changes: Vec<PushedChange>,
) -> Result<UpsertSummary, sqlx::Error> {
    let mut summary = UpsertSummary::default();
    for change in changes {
        let current: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT id, state FROM merge_requests \
             WHERE repo_id = $1 AND change_id = $2 \
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(repo_id)
        .bind(&change.change_id)
        .fetch_optional(pool)
        .await?;

        let mr_id = match current {
            None => {
                // new change: open an MR with its first revision
                let id: Uuid = sqlx::query_scalar(
                    "INSERT INTO merge_requests (repo_id, owner_id, change_id) \
                     VALUES ($1, $2, $3) RETURNING id",
                )
                .bind(repo_id)
                .bind(pusher_id)
                .bind(&change.change_id)
                .fetch_one(pool)
                .await?;
                append_revision(pool, id, &change.commit_id).await?;
                summary.opened += 1;
                id
            }
            Some((id, state)) => match state.as_str() {
                "open" => {
                    if append_revision(pool, id, &change.commit_id).await? {
                        summary.appended += 1;
                    }
                    id
                }
                "abandoned" => {
                    let flipped: i64 =
                        sqlx::query("UPDATE merge_requests SET state = 'open' WHERE id = $1")
                            .bind(id)
                            .execute(pool)
                            .await?
                            .rows_affected() as i64;
                    if flipped > 0 {
                        summary.reopened += 1;
                        append_revision(pool, id, &change.commit_id).await?;
                    }
                    id
                }
                // merged is terminal (spec: e03s03 § Business rules)
                _ => {
                    summary.skipped_merged += 1;
                    id
                }
            },
        };
        let _ = mr_id;
    }
    Ok(summary)
}

/// Append a revision unless this commit is already recorded. Returns true
/// when a row was actually inserted (re-push of the same commit is a no-op).
async fn append_revision(pool: &PgPool, mr_id: Uuid, commit_id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO revisions (mr_id, commit_id) VALUES ($1, $2) \
         ON CONFLICT (mr_id, commit_id) DO NOTHING",
    )
    .bind(mr_id)
    .bind(commit_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
