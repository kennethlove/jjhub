// story: e01s01
pub mod middleware;
pub mod password;
pub mod repo;
pub mod routes;
pub mod session;
pub mod token;
pub mod transport;

use axum::routing::{get, post};
use axum::Router;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub session_secret: Vec<u8>,
    pub storage_root: String,
}

#[derive(Clone, Copy)]
pub struct CurrentUser(pub uuid::Uuid);

pub fn app(pool: PgPool) -> Router {
    let storage_root =
        std::env::var("JJHUB_STORAGE_ROOT").unwrap_or_else(|_| "./data/repos".into());
    app_with_storage(pool, storage_root)
}

pub fn app_with_storage(pool: PgPool, storage_root: String) -> Router {
    let session_secret = std::env::var("SESSION_SECRET")
        .map(|s| s.into_bytes())
        .unwrap_or_else(|_| b"dev-only-secret-change-me".to_vec());
    app_with_secret(pool, session_secret, storage_root)
}

pub fn app_with_secret(pool: PgPool, session_secret: Vec<u8>, storage_root: String) -> Router {
    let state = AppState {
        pool,
        session_secret,
        storage_root,
    };
    let protected = Router::new()
        .route("/settings", get(routes::settings))
        .route("/settings/tokens", post(routes::create_token))
        .route("/repos", post(routes::create_repo))
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
        .route("/{owner}/{name}", get(routes::repo_page))
        .merge(protected)
        .merge(transport_routes(state.clone()))
        .with_state(state)
}

fn transport_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/git/whoami", get(routes::git_whoami))
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            transport::require_token,
        ))
}
