// story: e01s02
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use rand::RngCore;
use sqlx::PgPool;
use uuid::Uuid;

pub const TOKEN_PREFIX: &str = "jjh_";

pub struct NewToken {
    pub id: Uuid,
    pub plaintext: String,
}

pub fn hash(token: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(token.as_bytes(), &salt)
        .expect("argon2 hash")
        .to_string()
}

pub fn generate() -> (String, String) {
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    let plaintext = format!("{TOKEN_PREFIX}{}", hex::encode(bytes));
    let prefix = plaintext.chars().take(TOKEN_PREFIX.len() + 6).collect();
    (plaintext, prefix)
}

pub async fn create(pool: &PgPool, user_id: Uuid, label: &str) -> NewToken {
    let (plaintext, prefix) = generate();
    let token_hash = hash(&plaintext);
    let id: Uuid =
        sqlx::query_scalar("INSERT INTO tokens (user_id, label, prefix, token_hash) VALUES ($1, $2, $3, $4) RETURNING id")
            .bind(user_id)
            .bind(label)
            .bind(&prefix)
            .bind(&token_hash)
            .fetch_one(pool)
            .await
            .expect("insert token");
    NewToken { id, plaintext }
}

/// story: e01s02 — resolve a presented token value to its owning user, or None.
/// Compares by argon2-verifying candidates; MVP scale (few tokens per user).
pub async fn authenticate(pool: &PgPool, plaintext: &str) -> Option<Uuid> {
    let rows: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT t.user_id, t.token_hash FROM tokens t WHERE t.revoked_at IS NULL")
            .fetch_all(pool)
            .await
            .ok()?;
    rows.into_iter()
        .find(|(_, token_hash)| verify(plaintext, token_hash))
        .map(|(user_id, _)| user_id)
}

pub async fn revoke(pool: &PgPool, token_id: Uuid) {
    sqlx::query("UPDATE tokens SET revoked_at = now() WHERE id = $1")
        .bind(token_id)
        .execute(pool)
        .await
        .expect("revoke token");
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
