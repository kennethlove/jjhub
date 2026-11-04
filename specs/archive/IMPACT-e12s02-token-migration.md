# Impact assessment — e12s02 token migration

## Target
- `crates/server/src/routes.rs` — inline HTML page strings gain stylesheet link + classes
- `crates/server/src/lib.rs` — router gains one stylesheet route

## Dependents (2)
- `lib.rs` router: 59 `.route(` registrations over 26 `pub fn` handlers in routes.rs
- `crates/server/tests/`: 36 integration test files assert rendered HTML substrings

## Affected Stories
- Story e12s02: Site pages migrated to design tokens (primary)
- Stories e12s01 / e12s03: docs only, unaffected

## Test Coverage
- `tests/site_shell.rs`, `home_page.rs`, `tree_blob_pages.rs`, `mr_pages.rs`, `register.rs`, `login.rs`: cover page rendering
- Gap: no stylesheet endpoint exists yet (no `*.css` in repo, no asset route); no assertions on focus/disabled states

## Risk: Medium
One dependent (the router), but markup-substring assertions across ~20 render tests can break when class attributes are inserted into matched tags; routes.rs churns every feature PR.

## Recommended action
Proceed: add the stylesheet route as a NEW endpoint first (touches no existing string), migrate pages incrementally with `cargo test --workspace` green after each group.
