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

#[derive(Deserialize)]
pub struct RegisterForm {
    email: String,
    password: String,
    password_confirm: String,
}

fn register_page(error: &str, email: &str) -> String {
    let error_html = if error.is_empty() {
        String::new()
    } else {
        format!("<p role=\"alert\">{}</p>", crate::stack::html_escape(error))
    };
    let email = crate::stack::html_escape(email);
    format!(
        r#"<!doctype html><html><body>
<h1>Register</h1>
{error_html}
<form method="post" action="/register">
<label>Email <input type="email" name="email" value="{email}" required></label>
<label>Password <input type="password" name="password" required minlength="10"></label>
<label>Confirm password <input type="password" name="password_confirm" required minlength="10"></label>
<button type="submit">Register</button>
</form>
</body></html>"#
    )
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
                register_page(&err.to_string(), &form.email),
            )
                .into_response()
        }
    };

    // story: e01s03 — confirm the password at sign-up so a typo'd
    // password can't lock a new account out on first login
    if form.password != form.password_confirm {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            register_page("passwords do not match", &form.email),
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
            register_page("email already registered", &form.email),
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
                register_page("email already registered", &form.email),
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
    let error_html = if error.is_empty() {
        String::new()
    } else {
        format!("<p role=\"alert\">{}</p>", crate::stack::html_escape(error))
    };
    format!(
        r#"<!doctype html><html><body>
<h1>Log in</h1>
{error_html}
<form method="post" action="/login">
<label>Email <input type="email" name="email" required></label>
<label>Password <input type="password" name="password" required></label>
<button type="submit">Log in</button>
</form>
</body></html>"#
    )
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
            login_page("email or password is incorrect"),
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
    let nav = nav_html(handle.as_deref(), &csrf);
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

    let new_token_html = match jar.get("new_token") {
        Some(cookie) => {
            let value = cookie.value().to_string();
            format!(
                "<p><strong>Copy this token now, it will not be shown again:</strong> <code>{value}</code></p>"
            )
        }
        None => String::new(),
    };

    let token_rows: String = tokens
        .iter()
        .map(|(label, prefix)| format!("<li>{label} — {prefix}…</li>"))
        .collect();

    match email {
        Some(email) => Html(format!(
            r#"<!doctype html><html><body>
<h1>Settings</h1>
{nav}<p>Signed in as {email}</p>
<h2>Access tokens</h2>
{new_token_html}
<form method="post" action="/settings/tokens">
<input type="hidden" name="_csrf" value="{csrf}">
<label>Label <input name="label" required></label>
<button type="submit">Generate token</button>
</form>
<ul>{token_rows}</ul>
</body></html>"#
        ))
        .into_response(),
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
    Form(form): Form<CreateRepoForm>,
) -> Response {
    let name = form.name.trim().to_lowercase();
    if !domain::repo_name_valid(&name) {
        return (StatusCode::UNPROCESSABLE_ENTITY, "invalid repository name").into_response();
    }
    let visibility = match domain::Visibility::parse(&form.visibility) {
        Some(v) => v,
        None => return (StatusCode::UNPROCESSABLE_ENTITY, "invalid visibility").into_response(),
    };

    let owner: String = sqlx::query_scalar("SELECT handle FROM users WHERE id = $1")
        .bind(current.0)
        .fetch_one(&state.pool)
        .await
        .unwrap_or_default();

    // story: e05s01 — the DB row is the authoritative duplicate check, so a
    // known duplicate returns 409 before any store init (init_external_git
    // refuses to re-init an existing .jj, which would otherwise 500). The
    // insert+commit below stays one atomic write for the race the pre-check
    // can't see: no row can point at a missing store.
    let owner_id = current.0;
    let existing: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM repositories WHERE owner_id = $1 AND name = $2")
            .bind(owner_id)
            .bind(&name)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);
    if existing > 0 {
        return (StatusCode::CONFLICT, "repository already exists").into_response();
    }

    // story: e05s01 — create both stores, then commit the row in one
    // transaction. No manual compensation: if either init fails we clean up
    // two dirs we own, and if the transaction aborts the filesystem work is
    // discarded the same way.
    let bare = domain::storage_path(&state.storage_root, &owner, &name);
    if let Err(err) = crate::repo::init_bare(&bare) {
        let _ = std::fs::remove_dir_all(&bare);
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }

    // story: e05s01 — colocate jj metadata beside the bare repo.
    let meta = domain::meta_path(&state.storage_root, &owner, &name);
    if let Err(err) = crate::jj_meta::init(&meta, &bare).await {
        let _ = std::fs::remove_dir_all(&bare);
        let _ = std::fs::remove_dir_all(&meta);
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }

    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(err) => {
            let _ = std::fs::remove_dir_all(&bare);
            let _ = std::fs::remove_dir_all(&meta);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("db error: {err}"),
            )
                .into_response();
        }
    };
    let inserted: Result<(uuid::Uuid,), _> = sqlx::query_as(
        "INSERT INTO repositories (owner_id, name, visibility) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(owner_id)
    .bind(&name)
    .bind(visibility.as_str())
    .fetch_one(&mut *tx)
    .await;

    match inserted {
        Ok(_) => {
            if let Err(err) = tx.commit().await {
                let _ = std::fs::remove_dir_all(&bare);
                let _ = std::fs::remove_dir_all(&meta);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("db error: {err}"),
                )
                    .into_response();
            }
            Redirect::to(&format!("/{owner}/{name}")).into_response()
        }
        Err(err) => {
            drop(tx);
            let _ = std::fs::remove_dir_all(&bare);
            let _ = std::fs::remove_dir_all(&meta);
            match &err {
                sqlx::Error::Database(db)
                    if db.constraint() == Some("repositories_owner_id_name_key") =>
                {
                    (StatusCode::CONFLICT, "repository already exists").into_response()
                }
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("db error: {err}"),
                )
                    .into_response(),
            }
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
    let nav = nav_html(viewer.as_deref(), &csrf);
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

    // story: repo dashboard — the page surfaces open changes, not just
    // clone instructions
    let mrs = crate::stack::open_mrs(&state.pool, repo_id)
        .await
        .unwrap_or_default();
    let mr_items: Vec<String> = mrs
        .iter()
        .map(|mr| {
            format!(
                "<li><a href=\"/{owner}/{name}/merge-requests/{change}\">{change}</a></li>",
                owner = owner,
                name = name,
                change = mr.change_id
            )
        })
        .collect();
    let mr_section = if mr_items.is_empty() {
        "<p>No open merge requests — push a change to open one.</p>".to_string()
    } else {
        format!("<ul>{}</ul>", mr_items.join("\n"))
    };

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
        let pinned = crate::stack::ref_commit(&bare, &format!("refs/heads/{trunk}"))
            .map(|sha| format!("{link}?commit={sha}"))
            .unwrap_or(link);
        format!(" · <a href=\"{pinned}\">Browse (kata)</a>")
    })
    .unwrap_or_default();
    // story: e07s01 — native tree view; only when the repo has commits
    let bare_for_files = domain::storage_path(&state.storage_root, &owner, &name);
    let files_link =
        if crate::stack::ref_commit(&bare_for_files, &format!("refs/heads/{trunk}")).is_some() {
            format!(
                " · <a href=\"/{owner}/{name}/tree\">Files</a>",
                owner = owner,
                name = name
            )
        } else {
            String::new()
        };

    Html(format!(
        r#"<!doctype html><html><body>
<h1>{owner}/{name}</h1>
<p>{visibility} — trunk: {trunk}</p>
{nav}<p>{browse}{files_link}</p><h2>Open merge requests</h2>
{mr_section}
<p><a href="/{owner}/{name}/merge-requests">All merge requests</a>{browse}</p>
<h2>Add this repository</h2>
<p>Clone: <code>http://{host}/git/{owner}/{name}.git</code></p>
<pre>jj git remote add origin "http://x-access-token:&lt;TOKEN&gt;@{host}/git/{owner}/{name}.git"
jj git push --all</pre>
</body></html>"#
    ))
    .into_response()
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
    let new = crate::token::create(&state.pool, current.0, &form.label).await;
    let jar = jar.add(axum_extra::extract::cookie::Cookie::new(
        "new_token",
        new.plaintext,
    ));
    (jar, Redirect::to("/settings")).into_response()
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
    let pulls_root = format!("/{owner}/{name}/merge-requests");
    let mut position: Vec<String> = Vec::new();
    for (stack_idx, stack) in stacks.iter().enumerate() {
        for (mr_idx, change_id) in stack.iter().enumerate() {
            let review = crate::review_proxy::review_link(
                &state.kata_public_base,
                &state.storage_root,
                &owner,
                &name,
                change_id,
            )
            .map(|href| format!(" — <a href=\"{href}\">review</a>"))
            .unwrap_or_default();
            position.push(format!(
                "<li data-change=\"{change_id}\">#{stack_idx}:{mr_idx} \
                 <a href=\"{pulls_root}/{change_id}\">{change_id}</a>{review}</li>"
            ));
        }
    }

    Html(format!(
        r#"<!doctype html><html><body>
<h1>{owner}/{name} — merge requests</h1>
<ol>
{}
</ol>
</body></html>"#,
        position.join("\n")
    ))
    .into_response()
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
            let inner = match url {
                Some(u) if u.starts_with("http://") || u.starts_with("https://") => {
                    format!("<a href=\"{}\">{}</a>", crate::stack::html_escape(u), label)
                }
                Some(u) => format!("{} ({})", label, crate::stack::html_escape(u)),
                None => label,
            };
            format!(
                "<span data-status=\"{}\" data-state=\"{}\">{}</span>",
                crate::stack::html_escape(context),
                crate::stack::html_escape(state),
                inner
            )
        })
        .collect();
    let statuses_html = if badges.is_empty() {
        String::new()
    } else {
        format!("<p class=\"statuses\">{}</p>", badges.join(" "))
    };

    let bare = domain::storage_path(&state.storage_root, &owner, &name);
    let mut sections = Vec::new();
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
        let threads_html = if threads.is_empty() {
            String::new()
        } else {
            let items: Vec<String> = threads
                .iter()
                .map(|(handle, body)| {
                    format!(
                        "<li>{}: {}</li>",
                        crate::stack::html_escape(handle),
                        crate::stack::html_escape(body)
                    )
                })
                .collect();
            format!("<ul class=\"threads\">{}</ul>", items.join(""))
        };
        sections.push(format!(
            "<section data-revision=\"{commit}\"><h2>{commit}</h2><pre>{}</pre>{threads_html}\
             <form method=\"post\" action=\"/{owner}/{name}/merge-requests/{change_id}/comments\">\
             <input type=\"hidden\" name=\"revision\" value=\"{commit}\">\
             <input type=\"hidden\" name=\"_csrf\" value=\"{csrf}\">\
             <input name=\"body\" placeholder=\"Comment on this revision\" required>\
             <button type=\"submit\">Comment</button></form></section>",
            crate::stack::html_escape(&diff)
        ));
    }

    Html(format!(
        r#"<!doctype html><html><body>
<h1>{owner}/{name} — {change_id} ({mr_state})</h1>
{statuses_html}
{}
{review_section}
<form method="post" action="/{owner}/{name}/merge-requests/{change_id}/merge"><input type="hidden" name="_csrf" value="{csrf}"><button type="submit">Merge</button></form>
<form method="post" action="/{owner}/{name}/merge-requests/{change_id}/abandon"><input type="hidden" name="_csrf" value="{csrf}"><button type="submit">Abandon</button></form>
</body></html>"#,
        sections.join("\n"),
        review_section = match crate::review_proxy::review_link(
            &state.kata_public_base,
            &state.storage_root,
            &owner,
            &name,
            &change_id,
        ) {
            Some(href) => format!(
                "<p><a href=\"{href}\">Review in kata &rarr;</a></p>"
            ),
            None => String::new(),
        },
    ))
    .into_response()
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
// a CSRF-protected Log out button.
pub fn nav_html(user_handle: Option<&str>, csrf: &str) -> String {
    match user_handle {
        Some(handle) => format!(
            "<nav><a href=\"/\">jjhub</a> <a href=\"/\">Repos</a> \
             <a href=\"/settings\">Settings</a> <span>{handle}</span> \
             <form method=\"post\" action=\"/logout\" style=\"display:inline\">\
             <input type=\"hidden\" name=\"_csrf\" value=\"{csrf}\">\
             <button type=\"submit\">Log out</button></form></nav>"
        ),
        None => r#"<nav><a href="/">jjhub</a> <a href="/login">Log in</a></nav>"#.to_string(),
    }
}

