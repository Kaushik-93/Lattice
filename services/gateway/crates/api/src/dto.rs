//! REST and WebSocket message types. `#[ts(export)]` generates the matching TypeScript in
//! packages/shared-types whenever the tests run; the web app imports only those.

use chrono::{DateTime, Utc};
use lattice_project::ProjectSummary;
use lattice_security::{Principal, WorkspaceRole};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ApiErrorBody {
    pub error: ApiErrorDetail,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ApiErrorDetail {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct SignupRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
    pub workspace_name: String,
    #[serde(default)]
    pub remember: bool,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    /// "Keep me signed in for 30 days". Otherwise the session ends with the browser
    /// session (and at most after 12 hours).
    #[serde(default)]
    pub remember: bool,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct UserInfo {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub initials: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct WorkspaceInfo {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MeResponse {
    pub user: UserInfo,
    pub workspace: WorkspaceInfo,
    pub workspace_role: WorkspaceRole,
    pub session_expires_at: DateTime<Utc>,
}

impl From<Principal> for MeResponse {
    fn from(p: Principal) -> Self {
        Self {
            user: UserInfo {
                initials: lattice_project::initials(&p.display_name),
                id: p.user_id,
                email: p.email,
                display_name: p.display_name,
            },
            workspace: WorkspaceInfo {
                id: p.workspace_id,
                name: p.workspace_name,
                slug: p.workspace_slug,
            },
            workspace_role: p.workspace_role,
            session_expires_at: p.session_expires_at,
        }
    }
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct AuthProvider {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct AuthProvidersResponse {
    /// Identity providers configured on this deployment, in display order.
    pub providers: Vec<AuthProvider>,
    pub password: bool,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ProjectListResponse {
    pub projects: Vec<ProjectSummary>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct CreateProjectRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ReadyStatus {
    /// Every dependency is up.
    Ready,
    /// Required dependencies are up; optional ones (AI, object storage) are not.
    Degraded,
    /// A required dependency (Postgres, Redis) is down.
    Unavailable,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct DependencyCheck {
    pub name: String,
    pub ok: bool,
    pub required: bool,
    pub latency_ms: f64,
    pub detail: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ReadyReport {
    pub status: ReadyStatus,
    pub version: String,
    #[ts(type = "number")]
    pub uptime_s: u64,
    pub checks: Vec<DependencyCheck>,
}

/// Browser → gateway control messages on `/ws`. Data frames (Arrow IPC) arrive in phase 1.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ClientMessage {
    Ping { id: u32, client_time_ms: f64 },
}

/// Gateway → browser control messages on `/ws`.
#[derive(Debug, Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ServerMessage {
    Hello {
        gateway_version: String,
        user_id: Uuid,
    },
    Pong {
        id: u32,
        /// Echoed so the client computes round-trip time without keeping state.
        client_time_ms: f64,
        /// Time the gateway spent between receiving the ping and sending the pong.
        server_us: u32,
    },
    Error {
        code: String,
        message: String,
    },
}
