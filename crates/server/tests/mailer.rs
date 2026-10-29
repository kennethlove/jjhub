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
