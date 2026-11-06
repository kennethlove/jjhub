// story: e23s01 — provider payload shapes: resend and messagepit
// (SendGrid v3), no network.

#[test]
fn resend_payload_shape() {
    let payload = server::email::build_resend_payload(
        "jjhub@example.com",
        "new@example.com",
        "Confirm your email",
        "Click /verify?token=abc",
    );
    assert_eq!(payload["from"], "jjhub@example.com");
    assert_eq!(payload["to"], serde_json::json!(["new@example.com"]));
    assert_eq!(payload["subject"], "Confirm your email");
    assert_eq!(payload["text"], "Click /verify?token=abc");
}

#[test]
fn messagepit_payload_shape() {
    let payload = server::email::build_messagepit_payload(
        "jjhub@example.com",
        "new@example.com",
        "Confirm your email",
        "Click /verify?token=abc",
    );
    assert_eq!(
        payload["personalizations"][0]["to"][0]["email"],
        "new@example.com"
    );
    assert_eq!(payload["from"]["email"], "jjhub@example.com");
    assert_eq!(payload["subject"], "Confirm your email");
    assert_eq!(payload["content"][0]["type"], "text/plain");
    assert_eq!(payload["content"][0]["value"], "Click /verify?token=abc");
}

// story: SMTP TLS mode — 465 is SMTPS (lettre relay() = Tls::Wrapper),
// any other port is STARTTLS (starttls_relay() = Tls::Required). pairing
// relay() with the 587 default tried an implicit-TLS handshake against
// STARTTLS ports, so Resend's submission port never connected.
// WHY Debug: lettre exposes no TLS introspection; Debug prints the public
// Tls variant and SmtpInfo.port.
#[test]
fn smtp_transport_picks_tls_mode_by_port() {
    let smtps = format!(
        "{:?}",
        server::email::smtp_transport("smtp.resend.com", 465, "", "").unwrap()
    );
    assert!(smtps.contains("Wrapper"), "{smtps}");
    assert!(smtps.contains("465"), "{smtps}");

    let submission = format!(
        "{:?}",
        server::email::smtp_transport("smtp.resend.com", 587, "", "").unwrap()
    );
    assert!(submission.contains("Required"), "{submission}");
    assert!(submission.contains("587"), "{submission}");
}
