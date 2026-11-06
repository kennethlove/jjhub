//! RED suite for e36s01: the delivery readout.
//!
//! Written against the story's contract BEFORE the implementation: the mail
//! write point records `to`/`subject` only (no outcome) and `/admin` shows
//! no deliveries, so the outcome and readout cases fail until GREEN records
//! the send's result and lists it for the operator.

use std::sync::{Arc, LazyLock, Mutex};

use axum::http;
use axum::Router;
use tower::util::ServiceExt;

// --- env guard: provider and log dir must serialize across tests ---

static ENV: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<String>)>,
    _dir: tempfile::TempDir,
}

impl EnvGuard {
    /// Take the env lock for the whole test, point the log dir at a fresh
    /// temp dir, and apply the caller's provider settings.
    fn set(vars: &[(&'static str, &str)]) -> Self {
        const KEYS: [&str; 6] = [
            "JJHUB_EMAIL_PROVIDER",
            "JJHUB_SMTP_HOST",
            "JJHUB_SMTP_PORT",
            "JJHUB_SMTP_USERNAME",
            "JJHUB_SMTP_PASSWORD",
            "JJHUB_LOG_DIR",
        ];
        // A panicking test must not poison the guard for its peers.
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let saved = KEYS.iter().map(|k| (*k, std::env::var(k).ok())).collect();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("JJHUB_LOG_DIR", dir.path());
        for (k, v) in vars {
            std::env::set_var(k, v);
        }
        EnvGuard {
            _lock: lock,
            saved,
            _dir: dir,
        }
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

fn csrf(cookie: &str) -> String {
    server::session::csrf_token(
        b"delivery-readout-secret",
        cookie.strip_prefix("session=").unwrap_or(cookie),
    )
}

async fn post(
    app_: &Router,
    url: &str,
    form: &str,
    cookie: &str,
) -> http::Response<axum::body::Body> {
    app_.clone()
        .oneshot(
            axum::extract::Request::builder()
                .method("POST")
                .uri(url)
                .header(
                    http::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .header(http::header::COOKIE, cookie)
                .body(axum::body::Body::from(form.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn get(
    app_: &Router,
    url: &str,
    cookie: &str,
) -> http::Response<axum::body::Body> {
    let mut builder = axum::extract::Request::builder().method("GET").uri(url);
    if !cookie.is_empty() {
        builder = builder.header(http::header::COOKIE, cookie);
    }
    app_.clone()
        .oneshot(builder.body(axum::body::Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body_of(response: http::Response<axum::body::Body>) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Register, verify the address, and return the login session cookie.
async fn register(pool: &sqlx::PgPool, app_: &Router, handle: &str, email: &str) -> String {
    let form = format!(
        "email={email}&password=correct-horse-battery&password_confirm=correct-horse-battery&_csrf="
    );
    let response = post(app_, "/register", &form, "").await;
    assert_eq!(
        response.status(),
        http::StatusCode::SEE_OTHER,
        "register {handle} must still answer the page, never a 500"
    );
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE email = $1")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let login = post(
        app_,
        "/login",
        &format!("email={email}&password=correct-horse-battery"),
        "",
    )
    .await;
    assert_eq!(login.status(), http::StatusCode::SEE_OTHER, "login {handle}");
    login
        .headers()
        .get_all(http::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or("").to_string())
        .find(|v| v.starts_with("session="))
        .expect("session cookie")
}

/// Every parsed `mail_log` line written for the current month.
fn mail_lines() -> Vec<serde_json::Value> {
    let dir = std::env::var("JJHUB_LOG_DIR").expect("guard sets JJHUB_LOG_DIR");
    let path = std::path::PathBuf::from(dir).join(format!(
        "mail_log_{}.jsonl",
        chrono::Utc::now().format("%Y-%m")
    ));
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("valid JSON line"))
        .collect()
}

/// The single line addressed to `address` (each test uses a unique one).
fn line_for<'a>(lines: &'a [serde_json::Value], address: &str) -> &'a serde_json::Value {
    lines
        .iter()
        .find(|line| line["to"].as_str() == Some(address))
        .unwrap_or_else(|| panic!("no mail_log line for {address}"))
}

/// The env-built arm behind a one-line adapter (a boxed mailer is not
/// itself a `Mailer`); built fresh per send so the guard's env applies.
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

/// Same storage-and-mailer constructor as the other suites; the mailer comes
/// from env so the guard's provider arm is what actually runs.
fn app_with_env_mailer(pool: &sqlx::PgPool) -> (Router, tempfile::TempDir) {
    std::env::set_var("SESSION_SECRET", "delivery-readout-secret");
    let root = tempfile::tempdir().unwrap();
    let app_ = server::app_with_mailer(
        pool.clone(),
        root.path().to_str().unwrap().to_string(),
        Arc::new(EnvMailer),
    );
    (app_, root)
}

#[sqlx::test]
async fn failed_send_records_a_failed_outcome_with_reason(pool: sqlx::PgPool) {
    // Dead relay: the arm tries, fails, and must still be recorded.
    let _guard = EnvGuard::set(&[
        ("JJHUB_EMAIL_PROVIDER", "smtp"),
        ("JJHUB_SMTP_HOST", "127.0.0.1"),
        ("JJHUB_SMTP_PORT", "1"),
    ]);
    let (app, _root) = app_with_env_mailer(&pool);

    register(&pool, &app, "broken", "broken@example.test").await;

    let lines = mail_lines();
    let line = line_for(&lines, "broken@example.test");
    assert_eq!(
        line["outcome"].as_str(),
        Some("failed"),
        "a failed send must be recorded as failed: {line}"
    );
    let reason = line["reason"].as_str().unwrap_or_default();
    assert!(
        !reason.is_empty(),
        "the failure needs a structured reason: {line}"
    );
}

#[sqlx::test]
async fn successful_send_records_an_ok_outcome(pool: sqlx::PgPool) {
    // Unset provider: the in-box LogMailer arm succeeds.
    let _guard = EnvGuard::set(&[]);
    let (app, _root) = app_with_env_mailer(&pool);

    register(&pool, &app, "healthy", "healthy@example.test").await;

    let lines = mail_lines();
    let line = line_for(&lines, "healthy@example.test");
    assert_eq!(
        line["outcome"].as_str(),
        Some("ok"),
        "a delivered send must be recorded as ok: {line}"
    );
    assert!(
        line["ts"].is_string() && line["subject"].is_string(),
        "ts and subject stay on every line: {line}"
    );
}

#[sqlx::test]
async fn operator_admin_lists_the_recent_deliveries(pool: sqlx::PgPool) {
    let _guard = EnvGuard::set(&[]);
    let (app, _root) = app_with_env_mailer(&pool);

    let op_cookie = register(&pool, &app, "op", "op@example.test").await;
    sqlx::query("UPDATE users SET is_operator = true WHERE handle = 'op'")
        .execute(&pool)
        .await
        .unwrap();
    register(&pool, &app, "reader", "reader@example.test").await;

    let response = get(&app, "/admin", &op_cookie).await;
    assert_eq!(response.status(), http::StatusCode::OK, "operator /admin");
    let body = body_of(response).await;
    assert!(
        body.contains("reader@example.test"),
        "the admin readout must list recent deliveries: {body}"
    );
    assert!(
        body.contains("ok"),
        "the readout must show each delivery's outcome: {body}"
    );
}
