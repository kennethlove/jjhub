//! story: e23s01 — outbound email: a small provider-agnostic facade.
//!
//! Production is Resend (https://resend.com, REST /emails).
//! Development is messagepit's SendGrid-compatible stub
//! (https://github.com/coreydaley/messagepit, POST /v3/mail/send).
//! The fallback is LogMailer so tests never need a network.

/// The provider contract — one method, one outcome.
#[async_trait::async_trait]
pub trait Mailer: Send + Sync {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), MailError>;
}

#[derive(Debug)]
pub struct MailError {
    pub reason: String,
}

fn mail_error(reason: String) -> MailError {
    MailError { reason }
}

/// The dev-time fallback: nothing leaves the box.
pub struct LogMailer;

#[async_trait::async_trait]
impl Mailer for LogMailer {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), MailError> {
        tracing::info!(%to, %subject, %body, "email (log)");
        Ok(())
    }
}

/// Production provider — Resend.
pub struct ResendMailer {
    api_key: String,
    from: String,
}

#[async_trait::async_trait]
impl Mailer for ResendMailer {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), MailError> {
        let client = reqwest::Client::new();
        let response = client
            .post("https://api.resend.com/emails")
            .bearer_auth(&self.api_key)
            .json(&build_resend_payload(&self.from, to, subject, body))
            .send()
            .await
            .map_err(|e| mail_error(e.to_string()))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(mail_error(format!("resend: {}", response.status())))
        }
    }
}

/// Dev provider — messagepit's SendGrid-compatible stub.
pub struct SendGridMailer {
    base_url: String,
    api_key: String,
    from: String,
}

#[async_trait::async_trait]
impl Mailer for SendGridMailer {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), MailError> {
        let client = reqwest::Client::new();
        let mut request = client
            .post(format!(
                "{}/v3/mail/send",
                self.base_url.trim_end_matches('/')
            ))
            .json(&build_messagepit_payload(&self.from, to, subject, body));
        if !self.api_key.is_empty() {
            request = request.bearer_auth(&self.api_key);
        }
        let response = request
            .send()
            .await
            .map_err(|e| mail_error(e.to_string()))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(mail_error(format!("messagepit: {}", response.status())))
        }
    }
}

/// story: e35s01 — one wrapper around every provider: the single write
/// point for the mail rollup. Recipient and subject only, never the body
/// (bodies carry verification and reset tokens).
struct LoggedMailer(Box<dyn Mailer>);

#[async_trait::async_trait]
impl Mailer for LoggedMailer {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), MailError> {
        // story: e36s01 — the outcome rides the same line, recorded at the
        // single write point AFTER the send returns: the readout is the
        // fact, not the intention. Recipient and subject only, never the
        // body (bodies carry verification and reset tokens).
        let result = self.0.send(to, subject, body).await;
        let mut fields = serde_json::Map::new();
        fields.insert("to".into(), to.into());
        fields.insert("subject".into(), subject.into());
        match &result {
            Ok(()) => {
                fields.insert("outcome".into(), "ok".into());
            }
            Err(err) => {
                fields.insert("outcome".into(), "failed".into());
                fields.insert("reason".into(), err.reason.as_str().into());
            }
        }
        let _ = crate::retention::append("mail_log", chrono::Utc::now(), &fields);
        result
    }
}

/// story: e23s01 — env chooses the provider; log is the safe default.
pub fn from_env() -> Box<dyn Mailer> {
    let from = std::env::var("JJHUB_MAIL_FROM").unwrap_or_else(|_| "jjhub@localhost".into());
    let inner: Box<dyn Mailer> = match std::env::var("JJHUB_EMAIL_PROVIDER").as_deref() {
        Ok("resend") => {
            let api_key = std::env::var("RESEND_API_KEY").expect("RESEND_API_KEY for resend");
            Box::new(ResendMailer { api_key, from })
        }
        Ok("messagepit") => {
            let base_url = std::env::var("JJHUB_MESSAGEPIT_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8100".into());
            let api_key = std::env::var("MP_SENDGRID_API_KEY").unwrap_or_default();
            Box::new(SendGridMailer {
                base_url,
                api_key,
                from,
            })
        }
        // story: e34s03 — the relay arm exists ONLY on this explicit opt-in;
        // unset and unknown providers keep the in-box LogMailer.
        Ok("smtp") => Box::new(SmtpMailer::from_env()),
        _ => Box::new(LogMailer),
    };
    Box::new(LoggedMailer(inner))
}

/// story: e34s03 — SMTP transport configured by JJHUB_SMTP_* env only.
pub struct SmtpMailer {
    host: String,
    port: u16,
    username: String,
    password: String,
    from: String,
}

impl SmtpMailer {
    fn from_env() -> Self {
        SmtpMailer {
            host: std::env::var("JJHUB_SMTP_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            port: std::env::var("JJHUB_SMTP_PORT")
                .ok()
                .and_then(|port| port.parse().ok())
                .unwrap_or(587),
            username: std::env::var("JJHUB_SMTP_USERNAME").unwrap_or_default(),
            password: std::env::var("JJHUB_SMTP_PASSWORD").unwrap_or_default(),
            from: std::env::var("JJHUB_MAIL_FROM").unwrap_or_else(|_| "jjhub@localhost".into()),
        }
    }
}

/// story: SMTP TLS mode — 465 = SMTPS (lettre `relay`, TLS wrapper), any
/// other port = STARTTLS (`starttls_relay`, required upgrade).
/// WHY: `relay()` is implicit TLS only; pairing it with the 587 default
/// attempted a TLS handshake against STARTTLS ports and never connected.
pub fn smtp_transport(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
) -> Result<lettre::AsyncSmtpTransport<lettre::Tokio1Executor>, MailError> {
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, Tokio1Executor};

