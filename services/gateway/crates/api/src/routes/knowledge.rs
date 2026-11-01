//! The knowledge graph (docs/adr/0006): notes, their revisions and the review workflow.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use lattice_knowledge::{
    self as knowledge, KnowledgeGraph, NewNote, NoteDetail, NoteEdit, Provenance, QueueItem, Viewer,
};
use lattice_security::Principal;
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::Authed;
use crate::dto::{
    DeprecateNoteRequest, QueueResponse, RejectNoteRequest, RevertNoteRequest, SetOwnerRequest,
    WorkspaceMember, WorkspaceMembersResponse,
};
use crate::error::ApiError;
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

pub async fn approve(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(note_id): Path<Uuid>,
) -> Result<Json<NoteDetail>, ApiError> {
    let viewer = viewer(&state, &me).await?;
    knowledge::approve(&state.pool, &viewer, note_id).await?;
    Ok(Json(
        knowledge::detail(&state.pool, &viewer, note_id).await?,
    ))
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
