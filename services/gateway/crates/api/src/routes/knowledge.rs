//! The knowledge graph (docs/adr/0006): notes, their revisions and the review workflow.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use lattice_knowledge::{
    self as knowledge, KnowledgeGraph, NewNote, NoteDetail, NoteEdit, Provenance, QueueItem,
    RevisionStatus, Viewer,
};
use lattice_security::{Principal, ProjectRole};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::Authed;
use crate::capture;
use crate::dto::{
    AcceptedSuggestion, CaseRunResponse, DeprecateNoteRequest, ImpactResponse, ProjectSuggestions,
    QueueResponse, RejectNoteRequest, RevertNoteRequest, SetOwnerRequest, SuggestionRequest,
    SuggestionsResponse, WorkspaceMember, WorkspaceMembersResponse,
};
use crate::error::ApiError;
use crate::review;
use crate::routes::projects::require_role;
use crate::state::AppState;

async fn viewer(state: &AppState, me: &Principal) -> Result<Viewer, ApiError> {
    Ok(Viewer {
        user_id: me.user_id,
        workspace_id: me.workspace_id,
        workspace_role: me.workspace_role,
        projects: lattice_project::roles_for_user(&state.pool, me.workspace_id, me.user_id).await?,
    })
}

#[derive(Debug, Deserialize)]
pub struct GraphParams {
    pub project: Option<Uuid>,
}

pub async fn graph(
    State(state): State<AppState>,
    Authed(me): Authed,
    Query(params): Query<GraphParams>,
) -> Result<Json<KnowledgeGraph>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    if let Some(p) = params.project
        && !viewer.projects.contains_key(&p)
    {
        return Err(ApiError::NotFound);
    }
    Ok(Json(
        knowledge::graph(&state.pool, &viewer, params.project).await?,
    ))
}

pub async fn queue(
    State(state): State<AppState>,
    Authed(me): Authed,
) -> Result<Json<QueueResponse>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    let items: Vec<QueueItem> = knowledge::queue(&state.pool, &viewer).await?;
    Ok(Json(QueueResponse { items }))
}

pub async fn get(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(note_id): Path<Uuid>,
) -> Result<Json<NoteDetail>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    Ok(Json(
        knowledge::detail(&state.pool, &viewer, note_id).await?,
    ))
}

pub async fn create(
    State(state): State<AppState>,
    Authed(me): Authed,
    Json(req): Json<NewNote>,
) -> Result<(StatusCode, Json<NoteDetail>), ApiError> {
    let viewer = viewer(&state, &me).await?;
    let id = knowledge::create(&state.pool, &viewer, &req, Provenance::manual(&viewer)).await?;
    Ok((
        StatusCode::CREATED,
        Json(knowledge::detail(&state.pool, &viewer, id).await?),
    ))
}

pub async fn save_draft(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(note_id): Path<Uuid>,
    Json(req): Json<NoteEdit>,
) -> Result<Json<NoteDetail>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    knowledge::save_draft(&state.pool, &viewer, note_id, &req).await?;
    Ok(Json(
        knowledge::detail(&state.pool, &viewer, note_id).await?,
    ))
}

pub async fn submit(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(note_id): Path<Uuid>,
) -> Result<Json<NoteDetail>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    knowledge::submit(&state.pool, &viewer, note_id).await?;
    Ok(Json(
        knowledge::detail(&state.pool, &viewer, note_id).await?,
    ))
}

/// Approves the pending change, once its example cases pass (docs/adr/0006 decision 6).
pub async fn approve(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(note_id): Path<Uuid>,
) -> Result<Json<NoteDetail>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    let note = knowledge::detail(&state.pool, &viewer, note_id).await?;
    if !note.can.review {
        return Err(ApiError::Forbidden);
    }
    let pending = note
        .open
        .as_ref()
        .filter(|r| r.status == RevisionStatus::Pending);
    let version = match pending {
        Some(revision) => {
            let results = review::run_cases(&state, me.workspace_id, &note, revision).await?;
            let failing: Vec<&str> = results
                .iter()
                .filter(|r| !r.passed)
                .map(|r| r.name.as_str())
                .collect();
            if !failing.is_empty() {
                return Err(ApiError::Conflict(format!(
                    "Example cases must pass before this can be approved. Failing: {}.",
                    failing.join(", ")
                )));
            }
            Some(revision.version)
        }
        None => None,
    };
    knowledge::approve_version(&state.pool, &viewer, note_id, version).await?;
    Ok(Json(
        knowledge::detail(&state.pool, &viewer, note_id).await?,
    ))
}

