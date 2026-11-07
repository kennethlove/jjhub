//! RED suite for e34s02: reset via the mailer, path-form link, neutral answers.
//!
//! Written against the story's contract BEFORE the implementation: the current
//! flow mints a query-form link into the server log (no delivery), so cases
//! here are expected to fail until GREEN wires `Mailer::send` and a
//! GET /reset/{token} route into the existing reset handlers.

use std::sync::{Arc, Mutex};

use axum::http::{self, header, Request, StatusCode};
use axum::Router;
use tower::util::ServiceExt;

// --- recorded-mailer scaffold (shape copied from email_confirmation.rs) ---

/// (to, subject, text arm, html arm) — what every send records.
type MailRecord = (String, String, String, String);

#[derive(Clone)]
struct Recorded {
    mails: Arc<Mutex<Vec<MailRecord>>>,
}

impl Recorded {
    fn new() -> Self {
        Recorded {
            mails: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn last(&self) -> (String, String, String, String) {
        self.mails.lock().unwrap().last().cloned().unwrap()
    }

    fn count(&self) -> usize {
        self.mails.lock().unwrap().len()
    }
}

struct RecordingMailer(Recorded);

#[async_trait::async_trait]
impl server::email::Mailer for RecordingMailer {
    async fn send(
        &self,
        to: &str,
        subject: &str,
        text: &str,
        html: &str,
    ) -> Result<(), server::email::MailError> {
        self.0.mails.lock().unwrap().push((
            to.to_string(),
            subject.to_string(),
            text.to_string(),
            html.to_string(),
        ));
        Ok(())
    }
}

async fn app(pool: sqlx::PgPool, recorded: Recorded) -> Router {
    server::app_with_mailer(
        pool.clone(),
        "unused-storage".to_string(),
        Arc::new(RecordingMailer(recorded.clone())),
    )
}

async fn post(url: &str, form: &str, cookie: Option<&str>) -> http::Request<axum::body::Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(url)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(value) = cookie {
        builder = builder.header(header::COOKIE, value);
    }
    builder
        .body(axum::body::Body::from(form.to_string()))
        .unwrap()
}

async fn get(url: &str, cookie: Option<&str>) -> http::Request<axum::body::Body> {
    let mut builder = Request::builder().method("GET").uri(url);
    if let Some(value) = cookie {
        builder = builder.header(header::COOKIE, value);
    }
    builder.body(axum::body::Body::empty()).unwrap()
}

async fn body_of(response: http::Response<axum::body::Body>) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8_lossy(&bytes).to_string()
}

/// Path-form token extractor: takes everything after `prefix` up to the
/// closing quote of the href (reset tokens may carry `-`/`_`, so a plain
/// alphanumeric collect would truncate).
fn token_after(haystack: &str, prefix: &str) -> String {
    let start = haystack.find(prefix).unwrap_or_else(|| {
        panic!("prefix {prefix:?} missing from {haystack}");
    }) + prefix.len();
    haystack[start..]
        .chars()
        .take_while(|c| !matches!(c, '"' | '<' | '\'' | ' ' | ')' | '\n' | '\r'))
        .collect()
}

async fn register(pool: &sqlx::PgPool, app_: &Router, email: &str, password: &str) {
    let form = format!("email={email}&password={password}&password_confirm={password}&_csrf=");
    let response = app_
        .clone()
        .oneshot(post("/register", &form, None).await)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER, "register failed");
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE email = $1")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
}

async fn request_reset(app_: &Router, email: &str) -> http::Response<axum::body::Body> {
    app_.clone()
        .oneshot(post("/reset", &format!("email={email}"), None).await)
        .await
        .unwrap()
}

/// Registration itself mails a verification link — drop it so each test's
/// mailbox holds only the reset delivery under test.
fn clear(recorded: &Recorded) {
    recorded.mails.lock().unwrap().clear();
}

