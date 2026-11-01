//! Projects and their membership. Sheets, visuals and layouts join this crate in phase 2.

use chrono::{DateTime, Utc};
use lattice_security::audit::{self, Actor, AuditEvent};
use lattice_security::rbac::{ProjectRole, UnknownRole};
use serde::Serialize;
use sqlx::PgPool;
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("corrupt role in database: {0}")]
    CorruptRole(#[from] UnknownRole),
}

/// Tile colours from the design, assigned round-robin by project count.
const PALETTE: [&str; 6] = [
    "#0F766E", "#4338CA", "#B45309", "#7C3AED", "#0E7490", "#BE185D",
];

/// One card on the "All projects" screen.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProjectSummary {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub color: String,
    pub my_role: ProjectRole,
    pub owned_by_me: bool,
    pub updated_at: DateTime<Utc>,
    #[ts(type = "number")]
    pub member_count: i64,
    /// Initials of up to three members, for the avatar stack.
    pub member_initials: Vec<String>,
    // Counts the cards show; sheets (phase 2) and definitions (phase 3) stay zero until those exist.
    #[ts(type = "number")]
    pub source_count: i64,
    #[ts(type = "number")]
    pub sheet_count: i64,
    #[ts(type = "number")]
    pub definition_count: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProjectMember {
    pub user_id: Uuid,
    pub display_name: String,
    pub email: String,
    pub initials: String,
    pub role: ProjectRole,
    pub is_owner: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProjectDetail {
    pub project: ProjectSummary,
    pub members: Vec<ProjectMember>,
}

pub fn initials(display_name: &str) -> String {
    let mut parts = display_name.split_whitespace();
    let first = parts.next().and_then(|w| w.chars().next());
    let last = parts.last().and_then(|w| w.chars().next());
    first
        .into_iter()
        .chain(last)
        .flat_map(char::to_uppercase)
        .collect()
}

/// Projects in `workspace_id` that `user_id` is a member of, most recently updated first.
pub async fn list_for_user(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<ProjectSummary>, ProjectError> {
    let rows = sqlx::query!(
        r#"SELECT p.id, p.name, p.description, p.color, p.updated_at,
                  me.role AS my_role,
                  (p.created_by IS NOT DISTINCT FROM $2) AS "owned_by_me!",
                  (SELECT count(*) FROM project_members m WHERE m.project_id = p.id) AS "member_count!",
                  (SELECT count(*) FROM sources s WHERE s.project_id = p.id) AS "source_count!",
                  (SELECT count(*) FROM sheets sh WHERE sh.project_id = p.id) AS "sheet_count!",
                  ARRAY(SELECT u.display_name FROM project_members m JOIN users u ON u.id = m.user_id
                        WHERE m.project_id = p.id ORDER BY m.created_at LIMIT 3) AS "member_names!"
           FROM projects p
           JOIN project_members me ON me.project_id = p.id AND me.user_id = $2
           WHERE p.workspace_id = $1
           ORDER BY p.updated_at DESC"#,
        workspace_id,
        user_id,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|r| {
            Ok(ProjectSummary {
                id: r.id,
                name: r.name,
                description: r.description,
                color: r.color,
                my_role: r.my_role.parse()?,
                owned_by_me: r.owned_by_me,
                updated_at: r.updated_at,
                member_count: r.member_count,
                member_initials: r.member_names.iter().map(|n| initials(n)).collect(),
                source_count: r.source_count,
                sheet_count: r.sheet_count,
                definition_count: 0,
            })
        })
        .collect()
}

pub struct NewProject<'a> {
    pub workspace_id: Uuid,
    pub created_by: Uuid,
    pub name: &'a str,
    pub description: &'a str,
}

/// Creates a project; its creator becomes its first admin (shown as "Owner").
pub async fn create(pool: &PgPool, new: NewProject<'_>) -> Result<Uuid, ProjectError> {
    let mut tx = pool.begin().await?;
    let existing = sqlx::query_scalar!(
        r#"SELECT count(*) AS "n!" FROM projects WHERE workspace_id = $1"#,
        new.workspace_id
    )
    .fetch_one(&mut *tx)
    .await?;
    let color = PALETTE[usize::try_from(existing).unwrap_or(0) % PALETTE.len()];

    let id = sqlx::query_scalar!(
        "INSERT INTO projects (workspace_id, name, description, color, created_by)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
        new.workspace_id,
        new.name,
        new.description,
        color,
        new.created_by,
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO project_members (project_id, user_id, role) VALUES ($1, $2, 'admin')",
        id,
        new.created_by,
    )
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut *tx,
        AuditEvent {
            workspace_id: Some(new.workspace_id),
            actor: Actor::User(new.created_by),
            action: "project.create",
            target: Some(format!("project:{id}")),
            detail: serde_json::json!({ "name": new.name }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn add_member(
    pool: &PgPool,
    project_id: Uuid,
    user_id: Uuid,
    role: ProjectRole,
) -> Result<(), ProjectError> {
    sqlx::query!(
        "INSERT INTO project_members (project_id, user_id, role) VALUES ($1, $2, $3)
         ON CONFLICT (project_id, user_id) DO UPDATE SET role = EXCLUDED.role",
        project_id,
        user_id,
        role.as_str(),
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn remove_member(
    pool: &PgPool,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<bool, ProjectError> {
    let done = sqlx::query!(
        "DELETE FROM project_members WHERE project_id = $1 AND user_id = $2",
        project_id,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Who created the project (its "Owner", who always stays an admin), and how many admins it has.
pub async fn owner_and_admins(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<(Option<Uuid>, i64), ProjectError> {
    let row = sqlx::query!(
        r#"SELECT p.created_by,
                  (SELECT count(*) FROM project_members m
                   WHERE m.project_id = p.id AND m.role = 'admin') AS "admins!"
           FROM projects p WHERE p.id = $1"#,
        project_id
    )
    .fetch_one(pool)
    .await?;
    Ok((row.created_by, row.admins))
}

/// The caller's role in every project of their workspace they belong to.
pub async fn roles_for_user(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<std::collections::HashMap<Uuid, ProjectRole>, ProjectError> {
    let rows = sqlx::query!(
        "SELECT m.project_id, m.role FROM project_members m JOIN projects p ON p.id = m.project_id
         WHERE m.user_id = $1 AND p.workspace_id = $2",
        user_id,
        workspace_id,
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|r| Ok((r.project_id, r.role.parse()?)))
        .collect()
}

/// The caller's role in a project, scoped to their workspace. `None` means no access,
/// including when the project exists in another workspace (no existence oracle).
pub async fn role_of(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<Option<ProjectRole>, ProjectError> {
    let role = sqlx::query_scalar!(
        "SELECT m.role FROM project_members m JOIN projects p ON p.id = m.project_id
         WHERE m.project_id = $1 AND m.user_id = $2 AND p.workspace_id = $3",
        project_id,
        user_id,
        workspace_id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(role.map(|r| r.parse()).transpose()?)
}

/// Full project view for a caller whose access has already been checked.
pub async fn detail(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<Option<ProjectDetail>, ProjectError> {
    let Some(project) = list_for_user(pool, workspace_id, user_id)
        .await?
        .into_iter()
        .find(|p| p.id == project_id)
    else {
        return Ok(None);
    };

    let members = sqlx::query!(
        r#"SELECT u.id AS user_id, u.display_name, u.email, m.role,
                  (p.created_by IS NOT DISTINCT FROM u.id) AS "is_owner!"
           FROM project_members m
           JOIN users u ON u.id = m.user_id
           JOIN projects p ON p.id = m.project_id
           WHERE m.project_id = $1
           ORDER BY (p.created_by IS NOT DISTINCT FROM u.id) DESC,
                    CASE m.role WHEN 'admin' THEN 0 WHEN 'editor' THEN 1 ELSE 2 END,
                    u.display_name"#,
        project_id,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| {
        Ok(ProjectMember {
            initials: initials(&r.display_name),
            user_id: r.user_id,
            display_name: r.display_name,
            email: r.email,
            role: r.role.parse()?,
            is_owner: r.is_owner,
        })
    })
    .collect::<Result<Vec<_>, ProjectError>>()?;

    Ok(Some(ProjectDetail { project, members }))
}

#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn initials_from_names() {
        assert_eq!(initials("Kaushik S."), "KS");
        assert_eq!(initials("priya"), "P");
        assert_eq!(initials("Ana Lucia Lopez"), "AL");
        assert_eq!(initials(""), "");
    }
}
