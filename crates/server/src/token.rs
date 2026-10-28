// story: e01s02
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use sqlx::PgPool;
use uuid::Uuid;

pub const TOKEN_PREFIX: &str = "jjh_";

pub struct NewToken {
    pub id: Uuid,
    pub plaintext: String,
}

/// story: e22s02 — sha256 of the presented value; tokens are CSPRNG,
/// so this is the stored verification secret (see 0013 migration WHY).
fn lookup_key(plaintext: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    Sha256::digest(plaintext.as_bytes()).to_vec()
}

pub fn hash(token: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(token.as_bytes(), &salt)
        .expect("argon2 hash")
        .to_string()
}

pub fn generate() -> (String, String) {
    let bytes = rand::random::<[u8; 24]>();
    let plaintext = format!("{TOKEN_PREFIX}{}", hex::encode(bytes));
    let prefix = plaintext.chars().take(TOKEN_PREFIX.len() + 6).collect();
    (plaintext, prefix)
}

/// story: e22s03 — mint with explicit permissions; scopes/expiry/
/// repo pin come from the form or the caller's policy, never defaults.
pub async fn create_with(
    pool: &PgPool,
    user_id: Uuid,
    label: &str,
    scopes: &[&str],
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    repo_id: Option<Uuid>,
    allowed_inets: Option<Vec<std::net::IpAddr>>,
) -> NewToken {
    let (plaintext, prefix) = generate();
    let token_hash = hash(&plaintext);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO tokens (user_id, label, prefix, token_hash, lookup_hash, scopes, expires_at, repo_id, allowed_inets) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::inet[]) RETURNING id",
    )
    .bind(user_id)
    .bind(label)
    .bind(&prefix)
    .bind(&token_hash)
    .bind(lookup_key(&plaintext))
    .bind(scopes)
    .bind(expires_at)
    .bind(repo_id)
    .bind(
        allowed_inets
            .as_ref()
            .map(|v| v.iter().map(std::net::IpAddr::to_string).collect::<Vec<_>>()),
    )
    .fetch_one(pool)
    .await
    .expect("insert token");
    NewToken { id, plaintext }
}

/// story: e01s02 — the unadorned mint: read+write, no expiry, no pin.
pub async fn create(pool: &PgPool, user_id: Uuid, label: &str) -> NewToken {
    create_with(pool, user_id, label, &["read", "write"], None, None, None).await
}

