use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use lattice_project::{NewProject, ProjectDetail, ProjectSummary};
use lattice_security::audit::{self, Actor, AuditEvent};
use lattice_security::{Principal, ProjectRole};
use lattice_telemetry::ServerTiming;
use uuid::Uuid;

use crate::auth::Authed;
use crate::auth::password::required_text;
use crate::dto::{
    AddProjectMemberRequest, CreateProjectRequest, ProjectListResponse, UpdateProjectMemberRequest,
};
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

/// Adds someone from the workspace to the project, or changes their role if already a member.
pub async fn add_member(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(project_id): Path<Uuid>,
    Json(req): Json<AddProjectMemberRequest>,
) -> Result<Json<ProjectDetail>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Admin).await?;
    let email = req.email.trim().to_lowercase();
    // Only people with an account in this workspace (no email delivery for invitations yet).
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
    change_role(
        &state,
        &me,
        project_id,
        user_id,
        req.role,
        "project.member.add",
    )
    .await
}

pub async fn update_member(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, user_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateProjectMemberRequest>,
) -> Result<Json<ProjectDetail>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Admin).await?;
    let current = lattice_project::role_of(&state.pool, me.workspace_id, project_id, user_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let (owner, admins) = lattice_project::owner_and_admins(&state.pool, project_id).await?;
    if req.role != ProjectRole::Admin {
        if owner == Some(user_id) {
            return Err(ApiError::Unprocessable(
                "The project's owner always stays an admin.".into(),
            ));
        }
        if current == ProjectRole::Admin && admins <= 1 {
            return Err(ApiError::Unprocessable(
                "A project needs at least one admin.".into(),
            ));
        }
    }
    change_role(
        &state,
        &me,
        project_id,
        user_id,
        req.role,
        "project.member.role",
    )
    .await
}

pub async fn remove_member(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ProjectDetail>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Admin).await?;
    let current = lattice_project::role_of(&state.pool, me.workspace_id, project_id, user_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let (owner, admins) = lattice_project::owner_and_admins(&state.pool, project_id).await?;
    if owner == Some(user_id) {
        return Err(ApiError::Unprocessable(
            "The project's owner can't be removed.".into(),
        ));
    }
    if current == ProjectRole::Admin && admins <= 1 {
        return Err(ApiError::Unprocessable(
            "A project needs at least one admin.".into(),
        ));
    }
    lattice_project::remove_member(&state.pool, project_id, user_id).await?;
    record_membership(
        &state,
        &me,
        project_id,
        user_id,
        None,
        "project.member.remove",
    )
    .await?;
    detail_of(&state, &me, project_id).await
}

async fn change_role(
    state: &AppState,
    me: &Principal,
    project_id: Uuid,
    user_id: Uuid,
    role: ProjectRole,
    action: &str,
) -> Result<Json<ProjectDetail>, ApiError> {
    lattice_project::add_member(&state.pool, project_id, user_id, role).await?;
    record_membership(state, me, project_id, user_id, Some(role), action).await?;
    detail_of(state, me, project_id).await
}

async fn record_membership(
    state: &AppState,
    me: &Principal,
    project_id: Uuid,
    user_id: Uuid,
    role: Option<ProjectRole>,
    action: &str,
) -> Result<(), ApiError> {
    audit::record(
        &state.pool,
        AuditEvent {
            workspace_id: Some(me.workspace_id),
            actor: Actor::User(me.user_id),
            action,
            target: Some(format!("project:{project_id}")),
            detail: serde_json::json!({ "user_id": user_id, "role": role }),
        },
    )
    .await?;
    Ok(())
}

async fn detail_of(
    state: &AppState,
    me: &Principal,
    project_id: Uuid,
) -> Result<Json<ProjectDetail>, ApiError> {
    lattice_project::detail(&state.pool, me.workspace_id, project_id, me.user_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
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
