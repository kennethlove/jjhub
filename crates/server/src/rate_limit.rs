// story: e20s01 — token-bucket rate limiting: config from env,
// in-memory buckets keyed (action, client), POST /login charged per
// client IP. Middleware does no I/O.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Which policy row charges a request. story: e20s01 ships Login only;
/// e20s02 widens the table to push/create/token.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Login,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    pub per_window: u32,
    pub window_secs: u64,
    pub burst: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub window_secs: u64,
    pub login_ip: Budget,
}

impl RateLimitConfig {
    /// `load` takes the lookup fn so tests never race on process env.
    pub fn load(get: impl Fn(&str) -> Option<String>) -> Self {
        let enabled = get("JJHUB_RATE_LIMIT")
            .map(|v| matches!(v.as_str(), "on" | "1" | "true"))
            .unwrap_or(false);
        let window_secs: u64 = get("JJHUB_RATE_LIMIT_WINDOW")
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600);
        let login_raw = get("JJHUB_RATE_LOGIN_IP").unwrap_or_else(|| "20/3600:5".into());
        RateLimitConfig {
            enabled,
            window_secs,
            login_ip: parse_budget("JJHUB_RATE_LOGIN_IP", &login_raw, window_secs),
        }
    }

    pub fn from_env() -> Self {
        Self::load(|k| std::env::var(k).ok())
    }
}

// story: e20s01 — format N[/window-secs[:burst]]; a malformed row is an
// operator error: fail loud at boot, never silently unlimited.
fn parse_budget(name: &str, value: &str, default_window: u64) -> Budget {
    let fail = || -> ! { panic!("{name} must be N[/window-secs[:burst]], got {value:?}") };
    let (head, tail) = match value.split_once('/') {
        Some((h, t)) => (h, Some(t)),
        None => (value, None),
    };
    let per_window: u32 = head.trim().parse().unwrap_or_else(|_| fail());
    let default_burst = per_window.min(5);
    let (window_secs, burst) = match tail {
        None => (default_window, default_burst),
        Some(t) => {
            let (w, b) = match t.split_once(':') {
                Some((w, b)) => (w, Some(b)),
                None => (t, None),
            };
            let w = w.trim().parse::<u64>().unwrap_or_else(|_| fail());
            let burst = match b {
                Some(b) => b.trim().parse().unwrap_or_else(|_| fail()),
                None => default_burst,
            };
            (w, burst)
        }
    };
    Budget {
        per_window,
        window_secs,
        burst,
    }
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct RateLimiter {
    buckets: Mutex<HashMap<(Action, String), Bucket>>,
    login_ip: Budget,
    idle_window: Duration,
}

impl RateLimiter {
    pub fn new(cfg: RateLimitConfig) -> Self {
        RateLimiter {
            buckets: Mutex::new(HashMap::new()),
            login_ip: cfg.login_ip,
            idle_window: Duration::from_secs(cfg.login_ip.window_secs.max(cfg.window_secs)),
        }
    }

    /// Spend one token for (action, key). Err carries the suggested
    /// retry-after (whole seconds, at least 1).
    pub fn consume(&self, action: Action, key: &str) -> Result<(), Duration> {
        let budget = match action {
            Action::Login => self.login_ip,
        };
        let now = Instant::now();
        let mut buckets = self.buckets.lock().expect("rate-limit bucket lock");
        let bucket = buckets.entry((action, key.to_string())).or_insert(Bucket {
            tokens: budget.burst as f64,
            last: now,
        });
        // Windowed refill: per_window tokens accrue per window_secs,
        // capped at the burst allowance.
        let rate = budget.per_window as f64 / budget.window_secs as f64;
        let elapsed = now.duration_since(bucket.last).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * rate).min(budget.burst as f64);
        bucket.last = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let wait_secs = ((1.0 - bucket.tokens) / rate).ceil().max(1.0);
            Err(Duration::from_secs_f64(wait_secs))
        }
    }

    /// Drop buckets that have been idle for a full window. story:
    /// e20s01 lands the hook; e20s03's sweep calls it.
    pub fn prune_idle(&self) {
        let mut buckets = self.buckets.lock().expect("rate-limit bucket lock");
        buckets.retain(|_, b| b.last.elapsed() < self.idle_window);
    }
}

/// story: e20s01 — charge POST /login per client IP. Mounted only on
/// the /login route; GET never charges. AppState.limiter is None in
/// every test constructor: the handler's behavior is unchanged.
/// Connect-info is read as an extension, not an extractor: oneshot
/// requests (existing tests) carry none, and a rejected extractor
/// would 500 /login even with the limiter off.
pub async fn rate_limit_mw(
    State(state): State<crate::AppState>,
    req: Request,
    next: Next,
) -> Response {
    if let Some(limiter) = state.rate_limiter.as_ref() {
        if req.method() == Method::POST {
            // story: e20s01 — no connect info: share one bucket; the
            // production serve always supplies it.
            let ip = req
                .extensions()
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|c| c.0.ip().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            if let Err(retry) = limiter.consume(Action::Login, &ip) {
                let secs = retry.as_secs().max(1);
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    [(header::RETRY_AFTER, secs.to_string())],
                    "too many requests",
                )
                    .into_response();
            }
        }
    }
    next.run(req).await
}

pub fn share(limiter: RateLimiter) -> Arc<RateLimiter> {
    Arc::new(limiter)
}
