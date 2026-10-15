// story: e02s01
mod repo_model {
    use domain::{storage_path, VisibilityError};

    #[test]
    fn valid_names_pass() {
        for name in ["demo", "a", "my-repo_1.0", "0start"] {
            assert!(domain::repo_name_valid(name), "{name} should be valid");
        }
    }

    #[test]
    fn invalid_names_fail() {
        for name in [
            "",
            ".hidden",
            "-dash",
            "Upper",
            "has space",
            "a/b",
            "../up",
            "a\0b",
            &"x".repeat(101),
        ] {
            assert!(!domain::repo_name_valid(name), "{name:?} should be invalid");
        }
    }

    #[test]
    fn visibility_parse() {
        assert_eq!(
            domain::Visibility::parse("public"),
            Some(domain::Visibility::Public)
        );
        assert_eq!(
            domain::Visibility::parse("private"),
            Some(domain::Visibility::Private)
        );
        assert_eq!(domain::Visibility::parse("secret"), None);
        assert_eq!(
            VisibilityError::BadVisibility,
            VisibilityError::BadVisibility
        );
    }

    #[test]
    fn storage_path_joins_safely() {
        let path = storage_path("/srv/repos", "alice", "demo");
        assert_eq!(path, std::path::PathBuf::from("/srv/repos/alice/demo.git"));
    }
}
