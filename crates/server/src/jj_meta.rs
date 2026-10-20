// story: e05s01
//! Colocate a jj workspace metadata dir beside a bare git repo.
//!
//! Layout (spike: specs/archive/spikes/SPIKE-kata-review-integration.md):
//! bare repo at `{storage}/repos/{owner}/{name}.git`, jj workspace at
//! `{storage}/meta/{owner}/{name}/`. The kata sidecar loads the meta dir
//! via `Workspace::load`; git transport never sees it.

use std::path::Path;

// story: e11s01
/// One commit matched by a revset query.
#[derive(Debug, serde::Serialize)]
pub struct RevsetHit {
    pub change_id: String,
    pub commit_id: String,
    pub description: String,
    pub is_open: bool,
}

// story: e11s01
/// Why a revset query failed. `Invalid` maps to HTTP 400 (the caller shows
/// the message); `Internal` is a server-side fault.
#[derive(Debug)]
pub enum RevsetQueryError {
    Invalid(String),
    Internal(String),
}

// story: e11s01
/// Parse `query` with the jj_lib revset parser. `Ok(())` means the query is
/// syntactically valid in the server-side dialect (which adds a zero-arg
/// `open()` alias); `Err` is a message suitable for an HTTP 400 response.
pub fn parse_revset(query: &str) -> Result<(), String> {
    let settings = settings();
    let user_email = settings.user_email().to_owned();
    let extensions = jj_lib::revset::RevsetExtensions::default();
    lower_revset(query, &user_email, &extensions).map(|_| ())
}

// story: e11s01
/// parse + alias expansion + lowering: `UserRevsetExpression` ready for
/// symbol resolution against a repo.
fn lower_revset(
    query: &str,
    user_email: &str,
    extensions: &jj_lib::revset::RevsetExtensions,
) -> Result<std::sync::Arc<jj_lib::revset::UserRevsetExpression>, String> {
    lower_revset_dialect(query, user_email, extensions, "trunk")
}

// story: e11s01
/// Like [`lower_revset`], but `trunk_bookmark` names the repo's trunk
/// bookmark so `trunk()` resolves per repository.
fn lower_revset_dialect(
    query: &str,
    user_email: &str,
    extensions: &jj_lib::revset::RevsetExtensions,
    trunk_bookmark: &str,
) -> Result<std::sync::Arc<jj_lib::revset::UserRevsetExpression>, String> {
    use jj_lib::fileset::FilesetAliasesMap;
    use jj_lib::revset::{self, RevsetAliasesMap, RevsetDiagnostics, RevsetParseContext};

    let mut aliases_map = RevsetAliasesMap::new();
    // story: e11s01 — server dialect aliases jj_lib doesn't builtin:
    // `open()` = commits not reachable from any bookmark (`::x` = ancestors),
    // `trunk()` = the trunk bookmark symbol.
    aliases_map
        .insert("open()", "all() ~ ::bookmarks()")
        .map_err(|e| format!("revset alias: {e}"))?;
    aliases_map
        .insert("trunk()", trunk_bookmark)
        .map_err(|e| format!("revset alias: {e}"))?;
    let context = RevsetParseContext {
        aliases_map: &aliases_map,
        local_variables: std::collections::HashMap::new(),
        user_email,
        date_pattern_context: chrono::Utc::now().fixed_offset().into(),
        default_ignored_remote: None,
        fileset_aliases_map: &FilesetAliasesMap::new(),
        // story: e11s01 — jj CLI default: unquoted strings are literal
        // substrings (description("second") matches "second change").
        use_glob_by_default: false,
        extensions,
        workspace: None,
    };
    revset::parse(&mut RevsetDiagnostics::new(), query, &context).map_err(|e| e.to_string())
}

