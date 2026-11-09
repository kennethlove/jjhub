// story: e01s01
use axum::extract::{Request, State};
use axum::http::header;
use axum::http::StatusCode;
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
            // story: e29s01 — the existence check doubles as the wall:
            // a suspended account is an account this request may not see.
            let exists: Option<Uuid> =
                sqlx::query_scalar("SELECT id FROM users WHERE id = $1 AND suspended_at IS NULL")
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

/// verify the `_csrf` hidden field on session-authenticated
/// POSTs. Buffers the form body, compares its `_csrf` against the token
/// derived from the session cookie, then rebuilds the request. GET passes
/// through untouched.
pub async fn require_csrf(
    State(state): State<crate::AppState>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() != axum::http::Method::POST {
        return next.run(request).await;
    }
    let session_cookie = request
        .headers()
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';').map(str::trim).find_map(|pair| {
                pair.strip_prefix(&format!("{}=", crate::session::SESSION_COOKIE))
                    .map(str::to_string)
            })
        });
    let Some(session_cookie) = session_cookie else {
        // no session: the auth middleware owns the response (redirect/401)
        return next.run(request).await;
    };
    let expected = crate::session::csrf_token(&state.session_secret, &session_cookie);

    let (parts, body) = request.into_parts();
    let body_bytes = match axum::body::to_bytes(body, 64 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return (StatusCode::BAD_REQUEST, "unreadable body").into_response(),
    };
    let form = String::from_utf8_lossy(&body_bytes);
    let supplied: Option<String> = form
        .split('&')
        .find_map(|pair| pair.strip_prefix("_csrf=").map(urlencoding_decode));
    match supplied.as_deref() {
        Some(token) if constant_time_eq(token, &expected) => {}
        _ => return (StatusCode::FORBIDDEN, "invalid or missing CSRF token").into_response(),
    }

    let mut rebuilt = Request::from_parts(parts, axum::body::Body::from(body_bytes));
    // keep the buffered body for downstream handlers
    rebuilt
        .headers_mut()
        .remove(axum::http::header::CONTENT_LENGTH);
    next.run(rebuilt).await
}

fn urlencoding_decode(value: &str) -> String {
    value
        .replace('+', " ")
        .replace("%3D", "=")
        .replace("%3d", "=")
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// jjhub-6js — one structured line per HTTP request: method, path,
/// status, elapsed. healthz is liveness polling, not traffic: debug.
pub async fn request_log(request: Request, next: Next) -> Response {
    let start = std::time::Instant::now();
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let response = next.run(request).await;
    let elapsed_ms = start.elapsed().as_millis() as u64;
    let status = response.status().as_u16();
    if path == "/healthz" {
        tracing::debug!(%method, %path, status, elapsed_ms, "request");
    } else {
        tracing::info!(%method, %path, status, elapsed_ms, "request");
    }
    response
}
