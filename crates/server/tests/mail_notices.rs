//! RED suite for e34s03: moderation notices and the explicit SMTP arm.
//!
//! Written against the story's contract BEFORE the implementation: no notice
//! hooks exist and JJHUB_EMAIL_PROVIDER=smtp is ignored (the `_` arm keeps
//! returning LogMailer), so cases here fail until GREEN wires the four
//! moderation writes and the lettre arm.

use std::sync::{Arc, LazyLock, Mutex};

use axum::http;
use axum::Router;
use tower::util::ServiceExt;

// --- env guard: provider selection must serialize across tests ---

static ENV: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn set(vars: &[(&'static str, &str)]) -> Self {
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
        for (k, v) in vars {
            std::env::set_var(k, v);
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

// --- recorded-mailer scaffold (shape copied from email_confirmation.rs) ---

#[derive(Clone)]
struct Recorded {
    mails: Arc<Mutex<Vec<(String, String, String)>>>,
}

impl Recorded {
    fn new() -> Self {
        Recorded {
            mails: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Drop registration's verification mail so counts hold notices only.
    fn clear(&self) {
        self.mails.lock().unwrap().clear();
    }

    fn all(&self) -> Vec<(String, String, String)> {
        self.mails.lock().unwrap().clone()
    }
}

struct RecordingMailer(Recorded);

#[async_trait::async_trait]
impl server::email::Mailer for RecordingMailer {
    async fn send(
        &self,
        to: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), server::email::MailError> {
        self.0
            .mails
            .lock()
            .unwrap()
            .push((to.to_string(), subject.to_string(), body.to_string()));
        Ok(())
    }
}

/// Percent-encode the few characters a reason carries through a urlencoded
/// body, so the form arrives byte-identical to what the operator typed.
fn form_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            ' ' => out.push('+'),
            '&' => out.push_str("%26"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '"' => out.push_str("%22"),
            '\'' => out.push_str("%27"),
            '=' => out.push_str("%3D"),
            '%' => out.push_str("%25"),
            _ => out.push(c),
        }
    }
    out
}

fn csrf(cookie: &str) -> String {
    server::session::csrf_token(
        b"mail-notices-secret",
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

async fn register(pool: &sqlx::PgPool, app_: &Router, handle: &str, email: &str) {
    let form = format!("email={email}&password=correct-horse-battery&password_confirm=correct-horse-battery&_csrf=");
    let response = post(app_, "/register", &form, "").await;
    assert_eq!(
        response.status(),
        http::StatusCode::SEE_OTHER,
        "register {handle} failed"
    );
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE email = $1")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET is_operator = true WHERE handle = 'alice'")
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
    assert_eq!(
        login.status(),
        http::StatusCode::SEE_OTHER,
        "login {handle}"
    );
}

async fn suspend_actions_count(pool: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM moderation_actions WHERE action = 'suspend'")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// One recorded app per test: real storage (takedown needs a repo) and the
/// recording mailer in the same constructor.
async fn app(pool: &sqlx::PgPool, recorded: &Recorded) -> (Router, tempfile::TempDir) {
    // Same value every time, never removed — csrf minting and the server's
    // session verification must agree.
    std::env::set_var("SESSION_SECRET", "mail-notices-secret");
    let root = tempfile::tempdir().unwrap();
    let app_ = server::app_with_mailer(
        pool.clone(),
        root.path().to_str().unwrap().to_string(),
        Arc::new(RecordingMailer(recorded.clone())),
    );
    (app_, root)
}

#[sqlx::test]
async fn each_action_notices_affected_user_once_with_escaped_reason(pool: sqlx::PgPool) {
    let recorded = Recorded::new();
    let (app, _root) = app(&pool, &recorded).await;
    register(&pool, &app, "alice", "alice@example.test").await;
    register(&pool, &app, "bob", "bob@example.test").await;
    recorded.clear();
    let _ = &app;

    // Sessions: log in again and capture the Set-Cookie header.
    let login = |email: &'static str| {
        let app_ = app.clone();
        async move {
            app_.oneshot(
                axum::extract::Request::builder()
                    .method("POST")
                    .uri("/login")
                    .header(
                        http::header::CONTENT_TYPE,
                        "application/x-www-form-urlencoded",
                    )
                    .body(axum::body::Body::from(format!(
                        "email={email}&password=correct-horse-battery"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };
    let cookie_for = |response: http::Response<axum::body::Body>| -> String {
        response
            .headers()
            .get_all(http::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .map(|v| v.split(';').next().unwrap_or("").to_string())
            .find(|v| v.starts_with("session="))
            .expect("session cookie")
    };
    let alice_cookie = cookie_for(login("alice@example.test").await);
    let alice_csrf = csrf(&alice_cookie);
    let bob_cookie = cookie_for(login("bob@example.test").await);
    let bob_csrf = csrf(&bob_cookie);

    // Bob owns a repository so takedown/restore notices target him too.
    let create = post(
        &app,
        "/repos",
        &format!("name=doxrepo&visibility=public&_csrf={bob_csrf}"),
        &bob_cookie,
    )
    .await;
    assert_eq!(
        create.status(),
        http::StatusCode::SEE_OTHER,
        "create repo: {}",
        create.status()
    );

    let reason = "<b>loud</b> & clear";
    let encoded = form_encode(reason);

    let r1 = post(
        &app,
        "/admin/suspend",
        &format!("target=bob&reason={encoded}&_csrf={alice_csrf}"),
        &alice_cookie,
    )
    .await;
    assert_eq!(r1.status(), http::StatusCode::SEE_OTHER, "suspend");
    let r2 = post(
        &app,
        "/admin/unsuspend",
        &format!("target=bob&reason={encoded}&_csrf={alice_csrf}"),
        &alice_cookie,
    )
    .await;
    assert_eq!(r2.status(), http::StatusCode::SEE_OTHER, "unsuspend");
    let r3 = post(
        &app,
        "/admin/takedown",
        &format!("owner=bob&name=doxrepo&reason={encoded}&_csrf={alice_csrf}"),
        &alice_cookie,
    )
    .await;
    assert_eq!(r3.status(), http::StatusCode::SEE_OTHER, "takedown");
    let r4 = post(
        &app,
        "/admin/takedown/restore",
        &format!("owner=bob&name=doxrepo&reason={encoded}&_csrf={alice_csrf}"),
        &alice_cookie,
    )
    .await;
    assert_eq!(r4.status(), http::StatusCode::SEE_OTHER, "restore");

    let all = recorded.all();
    let to_bob: Vec<&(String, String, String)> = all
        .iter()
        .filter(|(to, _, _)| to == "bob@example.test")
        .collect();
    let to_alice = all
        .iter()
        .filter(|(to, _, _)| to == "alice@example.test")
        .count();

    assert_eq!(
        to_bob.len(),
        4,
        "exactly one notice per action to the affected user, got {:?}",
        to_bob
            .iter()
            .map(|(_, s, _)| s.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(to_alice, 0, "the operator must not be notified");

    // Four distinct action templates, each quoting the reason as TEXT, never
    // as live markup.
    for (_, _, body) in &to_bob {
        assert!(
            // Tera escapes `/` too: `&lt;&#x2F;b&gt;` decodes to `</b>`.
            body.contains("&lt;b&gt;loud&lt;&#x2F;b&gt; &amp; clear"),
            "reason not escaped verbatim: {body}"
        );
        assert!(
            !body.contains("<b>loud</b>"),
            "raw markup leaked into the notice: {body}"
        );
    }
    let mut bodies: Vec<&str> = to_bob.iter().map(|(_, _, b)| b.as_str()).collect();
    bodies.sort_unstable();
    bodies.dedup();
    assert_eq!(bodies.len(), 4, "each action must carry its own template");
}

#[sqlx::test]
async fn smtp_arm_is_selected_only_by_explicit_env(_pool: sqlx::PgPool) {
    // Unset: the in-box arm, no dial.
    {
        let _guard = EnvGuard::set(&[]);
        let outcome = server::email::from_env()
            .send("someone@example.test", "probe", "probe")
            .await;
        assert!(outcome.is_ok(), "unset provider must stay in-box");
    }

    // Unknown provider: same in-box arm (no implicit choice).
    {
        let _guard = EnvGuard::set(&[("JJHUB_EMAIL_PROVIDER", "purple-smoke")]);
        let outcome = server::email::from_env()
            .send("someone@example.test", "probe", "probe")
            .await;
        assert!(outcome.is_ok(), "unknown provider must fall back in-box");
    }

    // Explicit smtp: the arm must try the configured relay (dead local port
    // — config parse proven, no mail server needed).
    {
        let _guard = EnvGuard::set(&[
            ("JJHUB_EMAIL_PROVIDER", "smtp"),
            ("JJHUB_SMTP_HOST", "127.0.0.1"),
            ("JJHUB_SMTP_PORT", "1"),
        ]);
        let outcome = server::email::from_env()
            .send("someone@example.test", "probe", "probe")
            .await;
        assert!(
            outcome.is_err(),
            "smtp env must select the relay arm, not the log arm"
        );
    }
}

#[sqlx::test]
async fn a_dead_relay_never_fails_the_action_or_its_row(pool: sqlx::PgPool) {
    let recorded = Recorded::new();
    let _guard = EnvGuard::set(&[
        ("JJHUB_EMAIL_PROVIDER", "smtp"),
        ("JJHUB_SMTP_HOST", "127.0.0.1"),
        ("JJHUB_SMTP_PORT", "1"),
    ]);
    // Plain app: its mailer comes from env at construction — the dead-relay
    // arm, not the recorder.
    let (app, _root) = {
        let root = tempfile::tempdir().unwrap();
        (
            server::app_with_mailer(
                pool.clone(),
                root.path().to_str().unwrap().to_string(),
                Arc::new(RecordingMailer(recorded.clone())),
            ),
            root,
        )
    };
    let _ = &app;

    // The dead-relay path needs the env-built app: build it AFTER the guard.
    std::env::set_var("SESSION_SECRET", "mail-notices-secret");
    let app = server::app(pool.clone());
    register(&pool, &app, "alice", "alice@example.test").await;
    register(&pool, &app, "bob", "bob@example.test").await;

    let login = |email: &'static str| {
        let app_ = app.clone();
        async move {
            app_.oneshot(
                axum::extract::Request::builder()
                    .method("POST")
                    .uri("/login")
                    .header(
                        http::header::CONTENT_TYPE,
                        "application/x-www-form-urlencoded",
                    )
                    .body(axum::body::Body::from(format!(
                        "email={email}&password=correct-horse-battery"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };
    let cookie_for = |response: http::Response<axum::body::Body>| -> String {
        response
            .headers()
            .get_all(http::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .map(|v| v.split(';').next().unwrap_or("").to_string())
            .find(|v| v.starts_with("session="))
            .expect("session cookie")
    };
    let alice_cookie = cookie_for(login("alice@example.test").await);
    let alice_csrf = csrf(&alice_cookie);

    let response = post(
        &app,
        "/admin/suspend",
        &format!("target=bob&reason=relay+down&_csrf={alice_csrf}"),
        &alice_cookie,
    )
    .await;
    assert_eq!(
        response.status(),
        http::StatusCode::SEE_OTHER,
        "a dead relay must never fail the action"
    );
    assert_eq!(suspend_actions_count(&pool).await, 1, "the row must stand");
}

#[sqlx::test]
async fn unset_provider_keeps_every_notice_in_box(_pool: sqlx::PgPool) {
    let _guard = EnvGuard::set(&[]);
    let outcome = server::email::from_env()
        .send("someone@example.test", "probe", "probe")
        .await;
    assert!(
        outcome.is_ok(),
        "unset JJHUB_EMAIL_PROVIDER must keep the in-box arm"
    );
}
