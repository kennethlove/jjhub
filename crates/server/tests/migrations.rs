// story: e01s01
#[sqlx::test(migrations = "./migrations")]
async fn migrations_create_users_table(pool: sqlx::PgPool) {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_name = 'users')")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(exists, "users table missing after migrations");
}
