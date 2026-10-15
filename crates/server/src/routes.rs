// story: e01s01
use axum::Form;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
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

pub async fn register(
    State(state): State<AppState>,
    Form(form): Form<RegisterForm>,
) -> Response {
    let email = match domain::validate_registration(&form.email, &form.password) {
        Ok(email) => email,
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

    let user_id: uuid::Uuid = match sqlx::query_scalar("INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id")
        .bind(&email)
        .bind(&password_hash)
        .fetch_one(&state.pool)
        .await
    {
        Ok(id) => id,
        Err(sqlx::Error::Database(db)) if db.constraint() == Some("users_email_key") => {
            return (StatusCode::CONFLICT, "email already registered").into_response()
        }
        Err(err) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {err}")).into_response()
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
        [(header::SET_COOKIE, cookie), (header::LOCATION, "/settings".to_string())],
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

pub async fn login(
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> Response {
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
    headers: axum::http::HeaderMap,
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

    let user_id = cookie_value
        .and_then(|value| session::verify(&state.session_secret, &value));

    match user_id {
        Some(id) => {
            let email: Option<String> = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
                .bind(id)
                .fetch_optional(&state.pool)
                .await
                .ok()
                .flatten();
            match email {
                Some(email) => Html(format!(
                    r#"<!doctype html><html><body>
<h1>Settings</h1>
<p>Signed in as {email}</p>
</body></html>"#
                ))
                .into_response(),
                None => Redirect::to("/login").into_response(),
            }
        }
        None => Redirect::to("/login").into_response(),
    }
}
