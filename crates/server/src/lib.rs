// story: e01s01
pub mod middleware;
pub mod password;
pub mod routes;
pub mod session;
pub mod token;

use axum::routing::{get, post};
use axum::Router;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub session_secret: Vec<u8>,
}

#[derive(Clone, Copy)]
pub struct CurrentUser(pub uuid::Uuid);

pub fn app(pool: PgPool) -> Router {
    let session_secret = std::env::var("SESSION_SECRET")
        .map(|s| s.into_bytes())
        .unwrap_or_else(|_| b"dev-only-secret-change-me".to_vec());
    app_with_secret(pool, session_secret)
}

pub fn app_with_secret(pool: PgPool, session_secret: Vec<u8>) -> Router {
    let state = AppState {
        pool,
        session_secret,
    };
    let protected = Router::new()
        .route("/settings", get(routes::settings))
        .route("/settings/tokens", post(routes::create_token))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::require_session,
        ));
    Router::new()
        .route("/healthz", get(routes::healthz))
        .route(
            "/register",
            get(routes::register_form).post(routes::register),
        )
        .route("/login", get(routes::login_form).post(routes::login))
        .route("/logout", post(routes::logout))
        .merge(protected)
        .with_state(state)
}
