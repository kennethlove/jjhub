// story: e08s01 — git-plumbing builder for jj's git conflict representation,
// so transport tests need no jj CLI binary. Mirrors jj-lib 0.41's git
// backend: two term trees merged under `.jjconflict-base-N` /
// `.jjconflict-side-N`, a `JJ-CONFLICT-README` blob, a `jj:trees` commit
// header naming the term root trees, and `refs/jj/keep/*` refs pinning
// every commit's objects.

#![allow(dead_code)] // each test binary uses a subset of the helper API

use std::path::Path;

/// The fixture chain: a resolved parent commit and a commit carrying jj's
/// conflict representation. Both ids resolve through `refs/jj/keep/*`.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub parent: String,
    pub conflicted: String,
    /// change-id header of the conflicted commit (what `build` writes).
    pub change_id: String,
}

/// change-id written into the conflicted commit by [`build`]; the identity
/// tests seed their open merge request with this value.
pub const CONFLICT_CHANGE_ID: &str = "0e1f2d3c4b5a69788796a5b4c3d2e1f0";

/// Initialize a bare repo at `dir` when it has no `HEAD` yet (repos created
/// by the server already carry one).
pub fn ensure_bare(dir: &Path) {
    let _ = dir;
    unimplemented!("story: e08s01 — fixture helper lands with the implementation")
}

/// Build the fixture with the default pushing identity (alice) — the
/// transport tests' conflicted repo.
pub fn build(dir: &Path) -> Fixture {
    let _ = dir;
    unimplemented!("story: e08s01 — fixture helper lands with the implementation")
}

/// Build the fixture with an explicit identity and conflicted-commit
/// change-id: the identity test pushes a foreign author's conflicted commit.
pub fn build_as(dir: &Path, name: &str, email: &str, change_id: &str) -> Fixture {
    let _ = (dir, name, email, change_id);
    unimplemented!("story: e08s01 — fixture helper lands with the implementation")
}
