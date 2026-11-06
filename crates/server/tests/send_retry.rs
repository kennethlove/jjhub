//! RED suite for e36s02: a clear retry when the send path is broken.
//!
//! Written against the story's contract BEFORE the implementation: signup
//! with a dead relay currently answers the verify-sent page and keeps the
//! half-finished account (e36s01 stopped at "never a 500"), resend always
//! redirects, and the reset contract must stay byte-identical either way.

use std::sync::{Arc, LazyLock, Mutex};

use axum::http;
use axum::Router;
use tower::util::ServiceExt;

// --- env guard: provider flips mid-test, serialized across tests ---

static ENV: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    /// Take the lock and snapshot the keys; tests flip the provider
    /// themselves between setup and the failing call.
    fn hold() -> Self {
        const KEYS: [&str; 5] = [
            "JJHUB_EMAIL_PROVIDER",
            "JJHUB_SMTP_HOST",
            "JJHUB_SMTP_PORT",
            "JJHUB_SMTP_USERNAME",
            "JJHUB_SMTP_PASSWORD",
        ];
        // A panicking test must not poison the guard for its peers.
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let saved = KEYS.iter().map(|k| (*k, std::env::var(k).ok())).collect();
        for k in KEYS {
            std::env::remove_var(k);
        }
        EnvGuard { _lock: lock, saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in self.saved.clone() {
            match v {
                Some(value) => std::env::set_var(k, value),
                None => std::env::remove_var(k),
            }
        }
    }
}

/// Point the provider arm at a dead local relay (config parse, no server).
fn set_dead_relay() {
    std::env::set_var("JJHUB_EMAIL_PROVIDER", "smtp");
    std::env::set_var("JJHUB_SMTP_HOST", "127.0.0.1");
    std::env::set_var("JJHUB_SMTP_PORT", "1");
}

async fn post(app_: &Router, url: &str, form: &str) -> http::Response<axum::body::Body> {
    app_.clone()
        .oneshot(
            axum::extract::Request::builder()
                .method("POST")
                .uri(url)
                .header(
                    http::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(axum::body::Body::from(form.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn body_of(response: http::Response<axum::body::Body>) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// The env-built arm behind a one-line adapter (a boxed mailer is not
/// itself a `Mailer`); built fresh per send so the flip above applies.
struct EnvMailer;

#[async_trait::async_trait]
impl server::email::Mailer for EnvMailer {
    async fn send(
        &self,
        to: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), server::email::MailError> {
        server::email::from_env().send(to, subject, body).await
    }
}

fn app_with_mailer(
    pool: &sqlx::PgPool,
    mailer: Arc<dyn server::email::Mailer>,
) -> (Router, tempfile::TempDir) {
    std::env::set_var("SESSION_SECRET", "send-retry-secret");
    let root = tempfile::tempdir().unwrap();
    let app_ = server::app_with_mailer(
        pool.clone(),
        root.path().to_str().unwrap().to_string(),
        mailer,
    );
    (app_, root)
}

fn env_app(pool: &sqlx::PgPool) -> (Router, tempfile::TempDir) {
    app_with_mailer(pool, Arc::new(EnvMailer))
}

/// Setup registration under the working in-box arm; asserts the normal
/// 303 so a broken setup fails loudly, not silently.
async fn register_ok(pool: &sqlx::PgPool, app_: &Router, email: &str) {
    let response = post(
        app_,
        "/register",
        &format!(
            "email={email}&password=correct-horse-battery&password_confirm=correct-horse-battery&_csrf="
        ),
    )
    .await;
    assert_eq!(
        response.status(),
        http::StatusCode::SEE_OTHER,
        "setup register must work"
    );
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE email = $1")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
}

async fn user_count(pool: &sqlx::PgPool, email: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn failed_signup_shows_a_retry_and_leaves_no_account(pool: sqlx::PgPool) {
    let _guard = EnvGuard::hold();
    let (app, _root) = env_app(&pool);
    set_dead_relay();

    let email = "retry@example.test";
    let response = post(
        &app,
        "/register",
        &format!(
            "email={email}&password=correct-horse-battery&password_confirm=correct-horse-battery&_csrf="
        ),
    )
    .await;

    assert_eq!(
        response.status(),
        http::StatusCode::UNPROCESSABLE_ENTITY,
        "a dead send path answers with the retry, not a redirect or 500"
    );
    let body = body_of(response).await;
    assert!(
        body.contains("try again"),
        "the page must say to retry: {body}"
    );
    assert_eq!(
        user_count(&pool, email).await,
        0,
        "no half-finished account survives a failed send"
    );
}

#[sqlx::test]
async fn failed_resend_shows_a_retry(pool: sqlx::PgPool) {
    let _guard = EnvGuard::hold();
    let (app, _root) = env_app(&pool);
    let email = "resend-retry@example.test";
    register_ok(&pool, &app, email).await;

    // A pending confirmation, older than the cooldown, so the resend
    // really dials the dead relay.
    let user_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(&pool)
        .await
        .unwrap();
    server::email::send_verification(&pool, user_id).await;
    sqlx::query(
        "UPDATE email_verifications SET created_at = now() - interval '2 minutes' \
         WHERE user_id = $1",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    set_dead_relay();
    let response = post(&app, "/verify-resend", &format!("email={email}")).await;

    assert_eq!(
        response.status(),
        http::StatusCode::OK,
        "a failed resend answers with the page, never a 500 or a lie"
    );
    let body = body_of(response).await;
    assert!(
        body.contains("try again"),
        "the resend page must say to retry: {body}"
    );
}

#[sqlx::test]
async fn reset_response_is_identical_whether_or_not_the_send_fails(pool: sqlx::PgPool) {
    let _guard = EnvGuard::hold();
    let email = "reset-neutral@example.test";

    // Working arm first: the known contract's response.
    let (ok_app, _ok_root) = app_with_mailer(&pool, Arc::new(server::email::LogMailer));
    register_ok(&pool, &ok_app, email).await;
    let ok_response = post(&ok_app, "/reset", &format!("email={email}")).await;

    // Dead relay: same request, same bytes.
    let (dead_app, _dead_root) = env_app(&pool);
    set_dead_relay();
    let dead_response = post(&dead_app, "/reset", &format!("email={email}")).await;

    assert_eq!(
        ok_response.status(),
        dead_response.status(),
        "same status either way"
    );
    assert_eq!(
        ok_response.headers().get(http::header::LOCATION),
        dead_response.headers().get(http::header::LOCATION),
        "same Location either way"
    );
    let ok_body = body_of(ok_response).await;
    let dead_body = body_of(dead_response).await;
    assert_eq!(ok_body, dead_body, "byte-identical reset response");
}
