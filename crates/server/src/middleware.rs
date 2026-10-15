// story: e01s01
use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use uuid::Uuid;

use crate::session::{self, SESSION_COOKIE};
use crate::AppState;

pub async fn require_session(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    mut request: Request,
    next: Next,
) -> Response {
    let cookie_value = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .map(str::trim)
                .find_map(|pair| pair.strip_prefix(&format!("{SESSION_COOKIE}=")))
                .map(str::to_string)
        });

    let user_id: Option<Uuid> =
        cookie_value.and_then(|value| session::verify(&state.session_secret, &value));

    match user_id {
        Some(id) => {
            let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE id = $1")
                .bind(id)
                .fetch_optional(&state.pool)
                .await
                .ok()
                .flatten();
            if exists.is_some() {
                request.extensions_mut().insert(crate::CurrentUser(id));
                next.run(request).await
            } else {
                Redirect::to("/login").into_response()
            }
        }
        None => Redirect::to("/login").into_response(),
    }
}
