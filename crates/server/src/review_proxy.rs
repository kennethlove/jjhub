// story: e05s03
//! Auth proxy: jjhub session/token in front of the kata sidecar.
//!
//! `/review/{owner}/{name}/{*rest}` authenticates the request (session
//! cookie or transport token), applies repo-level authorization (invariant
//! 6 extended to `/review/**`), strips any client-supplied trust header,
//! injects jjhub's own, and streams the request to the sidecar. The sidecar
//! stays unpublished on the compose network; this proxy is the only way in.

use axum::extract::{Path, RawQuery, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::AppState;

/// story: e05s03 — authenticate without redirecting: session cookie first
/// (web), then transport token (API clients). Unauthenticated browsers get
/// the login redirect; anything else gets 401.
async fn authenticate(state: &AppState, headers: &HeaderMap) -> Option<uuid::Uuid> {
    if let Some(user_id) = session_user(state, headers) {
        return Some(user_id);
    }
    transport_user(state, headers).await
}

fn session_user(state: &AppState, headers: &HeaderMap) -> Option<uuid::Uuid> {
    let cookie = headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|pair| {
            pair.strip_prefix(&format!("{}=", crate::session::SESSION_COOKIE))
                .map(str::to_string)
        })?;
    crate::session::verify(&state.session_secret, &cookie)
}

async fn transport_user(state: &AppState, headers: &HeaderMap) -> Option<uuid::Uuid> {
    let credentials = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))?;
    use base64::Engine;
    let decoded = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(credentials.trim())
            .ok()?,
    )
    .ok()?;
    let (username, token_value) = decoded.split_once(':')?;
    if username != "x-access-token" {
        return None;
    }
    crate::token::authenticate(&state.pool, token_value).await
}

pub async fn proxy(
    State(state): State<AppState>,
    Path((owner, name)): axum::extract::Path<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    request: Request,
) -> Response {
    run_proxy(state, owner, name, query, headers, request).await
}

/// story: e05s03 — same as `proxy` for routes carrying a `{*rest}` segment.
pub async fn proxy_rest(
    State(state): State<AppState>,
    Path((owner, name, _rest)): axum::extract::Path<(String, String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    request: Request,
) -> Response {
    run_proxy(state, owner, name, query, headers, request).await
}

async fn run_proxy(
    state: AppState,
    owner: String,
    name: String,
    query: Option<String>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let user = match authenticate(&state, &headers).await {
        Some(user_id) => user_id,
        None => {
            // token clients get 401 JSON; everyone else gets the login
            // redirect (browsers follow it; unauthenticated curls see it too)
            return if headers.contains_key(axum::http::header::AUTHORIZATION) {
                (StatusCode::UNAUTHORIZED, "authentication required").into_response()
            } else {
                axum::response::Redirect::to("/login").into_response()
            };
        }
    };

    // story: e05s03 — repo authorization, invariant 6 on /review/**: public
    // read for any logged-in user, owner-only for private; unknown and
    // unauthorized both 404 (no existence leak).
    let repo: Option<(uuid::Uuid, String)> = sqlx::query_as(
        "SELECT r.owner_id, r.visibility FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(&owner)
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some((repo_owner, visibility)) = repo else {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    };
    if visibility == "private" && repo_owner != user {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    }

    // story: e05s03 — the sidecar runs in trust-forwarded-header mode; the
    // only header it trusts is the one the proxy sets (and strips from
    // clients). Trust is network-scoped: the sidecar is unpublished on the
    // compose network, so this proxy is the only caller that can reach it.
    let email: Option<String> = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(user)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();
    let Some(email) = email else {
        return (StatusCode::UNAUTHORIZED, "authentication required").into_response();
    };

    let (parts, body) = request.into_parts();
    // forward only the repo-scoped rest; the sidecar routes on its own paths
    let rest = parts
        .uri
        .path()
        .strip_prefix(&format!("/review/{owner}/{name}"))
        .unwrap_or("/")
        .to_string();

    // rebuild headers: drop any client identity header, forward the rest
    let mut upstream_headers = HeaderMap::new();
    for (name, value) in parts.headers.iter() {
        if name == "x-review-author" {
            continue;
        }
        upstream_headers.insert(name.clone(), value.clone());
    }
    upstream_headers.insert(
        HeaderName::from_static("x-review-author"),
        HeaderValue::from_str(&email).unwrap_or_else(|_| HeaderValue::from_static("invalid")),
    );

    let url = format!(
        "{}{}{}",
        state.kata_upstream.trim_end_matches('/'),
        rest,
        query
            .as_deref()
            .map(|q| format!("?{q}"))
            .unwrap_or_default()
    );

    let client = reqwest::Client::new();
    let upstream_request = client
        .request(
            Method::from_bytes(parts.method.as_str().as_bytes()).unwrap_or(Method::GET),
            &url,
        )
        .headers(
            upstream_headers
                .iter()
                .filter_map(|(k, v)| {
                    reqwest::header::HeaderName::from_bytes(k.as_str().as_bytes())
                        .ok()
                        .zip(reqwest::header::HeaderValue::from_bytes(v.as_bytes()).ok())
                })
                .collect::<reqwest::header::HeaderMap>(),
        )
        .body(reqwest::Body::wrap_stream(body.into_data_stream()));

    let upstream_response = match upstream_request.send().await {
        Ok(response) => response,
        Err(err) => {
            tracing::warn!(%err, "kata sidecar unreachable");
            return (StatusCode::BAD_GATEWAY, "review service unavailable").into_response();
        }
    };

    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream_response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| HeaderValue::from_bytes(v.as_bytes()).ok());
    let mut response = Response::new(axum::body::Body::from_stream(
        reqwest::Response::bytes_stream(upstream_response),
    ));
    *response.status_mut() = status;
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, content_type);
    }
    response
}

