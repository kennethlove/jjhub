// story: e01s02
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use crate::token;
use crate::AppState;

/// Extensions injected by the transport auth middleware.
#[derive(Clone, Copy)]
pub struct TransportUser(pub Uuid);

/// story: e01s02 — resolve HTTP Basic credentials (x-access-token + token value)
/// to the owning user for git transport routes.
pub async fn require_token(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    mut request: Request,
    next: Next,
) -> Response {
    let invalid = || (StatusCode::UNAUTHORIZED, "invalid token").into_response();

    let credentials = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|v| {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(v.trim())
                .ok()
        })
        .and_then(|bytes| String::from_utf8(bytes).ok());

    let Some(credentials) = credentials else {
        return invalid();
    };
    let Some((username, token_value)) = credentials.split_once(':') else {
        return invalid();
    };
    if username != "x-access-token" {
        return invalid();
    }

    match token::authenticate(&state.pool, token_value).await {
        Some(user_id) => {
            request.extensions_mut().insert(TransportUser(user_id));
            next.run(request).await
        }
        None => invalid(),
    }
}
