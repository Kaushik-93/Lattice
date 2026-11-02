//! REST and WebSocket message types. `#[ts(export)]` generates the matching TypeScript in
//! packages/shared-types whenever the tests run; the web app imports only those.

use chrono::{DateTime, Utc};
use lattice_project::ProjectSummary;
use lattice_security::{Principal, ProjectRole, WorkspaceRole};
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

/// Gives someone in the workspace a role in the project (design screen 07).
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct AddProjectMemberRequest {
    pub email: String,
    pub role: ProjectRole,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct UpdateProjectMemberRequest {
    pub role: ProjectRole,
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

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct SheetListResponse {
    pub sheets: Vec<lattice_sheets::SheetSummary>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct CreateSheetRequest {
    pub name: String,
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<lattice_sheets::SheetFilterDef>>")]
    pub filters: Vec<lattice_sheets::SheetFilterDef>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct SaveSheetResponse {
    #[ts(type = "number")]
    pub version: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ReorderSheetsRequest {
    pub ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct AddSheetMemberRequest {
    pub email: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct QueueResponse {
    pub items: Vec<lattice_knowledge::QueueItem>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct RejectNoteRequest {
    pub reason: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct RevertNoteRequest {
    pub version: i32,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct DeprecateNoteRequest {
    pub deprecated: bool,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct SetOwnerRequest {
    #[serde(default)]
    pub owner_id: Option<Uuid>,
    #[serde(default)]
    pub team: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct WorkspaceMember {
    pub user_id: Uuid,
    pub display_name: String,
    pub email: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct WorkspaceMembersResponse {
    pub members: Vec<WorkspaceMember>,
}

/// What a share link opens (design screen 08).
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct SharedSheetView {
    pub sheet: lattice_sheets::Sheet,
    pub project_name: String,
    /// For labelling fiscal periods (1 = January).
    pub fiscal_year_start_month: u32,
    pub shared_by: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub allow_export: bool,
    /// When the newest of the sheet's sources last synced.
    pub data_refreshed_at: Option<DateTime<Utc>>,
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
    /// A sheet's visuals in one message, compiled by the gateway against the sheet's state
    /// (filters, cross-filter selections, drill paths). Results are framed per visual, with
    /// `:compare` appended for a KPI's previous-period query.
    SheetBatch {
        batch_id: u32,
        project_id: Uuid,
        #[serde(default)]
        context: lattice_visuals::SheetContext,
        visuals: Vec<lattice_visuals::VisualRequest>,
    },
    /// The visuals of a sheet opened through its link. Needs no project role and no sign-in
    /// for public links; runs only that sheet's saved visuals.
    SharedBatch {
        batch_id: u32,
        token: String,
        #[serde(default)]
        context: lattice_visuals::SheetContext,
        visuals: Vec<SharedVisualRequest>,
    },
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct SharedVisualRequest {
    pub id: Uuid,
    #[serde(default)]
    pub drill: Vec<lattice_planner::Scalar>,
    #[serde(default)]
    #[ts(optional)]
    pub offset: Option<u32>,
}

/// Gateway → browser JSON messages on `/ws`. Query results arrive as binary frames instead:
/// `[u32 little-endian header length][ResultHeader JSON][Arrow IPC stream]`.
#[derive(Debug, Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ServerMessage {
    Hello {
        gateway_version: String,
        /// None on an anonymous socket (a public sheet link).
        user_id: Option<Uuid>,
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
    /// Changes exactly when the result's data may differ (see `query::fingerprint`).
    pub fingerprint: String,
    pub timings: QueryTimings,
    /// Business logic used that isn't approved yet ("Uses unapproved logic").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<UnapprovedLogic>>", optional)]
    pub unapproved: Vec<UnapprovedLogic>,
}

/// A draft or pending definition a result used.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UnapprovedLogic {
    pub note_id: Uuid,
    pub title: String,
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

/// A computable note as the project's model uses it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ModelDefinition {
    pub note_id: Uuid,
    pub title: String,
    pub version: i32,
    pub standing: lattice_semantic::Standing,
    /// Why it can't be used in this project yet (a bad formula, an unbound entity…).
    pub problem: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ModelTable {
    pub table_id: Uuid,
    pub name: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ModelEntity {
    #[serde(flatten)]
    pub definition: ModelDefinition,
    /// What formulas call it: "Product category" → `product_category`.
    pub ident: String,
    pub table: Option<ModelTable>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ModelMetric {
    #[serde(flatten)]
    pub definition: ModelDefinition,
    pub formula: Option<String>,
    /// The entity it counts over, once it checks out.
    pub grain: Option<String>,
    /// Other definitions it uses.
    pub uses: Vec<Uuid>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ModelDimension {
    #[serde(flatten)]
    pub definition: ModelDefinition,
    pub path: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ModelRule {
    #[serde(flatten)]
    pub definition: ModelDefinition,
    pub condition: Option<String>,
    /// Applies to every query that reaches its entity.
    pub always: bool,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ModelRelationship {
    #[serde(flatten)]
    pub definition: ModelDefinition,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// The semantic model one project sees (docs/adr/0006).
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ProjectModelResponse {
    /// 1 = January.
    pub fiscal_year_start_month: u32,
    pub entities: Vec<ModelEntity>,
    pub metrics: Vec<ModelMetric>,
    pub dimensions: Vec<ModelDimension>,
    pub rules: Vec<ModelRule>,
    pub relationships: Vec<ModelRelationship>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct BindEntityRequest {
    pub table_id: Uuid,
}

/// One example case of a definition, run against its own rows.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CaseResult {
    pub name: String,
    pub passed: bool,
    pub expected: Option<f64>,
    pub actual: Option<f64>,
    /// Why it couldn't be computed (a column the rows lack, a bad formula…).
    pub error: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CaseRunResponse {
    /// The revision the cases belong to.
    pub version: i32,
    pub results: Vec<CaseResult>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ImpactVisual {
    pub visual_id: Uuid,
    pub title: String,
    pub sheet_id: Uuid,
    pub sheet_name: String,
    pub project_id: Uuid,
    pub project_name: String,
}

/// What approving a change to a definition touches.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ImpactResponse {
    /// Definitions that use it, directly or through others.
    pub dependents: Vec<lattice_knowledge::NoteRef>,
    /// Visuals charting it or a dependent, in projects the viewer can open.
    pub visuals: Vec<ImpactVisual>,
    /// Affected visuals in projects the viewer can't open.
    pub hidden_visuals: u32,
}

/// Knowledge suggested from data profiles, for one project (docs/adr/0006).
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ProjectSuggestions {
    pub project_id: Uuid,
    pub project_name: String,
    pub items: Vec<lattice_knowledge::capture::Suggestion>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct SuggestionsResponse {
    /// Projects the viewer can edit that have open suggestions.
    pub projects: Vec<ProjectSuggestions>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct SuggestionRequest {
    pub key: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct AcceptedSuggestion {
    pub note_id: Uuid,
}
