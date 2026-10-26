// story: e01s01
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::password::{hash_password, verify_password};
use crate::session::{self, SESSION_COOKIE};
use crate::AppState;

pub async fn healthz() -> &'static str {
    "ok"
}

// story: e12s02 — embedded stylesheet: compiled into the binary, so the
// response never touches the filesystem and never hits the kata sidecar.
pub async fn app_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../assets/app.css"),
    )
}

#[derive(Deserialize)]
pub struct RegisterForm {
    email: String,
    password: String,
    password_confirm: String,
}

fn register_page(error: &str, email: &str) -> String {
    let mut context = page_context(None, "");
    context.insert("error", error);
    context.insert("email", email);
    crate::render::render("register.html", &context)
}

pub async fn register_form() -> Html<String> {
    Html(register_page("", ""))
}

pub async fn register(State(state): State<AppState>, Form(form): Form<RegisterForm>) -> Response {
    let (email, handle) = match domain::validate_registration(&form.email, &form.password) {
        Ok(pair) => pair,
        Err(err) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Html(register_page(&err.to_string(), &form.email)),
            )
                .into_response()
        }
    };

    // story: e01s03 — confirm the password at sign-up so a typo'd
    // password can't lock a new account out on first login
    if form.password != form.password_confirm {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Html(register_page("passwords do not match", &form.email)),
        )
            .into_response();
    }

    let password_hash = hash_password(&form.password);
    let existing: Option<String> =
        sqlx::query_scalar("SELECT id::text FROM users WHERE email = $1")
            .bind(&email)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    if existing.is_some() {
        return (
            StatusCode::CONFLICT,
            Html(register_page("email already registered", &form.email)),
        )
            .into_response();
    }

    let user_id: uuid::Uuid = match sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, handle) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(&email)
    .bind(&password_hash)
    .bind(&handle)
    .fetch_one(&state.pool)
    .await
    {
        Ok(id) => id,
        Err(sqlx::Error::Database(db))
            if db.constraint() == Some("users_email_key")
                || db.constraint() == Some("users_handle_unique") =>
        {
            return (
                StatusCode::CONFLICT,
                Html(register_page("email already registered", &form.email)),
            )
                .into_response()
        }
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("db error: {err}"),
            )
                .into_response()
        }
    };

    session_response(&state, user_id)
}

fn session_response(state: &AppState, user_id: uuid::Uuid) -> Response {
    let value = session::issue(&state.session_secret, user_id);
    let cookie = format!(
        "{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        30 * 24 * 60 * 60
    );
    (
        StatusCode::SEE_OTHER,
        [
            (header::SET_COOKIE, cookie),
            (header::LOCATION, "/settings".to_string()),
        ],
        Redirect::to("/settings"),
    )
        .into_response()
}

fn login_page(error: &str) -> String {
    let mut context = page_context(None, "");
    context.insert("error", error);
    crate::render::render("login.html", &context)
}

pub async fn login_form() -> Html<String> {
    Html(login_page(""))
}

#[derive(Deserialize)]
pub struct LoginForm {
    email: String,
    password: String,
}

pub async fn login(State(state): State<AppState>, Form(form): Form<LoginForm>) -> Response {
    let email = domain::normalize_email(&form.email);
    let row: Option<(uuid::Uuid, String)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE email = $1")
            .bind(&email)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);

    match row {
        Some((id, hash)) if verify_password(&form.password, &hash) => session_response(&state, id),
        // story: e01s03 — same message for unknown email and wrong
        // password: the page must not say which one was wrong
        _ => (
            StatusCode::UNAUTHORIZED,
            Html(login_page("email or password is incorrect")),
        )
            .into_response(),
    }
}

pub async fn logout() -> Response {
    (
        StatusCode::SEE_OTHER,
        [
            (
                header::SET_COOKIE,
                format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"),
            ),
            (header::LOCATION, "/login".to_string()),
        ],
    )
        .into_response()
}

// story: e02s03 — issue a fresh access token for the viewer and bounce
// back to the repo page; plaintext rides the one-time cookie only
pub async fn create_repo_token(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
    axum::extract::Extension(current): axum::extract::Extension<crate::CurrentUser>,
    jar: CookieJar,
    Form(form): Form<TokenForm>,
) -> Response {
    // story: e20s02 — token creation charges the token row (per-user
    // only; no IP row in the policy table).
    if let Some(limiter) = state.rate_limiter.as_ref() {
        if let Err(retry) =
            limiter.consume(crate::rate_limit::Action::Token, &current.0.to_string())
        {
            return crate::rate_limit::too_many_requests(retry);
        }
    }
    let new = crate::token::create(&state.pool, current.0, &form.label).await;
    let jar = jar.add(axum_extra::extract::cookie::Cookie::new(
        "new_token",
        new.plaintext,
    ));
    (jar, Redirect::to(&format!("/{owner}/{name}"))).into_response()
}

pub async fn settings(
    State(state): State<AppState>,
    axum::extract::Extension(current): axum::extract::Extension<crate::CurrentUser>,
    jar: CookieJar,
) -> Response {
    // forms echo the session-derived synchronizer token
    let csrf = jar
        .get(crate::session::SESSION_COOKIE)
        .map(|c| crate::session::csrf_token(&state.session_secret, c.value()))
        .unwrap_or_default();
    let handle: Option<String> = sqlx::query_scalar("SELECT handle FROM users WHERE id = $1")
        .bind(current.0)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();
    let email: Option<String> = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(current.0)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();
    let tokens: Vec<(String, String)> = sqlx::query_as(
        "SELECT label, prefix FROM tokens WHERE user_id = $1 AND revoked_at IS NULL ORDER BY created_at",
    )
    .bind(current.0)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    // story: e15s02 — the plaintext rides the template context once;
    // the template escapes it like any other value
    let new_token = jar
        .get("new_token")
        .map(|cookie| cookie.value().to_string())
        .unwrap_or_default();

    let token_items: Vec<serde_json::Value> = tokens
        .iter()
        .map(|(label, prefix)| {
            serde_json::json!({
                "label": label,
                "prefix": prefix
            })
        })
        .collect();

    match email {
        Some(email) => {
            let mut context = page_context(handle.as_deref(), &csrf);
            context.insert("email", &email);
            context.insert("new_token", &new_token);
            context.insert("tokens", &token_items);
            Html(crate::render::render("settings.html", &context)).into_response()
        }
        None => Redirect::to("/login").into_response(),
    }
}

