use std::fmt;

use sqlx::PgExecutor;
use uuid::Uuid;

/// Who performed an audited action.
#[derive(Debug, Clone, Copy)]
pub enum Actor {
    User(Uuid),
    Ai,
    System,
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Actor::User(id) => write!(f, "user:{id}"),
            Actor::Ai => f.write_str("ai"),
            Actor::System => f.write_str("system"),
        }
    }
}

pub struct AuditEvent<'a> {
    pub workspace_id: Option<Uuid>,
    pub actor: Actor,
    pub action: &'a str,
    pub target: Option<String>,
    pub detail: serde_json::Value,
}

/// Appends to the audit log. Pass a transaction to make the entry atomic with the change.
pub async fn record<'e, E: PgExecutor<'e>>(
    executor: E,
    event: AuditEvent<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO audit_log (workspace_id, actor, action, target, detail) VALUES ($1, $2, $3, $4, $5)",
        event.workspace_id,
        event.actor.to_string(),
        event.action,
        event.target,
        event.detail,
    )
    .execute(executor)
    .await?;
    Ok(())
}
