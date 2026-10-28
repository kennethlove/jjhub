# e25s01 — Settings shell: panels, states, kept features

## 1. Story

As a signed-in user I open /settings and read one designed page:
Account rows (email, handle with URL helper, member since),
Profile form, Access tokens (generate, reveal strip, ledger,
regenerate), and the Danger zone last — with panel states switched
by `:target` links, no JavaScript.

## 2. Business value

The approved preferences design ships (spec:
specs/designs/preferences.html) while the two already-built
settings features survive: the Profile form (e21s02) and the
Regenerate button (e21s03), per user decision recorded in the
epic. The ledger gains what the design needs: token id, prefix,
created date.

## 3. Type

feat

## 4. Risk / context

risk: P3 (presentation) — context: `routes.rs::settings`
context enrichment (member since, owned-repo count, tokens with
id/created), `settings.html` rewrite to panels + states,
`app.css` new section scoped `.prefs-main` (panel, def-row, state
machine, ledger table, danger), existing `/settings/profile` and
`/settings/regenerate` routes wired (they are absent on trunk
today; forms 404). Note: the user's uncommitted e21 work in the
main checkout also adds `/settings/profile` — expected conflict
surface, theirs to rebase.

## 5. Verification

`cargo test --workspace` (settings, theme_preference, site_shell,
tokens, csrf); CDP UAT /settings at 1440 + 390, :target states
walked by hash; `ux_audit` on app.css.
