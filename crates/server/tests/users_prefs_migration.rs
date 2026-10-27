// story: e21s01 — users gains display_name, theme, and the details
// completion marker with correct defaults; existing rows stay valid.

#[sqlx::test(migrations = "./migrations")]
async fn prefs_columns_have_expected_shape(pool: sqlx::PgPool) {
    let display_nullable: bool = sqlx::query_scalar(
        "SELECT is_nullable = 'YES' FROM information_schema.columns
         WHERE table_name = 'users' AND column_name = 'display_name'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(display_nullable, "display_name must be nullable");

    let theme_default: Option<String> = sqlx::query_scalar(
        "SELECT column_default FROM information_schema.columns
         WHERE table_name = 'users' AND column_name = 'theme'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        theme_default.as_deref(),
        Some("'system'::text"),
        "theme must default to 'system'"
    );

    let marker_default: Option<String> = sqlx::query_scalar(
        "SELECT column_default FROM information_schema.columns
         WHERE table_name = 'users' AND column_name = 'token_details_completed_at'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        marker_default, None,
        "completion marker has no default (NULL = not completed)"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn new_user_row_gets_valid_defaults(pool: sqlx::PgPool) {
    #[derive(sqlx::FromRow)]
    struct Row {
        theme: String,
        display_name: Option<String>,
        token_details_completed_at: Option<chrono::DateTime<chrono::Utc>>,
    }

    let row: Row = sqlx::query_as(
        "INSERT INTO users (email, password_hash, handle)
         VALUES ('prefs@example.com', 'x', 'prefsuser')
         RETURNING theme, display_name, token_details_completed_at",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.theme, "system");
    assert_eq!(
        row.display_name, None,
        "no backfill — display falls back to handle"
    );
    assert!(
        row.token_details_completed_at.is_none(),
        "fresh user has not completed the details screen"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn theme_check_rejects_unknown_value(pool: sqlx::PgPool) {
    let rejected = sqlx::query(
        "INSERT INTO users (email, password_hash, handle, theme)
         VALUES ('neon@example.com', 'x', 'neonuser', 'neon')",
    )
    .execute(&pool)
    .await;
    assert!(
        rejected.is_err(),
        "CHECK must reject themes outside dark|light|system"
    );
}
