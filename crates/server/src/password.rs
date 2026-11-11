// story: e01s01
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;

// WHY: opt-in cost cut so CI test runs are not dominated by hashing;
// unset keeps the RFC 9106 defaults, so production behavior is
// unchanged unless an operator deliberately sets it. Params travel
// inside each PHC hash, so verification honors them per hash either way.
pub(crate) fn hasher() -> Argon2<'static> {
    match std::env::var("JJHUB_ARGON2_M_COST")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    {
        // m must be >= 8 * p; p = 1 and t = 1 for the fastest valid run.
        Some(m) if m >= 8 => Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            argon2::Params::new(m, 1, 1, None).expect("argon2 params"),
        ),
        _ => Argon2::default(),
    }
}

pub fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    hasher()
        .hash_password(password.as_bytes(), &salt)
        .expect("argon2 hash")
        .to_string()
}

pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    match PasswordHash::new(stored_hash) {
        Ok(parsed) => hasher()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}
