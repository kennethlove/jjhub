// story: e01s01
use server::app;

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("connect to database");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("run migrations");

    // story: e02s01 — bind all interfaces so docker port mapping works.
    let port = std::env::var("JJHUB_PORT").unwrap_or_else(|_| "3000".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .unwrap_or_else(|e| panic!("failed to bind 0.0.0.0:{port}: {e}"));
    axum::serve(listener, app(pool))
        .await
        .expect("server error");
}
