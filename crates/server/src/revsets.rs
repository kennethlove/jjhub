// story: e11s01
//! Revset search endpoint: `GET /{owner}/{name}/revsets?q=<revset>`.
//!
//! Read-only jj_lib revset evaluation over the repo's meta workspace.
//! Private repos are invisible to non-collaborators (404, same as unknown).

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use axum_extra::extract::CookieJar;

use crate::jj_meta::{self, RevsetQueryError};
use crate::AppState;

/// story: e11s01 — hard server-side cap on result count.
pub(crate) const RESULT_CAP: usize = 50;

pub async fn revsets_query(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    jar: CookieJar,
) -> Result<Json<Vec<jj_meta::RevsetHit>>, (StatusCode, String)> {
    let query = params
        .get("q")
        .cloned()
        .ok_or((StatusCode::NOT_FOUND, "not found".to_string()))?;

    let Some((_, trunk)) = visible_repo(&state, &owner, &name, &jar).await else {
        return Err((StatusCode::NOT_FOUND, "repository not found".to_string()));
    };

    let meta = domain::meta_path(&state.storage_root, &owner, &name);
    let hits = jj_meta::revset_query(&meta, &trunk, &query, RESULT_CAP)
        .await
        .map_err(|e| match e {
            RevsetQueryError::Invalid(message) => (StatusCode::BAD_REQUEST, message),
            RevsetQueryError::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        })?;
    Ok(Json(hits))
}

// story: e11s01 — the repo a revset query runs against; `None` covers both
// unknown and invisible-to-this-viewer (callers answer 404 for both).
pub(crate) async fn visible_repo(
    state: &AppState,
    owner: &str,
    name: &str,
    jar: &CookieJar,
) -> Option<(uuid::Uuid, String)> {
    let found: (uuid::Uuid, uuid::Uuid, String, String) = sqlx::query_as(
        "SELECT r.id, r.owner_id, r.visibility, r.trunk FROM repositories r \
         JOIN users u ON u.id = r.owner_id WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(owner)
    .bind(name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten()?;
    let (repo_id, repo_owner_id, visibility, trunk) = found;
    // story: e11s01 — a private repo is indistinguishable from a missing one
    // to strangers.
    if visibility == "private" && !is_visible_to(state, repo_id, repo_owner_id, jar).await {
        return None;
    }
    Some((repo_id, trunk))
}

// story: e11s02 — the repository-scoped search page: a GET form over the
// e11s01 query path, rendered server-side with the repo-detail chrome.
pub async fn search_page(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    jar: CookieJar,
) -> Response {
    let Some((repo_id, trunk)) = visible_repo(&state, &owner, &name, &jar).await else {
        return (StatusCode::NOT_FOUND, "repository not found").into_response();
    };
    // story: e11s02 — absent or empty q means "show what is in flight".
    let query = params
        .get("q")
        .map(|q| q.trim())
        .filter(|q| !q.is_empty())
        .unwrap_or("open()")
        .to_string();
    let meta = domain::meta_path(&state.storage_root, &owner, &name);
    let (hits, error) = match jj_meta::revset_query(&meta, &trunk, &query, RESULT_CAP).await {
        Ok(hits) => (hits, None),
        // story: e11s02 — an invalid query stays on the page so the parser
        // message renders escaped; markup in the query never survives.
        Err(jj_meta::RevsetQueryError::Invalid(message)) => (Vec::new(), Some(message)),
        Err(jj_meta::RevsetQueryError::Internal(message)) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, message).into_response();
        }
    };

    let open_mrs: Vec<String> = sqlx::query_scalar(
        "SELECT change_id FROM merge_requests WHERE repo_id = $1 AND state = 'open'",
    )
    .bind(repo_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    let rows: Vec<serde_json::Value> = hits
        .iter()
        .map(|hit| {
            serde_json::json!({
                "change_id": hit.change_id,
                "mr": open_mrs
                    .contains(&hit.change_id)
                    .then(|| format!("/{owner}/{name}/merge-requests/{}", hit.change_id)),
                "commit_id": &hit.commit_id[..12],
                "description": hit.description.lines().next().unwrap_or_default(),
            })
        })
        .collect();

    let viewer = crate::routes::viewer_handle(&state, &jar).await;
    let csrf = jar
        .get(crate::session::SESSION_COOKIE)
        .map(|c| crate::session::csrf_token(&state.session_secret, c.value()))
        .unwrap_or_default();
    let mut context = crate::routes::page_context(viewer.as_deref(), &csrf);
    context.insert("owner", &owner);
    context.insert("name", &name);
    context.insert("query", &query);
    context.insert("hits", &rows);
    context.insert("error", &error);
    context.insert("at_cap", &(hits.len() == RESULT_CAP));
    Html(crate::render::render("search.html", &context)).into_response()
}

async fn is_visible_to(
    state: &AppState,
    repo_id: uuid::Uuid,
    repo_owner_id: uuid::Uuid,
    jar: &CookieJar,
) -> bool {
    let Some(cookie) = jar.get(crate::session::SESSION_COOKIE) else {
        return false;
    };
    let Some(user_id) = crate::session::verify(&state.session_secret, cookie.value()) else {
        return false;
    };
    if user_id == repo_owner_id {
        return true;
    }
    let collaborator: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT repo_id FROM collaborators WHERE repo_id = $1 AND user_id = $2")
            .bind(repo_id)
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    collaborator.is_some()
}