// story: e11s01
/// Evaluate `query` against the meta workspace at `meta_path`, read-only.
/// Returns matches in evaluation (topological) order, capped at `limit`.
/// Refs are re-imported first so a just-pushed commit is visible.
pub async fn revset_query(
    meta_path: &Path,
    trunk_bookmark: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<RevsetHit>, RevsetQueryError> {
    use jj_lib::backend::CommitId;
    use jj_lib::object_id::ObjectId;
    use jj_lib::repo::{Repo as _, StoreFactories};
    use jj_lib::workspace::default_working_copy_factories;
    use RevsetQueryError::{Internal, Invalid};

    let meta = meta_path.to_owned();
    let trunk_bookmark = trunk_bookmark.to_owned();
    let query = query.to_owned();
    tokio::task::spawn_blocking(move || -> Result<Vec<RevsetHit>, RevsetQueryError> {
        let rt = tokio::runtime::Handle::current();
        let settings = settings();
        let user_email = settings.user_email().to_owned();
        let workspace = jj_lib::workspace::Workspace::load(
            &settings,
            &meta,
            &StoreFactories::default(),
            &default_working_copy_factories(),
        )
        .map_err(|e| Internal(format!("workspace load: {e}")))?;
        let repo = rt
            .block_on(workspace.repo_loader().load_at_head())
            .map_err(|e| Internal(format!("load_at_head: {e}")))?;

        // story: e11s01 — refresh git refs so a just-pushed commit is visible.
        let options = jj_lib::git::GitImportOptions {
            auto_local_bookmark: true,
            abandon_unreachable_commits: false,
            remote_auto_track_bookmarks: Default::default(),
        };
        let mut tx = repo.start_transaction();
        rt.block_on(jj_lib::git::import_refs(tx.repo_mut(), &options))
            .map_err(|e| Internal(format!("import_refs: {e}")))?;
        let repo = rt
            .block_on(tx.commit("jjhub: import refs for revset query"))
            .map_err(|e| Internal(format!("tx commit: {e}")))?;

        let extensions = jj_lib::revset::RevsetExtensions::default();
        let expression = lower_revset_dialect(&query, &user_email, &extensions, &trunk_bookmark)
            .map_err(Invalid)?;
        let symbol_resolver =
            jj_lib::revset::SymbolResolver::new(repo.as_ref(), extensions.symbol_resolvers());
        let resolved = expression
            .resolve_user_expression(repo.as_ref(), &symbol_resolver)
            .map_err(|e| Invalid(format!("resolve: {e}")))?;
        let revset = resolved
            .evaluate(repo.as_ref())
            .map_err(|e| Invalid(format!("evaluate: {e}")))?;

        // story: e11s01 — a commit is open when no bookmark can reach it.
        let mut heads: Vec<CommitId> = Vec::new();
        for (_, target) in repo.view().local_bookmarks() {
            if let Some(id) = target.as_normal() {
                heads.push(id.clone());
            }
        }
        let all_remotes = jj_lib::str_util::StringMatcher::all();
        for (_, remote_ref) in repo
            .view()
            .remote_bookmarks_matching(&all_remotes, &all_remotes)
        {
            if let Some(id) = remote_ref.target.as_normal() {
                heads.push(id.clone());
            }
        }
        let index = repo.index();
        // story: e11s01 — the meta workspace's own working-copy commit is a
        // server artifact, never a search result.
        let working_copies: std::collections::HashSet<_> =
            repo.view().wc_commit_ids().values().cloned().collect();
        let mut hits = Vec::new();
        for pair in revset.commit_change_ids() {
            if hits.len() >= limit {
                break;
            }
            let (commit_id, change_id) =
                pair.map_err(|e| Invalid(format!("evaluate: {e}")))?;
            if working_copies.contains(&commit_id) {
                continue;
            }
            let commit = repo
                .store()
                .get_commit(&commit_id)
                .map_err(|e| Internal(format!("get_commit: {e}")))?;
            let mut is_open = true;
            for head in &heads {
                let reachable = head == &commit_id
                    || index
                        .is_ancestor(&commit_id, head)
                        .map_err(|e| Internal(format!("is_ancestor: {e}")))?;
                if reachable {
                    is_open = false;
                    break;
                }
            }
            hits.push(RevsetHit {
                change_id: change_id.hex(),
                commit_id: commit_id.hex(),
                // story: e11s01 — git commit messages carry a trailing
                // newline into the description; JSON consumers don't want it.
                description: commit.description().trim_end().to_owned(),
                is_open,
            });
        }
        Ok(hits)
    })
    .await
    .map_err(|e| RevsetQueryError::Internal(format!("spawn_blocking: {e}")))?
}

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
