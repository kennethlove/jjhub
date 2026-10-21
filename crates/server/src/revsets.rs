// story: e11s01
//! Revset search endpoint: `GET /{owner}/{name}/revsets?q=<revset>`.
//!
//! Read-only jj_lib revset evaluation over the repo's meta workspace.
//! Private repos are invisible to non-collaborators (404, same as unknown).

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use axum_extra::extract::CookieJar;

use crate::jj_meta::{self, RevsetQueryError};
use crate::AppState;

/// story: e11s01 — hard server-side cap on result count.
const RESULT_CAP: usize = 50;

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

    let repo: Option<(uuid::Uuid, uuid::Uuid, String, String)> = sqlx::query_as(
        "SELECT r.id, r.owner_id, r.visibility, r.trunk FROM repositories r \
         JOIN users u ON u.id = r.owner_id WHERE u.handle = $1 AND r.name = $2",
    )
    .bind(&owner)
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some((repo_id, repo_owner_id, visibility, trunk)) = repo else {
        return Err((StatusCode::NOT_FOUND, "repository not found".to_string()));
    };

    // story: e11s01 — a private repo is indistinguishable from a missing one
    // to strangers.
    if visibility == "private" && !is_visible_to(&state, repo_id, repo_owner_id, &jar).await {
        return Err((StatusCode::NOT_FOUND, "repository not found".to_string()));
    }

    let meta = domain::meta_path(&state.storage_root, &owner, &name);
    let hits = jj_meta::revset_query(&meta, &trunk, &query, RESULT_CAP)
        .await
        .map_err(|e| match e {
            RevsetQueryError::Invalid(message) => (StatusCode::BAD_REQUEST, message),
            RevsetQueryError::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        })?;
    Ok(Json(hits))
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
