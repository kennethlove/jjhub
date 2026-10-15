// story: e01s02
use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
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

/// story: e02s01 — git smart HTTP backend. Spawns `git http-backend` CGI for
/// the resolved repo path. No shell: env + argv only, user input goes through
/// validated owner/name before PATH_INFO construction.
pub async fn smart_http(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
    request: Request,
) -> Response {
    let name = name.strip_suffix(".git").unwrap_or(&name).to_string();
    if !domain::repo_name_valid(&name) || !domain::repo_name_valid(&owner) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let repo_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = $1 AND r.name = $2)",
    )
    .bind(&owner)
    .bind(&name)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(false);
    if !repo_exists {
        return StatusCode::NOT_FOUND.into_response();
    }

    let (parts, body) = request.into_parts();
    let path_info = parts.uri.path().to_string();
    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();

    let body_bytes = match axum::body::to_bytes(body, 64 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    let path_info = path_info
        .strip_prefix("/git/")
        .map(|rest| format!("/{rest}"))
        .unwrap_or(path_info);

    run_cgi(
        &state.storage_root,
        &parts.method,
        &path_info,
        query.as_deref().unwrap_or(""),
        &content_type,
        &body_bytes,
    )
}

fn run_cgi(
    project_root: &str,
    method: &Method,
    path_info: &str,
    query: &str,
    content_type: &str,
    body: &[u8],
) -> Response {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = match Command::new("git")
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", project_root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", path_info)
        .env("QUERY_STRING", query)
        .env("REQUEST_METHOD", method.as_str())
        .env("CONTENT_TYPE", content_type)
        .env("CONTENT_LEN", body.len().to_string())
        .env("GATEWAY_INTERFACE", "CGI/1.1")
        .env("SERVER_PROTOCOL", "HTTP/1.1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("spawn git http-backend: {err}"),
            )
                .into_response()
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(body);
    }

    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("wait git http-backend: {err}"),
            )
                .into_response()
        }
    };
    if !output.status.success() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "git http-backend: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        )
            .into_response();
    }

    parse_cgi_response(&output.stdout).unwrap_or_else(|| {
        (StatusCode::INTERNAL_SERVER_ERROR, "malformed CGI response").into_response()
    })
}

/// story: e02s01 — CGI response parse: optional Status header then body.
fn parse_cgi_response(raw: &[u8]) -> Option<Response> {
    use axum::body::Body;

    let split = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let header_bytes = &raw[..split];
    let body = raw[split + 4..].to_vec();
    let body_len = body.len();

    let mut status = StatusCode::OK;
    let mut headers = axum::http::HeaderMap::new();
    for line in String::from_utf8_lossy(header_bytes).lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            if key.eq_ignore_ascii_case("status") {
                status = StatusCode::from_bytes(value.split(' ').next()?.as_bytes()).ok()?;
            } else if let (Ok(name), Ok(val)) = (
                header::HeaderName::from_bytes(key.as_bytes()),
                header::HeaderValue::from_str(value),
            ) {
                headers.insert(name, val);
            }
        }
    }

    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    // story: e02s01 — git clients hang without a length or close: CGI bodies
    // are fully buffered here, so declare Content-Length when absent.
    if !response.headers().contains_key(header::CONTENT_LENGTH) {
        if let Ok(len) = header::HeaderValue::from_str(&body_len.to_string()) {
            response.headers_mut().insert(header::CONTENT_LENGTH, len);
        }
    }
    Some(response)
}
