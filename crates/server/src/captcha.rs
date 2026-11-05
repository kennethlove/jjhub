// story: e31s01 — env-shaped verifier, like mail: both keys set =
// challenge on, unset = signup behaves exactly as before.

const DEFAULT_SITEVERIFY: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";

pub struct Verifier {
    site_key: String,
    secret: String,
    endpoint: String,
}

impl Verifier {
    pub fn from_env() -> Option<Self> {
        let site_key = std::env::var("JJHUB_CAPTCHA_SITE_KEY").ok()?;
        let secret = std::env::var("JJHUB_CAPTCHA_SECRET_KEY").ok()?;
        if site_key.is_empty() || secret.is_empty() {
            return None;
        }
        let endpoint = std::env::var("JJHUB_CAPTCHA_SITEVERIFY_URL")
            .unwrap_or_else(|_| DEFAULT_SITEVERIFY.to_string());
        Some(Self {
            site_key,
            secret,
            endpoint,
        })
    }

    pub fn site_key(&self) -> &str {
        &self.site_key
    }

    // WHY: fail closed (story risk P1) — configured and rejected or
    // unreachable must refuse; never admit an unverifiable token.
    // The timeout keeps a hanging endpoint off the signup path.
    pub async fn verify(&self, token: &str) -> bool {
        let Ok(client) = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
        else {
            return false;
        };
        let Ok(response) = client
            .post(&self.endpoint)
            .form(&[("secret", self.secret.as_str()), ("response", token)])
            .send()
            .await
        else {
            return false;
        };
        let Ok(payload) = response.json::<serde_json::Value>().await else {
            return false;
        };
        payload
            .get("success")
            .and_then(|success| success.as_bool())
            .unwrap_or(false)
    }
}

/// Widget sitekey for the form, present only when the verifier is
/// fully configured — a half-configured node must not show a
/// challenge it never checks.
pub fn widget_sitekey() -> Option<String> {
    Verifier::from_env().map(|verifier| verifier.site_key().to_string())
}