/// story: e05s04 — kata review link for a change, through the auth proxy.
/// None when the repo has no colocated jj metadata (pre-backfill repos
/// must never show a dead link). The kata SPA deep-link is
/// `/r/{repo}/browse?change=<change-id>`; the sidecar names the repo by
/// its flattened `{owner}__{name}` workspace dir.
pub fn review_link(
    public_base: &str,
    storage_root: &str,
    owner: &str,
    name: &str,
    change_id: &str,
) -> Option<String> {
    let meta = domain::meta_path(storage_root, owner, name);
    if !meta.join(".jj").exists() {
        return None;
    }
    // story: kata links — the SPA is served at jjhub's origin (/r/…) with
    // identity injected per request; same origin, so routing, assets, and
    // API all resolve (the loopback shortcut dead-ended in kata's
    // unauthenticated /auth/login redirect loop).
    Some(if change_id.is_empty() {
        format!("{public_base}/r/{owner}__{name}/browse")
    } else {
        format!("{public_base}/r/{owner}__{name}/browse?change={change_id}")
    })
}

/// story: kata at jjhub origin — shared repo authz for SPA/API routes:
/// invariant 6 semantics, 404 no-leak.
async fn authorize_repo(state: &AppState, owner: &str, name: &str, user: uuid::Uuid) -> bool {
    let repo: Option<(uuid::Uuid, String)> = sqlx::query_as(
        "SELECT r.owner_id, r.visibility FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(owner)
    .bind(name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    match repo {
        Some((repo_owner, visibility)) => visibility != "private" || repo_owner == user,
        None => false,
    }
}

/// story: kata at jjhub origin — forward the request to the sidecar with the
/// client's identity header stripped and the authenticated user injected.
async fn forward_to_kata(state: &AppState, user: uuid::Uuid, request: Request) -> Response {
    let email: Option<String> = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(user)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();
    let Some(email) = email else {
        return (StatusCode::UNAUTHORIZED, "authentication required").into_response();
    };

    let (parts, body) = request.into_parts();
    let mut upstream_headers = HeaderMap::new();
    for (name, value) in parts.headers.iter() {
        if name == "x-review-author" {
            continue;
        }
        upstream_headers.insert(name.clone(), value.clone());
    }
    upstream_headers.insert(
        HeaderName::from_static("x-review-author"),
        HeaderValue::from_str(&email).unwrap_or_else(|_| HeaderValue::from_static("invalid")),
    );

    let url = format!(
        "{}{}{}",
        state.kata_upstream.trim_end_matches('/'),
        parts.uri.path(),
        parts
            .uri
            .query()
            .map(|q| format!("?{q}"))
            .unwrap_or_default()
    );

    let client = reqwest::Client::new();
    let upstream_request = client
        .request(
            Method::from_bytes(parts.method.as_str().as_bytes()).unwrap_or(Method::GET),
            &url,
        )
        .headers(
            upstream_headers
                .iter()
                .filter_map(|(k, v)| {
                    reqwest::header::HeaderName::from_bytes(k.as_str().as_bytes())
                        .ok()
                        .zip(reqwest::header::HeaderValue::from_bytes(v.as_bytes()).ok())
                })
                .collect::<reqwest::header::HeaderMap>(),
        )
        .body(reqwest::Body::wrap_stream(body.into_data_stream()));

    let upstream_response = match upstream_request.send().await {
        Ok(response) => response,
        Err(err) => {
            tracing::warn!(%err, "kata sidecar unreachable");
            return (StatusCode::BAD_GATEWAY, "review service unavailable").into_response();
        }
    };

    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream_response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| HeaderValue::from_bytes(v.as_bytes()).ok());
    let mut response = Response::new(axum::body::Body::from_stream(
        reqwest::Response::bytes_stream(upstream_response),
    ));
    *response.status_mut() = status;
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, content_type);
    }
    response
}