/// story: e22s02 — the transport authz bundle: identity PLUS the
/// restriction columns the shared admission decision enforces.
pub struct TransportAuth {
    pub user_id: Uuid,
    pub token_id: Uuid,
    pub scopes: Vec<String>,
    pub pinned_repo: Option<Uuid>,
    /// IPv4/6 allowlist; None = any client IP.
    pub allowed_inets: Option<Vec<std::net::IpAddr>>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// story: e01s02 — resolve a presented token value to its owning user, or None.
/// Compares by argon2-verifying candidates; MVP scale (few tokens per user).
/// story: e22s02 — one authentication stage now also enforces expiry
/// (in SQL, same stage as revoked) and the IP allowlist (fail-closed:
/// a listed token from an unverifiable or off-list client is None).
/// WHY argon2 full-scan kept: salts differ per row, so lookup can
/// never be an equality match; expiry filtering only narrows the scan.
// WHY type alias: clippy's type_complexity — the row shape stays inline
// once; a named struct would need FromRow derive for one caller.
type AuthRow = (
    Uuid,
    Uuid,
    Vec<String>,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<Uuid>,
    Option<String>,
    String,
);

pub async fn authenticate(
    pool: &PgPool,
    plaintext: &str,
    client_ip: Option<std::net::IpAddr>,
) -> Option<TransportAuth> {
    let rows: Vec<AuthRow> = sqlx::query_as(
        "SELECT t.id, t.user_id, t.scopes, t.expires_at, t.repo_id, t.allowed_inets::text, t.token_hash FROM tokens t WHERE t.revoked_at IS NULL AND t.lookup_hash IS NULL AND (t.expires_at IS NULL OR t.expires_at > now())",
    )
    .fetch_all(pool)
    .await
    .ok()?;
    // story: e22s02 — fast path first: an indexed exact match on the
    // sha256 lookup key finds the row without touching argon2; the
    // verify-scan below only ever sees legacy rows (lookup_hash NULL).
    let found = match sqlx::query_as::<_, AuthRow>(
        "SELECT t.id, t.user_id, t.scopes, t.expires_at, t.repo_id, t.allowed_inets::text, t.token_hash FROM tokens t WHERE t.revoked_at IS NULL AND (t.expires_at IS NULL OR t.expires_at > now()) AND t.lookup_hash = $1",
    )
    .bind(lookup_key(plaintext))
    .fetch_optional(pool)
    .await
    .ok()?
    {
        Some(row) => Some(row),
        // WHY spawn_blocking: argon2-verifying every active token is
        // synchronous CPU. On the async workers it stalled accept/route
        // tasks for seconds per request once rows piled up — the
        // shared-DB e2e wedged the whole server until the scan moved
        // to the blocking pool.
        None => {
            let plaintext = plaintext.to_string();
            tokio::task::spawn_blocking(move || {
                rows.into_iter()
                    .find(|(_, _, _, _, _, _, token_hash)| verify(&plaintext, token_hash))
            })
            .await
            .ok()
            .flatten()
        }
    };
    let (token_id, user_id, scopes, expires_at, pinned_repo, allowed_text, _) = found?;
    let allowed_inets = allowed_text.as_deref().map(parse_inet_list);
    if let Some(list) = allowed_inets.as_ref() {
        let on_list = client_ip.is_some_and(|ip| list.contains(&ip));
        if !on_list {
            return None;
        }
    }
    Some(TransportAuth {
        user_id,
        token_id,
        scopes,
        pinned_repo,
        allowed_inets,
        expires_at,
    })
}

/// `inet[]::text` = `{10.0.0.1,10.0.0.2}`. WHY manual parse: the
/// `ipnetwork` crate is not in this workspace and a text round-trip
/// keeps the dependency out. Unparseable content fails closed (deny-all).
fn parse_inet_list(text: &str) -> Vec<std::net::IpAddr> {
    let inner = text.trim().trim_start_matches('{').trim_end_matches('}');
    if inner.is_empty() {
        return Vec::new();
    }
    inner
        .split(',')
        .map(|part| part.trim().trim_matches('"').parse::<std::net::IpAddr>())
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default()
}

/// story: e22s02 — ONE admission gate for scope and repo pin, shared by
/// both transports: authorize calls it with the resolved repo id;
/// push-to-create calls it with None (repo_id absent = a pinned token
/// can never create a different repo, and scope gates admission
/// itself — the denied create never happens). None = no token
/// presented; anonymous rules stay in authorize.
// WHY Result<(),()>: the two callers map it straight onto their own
// denial shapes (HTTP 403 "forbidden" / SSH "forbidden"); no payload.
#[allow(clippy::result_unit_err)]
pub fn transport_gate(
    auth: Option<&TransportAuth>,
    op: crate::transport::GitOp,
    repo_id: Option<Uuid>,
) -> Result<(), ()> {
    let Some(auth) = auth else {
        return Ok(());
    };
    let needed = match op {
        crate::transport::GitOp::ReceivePack => "write",
        crate::transport::GitOp::UploadPack => "read",
    };
    if !auth.scopes.iter().any(|scope| scope == needed) {
        return Err(());
    }
    match (auth.pinned_repo, repo_id) {
        (Some(pinned), Some(id)) if pinned == id => Ok(()),
        (Some(_), _) => Err(()),
        (None, _) => Ok(()),
    }
}

pub async fn revoke(pool: &PgPool, token_id: Uuid) {
    sqlx::query("UPDATE tokens SET revoked_at = now() WHERE id = $1")
        .bind(token_id)
        .execute(pool)
        .await
        .expect("revoke token");
}

/// story: e24s03 — resolve a presented reset link to its row and owner
/// while it is unexpired and unused. Compares by argon2-verifying
/// candidates, same as authenticate: salts differ per row, so lookup
/// can never be an equality match.
pub async fn find_reset(pool: &PgPool, plaintext: &str) -> Option<(Uuid, Uuid)> {
    let rows: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT id, user_id, token_hash FROM reset_tokens WHERE used_at IS NULL AND expires_at > now()",
    )
    .fetch_all(pool)
    .await
    .ok()?;
    rows.into_iter()
        .find(|(_, _, token_hash)| verify(plaintext, token_hash))
        .map(|(id, user_id, _)| (id, user_id))
}

