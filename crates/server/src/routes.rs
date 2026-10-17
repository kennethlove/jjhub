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
}

pub async fn register_form() -> Html<&'static str> {
    Html(
        r#"<!doctype html><html><body>
<h1>Register</h1>
<form method="post" action="/register">
<label>Email <input type="email" name="email" required></label>
<label>Password <input type="password" name="password" required minlength="10"></label>
<button type="submit">Register</button>
</form>
</body></html>"#,
    )
}

pub async fn register(State(state): State<AppState>, Form(form): Form<RegisterForm>) -> Response {
    let (email, handle) = match domain::validate_registration(&form.email, &form.password) {
        Ok(pair) => pair,
        Err(err) => return (StatusCode::UNPROCESSABLE_ENTITY, err.to_string()).into_response(),
    };

    let password_hash = hash_password(&form.password);
    let existing: Option<String> =
        sqlx::query_scalar("SELECT id::text FROM users WHERE email = $1")
            .bind(&email)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    if existing.is_some() {
        return (StatusCode::CONFLICT, "email already registered").into_response();
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
            return (StatusCode::CONFLICT, "email already registered").into_response()
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

pub async fn login_form() -> Html<&'static str> {
    Html(
        r#"<!doctype html><html><body>
<h1>Log in</h1>
<form method="post" action="/login">
<label>Email <input type="email" name="email" required></label>
<label>Password <input type="password" name="password" required></label>
<button type="submit">Log in</button>
</form>
</body></html>"#,
    )
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
        _ => StatusCode::UNAUTHORIZED.into_response(),
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
    let nav = nav_html(handle.as_deref());
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
    let nav = nav_html(viewer.as_deref());
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
        format!(
            " · <a href=\"{}\">Browse files</a>",
            link.replace("browse?change=", "browse")
        )
    })
    .unwrap_or_default();

    Html(format!(
        r#"<!doctype html><html><body>
<h1>{owner}/{name}</h1>
<p>{visibility} — trunk: {trunk}</p>
{nav}<h2>Open merge requests</h2>
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
    let revisions: Vec<String> = sqlx::query_scalar(
        "SELECT commit_id FROM revisions WHERE mr_id = $1 ORDER BY pushed_at DESC",
    )
    .bind(mr_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let bare = domain::storage_path(&state.storage_root, &owner, &name);
    let mut sections = Vec::new();
    for commit in &revisions {
        let diff = crate::stack::unified_diff(&bare, commit);
        sections.push(format!(
            "<section data-revision=\"{commit}\"><h2>{commit}</h2><pre>{}</pre></section>",
            crate::stack::html_escape(&diff)
        ));
    }

    Html(format!(
        r#"<!doctype html><html><body>
<h1>{owner}/{name} — {change_id} ({mr_state})</h1>
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

// story: e06s01 — shared nav: brand + (when authed) Repos and Settings.
pub fn nav_html(user_handle: Option<&str>) -> String {
    match user_handle {
        Some(handle) => format!(
            "<nav><a href=\"/\">jjhub</a> <a href=\"/\">Repos</a> \
             <a href=\"/settings\">Settings</a> <span>{handle}</span></nav>"
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
            nav_html(None)
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
        nav_html(Some(&handle)),
        csrf = csrf,
        repos = repo_items.join("\n")
    ))
    .into_response()
}