    let builder = if port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
    }
    .map_err(|err| MailError {
        reason: format!("smtp relay: {err}"),
    })?
    .port(port);
    let builder = if username.is_empty() {
        builder.build()
    } else {
        builder
            .credentials(Credentials::new(username.into(), password.into()))
            .build()
    };
    Ok(builder)
}

#[async_trait::async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), MailError> {
        use lettre::AsyncTransport;

        let relay = smtp_transport(&self.host, self.port, &self.username, &self.password)?;
        let message = lettre::message::Message::builder()
            .from(
                self.from
                    .parse::<lettre::message::Mailbox>()
                    .map_err(|err| MailError {
                        reason: format!("smtp from: {err}"),
                    })?,
            )
            .to(to
                .parse::<lettre::message::Mailbox>()
                .map_err(|err| MailError {
                    reason: format!("smtp to: {err}"),
                })?)
            .subject(subject)
            .body(body.to_string())
            .map_err(|err| MailError {
                reason: format!("smtp message: {err}"),
            })?;
        relay.send(message).await.map_err(|err| MailError {
            reason: format!("smtp send: {err}"),
        })?;
        Ok(())
    }
}

/// Resend's JSON shape.
pub fn build_resend_payload(from: &str, to: &str, subject: &str, body: &str) -> serde_json::Value {
    serde_json::json!({
        "from": from,
        "to": [to],
        "subject": subject,
        "text": body,
    })
}

/// SendGrid v3 shape — what messagepit accepts.
pub fn build_messagepit_payload(
    from: &str,
    to: &str,
    subject: &str,
    body: &str,
) -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::from_iter(vec![
        (
            "personalizations".into(),
            serde_json::json!([{"to": [{"email": to}]}]),
        ),
        ("from".into(), serde_json::json!({"email": from})),
        ("subject".into(), serde_json::Value::String(subject.into())),
        (
            "content".into(),
            serde_json::json!([{"type": "text/plain", "value": body}]),
        ),
    ]))
}

/// The one link the user clicks — minted once, verified once.
pub struct VerificationToken {
    pub token: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

const VERIFY_TTL_HOURS: i64 = 24;

/// Generate a verification token with a 24-hour expiry.
/// WHY sha256 (not argon2): the token is 256 bits of entropy from the
/// server — nothing to brute-force, and lookup must be an index hit
/// (e22s02 lesson).
pub fn new_verification_token() -> VerificationToken {
    let bytes = rand::random::<[u8; 32]>();
    let plaintext = format!(
        "vfy_{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    VerificationToken {
        token: plaintext,
        expires_at: chrono::Utc::now() + chrono::Duration::hours(VERIFY_TTL_HOURS),
    }
}

/// Store the token hashed, return its plaintext form.
pub async fn send_verification(pool: &sqlx::PgPool, user_id: uuid::Uuid) -> VerificationToken {
    let verification = new_verification_token();
    let mut tx = pool.begin().await.expect("begin verification");
    sqlx::query("DELETE FROM email_verifications WHERE user_id = $1 AND used_at IS NULL")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .expect("delete pending verification");
    sqlx::query(
        "INSERT INTO email_verifications (user_id, token_hash, expires_at) VALUES ($1, $2, $3)",
    )
    .bind(user_id)
    .bind(hash_token(&verification.token))
    .bind(verification.expires_at)
    .execute(&mut *tx)
    .await
    .expect("insert verification");
    tx.commit().await.expect("commit verification");
    verification
}

/// story: e22s02 — hash, never store plaintext.
fn hash_token(token: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    Sha256::digest(token.as_bytes()).to_vec()
}

/// story: e23s01 — consume the link: mark used, mark user verified.
pub async fn verify_email(pool: &sqlx::PgPool, token: &str) -> Option<uuid::Uuid> {
    let hash = hash_token(token);
    let row = sqlx::query_as::<_, (uuid::Uuid,)>(
        "UPDATE email_verifications SET used_at = now()
         WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()
         RETURNING user_id",
    )
    .bind(&hash)
    .fetch_optional(pool)
    .await
    .ok()??;
    let user_id = row.0;
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .expect("mark verified");
    Some(user_id)
}

/// The link body the mail carries.
/// story: e34s01 — path form; the e23 query route keeps accepting ?token=.
pub fn verify_link(token: &str) -> String {
    format!("/verify/{token}")
}

/// The origin (scheme://host) mail links must carry: a recipient clicks
/// from an inbox, where no page context exists.
/// WHY: absolute URLs need a host known at send time — the request that
/// triggered the send is the only place it lives (precedent: the reset
/// link in routes.rs). X-Forwarded-Proto keeps a TLS-terminating proxy
/// honest; plain http keeps local dev working.
pub fn origin(headers: &axum::http::HeaderMap) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .filter(|scheme| *scheme == "https")
        .unwrap_or("http");
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost:3000");
    format!("{scheme}://{host}")
}

/// The refusal shown when a link is dead.
pub const LINK_REFUSED: &str =
    "That confirmation link has expired or was already used. Sign in to get a fresh one.";
