// story: e01s01

mod user_registration {
    use domain::{normalize_email, validate_registration, RegistrationError};
    #[test]
    fn email_normalization_lowercases() {
        assert_eq!(normalize_email("  Alice@Example.COM "), "alice@example.com");
    }

    #[test]
    fn registration_requires_min_password_length() {
        let err = validate_registration("alice@example.com", "short").unwrap_err();
        assert_eq!(err, RegistrationError::PasswordTooShort);
    }

    #[test]
    fn registration_rejects_invalid_email() {
        let err = validate_registration("not-an-email", "correct horse battery").unwrap_err();
        assert_eq!(err, RegistrationError::InvalidEmail);
    }

    #[test]
    fn registration_accepts_valid_input() {
        assert!(validate_registration("alice@example.com", "correct horse battery").is_ok());
    }
}
