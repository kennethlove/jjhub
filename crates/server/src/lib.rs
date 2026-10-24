// story: e01s01
pub mod browse;
pub mod divergence;
pub mod jj_meta;
pub mod merge;
pub mod middleware;
pub mod mr;
pub mod password;
pub mod receive_guard;
pub mod render;
pub mod repo;
pub mod review_proxy;
pub mod revsets;
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
    // story: kata links — loopback-published sidecar URL for browser links.
    pub kata_public_base: String,
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
    // story: kata links — same-origin now; base kept for future flexibility
    let kata_public_base = std::env::var("KATA_PUBLIC_BASE").unwrap_or_default();
    app_with_kata(pool, storage_root, kata_upstream, kata_public_base)
}

/// story: e05s03 — explicit kata wiring for tests.
pub fn app_with_kata(
    pool: PgPool,
    storage_root: String,
    kata_upstream: String,
    kata_public_base: String,
) -> Router {
    app_with_secret(
        pool,
        session_secret_from_env(),
        storage_root,
        kata_upstream,
        kata_public_base,
    )
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
    kata_public_base: String,
) -> Router {
    let state = AppState {
        pool,
        session_secret,
        storage_root,
        kata_upstream,
        kata_public_base,
    };
    let protected = Router::new()
        .route("/settings", get(routes::settings))
        .route("/settings/tokens", post(routes::create_token))
        // story: e02s03 — repo-page generate button (session + CSRF, same group)
        .route("/{owner}/{name}/tokens", post(routes::create_repo_token))
        .route("/repos", post(routes::create_repo))
        .route("/{owner}/{name}/settings", post(routes::repo_settings))
        .route(
            "/{owner}/{name}/settings/collaborators",
            post(routes::manage_collaborators),
        )
        .route(
            "/{owner}/{name}/merge-requests/{change_id}/merge",
            post(routes::merge_mr),
        )
        .route(
            "/{owner}/{name}/merge-requests/{change_id}/abandon",
            post(routes::abandon_mr),
        )
        .route(
            "/{owner}/{name}/merge-requests/{change_id}/comments",
            post(routes::add_comment),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::require_session,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::require_csrf,
        ));
    Router::new()
        .route("/healthz", get(routes::healthz))
        .route("/", get(routes::home))
        .route(
            "/register",
            get(routes::register_form).post(routes::register),
        )
        .route("/login", get(routes::login_form).post(routes::login))
        .route(
            "/logout",
            post(routes::logout).layer(axum::middleware::from_fn_with_state(
                state.clone(),
                middleware::require_csrf,
            )),
        )
        .route("/{owner}/{name}", get(routes::repo_page))
        // story: e11s01 — revset search endpoint
        .route("/{owner}/{name}/revsets", get(revsets::revsets_query))
        // story: e11s02 — repository-scoped revset search page
        .route("/{owner}/{name}/search", get(revsets::search_page))
        // story: e09s01 — the per-repository operation log
        .route("/{owner}/{name}/operations", get(routes::operations_page))
        // story: e09s02 — one operation in the audit log, with its
        // view's ref changes
        .route(
            "/{owner}/{name}/operations/{operation_id}",
            get(routes::operation_detail_page),
        )
        .route("/r/{*rest}", get(review_proxy::spa_route))
        .route("/api/{*rest}", get(review_proxy::api_route))
        // story: e12s02 — the site stylesheet shadows the kata asset proxy
        .route("/assets/app.css", get(routes::app_css))
        .route("/assets/{*rest}", get(review_proxy::assets_route))
        .route("/favicon.svg", get(review_proxy::assets_route))
        .route(
            "/{owner}/{name}/merge-requests",
            get(routes::merge_requests_page),
        )
        .route(
            "/{owner}/{name}/merge-requests/{change_id}",
            get(routes::mr_detail_page),
        )
        .route("/{owner}/{name}/tree", get(routes::tree_root))
        .route("/{owner}/{name}/tree/{git_ref}", get(routes::tree_page))
        .route(
            "/{owner}/{name}/tree/{git_ref}/{*path}",
            get(routes::tree_page_at),
        )
        .route(
            "/{owner}/{name}/blob/{git_ref}/{*path}",
            get(routes::blob_page),
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
        .merge(
            Router::new()
                .route(
                    "/{owner}/{name}/merge-requests/{change_id}/statuses",
                    post(routes::create_status),
                )
                .route_layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    transport::require_token,
                )),
        )
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

/// story: kata scan — the sidecar's workspace scanner watches the meta
/// root; create it (and the storage root) eagerly at boot so the watch
/// attaches on a fresh volume instead of kata falling back to polling
/// and warn-scanning a missing path every cycle.
pub fn ensure_storage_layout(storage_root: &str) -> Result<(), String> {
    let root = std::path::Path::new(storage_root);
    std::fs::create_dir_all(root.join(".meta")).map_err(|e| e.to_string())
}