async fn viewer_handle(state: &AppState, jar: &CookieJar) -> Option<String> {
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
        return Html(format!(
            r#"<!doctype html><html><body>
{}
<h1>jjhub</h1>
<p>Forge-style hosting for Jujutsu repositories.</p>
<p><a href="/register">Register</a> · <a href="/login">Log in</a></p>
</body></html>"#,
            nav_html(None, "")
        ))
        .into_response();
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
    let repo_items: Vec<String> = repos
        .iter()
        .map(|(name, visibility)| {
            format!(
                "<li><a href=\"/{handle}/{name}\">{handle}/{name}</a> <small>{visibility}</small></li>",
                handle = handle,
                name = name,
                visibility = visibility
            )
        })
        .collect();

    Html(format!(
        r#"<!doctype html><html><body>
{}
<h1>jjhub</h1>
<h2>Create repository</h2>
<form method="post" action="/repos">
<input type="hidden" name="_csrf" value="{csrf}">
<label>Name <input name="name" required pattern="[a-z0-9][a-z0-9._-]*"></label>
<label><input type="radio" name="visibility" value="public" checked> public</label>
<label><input type="radio" name="visibility" value="private"> private</label>
<button type="submit">Create</button>
</form>
<h2>Your repositories</h2>
<ul>{repos}</ul>
</body></html>"#,
        nav_html(Some(&handle), &csrf),
        csrf = csrf,
        repos = repo_items.join("\n")
    ))
    .into_response()
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
    let mut crumbs = vec![format!(
        "<a href=\"/{owner}/{name}/tree\">{owner}/{name}</a>",
        owner = owner,
        name = name
    )];
    let mut acc = String::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        acc = if acc.is_empty() {
            part.to_string()
        } else {
            format!("{acc}/{part}")
        };
        crumbs.push(format!(
            "<a href=\"/{owner}/{name}/tree/{git_ref}/{acc}\">{part}</a>",
            owner = owner,
            name = name,
            git_ref = git_ref,
            acc = acc,
            part = crate::stack::html_escape(part)
        ));
    }

    let items: Vec<String> = entries
        .iter()
        .map(|(entry_name, is_dir)| {
            let escaped = crate::stack::html_escape(entry_name);
            if *is_dir {
                let target = if path.is_empty() {
                    format!("/{owner}/{name}/tree/{git_ref}/{n}", n = entry_name)
                } else {
                    format!(
                        "/{owner}/{name}/tree/{git_ref}/{p}/{n}",
                        p = path,
                        n = entry_name
                    )
                };
                format!(
                    "<li>📁 <a href=\"{target}\">{escaped}/</a></li>",
                    target = target,
                    escaped = escaped
                )
            } else {
                let target = if path.is_empty() {
                    format!("/{owner}/{name}/blob/{git_ref}/{n}", n = entry_name)
                } else {
                    format!(
                        "/{owner}/{name}/blob/{git_ref}/{p}/{n}",
                        p = path,
                        n = entry_name
                    )
                };
                format!(
                    "<li>📄 <a href=\"{target}\">{escaped}</a></li>",
                    target = target,
                    escaped = escaped
                )
            }
        })
        .collect();

    Html(format!(
        r#"<!doctype html><html><body>
<h1>{owner}/{name} — {path_label}</h1>
<p>{crumbs}</p>
<ul>
{items}
</ul>
</body></html>"#,
        owner = owner,
        name = name,
        path_label = if path.is_empty() {
            "/".to_string()
        } else {
            crate::stack::html_escape(&path)
        },
        crumbs = crumbs.join(" / "),
        items = items.join("\n")
    ))
    .into_response()
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
            let numbered: Vec<String> = text
                .lines()
                .enumerate()
                .map(|(i, line)| format!("{:>4} {}", i + 1, crate::stack::html_escape(line)))
                .collect();
            Html(format!(
                r#"<!doctype html><html><body>
<h1>{owner}/{name} — {path}</h1>
<p><a href="{back}">back</a></p>
<pre>{numbered}</pre>
</body></html>"#,
                owner = owner,
                name = name,
                path = crate::stack::html_escape(&path),
                back = back,
                numbered = numbered.join("\n")
            ))
            .into_response()
        }
        Ok(crate::browse::Blob::Binary { len }) => Html(format!(
            r#"<!doctype html><html><body>
<h1>{owner}/{name} — {path}</h1>
<p><a href="{back}">back</a></p>
<p>{len} bytes (binary file — not shown)</p>
</body></html>"#,
            owner = owner,
            name = name,
            path = crate::stack::html_escape(&path),
            back = back,
            len = len
        ))
        .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "no such file").into_response(),
    }
}
