// story: e15s01 — the server renders pages through one shared Tera
// base layout; a render returns a full document.
use tera::Context;

#[test]
fn tera_layout_renders_full_document() {
    let html = server::render::render("base.html", &Context::new());
    assert!(html.starts_with("<!doctype html>"), "doctype: {html}");
    assert!(html.contains("<title>"), "title block: {html}");
    assert!(html.contains("<body>"), "body block: {html}");
    assert!(html.contains("</html>"), "closed document: {html}");
}
