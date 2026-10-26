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

/// Which policy row charges a request. story: e20s02 widens the table
/// beyond Login to the policy-table rows: push, create, token.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Login,
    Push,
    Create,
    Token,
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
    pub push_user: Budget,
    pub push_ip: Budget,
    pub create_user: Budget,
    pub create_ip: Budget,
    pub token_user: Budget,
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
        let push_user_raw = get("JJHUB_RATE_PUSH_USER").unwrap_or_else(|| "30/3600:10".into());
        let push_ip_raw = get("JJHUB_RATE_PUSH_IP").unwrap_or_else(|| "30/3600:10".into());
        let create_user_raw = get("JJHUB_RATE_CREATE_USER").unwrap_or_else(|| "10/3600:3".into());
        let create_ip_raw = get("JJHUB_RATE_CREATE_IP").unwrap_or_else(|| "10/3600:3".into());
        let token_user_raw = get("JJHUB_RATE_TOKEN_USER").unwrap_or_else(|| "10/3600:5".into());
        RateLimitConfig {
            enabled,
            window_secs,
            login_ip: parse_budget("JJHUB_RATE_LOGIN_IP", &login_raw, window_secs),
            push_user: parse_budget("JJHUB_RATE_PUSH_USER", &push_user_raw, window_secs),
            push_ip: parse_budget("JJHUB_RATE_PUSH_IP", &push_ip_raw, window_secs),
            create_user: parse_budget("JJHUB_RATE_CREATE_USER", &create_user_raw, window_secs),
            create_ip: parse_budget("JJHUB_RATE_CREATE_IP", &create_ip_raw, window_secs),
            token_user: parse_budget("JJHUB_RATE_TOKEN_USER", &token_user_raw, window_secs),
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

/// Clone shares the bucket map: main builds one limiter and shares it
/// with both transports (story: e20s03 — one policy row, two
/// transports needs ONE counter).
#[derive(Clone)]
pub struct RateLimiter {
    buckets: Arc<Mutex<HashMap<(Action, String), Bucket>>>,
    budgets: RateLimitConfig,
    idle_window: Duration,
}

impl RateLimiter {
    pub fn new(cfg: RateLimitConfig) -> Self {
        // story: e20s03 — a bucket may idle only as long as its LONGEST
        // policy window: pruning earlier would hand out a fresh full
        // burst sooner than the row's window allows. (Spec e20s03 §6:
        // pruned after the window — the row's own window, not a fixed
        // floor; the old 3600s floor made short windows unprunable.)
        let idle_window = Duration::from_secs(
            cfg.window_secs
                .max(cfg.login_ip.window_secs)
                .max(cfg.push_user.window_secs)
                .max(cfg.push_ip.window_secs)
                .max(cfg.create_user.window_secs)
                .max(cfg.create_ip.window_secs)
                .max(cfg.token_user.window_secs),
        );
        RateLimiter {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            budgets: cfg,
            idle_window,
        }
    }

    /// story: e20s03 — one env-built limiter shared by the HTTP app and
    /// the SSH listener (same bucket map; a fresh map per transport
    /// would double every budget).
    pub fn limiter_from_env() -> Option<RateLimiter> {
        let cfg = RateLimitConfig::from_env();
        cfg.enabled.then(|| RateLimiter::new(cfg))
    }

    /// Spend one token for (action, key). Err carries the suggested
    /// retry-after (whole seconds, at least 1).
    pub fn consume(&self, action: Action, key: &str) -> Result<(), Duration> {
        // story: e20s03 — prune on access: the map stays bounded with
        // no timer task.
        self.prune_idle();
        let budget = match action {
            Action::Login => self.budgets.login_ip,
            Action::Push => self.budgets.push_user,
            // story: e20s02 — push and create keep their user AND ip
            // rows; each dimension is its own (action, key) bucket, so
            // one consume() call charges exactly one row.
            Action::Create => self.budgets.create_user,
            Action::Token => self.budgets.token_user,
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

    /// story: e20s02 — check-then-charge for the two-dimension rows
    /// (push, create): both the user row and the IP row must pass, and
    /// only then are both charged — a failed check charges nothing, and
    /// one lock makes the pair atomic. Token has no IP row in the policy
    /// table: use `consume` for it.
    pub fn try_charge(&self, action: Action, user_key: &str, ip_key: &str) -> Result<(), Duration> {
        // story: e20s03 — prune on access: the map stays bounded with
        // no timer task.
        self.prune_idle();
        let (user_budget, ip_budget) = match action {
            Action::Push => (self.budgets.push_user, self.budgets.push_ip),
            Action::Create => (self.budgets.create_user, self.budgets.create_ip),
            Action::Login => (self.budgets.login_ip, self.budgets.login_ip),
            Action::Token => (self.budgets.token_user, self.budgets.token_user),
        };
        let now = Instant::now();
        let mut buckets = self.buckets.lock().expect("rate-limit bucket lock");
        let mut fill = |budget: Budget, key: &str| -> Duration {
            let rate = budget.per_window as f64 / budget.window_secs as f64;
            let bucket = buckets.entry((action, key.to_string())).or_insert(Bucket {
                tokens: budget.burst as f64,
                last: now,
            });
            let elapsed = now.duration_since(bucket.last).as_secs_f64();
            bucket.tokens = (bucket.tokens + elapsed * rate).min(budget.burst as f64);
            bucket.last = now;
            if bucket.tokens >= 1.0 {
                Duration::ZERO
            } else {
                Duration::from_secs_f64(((1.0 - bucket.tokens) / rate).ceil().max(1.0))
            }
        };
        let wait = fill(user_budget, user_key).max(fill(ip_budget, ip_key));
        if wait == Duration::ZERO {
            // both rows checked out a token — debit them.
            for key in [user_key, ip_key] {
                let bucket = buckets
                    .get_mut(&(action, key.to_string()))
                    .expect("bucket just filled");
                bucket.tokens -= 1.0;
            }
            Ok(())
        } else {
            Err(wait)
        }
    }

    /// Drop buckets that have been idle for a full window and report
    /// how many went. story: e20s01 lands the hook; e20s03 wires it
    /// into every charge (prune on access) and tests the count.
    pub fn prune_idle(&self) -> usize {
        let mut buckets = self.buckets.lock().expect("rate-limit bucket lock");
        let before = buckets.len();
        buckets.retain(|_, b| b.last.elapsed() < self.idle_window);
        before - buckets.len()
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
            let ip = peer_ip(req.extensions());
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

/// story: e20s02 — the client IP for a request: its connect-info
/// extension when the server supplied one (production: always), or a
/// shared "unknown" bucket for extension-less callers (oneshot tests).
pub fn peer_ip(extensions: &axum::http::Extensions) -> String {
    extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// story: e20s02 — opt-in extractor form of `peer_ip` for handlers that
/// need to charge an IP dimension. Never rejects: a missing connect-info
/// falls back to the shared bucket, so oneshot tests keep working.
pub struct OptionalPeerIp(pub String);

impl<S: Sync> axum::extract::FromRequestParts<S> for OptionalPeerIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self(peer_ip(&parts.extensions)))
    }
}

/// story: e20s02 — one shape for every rate-limit refusal: 429, a
/// Retry-After in whole seconds, plain text. Used by the login
/// middleware and the direct charge sites in the transport and route
/// handlers.
pub fn too_many_requests(retry: Duration) -> Response {
    let secs = retry.as_secs().max(1);
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, secs.to_string())],
        "too many requests",
    )
        .into_response()
}