/// Burns the link atomically; false means it was already used or expired.
pub async fn consume_reset(pool: &PgPool, id: Uuid) -> bool {
    sqlx::query(
        "UPDATE reset_tokens SET used_at = now() WHERE id = $1 AND used_at IS NULL AND expires_at > now()",
    )
    .bind(id)
    .execute(pool)
    .await
    .map(|done| done.rows_affected() == 1)
    .unwrap_or(false)
}

/// story: e21s03 — rotate: revoke every active token and issue one
/// replacement in a single transaction (no window where old and new
/// tokens both authenticate).
pub async fn regenerate(pool: &PgPool, user_id: Uuid) -> NewToken {
    regenerate_with(pool, user_id, &["read", "write"], None, None, None).await
}

/// story: e22s03 — rotate WITH permissions: revoke every active token
/// and issue the replacement carrying the requested permissions.
pub async fn regenerate_with(
    pool: &PgPool,
    user_id: Uuid,
    scopes: &[&str],
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    repo_id: Option<Uuid>,
    allowed_inets: Option<Vec<std::net::IpAddr>>,
) -> NewToken {
    let mut tx = pool.begin().await.expect("begin regenerate");
    sqlx::query("UPDATE tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .expect("revoke active tokens");
    let (plaintext, prefix) = generate();
    let token_hash = hash(&plaintext);
    let id: Uuid =
        sqlx::query_scalar("INSERT INTO tokens (user_id, label, prefix, token_hash, lookup_hash, scopes, expires_at, repo_id, allowed_inets) VALUES ($1, 'rotated', $2, $3, $4, $5, $6, $7, $8::inet[]) RETURNING id")
            .bind(user_id)
            .bind(&prefix)
            .bind(&token_hash)
            .bind(lookup_key(&plaintext))
            .bind(scopes)
            .bind(expires_at)
            .bind(repo_id)
            .bind(
                allowed_inets
                    .as_ref()
                    .map(|v| v.iter().map(std::net::IpAddr::to_string).collect::<Vec<_>>()),
            )
            .fetch_one(&mut *tx)
            .await
            .expect("insert regenerated token");
    tx.commit().await.expect("commit regenerate");
    NewToken { id, plaintext }
}

fn verify(plaintext: &str, token_hash: &str) -> bool {
    match PasswordHash::new(token_hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(plaintext.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Test helper: insert a token row directly, returning its plaintext and id.
pub async fn insert_test_token(pool: &PgPool) -> (String, Uuid) {
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, handle) VALUES ('tokentest@example.com', 'x', 'tokentest') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("insert test user");
    let (plaintext, prefix) = generate();
    let token_hash = hash(&plaintext);
    let id: Uuid =
        sqlx::query_scalar("INSERT INTO tokens (user_id, label, prefix, token_hash) VALUES ($1, 'ci', $2, $3) RETURNING id")
            .bind(user_id)
            .bind(&prefix)
            .bind(&token_hash)
            .fetch_one(pool)
            .await
            .expect("insert test token");
    (plaintext, id)
}
