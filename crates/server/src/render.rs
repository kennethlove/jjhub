// story: e15s01/e15s02 — one Tera engine for every page; templates are
// compiled in at build time so a missing template is a bug the tests
// catch, not a runtime failure
use std::sync::OnceLock;

use tera::{Context, Tera};

pub fn render(name: &str, context: &Context) -> String {
    engine()
        .render(name, context)
        .unwrap_or_else(|err| panic!("render {name}: {err}"))
}

fn engine() -> &'static Tera {
    static ENGINE: OnceLock<Tera> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut tera = Tera::default();
        for (name, source) in [
            ("base.html", include_str!("../templates/base.html")),
            ("nav.html", include_str!("../templates/nav.html")),
            ("home.html", include_str!("../templates/home.html")),
            ("login.html", include_str!("../templates/login.html")),
            (
                "login_card.html",
                include_str!("../templates/login_card.html"),
            ),
            ("register.html", include_str!("../templates/register.html")),
            (
                "register_card.html",
                include_str!("../templates/register_card.html"),
            ),
            ("reset.html", include_str!("../templates/reset.html")),
            (
                "reset_card.html",
                include_str!("../templates/reset_card.html"),
            ),
            (
                "reset_sent.html",
                include_str!("../templates/reset_sent.html"),
            ),
            (
                "reset_sent_card.html",
                include_str!("../templates/reset_sent_card.html"),
            ),
            ("settings.html", include_str!("../templates/settings.html")),
            ("repo.html", include_str!("../templates/repo.html")),
            ("tree.html", include_str!("../templates/tree.html")),
            ("blob.html", include_str!("../templates/blob.html")),
            ("mr_pages.html", include_str!("../templates/mr_pages.html")),
            (
                "mr_list_item.html",
                include_str!("../templates/mr_list_item.html"),
            ),
            (
                "mr_detail.html",
                include_str!("../templates/mr_detail.html"),
            ),
            (
                "mr_revision.html",
                include_str!("../templates/mr_revision.html"),
            ),
            (
                "status_badge.html",
                include_str!("../templates/status_badge.html"),
            ),
            // story: e11s02 — repository-scoped revset search page
            ("search.html", include_str!("../templates/search.html")),
            // story: e09s01 — the per-repository operation log page
            (
                "operations.html",
                include_str!("../templates/operations.html"),
            ),
            // story: e09s02 — one operation in the audit log, with its
            // view's ref changes
            (
                "operation_detail.html",
                include_str!("../templates/operation_detail.html"),
            ),
        ] {
            tera.add_raw_template(name, source)
                .unwrap_or_else(|err| panic!("load {name}: {err}"));
        }
        tera
    })
}
