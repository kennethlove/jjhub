// story: e05s01
//! Colocate a jj workspace metadata dir beside a bare git repo.
//!
//! Layout (spike: specs/archive/spikes/SPIKE-kata-review-integration.md):
//! bare repo at `{storage}/repos/{owner}/{name}.git`, jj workspace at
//! `{storage}/meta/{owner}/{name}/`. The kata sidecar loads the meta dir
//! via `Workspace::load`; git transport never sees it.

use std::path::Path;

fn settings() -> jj_lib::settings::UserSettings {
    use jj_lib::config::{ConfigLayer, ConfigSource, StackedConfig};
    let mut config = StackedConfig::with_defaults();
    let mut identity = ConfigLayer::empty(ConfigSource::Default);
    // read-only usage: identity would only land in commits we never write
    identity.set_value("user.name", "jjhub").ok();
    identity.set_value("user.email", "jjhub@invalid").ok();
    config.add_layer(identity);
    jj_lib::settings::UserSettings::from_config(config).expect("jj settings")
}

/// story: e05s01 — create a jj workspace at `meta_path` backed by the bare
/// git repo at `bare_path`, then import the existing git refs so bookmarks
/// (trunk) are visible to `Workspace::load`. Fails if the bare repo is missing.
pub async fn init(meta_path: &Path, bare_path: &Path) -> Result<(), String> {
    if !bare_path.join("HEAD").exists() {
        return Err(format!("not a bare git repo: {bare_path:?}"));
    }
    std::fs::create_dir_all(meta_path).map_err(|e| e.to_string())?;

    // `Workspace::init_external_git` creates the `.jj` workspace rooted at
    // `meta_path`, sharing the bare repo's object DB. Already-initialized
    // (a retried create, or a duplicate name) just re-syncs refs.
    if !meta_path.join(".jj").exists() {
        let meta = meta_path.to_owned();
        let bare = bare_path.to_owned();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let rt = tokio::runtime::Handle::current();
            let (workspace, _repo) = rt
                .block_on(jj_lib::workspace::Workspace::init_external_git(
                    &settings(),
                    &meta,
                    &bare,
                ))
                .map_err(|e| format!("jj workspace init: {e}"))?;
            drop(workspace);
            Ok(())
        })
        .await
        .map_err(|e| format!("spawn_blocking: {e}"))??;
    }

    sync_refs(meta_path).await
}

/// story: e05s02 — import git refs from the backing store into the meta
/// workspace's jj view. `init_external_git` shares the bare repo's object
/// DB, so objects are always present — only refs lag. Re-running this after
/// a push is what makes newly-pushed commits visible to the kata sidecar.
pub async fn sync_refs(meta_path: &Path) -> Result<(), String> {
    use jj_lib::repo::StoreFactories;
    use jj_lib::workspace::default_working_copy_factories;

    let meta = meta_path.to_owned();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let rt = tokio::runtime::Handle::current();
        let workspace = jj_lib::workspace::Workspace::load(
            &settings(),
            &meta,
            &StoreFactories::default(),
            &default_working_copy_factories(),
        )
        .map_err(|e| format!("workspace load: {e}"))?;
        let repo = rt
            .block_on(workspace.repo_loader().load_at_head())
            .map_err(|e| format!("load_at_head: {e}"))?;

        let options = jj_lib::git::GitImportOptions {
            auto_local_bookmark: true,
            abandon_unreachable_commits: false,
            remote_auto_track_bookmarks: Default::default(),
        };
        let mut tx = repo.start_transaction();
        rt.block_on(jj_lib::git::import_refs(tx.repo_mut(), &options))
            .map_err(|e| format!("import_refs: {e}"))?;
        rt.block_on(tx.commit("jjhub: import refs after push"))
            .map_err(|e| format!("tx commit: {e}"))?;
        drop(workspace);
        Ok(())
    })
    .await
    .map_err(|e| format!("spawn_blocking: {e}"))?
}

/// story: e05s01 — the kata-side load path: `Workspace::load` on the meta
/// dir, then resolve the `trunk` bookmark to its root-tree file listing.
pub async fn load_trunk_file_names(meta_path: &Path) -> Result<Vec<String>, String> {
    use jj_lib::backend::TreeValue;
    use jj_lib::repo::{Repo as _, StoreFactories};
    use jj_lib::workspace::default_working_copy_factories;

    let workspace = jj_lib::workspace::Workspace::load(
        &settings(),
        meta_path,
        &StoreFactories::default(),
        &default_working_copy_factories(),
    )
    .map_err(|e| format!("workspace load: {e}"))?;

    let repo = workspace
        .repo_loader()
        .load_at_head()
        .await
        .map_err(|e| format!("load_at_head: {e}"))?;

    // trunk bookmark -> commit id (skip conflicted bookmarks)
    let trunk_id = repo
        .view()
        .local_bookmarks()
        .find_map(|(name, target)| {
            (name.as_str() == "trunk")
                .then(|| target.as_normal().cloned())
                .flatten()
        })
        .ok_or("trunk bookmark not found")?;

    let commit = repo
        .store()
        .get_commit_async(&trunk_id)
        .await
        .map_err(|e| format!("get_commit: {e}"))?;

    let tree = commit
        .tree()
        .trees()
        .await
        .map_err(|e| format!("trees: {e}"))?;
    let resolved = tree.iter().next().ok_or("trunk root tree is a conflict")?;

    Ok(resolved
        .entries_non_recursive()
        .filter_map(|entry| {
            matches!(entry.value(), TreeValue::File { .. })
                .then(|| entry.name().as_internal_str().to_owned())
        })
        .collect())
}
