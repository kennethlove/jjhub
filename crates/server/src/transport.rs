// story: e01s02
use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use crate::token;
use crate::AppState;

/// story: e02s02 — which git operation a transport request performs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GitOp {
    UploadPack,
    ReceivePack,
}

/// Extensions injected by the transport auth middleware.
#[derive(Clone, Copy)]
pub struct TransportUser(pub Uuid);

/// story: e02s02 — resolve HTTP Basic credentials (x-access-token + token value)
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

    // story: e22s02 — the allowlist keys on the peer address the same
    // way the rate limiter reads it; absent ConnectInfo = unverifiable
    // = fail-closed inside authenticate.
    let ip = request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|connect| connect.0.ip());
    match token::authenticate(&state.pool, token_value, ip).await {
        Some(auth) => {
            request.extensions_mut().insert(TransportUser(auth.user_id));
            next.run(request).await
        }
        None => invalid(),
    }
}

/// story: e19s02 — one admission decision for a missing row on the
/// smart-HTTP path: a valid token presenter pushing ReceivePack into
/// their own namespace creates the repo (PRIVATE) through the shared
/// creation service. That ownership gate is this path's
/// authorization — it runs before any subprocess — and every other
/// request falls through to 404, indistinguishable from a repo that
/// does not exist.
// story: e19s02 — admission decides a missing repo either becomes a
// PRIVATE create or nothing. story: e20s02 — it charges the create
// row before creating, so the error carries a ready 429/500 Response.
// WHY: allow, not a params struct — the two charge args travel with
// every admit_create call and a struct would hide the seam.
#[allow(clippy::too_many_arguments)]
pub async fn admit_create(
    pool: &sqlx::PgPool,
    storage_root: &str,
    owner: &str,
    name: &str,
    op: GitOp,
    user: Option<Uuid>,
    // story: e20s02 — the create row is charged here, only when a repo
    // is actually about to be created; SSH passes no limiter until
    // e20s03 lands its disconnect policy.
    peer_ip: &str,
    limiter: Option<&std::sync::Arc<crate::rate_limit::RateLimiter>>,
) -> Result<Option<(Uuid, Uuid, String)>, Box<axum::response::Response>> {
    if op != GitOp::ReceivePack {
        return Ok(None);
    }
    let Some(user_id) = user else {
        return Ok(None);
    };
    let handle: Option<String> = sqlx::query_scalar("SELECT handle FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    let Some(handle) = handle else {
        return Ok(None);
    };
    if owner != handle {
        return Ok(None);
    }
    // story: e20s02 — push-to-create charges BOTH rows: the push row
    // was charged at the transport edge; the create row is charged
    // here, before any creation work.
    if let Some(limiter) = limiter {
        if let Err(retry) = limiter.try_charge(
            crate::rate_limit::Action::Create,
            &user_id.to_string(),
            peer_ip,
        ) {
            return Err(Box::new(crate::rate_limit::too_many_requests(retry)));
        }
    }
    match crate::creation::create_repository(pool, storage_root, user_id, name, "private").await {
        Ok(created) => Ok(Some((created.id, user_id, "private".to_string()))),
        // story: e19s02 — race loser: the winner's row is the admission.
        Err(crate::creation::CreateError::Duplicate) => {
            let row: Option<(Uuid, Uuid, String)> = sqlx::query_as(
                "SELECT id, owner_id, visibility FROM repositories WHERE owner_id = $1 AND name = $2",
            )
            .bind(user_id)
            .bind(name)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
            Ok(row)
        }
        // story: e19s02 — validated upstream; treat as not-a-create.
        Err(crate::creation::CreateError::InvalidName)
        | Err(crate::creation::CreateError::InvalidVisibility) => Ok(None),
        Err(crate::creation::CreateError::Storage(_))
        | Err(crate::creation::CreateError::Db(_)) => Err(Box::new(
            (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response(),
        )),
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

    let (parts, body) = request.into_parts();
    let path_info = parts.uri.path().to_string();

    // story: e02s02 — authz before spawning any subprocess.
    let op = if path_info.contains("git-receive-pack")
        || query.as_deref().unwrap_or("").contains("git-receive-pack")
    {
        GitOp::ReceivePack
    } else {
        GitOp::UploadPack
    };
    let user = token_user(&parts.headers, &state, client_ip(&parts.extensions)).await;
    let transport_user = user.as_ref().map(|auth| auth.user_id);

    // story: e20s02 — push charges the push row BEFORE any git work:
    // the budget protects work, not admission, so token and IP rows
    // are checked, then both charged (check-then-charge). Anonymous
    // pushes share one bucket per client IP.
    if op == GitOp::ReceivePack {
        if let Some(limiter) = state.rate_limiter.as_ref() {
            let ip = crate::rate_limit::peer_ip(&parts.extensions);
            let who = transport_user
                .map(|u| u.to_string())
                .unwrap_or_else(|| "anonymous".to_string());
            if let Err(retry) = limiter.try_charge(crate::rate_limit::Action::Push, &who, &ip) {
                return crate::rate_limit::too_many_requests(retry);
            }
        }
    }

    let repo: Option<(uuid::Uuid, uuid::Uuid, String)> = sqlx::query_as(
        "SELECT r.id, r.owner_id, r.visibility FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(&owner)
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    // story: e19s02 — a missing row decides create-or-404 here, once,
    // in the shared transport path; http-backend never spawns before
    // the row exists (the receive guard would reject it anyway).
    let repo = match repo {
        Some(repo) => Some(repo),
        None => {
            // story: e22s02 — scope and pin gate admission ITSELF, so a
            // denied push-to-create never creates (no orphan row).
            if crate::token::transport_gate(user.as_ref(), op, None).is_err() {
                return (StatusCode::FORBIDDEN, "forbidden").into_response();
            }
            let ip = crate::rate_limit::peer_ip(&parts.extensions);
            match admit_create(
                &state.pool,
                &state.storage_root,
                &owner,
                &name,
                op,
                transport_user,
                &ip,
                state.rate_limiter.as_ref(),
            )
            .await
            {
                Ok(repo) => repo,
                Err(response) => return *response,
            }
        }
    };
    let Some((repo_id, repo_owner, visibility)) = repo else {
        return StatusCode::NOT_FOUND.into_response();
    };

    if let Err((status, extra_headers)) = authorize(
        op,
        transport_user,
        repo_owner,
        &visibility,
        &state.pool,
        repo_id,
        user.as_ref(),
    )
    .await
    {
        let mut response = (status, "forbidden").into_response();
        for (name, value) in extra_headers {
            if let Ok(value) = header::HeaderValue::from_str(value) {
                response.headers_mut().insert(name, value);
            }
        }
        return response;
    }

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

    let response = run_cgi(
        &state.storage_root,
        &parts.method,
        &path_info,
        query.as_deref().unwrap_or(""),
        &content_type,
        &body_bytes,
    );

    // story: e05s02 — a successful push writes objects into the bare repo
    // but leaves the colocated meta workspace's refs stale, so the kata
    // sidecar would never see the new commits. Re-import refs after a
    // successful receive-pack. A sync failure must not fail the push (the
    // objects are already durable).
    //
    // story: e17s02 — HTTP and SSH run ONE post-receive pipeline.
    if op == GitOp::ReceivePack && response.status().is_success() {
        post_receive(
            &state.pool,
            &state.storage_root,
            &owner,
            &name,
            transport_user,
            &body_bytes,
        )
        .await;
    }
    response
}

/// story: e03s01, e17s02 — the single post-receive pipeline: ref sync,
/// delete-abandon, and MR upsert from the pkt-line push body. HTTP
/// smart-http and the SSH receive-pack exec both call this (no second
/// implementation).
pub(crate) async fn post_receive(
    pool: &sqlx::PgPool,
    storage_root: &str,
    owner: &str,
    name: &str,
    pusher: Option<uuid::Uuid>,
    pkt_bytes: &[u8],
) {
    let meta = domain::meta_path(storage_root, owner, name);
    if let Err(err) = crate::jj_meta::sync_refs(&meta).await {
        tracing::warn!(%owner, %name, %err, "post-push ref sync failed");
    }
    // story: e03s01 — the same hook upserts merge requests from the pushed
    // change-ids (post-receive half of the review model; the pre-receive
    // half already refused foreign-author rewrites).
    let Some(pusher) = pusher else {
        return;
    };
    // story: e03s04 — deleted refs abandon their open MRs
    for update in crate::receive_guard::parse_push_commands(pkt_bytes) {
        if update.new.chars().all(|c| c == '0') && update.ref_name.starts_with("refs/heads/") {
            if let Err(err) = crate::mr::abandon_for_deleted_ref(
                pool,
                repo_owner_id(pool, owner, name).await,
                &update.old,
            )
            .await
            {
                tracing::warn!(%err, "abandon-on-delete failed");
            }
        }
    }
    upsert_mrs_after_push(pool, storage_root, owner, name, pusher, pkt_bytes).await;
}

/// story: e03s01 — walk the receive-pack command list, read each new head's
/// identity from the (just-updated) bare repo, and upsert MRs. Failures log;
/// the push itself already succeeded and the pre-receive guard covered
/// invariant 2.
async fn upsert_mrs_after_push(
    pool: &sqlx::PgPool,
    storage_root: &str,
    owner: &str,
    name: &str,
    pusher: uuid::Uuid,
    pkt_bytes: &[u8],
) {
    let Some(repo_id): Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id          WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(owner)
    .bind(name)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    else {
        return;
    };
    let bare = domain::storage_path(storage_root, owner, name);

    let mut pushed = Vec::new();
    for update in crate::receive_guard::parse_push_commands(pkt_bytes) {
        if !update.ref_name.starts_with("refs/heads/") || update.new.chars().all(|c| c == '0') {
            continue;
        }
        let Ok(output) = std::process::Command::new("git")
            .arg("-C")
            .arg(&bare)
            .args(["cat-file", "commit", &update.new])
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            eprintln!(
                "HOOK: cat-file failed for {}: {}",
                update.new,
                String::from_utf8_lossy(&output.stderr)
            );
            continue;
        }
        let Some(identity) =
            crate::receive_guard::parse_commit_identity(&String::from_utf8_lossy(&output.stdout))
        else {
            continue;
        };
        pushed.push(crate::mr::PushedChange {
            change_id: identity.change_id,
            commit_id: update.new.clone(),
            author_email: identity.author_email,
        });
    }
    eprintln!(
        "HOOK: parsed {} ref updates from pkt-line body",
        crate::receive_guard::parse_push_commands(pkt_bytes).len()
    );
    if pushed.is_empty() {
        return;
    }
    match crate::mr::upsert_after_push(pool, repo_id, pusher, pushed).await {
        Ok(summary) => tracing::debug!(?summary, "mr upsert after push"),
        Err(err) => tracing::warn!(%err, "mr upsert failed"),
    }
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

/// story: e02s02 — push needs a token whose user owns the repo OR is a
/// collaborator (story e03s04); private clone likewise needs owner OR
/// collaborator, and public clone stays open.
/// story: e02s02 — 401s carry a Basic challenge so git clients retry with
/// credentials from the remote URL.
fn challenge() -> Vec<(header::HeaderName, &'static str)> {
    vec![(header::WWW_AUTHENTICATE, "Basic realm=\"jjhub\"")]
}

/// story: e03s04 — owner OR collaborator for push/private reads.
pub async fn push_or_read_allowed(
    pool: &sqlx::PgPool,
    repo_id: uuid::Uuid,
    user: uuid::Uuid,
    repo_owner: uuid::Uuid,
) -> bool {
    if user == repo_owner {
        return true;
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM collaborators WHERE repo_id = $1 AND user_id = $2",
    )
    .bind(repo_id)
    .bind(user)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    count > 0
}

/// story: e02s02 — authz failure the smart_http handler renders.
type AuthzError = (StatusCode, Vec<(header::HeaderName, &'static str)>);

/// story: e17s01 — (repo id, owner id, visibility) for an owner/name pair;
/// shared by the HTTP smart transport and the SSH listener so both resolve
/// repos identically.
pub(crate) async fn lookup_repo(
    pool: &sqlx::PgPool,
    owner: &str,
    name: &str,
) -> Option<(uuid::Uuid, uuid::Uuid, String)> {
    sqlx::query_as(
        "SELECT r.id, r.owner_id, r.visibility FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(owner)
    .bind(name)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

// WHY: pub(crate) — ssh.rs runs this exact chain for exec admission:
// one admission decision for both transports, no SSH-specific bypass.
pub(crate) async fn authorize(
    op: GitOp,
    user: Option<uuid::Uuid>,
    repo_owner: uuid::Uuid,
    visibility: &str,
    pool: &sqlx::PgPool,
    repo_id: uuid::Uuid,
    // story: e22s02 — scope + pin ride the shared gate for both
    // transports; absent token keeps the anonymous rules below.
    authz: Option<&crate::token::TransportAuth>,
) -> Result<(), AuthzError> {
    if crate::token::transport_gate(authz, op, Some(repo_id)).is_err() {
        return Err((StatusCode::FORBIDDEN, Vec::new()));
    }
    match (op, user) {
        (GitOp::ReceivePack, None) => Err((StatusCode::UNAUTHORIZED, challenge())),
        (GitOp::ReceivePack, Some(uid)) => {
            if push_or_read_allowed(pool, repo_id, uid, repo_owner).await {
                Ok(())
            } else {
                Err((StatusCode::FORBIDDEN, Vec::new()))
            }
        }
        (GitOp::UploadPack, _) if visibility == "public" => Ok(()),
        (GitOp::UploadPack, None) => Err((StatusCode::UNAUTHORIZED, challenge())),
        (GitOp::UploadPack, Some(uid)) => {
            if push_or_read_allowed(pool, repo_id, uid, repo_owner).await {
                Ok(())
            } else {
                Err((StatusCode::FORBIDDEN, Vec::new()))
            }
        }
    }
}

/// story: e03s04 — repository id for an owner/name pair.
async fn repo_owner_id(pool: &sqlx::PgPool, owner: &str, name: &str) -> uuid::Uuid {
    sqlx::query_scalar(
        "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id \
         WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(owner)
    .bind(name)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or(uuid::Uuid::nil())
}

/// story: e02s02 — parse Basic credentials from headers to a user id.
/// story: e22s02 — peer address from request extensions (ConnectInfo
/// wired by `into_make_service_with_connect_info`); None when absent.
fn client_ip(extensions: &axum::http::Extensions) -> Option<std::net::IpAddr> {
    extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|connect| connect.0.ip())
}

async fn token_user(
    headers: &axum::http::HeaderMap,
    state: &AppState,
    client_ip: Option<std::net::IpAddr>,
) -> Option<crate::token::TransportAuth> {
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
        .and_then(|bytes| String::from_utf8(bytes).ok())?;
    let (username, token_value) = credentials.split_once(':')?;
    if username != "x-access-token" {
        return None;
    }
    token::authenticate(&state.pool, token_value, client_ip).await
}
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
