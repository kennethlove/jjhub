// story: e01s01
pub mod routes;

use axum::{Router, routing::get};

pub fn app() -> Router {
    Router::new().route("/healthz", get(routes::healthz))
}