/// story: kata at jjhub origin — the SPA's own path (`/r/{repo}/…`). The
/// first segment is the flattened `{owner}__{name}` repo id.
pub async fn spa_route(
    State(state): State<AppState>,
    axum::extract::Path(rest): axum::extract::Path<String>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let (repo_part, _) = rest.split_once('/').unwrap_or((rest.as_str(), ""));
    let Some((owner, name)) = repo_part.split_once("__") else {
        return (StatusCode::NOT_FOUND, "unknown repository").into_response();
    };
    let Some(user) = authenticate(&state, &headers).await else {
        return axum::response::Redirect::to("/login").into_response();
    };
    if !authorize_repo(&state, owner, name, user).await {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    }
    forward_to_kata(&state, user, request).await
}

/// story: kata at jjhub origin — the SPA's API. Repo-scoped paths
/// (`/api/repos/{owner}__{name}/…`) get invariant-6 authz; the rest
/// (whoami, events) needs any session.
pub async fn api_route(
    State(state): State<AppState>,
    axum::extract::Path(rest): axum::extract::Path<String>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let Some(user) = authenticate(&state, &headers).await else {
        return if headers.contains_key(axum::http::header::AUTHORIZATION) {
            (StatusCode::UNAUTHORIZED, "authentication required").into_response()
        } else {
            axum::response::Redirect::to("/login").into_response()
        };
    };
    if let Some(repo_part) = rest.strip_prefix("repos/") {
        let repo_id = repo_part.split('/').next().unwrap_or("");
        if let Some((owner, name)) = repo_id.split_once("__") {
            if !authorize_repo(&state, owner, name, user).await {
                return (StatusCode::NOT_FOUND, "repository not found").into_response();
            }
        }
    }
    forward_to_kata(&state, user, request).await
}

/// story: kata at jjhub origin — static assets. No session: they carry no
/// repository data and the SPA page that requests them is already authed.
pub async fn assets_route(State(state): State<AppState>, request: Request) -> Response {
    forward_anonymous(&state, request).await
}

async fn forward_anonymous(state: &AppState, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let url = format!(
        "{}{}{}",
        state.kata_upstream.trim_end_matches('/'),
        parts.uri.path(),
        parts
            .uri
            .query()
            .map(|q| format!("?{q}"))
            .unwrap_or_default()
    );
    let client = reqwest::Client::new();
    let upstream_response = match client
        .request(
            Method::from_bytes(parts.method.as_str().as_bytes()).unwrap_or(Method::GET),
            &url,
        )
        .body(reqwest::Body::wrap_stream(body.into_data_stream()))
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => {
            tracing::warn!(%err, "kata sidecar unreachable");
            return (StatusCode::BAD_GATEWAY, "review service unavailable").into_response();
        }
    };
    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream_response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| HeaderValue::from_bytes(v.as_bytes()).ok());
    let mut response = Response::new(axum::body::Body::from_stream(
        reqwest::Response::bytes_stream(upstream_response),
    ));
    *response.status_mut() = status;
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, content_type);
    }
    response
}
