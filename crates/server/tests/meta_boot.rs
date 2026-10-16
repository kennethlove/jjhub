// story: kata scan — the sidecar watches the meta root; create it at boot
// so its inotify watch attaches on a fresh volume instead of falling back
// to30s polling and warn-spamming every cycle.
mod meta_boot {
    #[test]
    fn ensure_storage_layout_creates_root_and_meta() {
        let root = tempfile::tempdir().unwrap();
        let storage = root.path().join("repos");
        server::ensure_storage_layout(storage.to_str().unwrap()).unwrap();
        assert!(storage.is_dir(), "storage root created");
        assert!(
            storage.join(".meta").is_dir(),
            "meta root created eagerly for the sidecar scanner"
        );
        // idempotent
        server::ensure_storage_layout(storage.to_str().unwrap()).unwrap();
    }
}
