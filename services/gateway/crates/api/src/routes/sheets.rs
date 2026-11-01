use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use lattice_security::audit::{self, Actor, AuditEvent};
use lattice_security::{ProjectRole, SessionToken};
use lattice_sheets::{self as sheets, SaveSheet, SharingUpdate, Sheet, SheetSharing};
use serde_json::json;
use uuid::Uuid;

use crate::auth::{Authed, MaybeAuthed};
use crate::dto::{
    AddSheetMemberRequest, CreateSheetRequest, ReorderSheetsRequest, SaveSheetResponse,
    SharedSheetView, SheetListResponse,
};
use crate::error::ApiError;
use crate::routes::projects::require_role;
use crate::sheets::{data_refreshed_at, open_link};
use crate::state::AppState;

pub async fn list(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(project_id): Path<Uuid>,
) -> Result<Json<SheetListResponse>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Viewer).await?;
    Ok(Json(SheetListResponse {
        sheets: sheets::list(&state.pool, project_id).await?,
    }))
}

pub async fn create(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(project_id): Path<Uuid>,
    Json(req): Json<CreateSheetRequest>,
) -> Result<(StatusCode, Json<Sheet>), ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    let id = sheets::create(&state.pool, project_id, &req.name, &req.filters, me.user_id).await?;
    let sheet = sheets::get(&state.pool, project_id, id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok((StatusCode::CREATED, Json(sheet)))
}

pub async fn get(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, sheet_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Sheet>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Viewer).await?;
    sheets::get(&state.pool, project_id, sheet_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

/// Autosave: replaces the sheet's name, filters, visuals and layout. 409 when someone else
/// saved since `version`.
pub async fn save(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, sheet_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<SaveSheet>,
) -> Result<Json<SaveSheetResponse>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    let version = sheets::save(&state.pool, project_id, sheet_id, &req, me.user_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(SaveSheetResponse { version }))
}

pub async fn delete(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, sheet_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    if !sheets::delete(&state.pool, project_id, sheet_id).await? {
        return Err(ApiError::NotFound);
    }
    audit::record(
        &state.pool,
        AuditEvent {
            workspace_id: Some(me.workspace_id),
            actor: Actor::User(me.user_id),
            action: "sheet.delete",
            target: Some(format!("sheet:{sheet_id}")),
            detail: json!({ "project_id": project_id }),
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn reorder(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(project_id): Path<Uuid>,
    Json(req): Json<ReorderSheetsRequest>,
) -> Result<StatusCode, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    if req.ids.len() > sheets::MAX_SHEETS_PER_PROJECT as usize {
        return Err(ApiError::BadRequest("Too many sheets.".into()));
    }
    sheets::reorder(&state.pool, project_id, &req.ids).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_sharing(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, sheet_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<SheetSharing>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    sheets::sharing(&state.pool, project_id, sheet_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

/// Link settings (design screen 07). Editors may share sheets by link.
pub async fn set_sharing(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, sheet_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<SharingUpdate>,
) -> Result<Json<SheetSharing>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    if req.expires_in_days.is_some_and(|d| d == 0 || d > 3650) {
        return Err(ApiError::BadRequest(
            "A link expires in 1–3650 days, or never.".into(),
        ));
    }
    let token = SessionToken::generate();
    if !sheets::set_sharing(&state.pool, project_id, sheet_id, &req, token.as_str()).await? {
        return Err(ApiError::NotFound);
    }
    audit::record(
        &state.pool,
        AuditEvent {
            workspace_id: Some(me.workspace_id),
            actor: Actor::User(me.user_id),
            action: "sheet.share",
            target: Some(format!("sheet:{sheet_id}")),
            detail: json!({
                "project_id": project_id,
                "audience": req.audience,
                "expires_in_days": req.expires_in_days,
                "rotated": req.rotate,
                "allow_export": req.allow_export,
            }),
        },
    )
    .await?;
    sheets::sharing(&state.pool, project_id, sheet_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

/// Invites someone from the workspace to this sheet only.
pub async fn add_member(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, sheet_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<AddSheetMemberRequest>,
) -> Result<Json<SheetSharing>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    let email = req.email.trim().to_lowercase();
    // Only people who already have an account in this workspace (there is no email delivery
    // for outside invitations yet).
    let Some(user_id) = sqlx::query_scalar!(
        "SELECT u.id FROM users u JOIN workspace_members m ON m.user_id = u.id
         WHERE u.email = $1 AND m.workspace_id = $2",
        email,
        me.workspace_id
    )
    .fetch_optional(&state.pool)
    .await?
    else {
        return Err(ApiError::Unprocessable(format!(
            "{email} isn't in this workspace yet."
        )));
    };
    if sheets::get(&state.pool, project_id, sheet_id)
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    sheets::add_member(&state.pool, sheet_id, user_id, me.user_id).await?;
    audit::record(
        &state.pool,
        AuditEvent {
            workspace_id: Some(me.workspace_id),
            actor: Actor::User(me.user_id),
            action: "sheet.invite",
            target: Some(format!("sheet:{sheet_id}")),
            detail: json!({ "project_id": project_id, "user_id": user_id }),
        },
    )
    .await?;
    sheets::sharing(&state.pool, project_id, sheet_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn remove_member(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, sheet_id, user_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<SheetSharing>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    if sheets::get(&state.pool, project_id, sheet_id)
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    sheets::remove_member(&state.pool, sheet_id, user_id).await?;
    sheets::sharing(&state.pool, project_id, sheet_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

/// A sheet opened through its link (design screen 08). Public links need no sign-in.
pub async fn shared(
    State(state): State<AppState>,
    MaybeAuthed(viewer): MaybeAuthed,
    Path(token): Path<String>,
) -> Result<Json<SharedSheetView>, ApiError> {
    let shared = open_link(&state, &token, viewer.as_ref()).await?;
    if shared.track_views {
        sheets::record_view(&state.pool, shared.sheet.id, viewer.map(|v| v.user_id)).await?;
    }
    let data_refreshed_at = data_refreshed_at(&state, &shared.sheet).await?;
    let fiscal_year_start_month =
        crate::semantic::load_for_project(&state, shared.sheet.project_id)
            .await?
            .model
            .fiscal_year_start_month;
    Ok(Json(SharedSheetView {
        fiscal_year_start_month,
        sheet: shared.sheet,
        project_name: shared.project_name,
        shared_by: shared.shared_by,
        expires_at: shared.expires_at,
        allow_export: shared.allow_export,
        data_refreshed_at,
    }))
}
