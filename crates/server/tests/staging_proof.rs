//! e36s03t1 — the staging-proof procedure contract: the README must
//! carry the gate procedure (SMTP env family, /admin readout, the
//! keep-log-mailer rule) so it cannot rot silently. File-content
//! pin — no server, no database.

#[test]
fn readme_carries_the_staging_proof_procedure() {
    let readme = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md"),
    )
    .expect("README.md readable");

    let heading = "### Staging proof before you switch it on";
    let rest = readme
        .split_once(heading)
        .unwrap_or_else(|| panic!("README missing staging-proof heading: {heading}"))
        .1;
    let section = &rest[..rest.find("\n## ").unwrap_or(rest.len())];
    // Collapse line-wraps so anchors do not depend on prose wrapping.
    let section = section.split_whitespace().collect::<Vec<_>>().join(" ");

    for anchor in [
        "JJHUB_EMAIL_PROVIDER=smtp",
        "JJHUB_SMTP_*",
        "Recent deliveries",
        "log mailer until this reads",
    ] {
        assert!(
            section.contains(anchor),
            "staging-proof section missing anchor: {anchor}"
        );
    }
}
