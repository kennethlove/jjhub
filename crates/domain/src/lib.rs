// story: e01s01
use std::fmt;

pub const MIN_PASSWORD_LEN: usize = 10;
pub const MAX_REPO_NAME_LEN: usize = 100;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Visibility {
    Public,
    Private,
}

impl Visibility {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Visibility::Public),
            "private" => Some(Visibility::Private),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Private => "private",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum VisibilityError {
    BadVisibility,
}

/// story: e02s01 — repo names: ^[a-z0-9][a-z0-9._-]{0,99}$
pub fn repo_name_valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_REPO_NAME_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
        && !name.starts_with(['.', '-'])
}

/// story: e02s01 — safe storage path: validated owner/name only, then join.
pub fn storage_path(root: &str, owner: &str, name: &str) -> std::path::PathBuf {
    debug_assert!(repo_name_valid(name));
    std::path::Path::new(root)
        .join(owner)
        .join(format!("{name}.git"))
}

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

pub fn validate_registration(
    email: &str,
    password: &str,
) -> Result<(String, String), RegistrationError> {
    let email = normalize_email(email);
    if !is_valid_email(&email) {
        return Err(RegistrationError::InvalidEmail);
    }
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(RegistrationError::PasswordTooShort);
    }
    let handle = handle_from_email(&email);
    Ok((email, handle))
}

/// story: e02s01 — default handle = email local part, lowercased, sanitized.
pub fn handle_from_email(email: &str) -> String {
    let local = email.split('@').next().unwrap_or("user");
    let cleaned: String = local
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "u".to_string()
    } else if cleaned.starts_with(['.', '-']) {
        format!("u{}", cleaned.trim_matches(['.', '-']))
    } else {
        cleaned
    }
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
