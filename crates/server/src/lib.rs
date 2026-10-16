// story: e01s01
pub mod jj_meta;
pub mod middleware;
pub mod mr;
pub mod password;
pub mod receive_guard;
pub mod repo;
pub mod review_proxy;
pub mod routes;
pub mod session;
pub mod stack;
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
    // story: e05s03 — kata sidecar upstream.
    pub kata_upstream: String,
}

#[derive(Clone, Copy)]
pub struct CurrentUser(pub uuid::Uuid);

pub fn app(pool: PgPool) -> Router {
    let storage_root =
        std::env::var("JJHUB_STORAGE_ROOT").unwrap_or_else(|_| "./data/repos".into());
    app_with_storage(pool, storage_root)
}

pub fn app_with_storage(pool: PgPool, storage_root: String) -> Router {
    let kata_upstream =
        std::env::var("KATA_UPSTREAM").unwrap_or_else(|_| "http://127.0.0.1:7878".into());
    app_with_kata(pool, storage_root, kata_upstream)
}

/// story: e05s03 — explicit kata wiring for tests.
pub fn app_with_kata(pool: PgPool, storage_root: String, kata_upstream: String) -> Router {
    app_with_secret(pool, session_secret_from_env(), storage_root, kata_upstream)
}

fn session_secret_from_env() -> Vec<u8> {
    std::env::var("SESSION_SECRET")
        .map(|s| s.into_bytes())
        .unwrap_or_else(|_| b"dev-only-secret-change-me".to_vec())
}

pub fn app_with_secret(
    pool: PgPool,
    session_secret: Vec<u8>,
    storage_root: String,
    kata_upstream: String,
) -> Router {
    let state = AppState {
        pool,
        session_secret,
        storage_root,
        kata_upstream,
    };
    let protected = Router::new()
        .route("/settings", get(routes::settings))
        .route("/settings/tokens", post(routes::create_token))
        .route("/repos", post(routes::create_repo))
        .route("/{owner}/{name}/settings", post(routes::repo_settings))
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
        .route("/{owner}/{name}/pulls", get(routes::pulls_page))
        .route(
            "/{owner}/{name}/pulls/{change_id}",
            get(routes::mr_detail_page),
        )
        .route(
            "/review/{owner}/{name}",
            get(review_proxy::proxy).post(review_proxy::proxy),
        )
        .route(
            "/review/{owner}/{name}/",
            get(review_proxy::proxy).post(review_proxy::proxy),
        )
        .route(
            "/review/{owner}/{name}/{*rest}",
            get(review_proxy::proxy_rest).post(review_proxy::proxy_rest),
        )
        .merge(protected)
        .merge(transport_routes(state.clone()))
        .with_state(state)
}

fn transport_routes(state: AppState) -> Router<AppState> {
    let whoami = Router::new()
        .route("/git/whoami", get(routes::git_whoami))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            transport::require_token,
        ));
    whoami
        .route(
            "/git/{owner}/{name}/info/refs",
            get(transport::smart_http).post(transport::smart_http),
        )
        .route(
            "/git/{owner}/{name}/git-upload-pack",
            post(transport::smart_http),
        )
        .route(
            "/git/{owner}/{name}/git-receive-pack",
            post(transport::smart_http),
        )
        .with_state(state)
}
