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
<p>Signed in as {email}</p>
<h2>Access tokens</h2>
{new_token_html}
<form method="post" action="/settings/tokens">
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

    // story: e05s01 — claim the row FIRST so the unique constraint is the
    // authoritative conflict check (never disk state), then create storage.
    // A duplicate name now returns 409 without touching disk, and orphaned
    // bare/meta dirs from a past failed run can't break a retry.
    let inserted: Result<(uuid::Uuid,), _> = sqlx::query_as(
        "INSERT INTO repositories (owner_id, name, visibility) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(current.0)
    .bind(&name)
    .bind(visibility.as_str())
    .fetch_one(&state.pool)
    .await;

    let repo_id = match inserted {
        Ok((id,)) => id,
        Err(sqlx::Error::Database(db))
            if db.constraint() == Some("repositories_owner_id_name_key") =>
        {
            return (StatusCode::CONFLICT, "repository already exists").into_response();
        }
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("db error: {err}"),
            )
                .into_response();
        }
    };

    // Roll the DB row back if storage creation fails, so no repo row points
    // at a missing store.
    let rollback_row = || async {
        let _ = sqlx::query("DELETE FROM repositories WHERE id = $1")
            .bind(repo_id)
            .execute(&state.pool)
            .await;
    };

    let bare = domain::storage_path(&state.storage_root, &owner, &name);
    if let Err(err) = crate::repo::init_bare(&bare) {
        rollback_row().await;
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }

    // story: e05s01 — colocate jj metadata; roll back bare repo + row on failure.
    let meta = domain::meta_path(&state.storage_root, &owner, &name);
    if let Err(err) = crate::jj_meta::init(&meta, &bare).await {
        let _ = std::fs::remove_dir_all(&bare);
        rollback_row().await;
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }

    Redirect::to(&format!("/{owner}/{name}")).into_response()
}

pub async fn repo_page(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
) -> Response {
    let repo: Option<(String, String)> = sqlx::query_as(
        "SELECT r.visibility, r.trunk FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(&owner)
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();

    match repo {
        Some((visibility, trunk)) => Html(format!(
            r#"<!doctype html><html><body>
<h1>{owner}/{name}</h1>
<p>{visibility} — trunk: {trunk}</p>
<p>Clone: <code>http://localhost:3000/git/{owner}/{name}.git</code></p>
</body></html>"#
        ))
        .into_response(),
        None => (StatusCode::NOT_FOUND, "repository not found").into_response(),
    }
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