#[derive(Deserialize)]
pub struct CreateRepoForm {
    name: String,
    visibility: String,
}

pub async fn create_repo(
    State(state): State<AppState>,
    axum::extract::Extension(current): axum::extract::Extension<crate::CurrentUser>,
    ip: crate::rate_limit::OptionalPeerIp,
    Form(form): Form<CreateRepoForm>,
) -> Response {
    // story: e20s02 — the form create charges the same create row as
    // push-to-create: one action, one budget.
    if let Some(limiter) = state.rate_limiter.as_ref() {
        if let Err(retry) = limiter.try_charge(
            crate::rate_limit::Action::Create,
            &current.0.to_string(),
            &ip.0,
        ) {
            return crate::rate_limit::too_many_requests(retry);
        }
    }
    // story: e19s01 — the sequence lives in the creation service; this
    // handler keeps only form/session/response-shape concerns.
    match crate::creation::create_repository(
        &state.pool,
        &state.storage_root,
        current.0,
        &form.name,
        &form.visibility,
    )
    .await
    {
        Ok(created) => {
            Redirect::to(&format!("/{}/{}", created.owner, created.name)).into_response()
        }
        Err(crate::creation::CreateError::InvalidName) => {
            (StatusCode::UNPROCESSABLE_ENTITY, "invalid repository name").into_response()
        }
        Err(crate::creation::CreateError::InvalidVisibility) => {
            (StatusCode::UNPROCESSABLE_ENTITY, "invalid visibility").into_response()
        }
        Err(crate::creation::CreateError::Duplicate) => {
            (StatusCode::CONFLICT, "repository already exists").into_response()
        }
        Err(crate::creation::CreateError::Storage(err)) => {
            (StatusCode::INTERNAL_SERVER_ERROR, err).into_response()
        }
        Err(crate::creation::CreateError::Db(err)) => {
            (StatusCode::INTERNAL_SERVER_ERROR, err).into_response()
        }
    }
}

pub async fn repo_page(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
) -> Response {
    let viewer = viewer_handle(&state, &jar).await;
    let csrf = jar
        .get(crate::session::SESSION_COOKIE)
        .map(|c| crate::session::csrf_token(&state.session_secret, c.value()))
        .unwrap_or_default();
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:3000")
        .to_string();
    let repo: Option<(uuid::Uuid, String, String)> = sqlx::query_as(
        "SELECT r.id, r.visibility, r.trunk FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(&owner)
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();

    let Some((repo_id, visibility, trunk)) = repo else {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    };

    // story: e10s02 — the settings listing shares the push gate's resolver;
    // display and enforcement can never diverge
    let protected = crate::receive_guard::protected_refs(&state.pool, repo_id)
        .await
        .unwrap_or_default();

    // story: repo dashboard — the page surfaces open changes, not just
    // clone instructions
    let mrs = crate::stack::open_mrs(&state.pool, repo_id)
        .await
        .unwrap_or_default();
    let mr_ids: Vec<String> = mrs.iter().map(|mr| mr.change_id.clone()).collect();

    // story: Browse files — kata's browse view through the auth proxy;
    // same metadata guard as review links (no dead links, ever)
    let browse = crate::review_proxy::review_link(
        &state.kata_public_base,
        &state.storage_root,
        &owner,
        &name,
        "",
    )
    .map(|link| {
        // story: browse pin — kata's browse is commit-centric; pin it to
        // the trunk head so it opens on the current revision's detail,
        // not an arbitrary log position
        let bare = domain::storage_path(&state.storage_root, &owner, &name);
        crate::stack::ref_commit(&bare, &format!("refs/heads/{trunk}"))
            .map(|sha| format!("{link}?commit={sha}"))
            .unwrap_or(link)
    })
    .unwrap_or_default();
    // story: e07s01 — native tree view; only when the repo has commits
    let bare_for_files = domain::storage_path(&state.storage_root, &owner, &name);
    let trunk_head = crate::stack::ref_commit(&bare_for_files, &format!("refs/heads/{trunk}"));
    let has_files = trunk_head.is_some();

    // story: e16s03 — the Open MRs list marks divergent changes with
    // the shared divergence mark (e16s02's class); detection is bounded
    // to the changes already listed here and checks every pushed
    // revision of each against the bare graph via e16s01's helper
    // (fail-safe empty — never a 500, linear lists stay unchanged)
    let mr_revisions: Vec<(String, String)> = sqlx::query_as(
        "SELECT mr.change_id, rev.commit_id FROM merge_requests mr JOIN revisions rev ON rev.mr_id = mr.id WHERE mr.repo_id = $1 AND mr.state = 'open'",
    )
    .bind(repo_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    let mut commits_by_change: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for (change_id, commit_id) in mr_revisions {
        commits_by_change
            .entry(change_id)
            .or_default()
            .push(commit_id);
    }
    let divergent_mrs: Vec<String> = mr_ids
        .iter()
        .filter(|change| {
            let none = Vec::new();
            let commits = commits_by_change.get(*change).unwrap_or(&none);
            !crate::divergence::divergent_heads(&bare_for_files, commits).is_empty()
        })
        .cloned()
        .collect();
    // story: e08s03 — the landing flag shares the commit-header rule with
    // every other flag; presentation only, a conflicted trunk still 200s
    let trunk_conflicted = trunk_head
        .as_deref()
        .map(|sha| crate::browse::commit_is_conflicted(&bare_for_files, sha))
        .unwrap_or(false);

    // story: e13s01 — the landing page's root file tree at the trunk
    // revision; one list_tree subprocess per render, names escaped (XSS)
    let tree: Vec<serde_json::Value> = crate::browse::list_tree(&bare_for_files, &trunk, "")
        .unwrap_or_default()
        .into_iter()
        .map(|(entry_name, is_dir)| {
            serde_json::json!({
                "is_dir": is_dir,
                "label": crate::stack::html_escape(&entry_name),
            })
        })
        .collect();

    // story: e02s03 — the viewer's own token lands in the snippet via the
    // one-time cookie; everyone else keeps the placeholder
    let clone_token = jar
        .get("new_token")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| "<TOKEN>".to_string());
    let generate = viewer.is_some();

    // story: e17s02 — ssh:// rides the clone block only when the SSH
    // listener is enabled; host keeps the hostname, port is the SSH port.
    // Template carries the literal `ssh://`/`/` slashes (Tera escapes
    // slash inside interpolated values).
    let ssh_host = host.split(':').next().unwrap_or(host.as_str()).to_string();

    let mut context = page_context(viewer.as_deref(), &csrf);
    context.insert("owner", &owner);
    context.insert("name", &name);
    context.insert("visibility", &visibility);
    context.insert("trunk", &trunk);
    context.insert("protected_refs", &protected);
    context.insert("browse_href", &browse);
    context.insert("files", &has_files);
    context.insert("trunk_conflicted", &trunk_conflicted);
    context.insert("tree", &tree);
    context.insert("mrs", &mr_ids);
    context.insert("mr_divergent", &divergent_mrs); // story: e16s03
    context.insert("host", &host);
    context.insert("clone_token", &clone_token);
    context.insert("generate", &generate);
    context.insert("ssh_enabled", &state.ssh.enabled);
    context.insert("ssh_host", &ssh_host);
    context.insert("ssh_port", &state.ssh.bind.port());
    Html(crate::render::render("repo.html", &context)).into_response()
}

#[derive(Deserialize)]
pub struct VisibilityForm {
    visibility: String,
}

pub async fn repo_settings(
    State(state): State<AppState>,
    axum::extract::Extension(current): axum::extract::Extension<crate::CurrentUser>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
    Form(form): Form<VisibilityForm>,
) -> Response {
    let visibility = match domain::Visibility::parse(&form.visibility) {
        Some(v) => v,
        None => return (StatusCode::UNPROCESSABLE_ENTITY, "invalid visibility").into_response(),
    };
    let updated: Option<i64> = match sqlx::query_scalar::<_, i64>(
        "UPDATE repositories r SET visibility = $3 FROM users u WHERE u.id = r.owner_id AND u.handle = $1 AND r.name = $2 AND r.owner_id = $4 RETURNING 1::bigint",
    )
    .bind(&owner)
    .bind(&name)
    .bind(visibility.as_str())
    .bind(current.0)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(row) => row,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {err}")).into_response(),
    };

    if updated.is_none() {
        (StatusCode::NOT_FOUND, "repository not found").into_response()
    } else {
        Redirect::to(&format!("/{owner}/{name}")).into_response()
    }
}

