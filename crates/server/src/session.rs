// story: e01s01
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "session";
const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sign(secret: &[u8], payload: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac key");
    mac.update(payload.as_bytes());
    hex(&mac.finalize().into_bytes())
}

pub fn issue(secret: &[u8], user_id: Uuid) -> String {
    let expiry = chrono::Utc::now().timestamp() + SESSION_TTL_SECS;
    let payload = format!("{user_id}.{expiry}");
    format!("{payload}.{}", sign(secret, &payload))
}

pub fn verify(secret: &[u8], value: &str) -> Option<Uuid> {
    let (payload, signature) = value.rsplit_once('.')?;
    if !constant_time_eq(&sign(secret, payload), signature) {
        return None;
    }
    let (id, expiry) = payload.split_once('.')?;
    if chrono::Utc::now().timestamp() >= expiry.parse::<i64>().ok()? {
        return None;
    }
    Uuid::parse_str(id).ok()
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}
