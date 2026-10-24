//! story: e17s01 (AC t2) — host keys generate once, persist, and reload.

use server::ssh::load_or_generate_keys;

#[test]
fn host_keys_generate_persist_and_reload() {
    let dir = tempfile::tempdir().expect("tempdir");
    let key_path = dir.path().join("ssh_host_ed25519_key");

    let (keys1, fp1) = load_or_generate_keys(dir.path()).expect("first generation");
    assert_eq!(keys1.len(), 1, "exactly one host key");
    assert!(fp1.starts_with("SHA256:"), "fingerprint format, got {fp1}");
    assert!(key_path.exists(), "private key written to {key_path:?}");

    // story: e17s01 — private key is owner-readable only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&key_path)
            .expect("key metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "key file mode must be 0600");
    }

    // Reload: same bytes on disk, same fingerprint — no regeneration.
    let (keys2, fp2) = load_or_generate_keys(dir.path()).expect("reload");
    assert_eq!(fp1, fp2, "fingerprint stable across restarts");
    let bytes1 = std::fs::read(&key_path).expect("read key");
    let (keys3, fp3) = load_or_generate_keys(dir.path()).expect("reload again");
    let bytes2 = std::fs::read(&key_path).expect("read key again");
    assert_eq!(bytes1, bytes2, "key file untouched by reloads");
    assert_eq!(fp1, fp3);
    assert_eq!(
        keys1[0]
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string(),
        keys2[0]
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string()
    );
    assert_eq!(
        keys3[0]
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string(),
        fp1
    );
}

#[test]
fn host_keys_missing_dir_is_created() {
    let dir = tempfile::tempdir().expect("tempdir");
    let nested = dir.path().join("nested").join("keys");
    let (keys, fp) = load_or_generate_keys(&nested).expect("create nested dir");
    assert_eq!(keys.len(), 1);
    assert!(fp.starts_with("SHA256:"));
    assert!(nested.join("ssh_host_ed25519_key").exists());
}