/// Runs the example cases of the change under way, or else of the approved version.
pub async fn run_cases(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(note_id): Path<Uuid>,
) -> Result<Json<CaseRunResponse>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    let note = knowledge::detail(&state.pool, &viewer, note_id).await?;
    let Some(revision) = note.open.as_ref().or(note.live.as_ref()) else {
        return Err(ApiError::NotFound);
    };
    let results = review::run_cases(&state, me.workspace_id, &note, revision).await?;
    Ok(Json(CaseRunResponse {
        version: revision.version,
        results,
    }))
}

/// What approving a change to this note would touch.
pub async fn impact(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(note_id): Path<Uuid>,
) -> Result<Json<ImpactResponse>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    let note = knowledge::detail(&state.pool, &viewer, note_id).await?;
    Ok(Json(review::impact(&state, &viewer, &note).await?))
}

pub async fn reject(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(note_id): Path<Uuid>,
    Json(req): Json<RejectNoteRequest>,
) -> Result<Json<NoteDetail>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    knowledge::reject(&state.pool, &viewer, note_id, &req.reason).await?;
    Ok(Json(
        knowledge::detail(&state.pool, &viewer, note_id).await?,
    ))
}

/// 204 when the note itself went too (it was only ever a draft).
pub async fn discard(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(note_id): Path<Uuid>,
) -> Result<(StatusCode, Json<Option<NoteDetail>>), ApiError> {
    let viewer = viewer(&state, &me).await?;
    if knowledge::discard(&state.pool, &viewer, note_id).await? {
        return Ok((StatusCode::OK, Json(None)));
    }
    Ok((
        StatusCode::OK,
        Json(Some(
            knowledge::detail(&state.pool, &viewer, note_id).await?,
        )),
    ))
}

pub async fn revert(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(note_id): Path<Uuid>,
    Json(req): Json<RevertNoteRequest>,
) -> Result<Json<NoteDetail>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    knowledge::revert(&state.pool, &viewer, note_id, req.version).await?;
    Ok(Json(
        knowledge::detail(&state.pool, &viewer, note_id).await?,
    ))
}

pub async fn deprecate(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(note_id): Path<Uuid>,
    Json(req): Json<DeprecateNoteRequest>,
) -> Result<Json<NoteDetail>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    knowledge::set_deprecated(&state.pool, &viewer, note_id, req.deprecated).await?;
    Ok(Json(
        knowledge::detail(&state.pool, &viewer, note_id).await?,
    ))
}

pub async fn set_owner(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(note_id): Path<Uuid>,
    Json(req): Json<SetOwnerRequest>,
) -> Result<Json<NoteDetail>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    knowledge::set_owner(
        &state.pool,
        &viewer,
        note_id,
        req.owner_id,
        req.team.as_deref(),
    )
    .await?;
    Ok(Json(
        knowledge::detail(&state.pool, &viewer, note_id).await?,
    ))
}

/// People in the caller's workspace (owner pickers).
pub async fn workspace_members(
    State(state): State<AppState>,
    Authed(me): Authed,
) -> Result<Json<WorkspaceMembersResponse>, ApiError> {
    let members = sqlx::query_as!(
        WorkspaceMember,
        "SELECT u.id AS user_id, u.display_name, u.email FROM workspace_members m
         JOIN users u ON u.id = m.user_id WHERE m.workspace_id = $1 ORDER BY u.display_name",
        me.workspace_id
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(WorkspaceMembersResponse { members }))
}

/// Open suggestions in every project the viewer can edit.
pub async fn suggestions(
    State(state): State<AppState>,
    Authed(me): Authed,
) -> Result<Json<SuggestionsResponse>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    let projects = lattice_project::list_for_user(&state.pool, me.workspace_id, me.user_id).await?;
    let mut out = Vec::new();
    for p in projects
        .into_iter()
        .filter(|p| p.my_role >= ProjectRole::Editor)
    {
        let items = capture::open(&state, &viewer, p.id).await?;
        if !items.is_empty() {
            out.push(ProjectSuggestions {
                project_id: p.id,
                project_name: p.name,
                items,
            });
        }
    }
    Ok(Json(SuggestionsResponse { projects: out }))
}

pub async fn accept_suggestion(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(project_id): Path<Uuid>,
    Json(req): Json<SuggestionRequest>,
) -> Result<Json<AcceptedSuggestion>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    let viewer = viewer(&state, &me).await?;
    let note_id = capture::accept(&state, &viewer, project_id, &req.key).await?;
    Ok(Json(AcceptedSuggestion { note_id }))
}

pub async fn dismiss_suggestion(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(project_id): Path<Uuid>,
    Json(req): Json<SuggestionRequest>,
) -> Result<StatusCode, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    let viewer = viewer(&state, &me).await?;
    capture::dismiss(&state, &viewer, project_id, &req.key).await?;
    Ok(StatusCode::NO_CONTENT)
}