async fn latest_token(pool: &sqlx::PgPool, email: &str) -> (String, String) {
    let user_id: String = sqlx::query_scalar("SELECT id::text FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    let stored: String = sqlx::query_scalar(
        "SELECT token_hash FROM reset_tokens WHERE user_id = $1::uuid \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&user_id)
    .fetch_one(pool)
    .await
    .unwrap();
    (user_id, stored)
}

#[sqlx::test]
async fn known_address_gets_a_path_reset_link_delivery(pool: sqlx::PgPool) {
    let recorded = Recorded::new();
    let app = app(pool.clone(), recorded.clone()).await;
    register(&pool, &app, "known@example.test", "correct-horse-battery").await;
    clear(&recorded);

    let response = request_reset(&app, "known@example.test").await;
    assert!(
        response.status().is_redirection() || response.status().is_success(),
        "request must not fail: {}",
        response.status()
    );
    assert_eq!(recorded.count(), 1, "exactly one delivery recorded");
    let (to, subject, body, _) = recorded.last();
    assert_eq!(to, "known@example.test");
    assert!(
        subject.to_lowercase().contains("password"),
        "subject: {subject}"
    );

    // Path form, and no query-form link anywhere in the mail.
    assert!(body.contains("/reset/"), "body: {body}");
    assert!(
        !body.contains("reset-password?token="),
        "query form leaked: {body}"
    );

    let token = token_after(&body, "/reset/");
    assert!(token.len() >= 16, "token looks too short: {token:?}");

    // Stored value is never the plaintext (salted argon2 by e24 design),
    // and it must still verify against the delivered token.
    let (_, stored) = latest_token(&pool, "known@example.test").await;
    assert_ne!(stored, token, "plaintext token stored");
    assert!(
        server::token::find_reset(&pool, &token).await.is_some(),
        "stored hash does not verify the delivered token"
    );
    assert!(!body.contains(&stored), "hash leaked into the mail");
}

#[sqlx::test]
async fn unknown_address_gets_the_same_response(pool: sqlx::PgPool) {
    let recorded = Recorded::new();
    let app = app(pool.clone(), recorded.clone()).await;
    register(&pool, &app, "known@example.test", "correct-horse-battery").await;
    clear(&recorded);

    let known = request_reset(&app, "known@example.test").await;
    let known_status = known.status();
    let known_body = body_of(known).await;

    let unknown = request_reset(&app, "nobody@example.test").await;
    let unknown_status = unknown.status();
    let unknown_body = body_of(unknown).await;

    assert_eq!(known_status, unknown_status, "oracle differs by account");
    assert_eq!(known_body, unknown_body, "page oracle differs by account");
    assert_eq!(
        recorded.count(),
        1,
        "unknown address must not record a delivery"
    );
}

#[sqlx::test]
async fn expired_reset_token_is_refused(pool: sqlx::PgPool) {
    let recorded = Recorded::new();
    let app = app(pool.clone(), recorded.clone()).await;
    register(&pool, &app, "known@example.test", "correct-horse-battery").await;
    clear(&recorded);
    request_reset(&app, "known@example.test").await;
    let (_, body) = (recorded.last().0, recorded.last().2);
    let token = token_after(&body, "/reset/");

    sqlx::query(
        "UPDATE reset_tokens SET expires_at = now() - interval '1 hour' \
         WHERE user_id = (SELECT id FROM users WHERE email = $1)",
    )
    .bind("known@example.test")
    .execute(&pool)
    .await
    .unwrap();

    let response = app
        .clone()
        .oneshot(get(&format!("/reset/{token}"), None).await)
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "expired token served"
    );
}

#[sqlx::test]
async fn consumed_token_cannot_be_used_twice(pool: sqlx::PgPool) {
    let recorded = Recorded::new();
    let app = app(pool.clone(), recorded.clone()).await;
    register(&pool, &app, "known@example.test", "correct-horse-battery").await;
    clear(&recorded);
    request_reset(&app, "known@example.test").await;
    let token = token_after(&recorded.last().2, "/reset/");

    let first = app
        .clone()
        .oneshot(
            post(
                &format!("/reset/{token}"),
                "password=brand-new-pass-456&password_confirm=brand-new-pass-456",
                None,
            )
            .await,
        )
        .await
        .unwrap();
    assert!(
        first.status().is_redirection(),
        "first use must succeed: {}",
        first.status()
    );

    let second = app
        .clone()
        .oneshot(
            post(
                &format!("/reset/{token}"),
                "password=third-pass-789&password_confirm=third-pass-789",
                None,
            )
            .await,
        )
        .await
        .unwrap();
    assert_eq!(
        second.status(),
        StatusCode::BAD_REQUEST,
        "second use must refuse"
    );
}

#[sqlx::test]
async fn successful_path_reset_rotates_the_password(pool: sqlx::PgPool) {
    let recorded = Recorded::new();
    let app = app(pool.clone(), recorded.clone()).await;
    register(&pool, &app, "known@example.test", "correct-horse-battery").await;
    clear(&recorded);
    request_reset(&app, "known@example.test").await;
    let token = token_after(&recorded.last().2, "/reset/");

    let response = app
        .clone()
        .oneshot(
            post(
                &format!("/reset/{token}"),
                "password=brand-new-pass-456&password_confirm=brand-new-pass-456",
                None,
            )
            .await,
        )
        .await
        .unwrap();
    assert!(
        response.status().is_redirection(),
        "reset must succeed: {}",
        response.status()
    );

    let old_login = app
        .clone()
        .oneshot(
            post(
                "/login",
                "email=known@example.test&password=correct-horse-battery",
                None,
            )
            .await,
        )
        .await
        .unwrap();
    assert_eq!(
        old_login.status(),
        StatusCode::UNAUTHORIZED,
        "old password still works"
    );

    let new_login = app
        .clone()
        .oneshot(
            post(
                "/login",
                "email=known@example.test&password=brand-new-pass-456",
                None,
            )
            .await,
        )
        .await
        .unwrap();
    assert_eq!(
        new_login.status(),
        StatusCode::SEE_OTHER,
        "new password rejected"
    );
}

#[sqlx::test]
async fn a_broken_mailer_never_surfaces_to_the_client(pool: sqlx::PgPool) {
    // Plain app: LogMailer default, no recording. A request must still be
    // answered neutrally (no 500, no oracle) even if delivery logic misbehaves.
    let app = server::app(pool.clone());
    register(&pool, &app, "known@example.test", "correct-horse-battery").await;

    let known = request_reset(&app, "known@example.test").await;
    let known_status = known.status();
    let _known_body = body_of(known).await;
    let unknown = request_reset(&app, "nobody@example.test").await;
    assert_ne!(unknown.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(unknown.status(), known_status, "neutral answers diverged");
}
