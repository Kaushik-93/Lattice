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

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct SourceListResponse {
    pub sources: Vec<lattice_catalog::SourceSummary>,
}

/// Adds a PostgreSQL or MySQL database as a live source.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct CreateDatabaseSourceRequest {
    /// `postgres` or `mysql`.
    pub kind: lattice_connectors::SourceKind,
    pub name: String,
    pub config: lattice_connectors::DatabaseConfig,
    pub password: String,
}

/// One query in a batch. `id` is chosen by the client and echoed in its result.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct BatchQuery {
    pub id: String,
    pub query: lattice_planner::TableQuery,
}

/// Browser → gateway messages on `/ws`.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ClientMessage {
    Ping {
        id: u32,
        client_time_ms: f64,
    },
    /// All queries of a view in one message (spec §3 rule 7). Sending a new batch cancels the
    /// previous one end to end (rule 6): the newest filter state wins.
    QueryBatch {
        batch_id: u32,
        project_id: Uuid,
        queries: Vec<BatchQuery>,
    },
    Cancel {
        batch_id: u32,
    },
}

/// Gateway → browser JSON messages on `/ws`. Query results arrive as binary frames instead:
/// `[u32 little-endian header length][ResultHeader JSON][Arrow IPC stream]`.
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
    QueryError {
        batch_id: u32,
        query_id: String,
        code: String,
        message: String,
    },
    /// Every query of the batch has answered (or the batch was cancelled).
    BatchDone {
        batch_id: u32,
        total_ms: f64,
        cancelled: bool,
    },
    Error {
        code: String,
        message: String,
    },
}

/// Precedes each result's Arrow data in a binary frame.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ResultHeader {
    pub batch_id: u32,
    pub query_id: String,
    pub rows: u32,
    /// More rows matched than were returned (see the query's limit).
    pub truncated: bool,
    /// Served from the result cache.
    pub cached: bool,
    pub columns: Vec<lattice_planner::OutputColumn>,
    pub timings: QueryTimings,
}

/// Where the gateway spent its time on one query (spec §3 rule 8). The browser adds transfer,
/// decode and render.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueryTimings {
    pub compile_ms: f64,
    pub cache_ms: f64,
    pub queue_ms: f64,
    pub execute_ms: f64,
    pub serialize_ms: f64,
    pub total_ms: f64,
}
