// story: e22s01 — the storage contract exists before enforcement:
// permission columns on tokens with defaults that keep every legacy
// INSERT and the transport SELECT path valid.

#[sqlx::test(migrations = "./migrations")]
async fn permission_columns_have_documented_shape(pool: sqlx::PgPool) {
    // scopes: TEXT[] NOT NULL DEFAULT {read,write}
    let (data_type, default, is_nullable): (String, Option<String>, String) = sqlx::query_as(
        "SELECT data_type, column_default, is_nullable
         FROM information_schema.columns
         WHERE table_name = 'tokens' AND column_name = 'scopes'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(data_type, "ARRAY", "scopes is a TEXT[] array");
    assert!(
        default.unwrap_or_default().contains("{read,write}"),
        "scopes defaults to exactly {{read,write}}"
    );
    assert_eq!(is_nullable, "NO", "scopes is NOT NULL");

    // expires_at: TIMESTAMPTZ NULL, no default (NULL = never expires)
    let (expires_type, expires_default, expires_nullable): (String, Option<String>, String) =
        sqlx::query_as(
            "SELECT data_type, column_default, is_nullable
             FROM information_schema.columns
             WHERE table_name = 'tokens' AND column_name = 'expires_at'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(expires_type, "timestamp with time zone");
    assert_eq!(
        expires_default, None,
        "no default — NULL means never expires"
    );
    assert_eq!(expires_nullable, "YES");

    // allowed_inets: INET[] NULL — NULL means no restriction
    let (inet_type, inet_nullable): (String, String) = sqlx::query_as(
        "SELECT data_type, is_nullable
         FROM information_schema.columns
         WHERE table_name = 'tokens' AND column_name = 'allowed_inets'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(inet_type, "ARRAY");
    assert_eq!(inet_nullable, "YES");

    // repo_id: nullable FK to repositories, pin dropped (SET NULL) when
    // the repo goes away — the token itself outlives one repo pin.
    let (referenced, on_delete): (String, Option<String>) = sqlx::query_as(
        "SELECT ccu.table_name AS referenced, rc.delete_rule
         FROM information_schema.constraint_column_usage ccu
         JOIN information_schema.referential_constraints rc
           ON rc.constraint_name = ccu.constraint_name
         WHERE ccu.table_name = 'repositories'
           AND ccu.column_name = 'id'
           AND ccu.constraint_name IN (
             SELECT constraint_name FROM information_schema.key_column_usage
             WHERE table_name = 'tokens' AND column_name = 'repo_id')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(referenced, "repositories");
    assert_eq!(
        on_delete.as_deref(),
        Some("SET NULL"),
        "documented in migration"
    );

    let repo_nullable: String = sqlx::query_scalar(
        "SELECT is_nullable FROM information_schema.columns
         WHERE table_name = 'tokens' AND column_name = 'repo_id'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(repo_nullable, "YES");
}

/// A token row built the way pre-e22 code built it (only the original
/// columns) must come out with full read+write and no restrictions.
#[sqlx::test(migrations = "./migrations")]
async fn legacy_shaped_insert_gets_full_default_permissions(pool: sqlx::PgPool) {
    let user_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, handle)
         VALUES ('legacy@example.com', 'x', 'legacyuser') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // exactly the old column set — no permission columns supplied
    sqlx::query(
        "INSERT INTO tokens (user_id, label, prefix, token_hash)
         VALUES ($1, 'legacy', 'jjh_ab', 'hash')",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .expect("legacy-shaped INSERT must still work");

    #[derive(sqlx::FromRow)]
    struct Perms {
        scopes: Vec<String>,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
        repo_id: Option<uuid::Uuid>,
    }

    let perms: Perms = sqlx::query_as(
        "SELECT scopes, expires_at, repo_id
         FROM tokens WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        perms.scopes,
        vec!["read", "write"],
        "default scope set is exactly read+write"
    );
    assert!(perms.expires_at.is_none());
    assert!(perms.repo_id.is_none());
    // INET[] read as a NULL check — no ipnetwork dep needed for a shape test
    let has_allowlist: bool =
        sqlx::query_scalar("SELECT allowed_inets IS NOT NULL FROM tokens WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!has_allowlist, "NULL allowlist = unrestricted");
}
