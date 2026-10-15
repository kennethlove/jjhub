// story: e01s01
use std::fmt;

pub const MIN_PASSWORD_LEN: usize = 10;

#[derive(Debug, PartialEq, Eq)]
pub enum RegistrationError {
    InvalidEmail,
    PasswordTooShort,
}

impl fmt::Display for RegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistrationError::InvalidEmail => write!(f, "invalid email address"),
            RegistrationError::PasswordTooShort => {
                write!(f, "password must be at least {MIN_PASSWORD_LEN} characters")
            }
        }
    }
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub fn validate_registration(email: &str, password: &str) -> Result<String, RegistrationError> {
    let email = normalize_email(email);
    if !is_valid_email(&email) {
        return Err(RegistrationError::InvalidEmail);
    }
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(RegistrationError::PasswordTooShort);
    }
    Ok(email)
}

fn is_valid_email(email: &str) -> bool {
    let (local, domain) = match email.split_once('@') {
        Some(pair) => pair,
        None => return false,
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.contains(char::is_whitespace)
}