pub async fn git_whoami(
    axum::extract::Extension(current): axum::extract::Extension<crate::transport::TransportUser>,
) -> impl IntoResponse {
    axum::Json(serde_json::json!({ "user_id": current.0.to_string() }))
}

#[derive(Deserialize)]
pub struct TokenForm {
    label: String,
}

pub async fn create_token(
    State(state): State<AppState>,
    axum::extract::Extension(current): axum::extract::Extension<crate::CurrentUser>,
    jar: CookieJar,
    Form(form): Form<TokenForm>,
) -> Response {
    // story: e20s02 — token creation charges the token row (per-user
    // only; no IP row in the policy table).
    if let Some(limiter) = state.rate_limiter.as_ref() {
        if let Err(retry) =
            limiter.consume(crate::rate_limit::Action::Token, &current.0.to_string())
        {
            return crate::rate_limit::too_many_requests(retry);
        }
    }
    let new = crate::token::create(&state.pool, current.0, &form.label).await;
    let jar = jar.add(axum_extra::extract::cookie::Cookie::new(
        "new_token",
        new.plaintext,
    ));
    (jar, Redirect::to("/settings")).into_response()
}

// story: e16s02 — the one seam MR surfaces use for divergence
// detection. e16s01 owns the predicate: crate::divergence takes the
// bare repo path plus the pushed commit ids of ONE change and returns
// the diverging heads (empty = not divergent, fail-safe).
fn divergent_heads(bare: &std::path::Path, commits: &[String]) -> Vec<String> {
    crate::divergence::divergent_heads(bare, commits)
}

// story: e03s02 — MR list: open merge requests grouped into stacks, each
// stack bottom-first (invariant 3: derived from commit parents at render
// time, never stored).
pub async fn merge_requests_page(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
) -> Response {
    let repo: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id \
         WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(&owner)
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some((repo_id,)) = repo else {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    };

    let mrs = match crate::stack::open_mrs(&state.pool, repo_id).await {
        Ok(mrs) => mrs,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("db error: {err}"),
            )
                .into_response();
        }
    };
    let bare = domain::storage_path(&state.storage_root, &owner, &name);
    let stacks = crate::stack::derive_page_model(&bare, &mrs);

    // change_id -> stack position labels for rendering
    let mut items: Vec<String> = Vec::new();
    for (stack_idx, stack) in stacks.iter().enumerate() {
        for (mr_idx, change_id) in stack.iter().enumerate() {
            let mut item = tera::Context::new();
            item.insert("owner", &owner);
            item.insert("name", &name);
            item.insert("change_id", change_id);
            // story: e16s02 — divergence mark: one detection call per
            // listed MR over its own pushed revisions (bounded by the
            // page's MR count; empty heads render nothing)
            let revision_commits: Vec<String> = sqlx::query_scalar(
                "SELECT rev.commit_id FROM revisions rev \
                 JOIN merge_requests mr ON mr.id = rev.mr_id \
                 WHERE mr.repo_id = $1 AND mr.change_id = $2",
            )
            .bind(repo_id)
            .bind(change_id)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();
            item.insert(
                "divergent",
                &!divergent_heads(&bare, &revision_commits).is_empty(),
            );
            item.insert("stack_idx", &stack_idx);
            item.insert("mr_idx", &mr_idx);
            item.insert(
                "review_href",
                &crate::review_proxy::review_link(
                    &state.kata_public_base,
                    &state.storage_root,
                    &owner,
                    &name,
                    change_id,
                ),
            );
            items.push(crate::render::render("mr_list_item.html", &item));
        }
    }
    let mut context = tera::Context::new();
    context.insert("owner", &owner);
    context.insert("name", &name);
    context.insert("position_html", &items.join("\n"));
    Html(crate::render::render("mr_pages.html", &context)).into_response()
}

