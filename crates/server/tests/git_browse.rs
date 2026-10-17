// story: e07s01
mod git_browse {
    /// seed a bare repo with a nested structure; returns bare path
    fn seed_bare(root: &std::path::Path) -> std::path::PathBuf {
        let bare = root.join("bare.git");
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .current_dir(&seed)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "--bare", bare.to_str().unwrap()]);
        git(&["init", "-b", "trunk"]);
        std::fs::create_dir_all(seed.join("docs")).unwrap();
        std::fs::write(seed.join("README.md"), "# hello\n").unwrap();
        std::fs::write(seed.join("docs/guide.md"), "guide\n").unwrap();
        std::fs::write(seed.join("logo.bin"), [0u8, 1, 2, 255, 128].repeat(100)).unwrap();
        git(&["add", "."]);
        git(&["-c", "user.email=a@e.com", "-c", "user.name=A", "commit", "-m", "one"]);
        git(&["push", bare.to_str().unwrap(), "trunk"]);
        bare
    }

    #[test]
    fn lists_top_level_entries() {
        let root = tempfile::tempdir().unwrap();
        let bare = seed_bare(root.path());
        let entries = server::browse::list_tree(&bare, "trunk", "").expect("list");
        let names: Vec<&str> = entries.iter().map(|(name, is_dir)| {
            if *is_dir { return Box::leak(format!("{name}/").into_boxed_str()) as &str; }
            Box::leak(name.clone().into_boxed_str()) as &str
        }).collect();
        assert!(names.contains(&"README.md"), "{names:?}");
        assert!(names.contains(&"docs/"), "{names:?}");
        assert!(names.contains(&"logo.bin"), "{names:?}");
    }

    #[test]
    fn lists_subdirectory() {
        let root = tempfile::tempdir().unwrap();
        let bare = seed_bare(root.path());
        let entries = server::browse::list_tree(&bare, "trunk", "docs").expect("list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "guide.md");
        assert!(!entries[0].1);
    }

    #[test]
    fn reads_blob_text() {
        let root = tempfile::tempdir().unwrap();
        let bare = seed_bare(root.path());
        let blob = server::browse::read_blob(&bare, "trunk", "README.md").expect("read");
        assert!(matches!(blob, server::browse::Blob::Text(_)));
        if let server::browse::Blob::Text(text) = blob {
            assert_eq!(text, "# hello\\n");
        }
    }

    #[test]
    fn reads_blob_binary() {
        let root = tempfile::tempdir().unwrap();
        let bare = seed_bare(root.path());
        let blob = server::browse::read_blob(&bare, "trunk", "logo.bin").expect("read");
        assert!(matches!(blob, server::browse::Blob::Binary { len: 500 }), "{blob:?}");
    }

    #[test]
    fn unknown_ref_is_none() {
        let root = tempfile::tempdir().unwrap();
        let bare = seed_bare(root.path());
        assert!(server::browse::list_tree(&bare, "nope", "").is_none());
    }

    #[test]
    fn traversal_paths_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let bare = seed_bare(root.path());
        // the validation gate lives in validate_path; traversal never reaches git
        assert!(server::browse::validate_path("../etc").is_err());
        assert!(server::browse::validate_path("/absolute").is_err());
        assert!(server::browse::validate_path("a/../../b").is_err());
        assert!(server::browse::validate_path("docs/guide.md").is_ok());
    }
}
