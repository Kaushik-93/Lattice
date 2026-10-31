use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use lattice_project::{NewProject, ProjectDetail, ProjectSummary};
use lattice_security::{Principal, ProjectRole};
use lattice_telemetry::ServerTiming;
use uuid::Uuid;

use crate::auth::Authed;
use crate::auth::password::required_text;
use crate::dto::{CreateProjectRequest, ProjectListResponse};
use crate::error::ApiError;
use crate::state::AppState;

pub async fn list(
    State(state): State<AppState>,
    Extension(timing): Extension<ServerTiming>,
    Authed(me): Authed,
) -> Result<Json<ProjectListResponse>, ApiError> {
    let projects = timing
        .measure(
            "db",
            lattice_project::list_for_user(&state.pool, me.workspace_id, me.user_id),
        )
        .await?;
    Ok(Json(ProjectListResponse { projects }))
}

/// Any workspace member may create a project; they become its admin ("Owner").
pub async fn create(
    State(state): State<AppState>,
    Extension(timing): Extension<ServerTiming>,
    Authed(me): Authed,
    Json(req): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<ProjectSummary>), ApiError> {
    let name = required_text(&req.name, "Project name", 80)?;
    let description = req.description.trim();
    if description.chars().count() > 500 {
        return Err(ApiError::BadRequest(
            "Description must be at most 500 characters.".into(),
        ));
    }
    let detail = timing
        .measure("db", async {
            let id = lattice_project::create(
                &state.pool,
                NewProject {
                    workspace_id: me.workspace_id,
                    created_by: me.user_id,
                    name,
                    description,
                },
            )
            .await?;
            lattice_project::detail(&state.pool, me.workspace_id, id, me.user_id).await
        })
        .await?
        .ok_or_else(|| ApiError::internal("created project is not visible to its creator"))?;
    Ok((StatusCode::CREATED, Json(detail.project)))
}

pub async fn get(
    State(state): State<AppState>,
    Extension(timing): Extension<ServerTiming>,
    Authed(me): Authed,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ProjectDetail>, ApiError> {
    timing
        .measure(
            "rbac",
            require_role(&state, &me, project_id, ProjectRole::Viewer),
        )
        .await?;
    let detail = timing
        .measure(
            "db",
            lattice_project::detail(&state.pool, me.workspace_id, project_id, me.user_id),
        )
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(detail))
}

/// RBAC gate for every project-scoped call. Non-members get 404 rather than 403 so
/// project ids can't be probed for existence.
pub async fn require_role(
    state: &AppState,
    me: &Principal,
    project_id: Uuid,
    minimum: ProjectRole,
) -> Result<ProjectRole, ApiError> {
    match lattice_project::role_of(&state.pool, me.workspace_id, project_id, me.user_id).await? {
        Some(role) if role >= minimum => Ok(role),
        Some(_) => Err(ApiError::Forbidden),
        None => Err(ApiError::NotFound),
    }
}