// story: e03s02 — MR detail: revisions newest-first, each with its
// unified diff. Repo-controlled content is escaped (no HTML from files).
pub async fn mr_detail_page(
    State(state): State<AppState>,
    axum::extract::Path((owner, name, change_id)): axum::extract::Path<(String, String, String)>,
    jar: CookieJar,
) -> Response {
    // the public detail page renders action forms; they
    // echo the session token when the viewer has one (anonymous submits are
    // refused by the auth middleware anyway)
    let csrf = jar
        .get(crate::session::SESSION_COOKIE)
        .map(|c| crate::session::csrf_token(&state.session_secret, c.value()))
        .unwrap_or_default();
    if !change_id.chars().all(|c| c.is_ascii_alphabetic()) {
        return (StatusCode::NOT_FOUND, "merge request not found").into_response();
    }
    let repo: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id \
         WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(&owner)
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some((repo_id,)) = repo else {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    };
    let mr: Option<(uuid::Uuid, String)> = sqlx::query_as(
        "SELECT id, state FROM merge_requests WHERE repo_id = $1 AND change_id = $2",
    )
    .bind(repo_id)
    .bind(&change_id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some((mr_id, mr_state)) = mr else {
        return (StatusCode::NOT_FOUND, "merge request not found").into_response();
    };
    let revisions: Vec<(uuid::Uuid, String)> = sqlx::query_as(
        "SELECT id, commit_id FROM revisions WHERE mr_id = $1 ORDER BY pushed_at DESC",
    )
    .bind(mr_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    // story: e04s02 — badges for the LATEST revision only
    let statuses: Vec<(String, String, Option<String>)> = match revisions.first() {
        Some((latest_id, _)) => sqlx::query_as(
            "SELECT context, state, target_url FROM statuses \
             WHERE revision_id = $1 ORDER BY context",
        )
        .bind(latest_id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default(),
        None => Vec::new(),
    };
    let badges: Vec<String> = statuses
        .iter()
        .map(|(context, state, url)| {
            let label = crate::stack::html_escape(&format!("{context}: {state}"));
            // why: only http(s) may become a link; javascript: URLs
            // render as plain text instead of being dropped silently
            let href = match url {
                Some(u) if u.starts_with("http://") || u.starts_with("https://") => {
                    Some(crate::stack::html_escape(u))
                }
                _ => None,
            };
            let mut badge = tera::Context::new();
            badge.insert("context", &crate::stack::html_escape(context));
            badge.insert("state", &crate::stack::html_escape(state));
            badge.insert("label", &label);
            badge.insert("url", &url.as_deref().map(crate::stack::html_escape));
            badge.insert("href", &href);
            crate::render::render("status_badge.html", &badge)
        })
        .collect();

    let bare = domain::storage_path(&state.storage_root, &owner, &name);
    // story: e16s02 — divergence notice: e16s01's helper over the MR's
    // pushed revisions; empty for linear sets, so nothing renders
    let divergent_heads = divergent_heads(
        &bare,
        &revisions
            .iter()
            .map(|(_, commit)| commit.clone())
            .collect::<Vec<_>>(),
    );
    let mut sections: Vec<String> = Vec::new();
    for (revision_id, commit) in &revisions {
        let diff = crate::stack::unified_diff(&bare, commit);
        // story: e04s01 — threads of THIS revision only; revision_id is a
        // hard FK, so a thread can never drift to another revision
        let threads: Vec<(String, String)> = sqlx::query_as(
            "SELECT u.handle, c.body FROM comments c JOIN users u ON u.id = c.author_id \
             WHERE c.revision_id = $1 ORDER BY c.created_at",
        )
        .bind(revision_id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
        let threads: Vec<serde_json::Value> = threads
            .iter()
            .map(|(handle, body)| {
                serde_json::json!({
                    "handle_html": crate::stack::html_escape(handle),
                    "body_html": crate::stack::html_escape(body),
                })
            })
            .collect();
        let mut revision = tera::Context::new();
        // story: e08s03 — presentation-only flag from the shared header rule
        revision.insert(
            "conflicted",
            &crate::browse::commit_is_conflicted(&bare, commit),
        );
        revision.insert("owner", &owner);
        revision.insert("name", &name);
        revision.insert("change_id", &change_id);
        revision.insert("csrf", &csrf);
        revision.insert("commit", commit);
        revision.insert("diff_html", &crate::stack::html_escape(&diff));
        revision.insert("threads", &threads);
        sections.push(crate::render::render("mr_revision.html", &revision));
    }

    let mut context = tera::Context::new();
    context.insert("owner", &owner);
    context.insert("name", &name);
    context.insert("change_id", &change_id);
    context.insert("mr_state", &mr_state);
    context.insert("csrf", &csrf);
    context.insert("badges", &badges);
    context.insert("sections", &sections);
    // story: e16s02 — both heads of a diverged change (empty = no notice)
    context.insert("divergent_heads", &divergent_heads);
    context.insert(
        "review_href",
        &crate::review_proxy::review_link(
            &state.kata_public_base,
            &state.storage_root,
            &owner,
            &name,
            &change_id,
        ),
    );
    Html(crate::render::render("mr_detail.html", &context)).into_response()
}

// story: e04s01 — post one comment to one immutable revision. Public repos
// take any signed-in commenter; private repos require owner or
// collaborator. The comment FKs an existing revision of this MR — threads
// are pinned, never re-anchored.
#[derive(Deserialize)]
pub struct CommentForm {
    revision: String,
    body: String,
}

pub async fn add_comment(
    State(state): State<AppState>,
    axum::extract::Extension(current): axum::extract::Extension<crate::CurrentUser>,
    axum::extract::Path((owner, name, change_id)): axum::extract::Path<(String, String, String)>,
    Form(form): Form<CommentForm>,
) -> Response {
    let repo: Option<(uuid::Uuid, uuid::Uuid, String)> = sqlx::query_as(
        "SELECT r.id, r.owner_id, r.visibility FROM repositories r \
         JOIN users u ON u.id = r.owner_id WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(&owner)
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some((repo_id, repo_owner_id, visibility)) = repo else {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    };
    let mr: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT id FROM merge_requests WHERE repo_id = $1 AND change_id = $2")
            .bind(repo_id)
            .bind(&change_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    let Some((mr_id,)) = mr else {
        return (StatusCode::NOT_FOUND, "merge request not found").into_response();
    };

    let body = form.body.trim();
    if body.is_empty() {
        return (StatusCode::UNPROCESSABLE_ENTITY, "comment body required").into_response();
    }

    if visibility == "private" && current.0 != repo_owner_id {
        let collaborator: Option<uuid::Uuid> = sqlx::query_scalar(
            "SELECT repo_id FROM collaborators WHERE repo_id = $1 AND user_id = $2",
        )
        .bind(repo_id)
        .bind(current.0)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();
        if collaborator.is_none() {
            return (
                StatusCode::FORBIDDEN,
                "only collaborators can comment on a private repository",
            )
                .into_response();
        }
    }

    let revision: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT id FROM revisions WHERE mr_id = $1 AND commit_id = $2")
            .bind(mr_id)
            .bind(&form.revision)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    let Some((revision_id,)) = revision else {
        return (StatusCode::NOT_FOUND, "revision not found").into_response();
    };

    if let Err(err) = sqlx::query(
        "INSERT INTO comments (mr_id, revision_id, author_id, body) VALUES ($1, $2, $3, $4)",
    )
    .bind(mr_id)
    .bind(revision_id)
    .bind(current.0)
    .bind(body)
    .execute(&state.pool)
    .await
    {
        tracing::error!(%err, "comment insert failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "comment failed").into_response();
    }

    Redirect::to(&format!("/{owner}/{name}/merge-requests/{change_id}")).into_response()
}

// story: e04s02 — external CI posts one status per (revision, context).
// Token auth (Basic x-access-token) with push rights: owner or
// collaborator. Re-reporting a context replaces its previous state.
#[derive(Deserialize)]
pub struct StatusForm {
    revision: String,
    context: String,
    state: String,
    target_url: Option<String>,
}

pub async fn create_status(
    State(state): State<AppState>,
    axum::extract::Extension(ci): axum::extract::Extension<crate::transport::TransportUser>,
    axum::extract::Path((owner, name, change_id)): axum::extract::Path<(String, String, String)>,
    Form(form): Form<StatusForm>,
) -> Response {
    let repo: Option<(uuid::Uuid, uuid::Uuid)> = sqlx::query_as(
        "SELECT r.id, r.owner_id FROM repositories r \
         JOIN users u ON u.id = r.owner_id WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(&owner)
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some((repo_id, repo_owner_id)) = repo else {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    };
    let mr: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT id FROM merge_requests WHERE repo_id = $1 AND change_id = $2")
            .bind(repo_id)
            .bind(&change_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    let Some((mr_id,)) = mr else {
        return (StatusCode::NOT_FOUND, "merge request not found").into_response();
    };

    if !matches!(
        form.state.as_str(),
        "pending" | "success" | "failure" | "error"
    ) {
        return (StatusCode::UNPROCESSABLE_ENTITY, "invalid status state").into_response();
    }
    let context = form.context.trim();
    if context.is_empty() {
        return (StatusCode::UNPROCESSABLE_ENTITY, "context required").into_response();
    }

    // same write gate as push: a CI token must belong to the owner or a
    // collaborator
    if !crate::transport::push_or_read_allowed(&state.pool, repo_id, ci.0, repo_owner_id).await {
        return (
            StatusCode::FORBIDDEN,
            "token lacks write access to this repository",
        )
            .into_response();
    }

    let revision: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT id FROM revisions WHERE mr_id = $1 AND commit_id = $2")
            .bind(mr_id)
            .bind(&form.revision)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    let Some((revision_id,)) = revision else {
        return (StatusCode::NOT_FOUND, "revision not found").into_response();
    };

    if let Err(err) = sqlx::query(
        "INSERT INTO statuses (mr_id, revision_id, context, state, target_url) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (revision_id, context) \
         DO UPDATE SET state = EXCLUDED.state, \
                       target_url = EXCLUDED.target_url, \
                       created_at = now()",
    )
    .bind(mr_id)
    .bind(revision_id)
    .bind(context)
    .bind(&form.state)
    .bind(&form.target_url)
    .execute(&state.pool)
    .await
    {
        tracing::error!(%err, "status upsert failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "status not recorded").into_response();
    }

    (StatusCode::CREATED, "status recorded").into_response()
}

// story: e03s03 — merge button target: owner-only, bottom MR, one click.
pub async fn merge_mr(
    State(state): State<AppState>,
    axum::extract::Extension(current): axum::extract::Extension<crate::CurrentUser>,
    axum::extract::Path((owner, name, change_id)): axum::extract::Path<(String, String, String)>,
) -> Response {
    let repo: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id \
         WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(&owner)
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some((repo_id,)) = repo else {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    };

    use crate::merge::MergeError;
    match crate::merge::merge_bottom(
        &state.pool,
        &state.storage_root,
        repo_id,
        &change_id,
        current.0,
    )
    .await
    {
        Ok(_) => {
            Redirect::to(&format!("/{owner}/{name}/merge-requests/{change_id}")).into_response()
        }
        Err(MergeError::NotFound) => {
            (StatusCode::NOT_FOUND, "merge request not found").into_response()
        }
        Err(MergeError::NotOwner) => {
            (StatusCode::FORBIDDEN, "only the owner can merge").into_response()
        }
        // invariant 7: descendants wait for the bottom
        Err(MergeError::NotBottom) => (
            StatusCode::CONFLICT,
            "only the bottom of a stack can merge first",
        )
            .into_response(),
        Err(MergeError::Internal(err)) => {
            tracing::error!(%err, "merge failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "merge failed").into_response()
        }
    }
}

// story: e03s04 — abandon: owner-only; merged is terminal (state machine).
pub async fn abandon_mr(
    State(state): State<AppState>,
    axum::extract::Extension(current): axum::extract::Extension<crate::CurrentUser>,
    axum::extract::Path((owner, name, change_id)): axum::extract::Path<(String, String, String)>,
) -> Response {
    let repo: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id \
         WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(&owner)
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some((repo_id,)) = repo else {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    };
    let mr: Option<(uuid::Uuid, String, String)> = sqlx::query_as(
        "SELECT id, owner_id::text, state FROM merge_requests \
         WHERE repo_id = $1 AND change_id = $2",
    )
    .bind(repo_id)
    .bind(&change_id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some((mr_id, owner_id, mr_state)) = mr else {
        return (StatusCode::NOT_FOUND, "merge request not found").into_response();
    };
    if owner_id != current.0.to_string() {
        return (StatusCode::FORBIDDEN, "only the owner can abandon").into_response();
    }
    match mr_state.as_str() {
        "merged" => (StatusCode::CONFLICT, "merged is terminal").into_response(),
        "abandoned" => {
            Redirect::to(&format!("/{owner}/{name}/merge-requests/{change_id}")).into_response()
        }
        _ => {
            sqlx::query("UPDATE merge_requests SET state = 'abandoned' WHERE id = $1")
                .bind(mr_id)
                .execute(&state.pool)
                .await
                .ok();
            Redirect::to(&format!("/{owner}/{name}/merge-requests/{change_id}")).into_response()
        }
    }
}

// story: e03s04 — collaborator management: owner-only; removal revokes
// rights immediately (authorize() checks per request).
#[derive(Deserialize)]
pub struct CollaboratorForm {
    email: String,
    #[serde(default)]
    remove: Option<String>,
}

pub async fn manage_collaborators(
    State(state): State<AppState>,
    axum::extract::Extension(current): axum::extract::Extension<crate::CurrentUser>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
    Form(form): Form<CollaboratorForm>,
) -> Response {
    let repo: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT r.id FROM repositories r JOIN users u ON u.id = r.owner_id \
         WHERE u.handle = $1 AND r.name = $2 AND r.owner_id = $3",
    )
    .bind(&owner)
    .bind(&name)
    .bind(current.0)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some((repo_id,)) = repo else {
        // owner-only: non-owners get the no-leak 404
        return (
            StatusCode::FORBIDDEN,
            "only the owner manages collaborators",
        )
            .into_response();
    };

    let collaborator: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
            .bind(&form.email)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    let Some(collaborator_id) = collaborator else {
        return (StatusCode::NOT_FOUND, "no such user").into_response();
    };

    if form.remove.is_some() {
        sqlx::query("DELETE FROM collaborators WHERE repo_id = $1 AND user_id = $2")
            .bind(repo_id)
            .bind(collaborator_id)
            .execute(&state.pool)
            .await
            .ok();
    } else {
        sqlx::query(
            "INSERT INTO collaborators (repo_id, user_id) VALUES ($1, $2) \
             ON CONFLICT (repo_id, user_id) DO NOTHING",
        )
        .bind(repo_id)
        .bind(collaborator_id)
        .execute(&state.pool)
        .await
        .ok();
    }
    Redirect::to(&format!("/{owner}/{name}")).into_response()
}

// story: e06s01 — shared nav: brand + (when authed) Repos, Settings and
// a CSRF-protected Log out button. Body lives in templates/nav.html.
pub fn nav_html(user_handle: Option<&str>, csrf: &str) -> String {
    crate::render::render("nav.html", &page_context(user_handle, csrf))
}

// story: e15s02 — the context every page template builds on: viewer
// handle (empty for visitors) and the session-derived CSRF token
// story: e11s01/e15s01 — shared page chrome (nav identity + CSRF token)
pub(crate) fn page_context(user_handle: Option<&str>, csrf: &str) -> tera::Context {
    let mut context = tera::Context::new();
    context.insert("user_handle", &user_handle.unwrap_or_default());
    context.insert("csrf", csrf);
    context
}

pub(crate) async fn viewer_handle(state: &AppState, jar: &CookieJar) -> Option<String> {
    let cookie = jar.get(crate::session::SESSION_COOKIE)?;
    let user_id = crate::session::verify(&state.session_secret, cookie.value())?;
    sqlx::query_scalar("SELECT handle FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten()
}

// story: e06s01 — the front door: landing for visitors, dashboard for
// users (create form + owned repos).
pub async fn home(State(state): State<AppState>, jar: CookieJar) -> Response {
    let Some(handle) = viewer_handle(&state, &jar).await else {
        let mut context = page_context(None, "");
        context.insert("repos", &Vec::<serde_json::Value>::new());
        return Html(crate::render::render("home.html", &context)).into_response();
    };

    let csrf = jar
        .get(crate::session::SESSION_COOKIE)
        .map(|c| crate::session::csrf_token(&state.session_secret, c.value()))
        .unwrap_or_default();
    let repos: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, visibility FROM repositories r \
         JOIN users u ON u.id = r.owner_id WHERE u.handle = $1 ORDER BY name",
    )
    .bind(&handle)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    let repo_items: Vec<serde_json::Value> = repos
        .iter()
        .map(|(name, visibility)| {
            serde_json::json!({
                "owner": handle,
                "name": name,
                "visibility": visibility
            })
        })
        .collect();

    let mut context = page_context(Some(&handle), &csrf);
    context.insert("repos", &repo_items);
    Html(crate::render::render("home.html", &context)).into_response()
}

// story: e07s01 — default tree route: trunk view rendered directly.
pub async fn tree_root(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
) -> Response {
    render_tree(state, owner, name, "trunk".to_string(), String::new()).await
}

// story: e07s01 — directory listing at ref+path.
pub async fn tree_page(
    State(state): State<AppState>,
    axum::extract::Path((owner, name, git_ref)): axum::extract::Path<(String, String, String)>,
) -> Response {
    render_tree(state, owner, name, git_ref, String::new()).await
}

// story: e07s01 — directory listing at ref+path.
pub async fn tree_page_at(
    State(state): State<AppState>,
    axum::extract::Path((owner, name, git_ref, path)): axum::extract::Path<(
        String,
        String,
        String,
        String,
    )>,
) -> Response {
    render_tree(state, owner, name, git_ref, path).await
}

async fn render_tree(
    state: AppState,
    owner: String,
    name: String,
    git_ref: String,
    path: String,
) -> Response {
    let bare = domain::storage_path(&state.storage_root, &owner, &name);
    if !bare.join("HEAD").exists() {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    }
    let Some(entries) = crate::browse::list_tree(&bare, &git_ref, &path) else {
        return (StatusCode::NOT_FOUND, "unknown revision").into_response();
    };

    // breadcrumbs: repo root, then each ancestor of the current path
    let mut crumbs = vec![serde_json::json!({
        "href": format!("/{owner}/{name}/tree"),
        "label": format!("{owner}/{name}")
    })];
    let mut acc = String::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        acc = if acc.is_empty() {
            part.to_string()
        } else {
            format!("{acc}/{part}")
        };
        crumbs.push(serde_json::json!({
            "href": format!("/{owner}/{name}/tree/{git_ref}/{acc}"),
            "label": part
        }));
    }

    let items: Vec<serde_json::Value> = {
        // story: e08s02 — group jj's .jjconflict* dirs into one conflict
        // entry; ordinary entries render unchanged
        let mut grouped: Vec<serde_json::Value> = Vec::new();
        let mut conflict_emitted = false;
        for (entry_name, is_dir) in entries.iter() {
            let kind = if *is_dir { "tree" } else { "blob" };
            let base = if path.is_empty() {
                String::new()
            } else {
                format!("/{path}")
            };
            let item = |extra: serde_json::Value| {
                let mut value = serde_json::json!({
                    "is_dir": is_dir,
                    "href": format!("/{owner}/{name}/{kind}/{git_ref}{base}/{entry_name}"),
                    "label": entry_name
                });
                if let Some(map) = value.as_object_mut() {
                    for (key, value) in extra.as_object().unwrap().iter() {
                        map.insert(key.clone(), value.clone());
                    }
                }
                value
            };
            if *is_dir && entry_name.starts_with(".jjconflict-") {
                if !conflict_emitted {
                    conflict_emitted = true;
                    let mut bases = 0usize;
                    let mut sides = 0usize;
                    for (name, _) in entries.iter() {
                        if name.starts_with(".jjconflict-base-") {
                            bases += 1;
                        } else if name.starts_with(".jjconflict-side-") {
                            sides += 1;
                        }
                    }
                    grouped.push(item(serde_json::json!({
                        "is_conflict": true,
                        "summary": format!(
                            "conflict ({bases} base{} + {sides} side{})",
                            if bases == 1 { "" } else { "s" },
                            if sides == 1 { "" } else { "s" }
                        )
                    })));
                }
            } else {
                grouped.push(item(serde_json::json!({})));
            }
        }
        grouped
    };

    let path_label = if path.is_empty() {
        "/".to_string()
    } else {
        path.clone()
    };
    let mut context = page_context(None, "");
    context.insert("owner", &owner);
    context.insert("name", &name);
    context.insert("path_label", &path_label);
    context.insert("crumbs", &crumbs);
    context.insert("items", &items);
    Html(crate::render::render("tree.html", &context)).into_response()
}

// story: e07s01 — file contents at ref+path, line-numbered and escaped.
pub async fn blob_page(
    State(state): State<AppState>,
    axum::extract::Path((owner, name, git_ref, path)): axum::extract::Path<(
        String,
        String,
        String,
        String,
    )>,
) -> Response {
    let bare = domain::storage_path(&state.storage_root, &owner, &name);
    if !bare.join("HEAD").exists() {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    }
    let back = if path.contains('/') {
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        format!(
            "/{owner}/{name}/tree/{git_ref}/{dir}",
            owner = owner,
            name = name,
            git_ref = git_ref,
            dir = dir
        )
    } else {
        format!(
            "/{owner}/{name}/tree/{git_ref}",
            owner = owner,
            name = name,
            git_ref = git_ref
        )
    };
    match crate::browse::read_blob(&bare, &git_ref, &path) {
        Ok(crate::browse::Blob::Text(text)) => {
            let mut context = page_context(None, "");
            context.insert("owner", &owner);
            context.insert("name", &name);
            context.insert("path", &path);
            context.insert("back", &back);
            // story: e08s02 — a conflicted path renders all terms with
            // markers instead of one side's content
            if let Some(conflict) = crate::browse::read_conflict(&bare, &git_ref, &path) {
                context.insert("conflict", &true);
                context.insert("parts", &conflict_parts(&conflict));
                context.insert("binary", &false);
                context.insert("len", &0);
                context.insert("lines", &Vec::<serde_json::Value>::new());
            } else {
                let lines: Vec<serde_json::Value> = text
                    .lines()
                    .enumerate()
                    .map(|(i, line)| {
                        serde_json::json!({
                            "number": format!("{:>4}", i + 1),
                            "text": line
                        })
                    })
                    .collect();
                context.insert("conflict", &false);
                context.insert("binary", &false);
                context.insert("len", &0);
                context.insert("lines", &lines);
            }
            Html(crate::render::render("blob.html", &context)).into_response()
        }
        Ok(crate::browse::Blob::Binary { len }) => {
            let mut context = page_context(None, "");
            context.insert("owner", &owner);
            context.insert("name", &name);
            context.insert("path", &path);
            context.insert("back", &back);
            context.insert("conflict", &false);
            context.insert("binary", &true);
            context.insert("len", &len);
            context.insert("lines", &Vec::<serde_json::Value>::new());
            Html(crate::render::render("blob.html", &context)).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "no such file").into_response(),
    }
}

// story: e08s02 — flatten a conflict into template parts: side-0 first,
// then every base, then the remaining sides, then the closing marker.
// Glyphs are constants here so the template can emit them raw; labels and
// content go through Tera's autoescape.
fn conflict_parts(conflict: &crate::browse::Conflict) -> Vec<serde_json::Value> {
    use crate::browse::Side;
    fn side_lines(side: &Side) -> Vec<serde_json::Value> {
        match side {
            Side::Text { text, .. } => text
                .lines()
                .enumerate()
                .map(|(i, line)| {
                    serde_json::json!({
                        "number": format!("{:>4}", i + 1),
                        "text": line
                    })
                })
                .collect(),
            Side::Binary { len } => vec![serde_json::json!({
                "number": "",
                "text": format!("{len} bytes (binary file — not shown)")
            })],
            Side::Missing => vec![serde_json::json!({
                "number": "",
                "text": "(not present in this side)"
            })],
        }
    }
    fn part(glyph: &str, label: &str, lines: &[&crate::browse::Side]) -> serde_json::Value {
        let merged: Vec<serde_json::Value> =
            lines.iter().flat_map(|side| side_lines(side)).collect();
        serde_json::json!({ "glyph": glyph, "label": label, "lines": merged })
    }

    let mut parts = vec![part(
        "<<<<<<< ",
        conflict
            .side_labels
            .first()
            .map(String::as_str)
            .unwrap_or("side-0"),
        &[&conflict.sides[0]],
    )];
    for (index, base) in conflict.bases.iter().enumerate() {
        let label = conflict
            .base_labels
            .get(index)
            .map(String::as_str)
            .unwrap_or("base");
        parts.push(part("||||||| ", label, &[base]));
    }
    for (index, side) in conflict.sides.iter().enumerate().skip(1) {
        let label = conflict
            .side_labels
            .get(index)
            .map(String::as_str)
            .unwrap_or("side");
        parts.push(part("======= ", label, &[side]));
    }
    if let Some(last) = conflict.sides.last() {
        let label = conflict
            .side_labels
            .last()
            .map(String::as_str)
            .unwrap_or("side");
        parts.push(part(">>>>>>> ", label, &[last]));
    }
    parts
}

/// story: e09s01 — server-side cap on rendered operations.
const OP_LOG_CAP: usize = 50;

// story: e09s01 — the per-repository operation log, newest first, from
// jj's own .meta store. Repo read rules apply; a missing or unreadable
// .meta store is a visible "no operation log" state, never a failure.
pub async fn operations_page(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
    jar: CookieJar,
) -> Response {
    if crate::revsets::visible_repo(&state, &owner, &name, &jar)
        .await
        .is_none()
    {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    }

    let meta = domain::meta_path(&state.storage_root, &owner, &name);
    let operations = crate::jj_meta::load_operations(&meta, OP_LOG_CAP)
        .await
        .unwrap_or_default();

    let viewer = viewer_handle(&state, &jar).await;
    let csrf = jar
        .get(SESSION_COOKIE)
        .map(|c| crate::session::csrf_token(&state.session_secret, c.value()))
        .unwrap_or_default();
    let mut context = page_context(viewer.as_deref(), &csrf);
    context.insert("owner", &owner);
    context.insert("name", &name);
    context.insert("operations", &operations);
    context.insert("has_log", &!operations.is_empty());
    Html(crate::render::render("operations.html", &context)).into_response()
}

// story: e09s02 — one operation in the audit log: who did what to which
// ref. Same read rules as the list page; an id that does not resolve in
// the .meta store is a 404; a missing .meta store is the list page's
// "no operation log" empty state, never a 500.
pub async fn operation_detail_page(
    State(state): State<AppState>,
    axum::extract::Path((owner, name, operation_id)): axum::extract::Path<(String, String, String)>,
    jar: CookieJar,
) -> Response {
    if crate::revsets::visible_repo(&state, &owner, &name, &jar)
        .await
        .is_none()
    {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    }

    let meta = domain::meta_path(&state.storage_root, &owner, &name);
    let detail = match crate::jj_meta::load_operation_detail(&meta, &operation_id).await {
        Ok(detail) => detail,
        // story: e09s01 defensive rule — degraded data renders the
        // placeholder, not a failure.
        Err(_) => crate::jj_meta::OpDetailOutcome::NoLog,
    };

    let viewer = viewer_handle(&state, &jar).await;
    let csrf = jar
        .get(SESSION_COOKIE)
        .map(|c| crate::session::csrf_token(&state.session_secret, c.value()))
        .unwrap_or_default();
    let mut context = page_context(viewer.as_deref(), &csrf);
    context.insert("owner", &owner);
    context.insert("name", &name);
    match detail {
        crate::jj_meta::OpDetailOutcome::Found(op) => {
            context.insert("op", &op);
            Html(crate::render::render("operation_detail.html", &context)).into_response()
        }
        crate::jj_meta::OpDetailOutcome::NoLog => {
            // story: e09s02 — the list page's empty state, unchanged.
            context.insert("operations", &Vec::<crate::jj_meta::OpLogEntry>::new());
            context.insert("has_log", &false);
            Html(crate::render::render("operations.html", &context)).into_response()
        }
        crate::jj_meta::OpDetailOutcome::NotFound => {
            (StatusCode::NOT_FOUND, "operation not found").into_response()
        }
    }
}
