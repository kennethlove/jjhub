// story: e01s01
pub mod password;
pub mod routes;
pub mod session;

use axum::Router;
use axum::routing::{get, post};
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub session_secret: Vec<u8>,
}

pub fn app(pool: PgPool) -> Router {
    let session_secret = std::env::var("SESSION_SECRET")
        .map(|s| s.into_bytes())
        .unwrap_or_else(|_| b"dev-only-secret-change-me".to_vec());
    app_with_secret(pool, session_secret)
}

pub fn app_with_secret(pool: PgPool, session_secret: Vec<u8>) -> Router {
    let state = AppState { pool, session_secret };
    Router::new()
        .route("/healthz", get(routes::healthz))
        .route("/register", get(routes::register_form).post(routes::register))
        .route("/settings", get(routes::settings))
        .with_state(state)
}
