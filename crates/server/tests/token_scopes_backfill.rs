// story: e22s01 — backfill guarantee: every token that existed before
// the migration carries full {read,write} with NULL restrictions, and
// no token is born more restricted than today.

use server::token;

/// Gherkin: fresh token defaults — created without explicit
/// permissions, it is read+write with no expiry, pin, or allowlist.
#[sqlx::test(migrations = "./migrations")]
async fn fresh_token_defaults_to_full_access(pool: sqlx::PgPool) {
    let user_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, handle)
         VALUES ('fresh@example.com', 'x', 'freshuser') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let new_token = token::create(&pool, user_id, "default").await;

    #[derive(sqlx::FromRow)]
    struct Perms {
        scopes: Vec<String>,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
        repo_id: Option<uuid::Uuid>,
    }

    let perms: Perms = sqlx::query_as(
        "SELECT scopes, expires_at, repo_id
         FROM tokens WHERE id = $1",
    )
    .bind(new_token.id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(perms.scopes, vec!["read", "write"]);
    assert!(perms.expires_at.is_none());
    assert!(perms.repo_id.is_none());
    // INET[] as a NULL check — no ipnetwork dep needed for a shape test
    let has_allowlist: bool =
        sqlx::query_scalar("SELECT allowed_inets IS NOT NULL FROM tokens WHERE id = $1")
            .bind(new_token.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!has_allowlist);
}

/// Gherkin: pre-existing rows survive the migration — whatever mix of
/// tokens exists, every row reads {read,write} and NULL restrictions
/// until enforcement (e22s02) starts writing the columns deliberately.
#[sqlx::test(migrations = "./migrations")]
async fn every_row_reads_read_write_and_unrestricted(pool: sqlx::PgPool) {
    let user_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, handle)
         VALUES ('mix@example.com', 'x', 'mixuser') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // one minted through today's code...
    token::create(&pool, user_id, "minted").await;
    // ...one inserted exactly as pre-e22 code inserted it
    sqlx::query(
        "INSERT INTO tokens (user_id, label, prefix, token_hash)
         VALUES ($1, 'legacy', 'jjh_ol', 'old-hash')",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    let violating: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM tokens
         WHERE scopes <> '{read,write}'
            OR expires_at IS NOT NULL
            OR repo_id IS NOT NULL
            OR allowed_inets IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        violating, 0,
        "no row may be more restricted (or less) than the documented defaults"
    );
}
