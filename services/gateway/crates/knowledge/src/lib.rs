//! The knowledge graph (docs/adr/0006): business knowledge as linked notes, one graph per
//! workspace. Every change is a revision that moves draft → pending → approved (or rejected);
//! a note's live content is its latest approved revision, so approved knowledge keeps serving
//! while a change to it waits for review.

pub mod capture;
pub mod links;

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use lattice_security::audit::{self, Actor, AuditEvent};
use lattice_security::{ProjectRole, WorkspaceRole};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use ts_rs::TS;
use uuid::Uuid;

pub use links::title_key;

#[derive(Debug, thiserror::Error)]
pub enum KnowledgeError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("corrupt knowledge row: {0}")]
    Corrupt(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("you can't do that with this note")]
    Forbidden,
    #[error("not found")]
    NotFound,
}

type Result<T> = std::result::Result<T, KnowledgeError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NoteKind {
    Entity,
    Metric,
    Dimension,
    Rule,
    Relationship,
    Term,
    Fact,
}

impl NoteKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            NoteKind::Entity => "entity",
            NoteKind::Metric => "metric",
            NoteKind::Dimension => "dimension",
            NoteKind::Rule => "rule",
            NoteKind::Relationship => "relationship",
            NoteKind::Term => "term",
            NoteKind::Fact => "fact",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "entity" => Self::Entity,
            "metric" => Self::Metric,
            "dimension" => Self::Dimension,
            "rule" => Self::Rule,
            "relationship" => Self::Relationship,
            "term" => Self::Term,
            "fact" => Self::Fact,
            other => return Err(KnowledgeError::Corrupt(format!("kind {other:?}"))),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RevisionStatus {
    Draft,
    Pending,
    Approved,
    Rejected,
    /// An approved revision replaced by a newer approved one.
    Superseded,
}

impl RevisionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            RevisionStatus::Draft => "draft",
            RevisionStatus::Pending => "pending",
            RevisionStatus::Approved => "approved",
            RevisionStatus::Rejected => "rejected",
            RevisionStatus::Superseded => "superseded",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "draft" => Self::Draft,
            "pending" => Self::Pending,
            "approved" => Self::Approved,
            "rejected" => Self::Rejected,
            "superseded" => Self::Superseded,
            other => return Err(KnowledgeError::Corrupt(format!("status {other:?}"))),
        })
    }
}

/// A note's state at a glance (the graph's colours, the panel's pill).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NoteState {
    /// Never approved; being written.
    Draft,
    /// Never approved; waiting for review.
    Pending,
    /// Live, nothing waiting.
    Approved,
    /// Live, with a change being written.
    ChangeDraft,
    /// Live, with a change waiting for review.
    ChangePending,
    /// Never approved; its proposal was turned down.
    Rejected,
    Deprecated,
}

fn state_of(has_live: bool, open: Option<RevisionStatus>, deprecated: bool) -> NoteState {
    match (deprecated, has_live, open) {
        (true, _, _) => NoteState::Deprecated,
        (false, true, Some(RevisionStatus::Pending)) => NoteState::ChangePending,
        (false, true, Some(_)) => NoteState::ChangeDraft,
        (false, true, None) => NoteState::Approved,
        (false, false, Some(RevisionStatus::Pending)) => NoteState::Pending,
        (false, false, Some(_)) => NoteState::Draft,
        (false, false, None) => NoteState::Rejected,
    }
}

/// Who is asking, with what rights. Built by the caller from the session.
#[derive(Debug, Clone)]
pub struct Viewer {
    pub user_id: Uuid,
    pub workspace_id: Uuid,
    pub workspace_role: WorkspaceRole,
    /// The caller's role in each project they belong to.
    pub projects: HashMap<Uuid, ProjectRole>,
}

impl Viewer {
    fn sees(&self, project_id: Option<Uuid>) -> bool {
        project_id.is_none_or(|p| self.projects.contains_key(&p))
    }

    /// Writing drafts: anyone in the workspace for shared notes, editors for a project's own.
    fn can_draft(&self, project_id: Option<Uuid>) -> bool {
        match project_id {
            None => true,
            Some(p) => self
                .projects
                .get(&p)
                .is_some_and(|r| *r >= ProjectRole::Editor),
        }
    }

    /// Approving, rejecting, deprecating: the note's owner or a workspace admin for shared
    /// notes; the owner or a project admin for a project's own (ADR 0006).
    fn can_review(&self, project_id: Option<Uuid>, owner_id: Option<Uuid>) -> bool {
        if owner_id == Some(self.user_id) {
            return true;
        }
        match project_id {
            None => self.workspace_role == WorkspaceRole::Admin,
            Some(p) => self.projects.get(&p) == Some(&ProjectRole::Admin),
        }
    }

    pub fn actor(&self) -> String {
        format!("user:{}", self.user_id)
    }
}

/// What a viewer may do with a note.
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[ts(export)]
pub struct Permissions {
    pub edit: bool,
    pub review: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Revision {
    pub id: Uuid,
    pub version: i32,
    pub title: String,
    pub body: String,
    #[ts(type = "Record<string, unknown> | null")]
    pub definition: Option<serde_json::Value>,
    pub status: RevisionStatus,
    /// "user:<id>", "ai" or "system".
    pub author: String,
    pub author_name: Option<String>,
    #[ts(type = "Record<string, unknown>")]
    pub provenance: serde_json::Value,
    pub change_note: Option<String>,
    pub reviewed_by_name: Option<String>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub review_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct NoteOwner {
    pub user_id: Option<Uuid>,
    pub name: Option<String>,
    pub team: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct NoteRef {
    pub id: Uuid,
    pub kind: NoteKind,
    pub title: String,
    pub state: NoteState,
}

/// A `[[link]]` from a note: to another note, or to a title no note has yet.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct NoteLink {
    pub title: String,
    pub note: Option<NoteRef>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct NoteDetail {
    pub id: Uuid,
    pub kind: NoteKind,
    pub project_id: Option<Uuid>,
    pub state: NoteState,
    pub owner: NoteOwner,
    /// The approved revision everything uses, if any.
    pub live: Option<Revision>,
    /// The change being written or reviewed, if any.
    pub open: Option<Revision>,
    /// Every revision, newest first (bodies omitted for older ones: fetch on demand).
    pub history: Vec<Revision>,
    pub links: Vec<NoteLink>,
    pub backlinks: Vec<NoteRef>,
    pub can: Permissions,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GraphNode {
    pub id: Uuid,
    pub kind: NoteKind,
    pub title: String,
    pub state: NoteState,
    pub project_id: Option<Uuid>,
    pub owner_team: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GraphEdge {
    pub from: Uuid,
    pub to: Uuid,
}

/// A link to a title no note has yet (Obsidian's unresolved links).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct UnresolvedLink {
    pub from: Uuid,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct KnowledgeGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub unresolved: Vec<UnresolvedLink>,
}

/// A change waiting for someone's review, or a draft of the viewer's own.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct QueueItem {
    pub note: NoteRef,
    pub revision: Revision,
    /// The live revision the change would replace.
    pub live: Option<Revision>,
    pub can_review: bool,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct NewNote {
    pub kind: NoteKind,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    #[ts(optional, type = "Record<string, unknown>")]
    pub definition: Option<serde_json::Value>,
    /// None: shared across the workspace.
    #[serde(default)]
    #[ts(optional)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    #[ts(optional)]
    pub owner_team: Option<String>,
    /// Submit for review straight away instead of saving a draft.
    #[serde(default)]
    #[ts(optional)]
    pub submit: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct NoteEdit {
    pub title: String,
    pub body: String,
    #[serde(default)]
    #[ts(optional, type = "Record<string, unknown>")]
    pub definition: Option<serde_json::Value>,
    #[serde(default)]
    #[ts(optional)]
    pub change_note: Option<String>,
}

/// Where a revision came from (ADR 0006). The API sets `manual`; capture paths set their own.
pub struct Provenance {
    pub author: String,
    pub value: serde_json::Value,
}

impl Provenance {
    pub fn manual(viewer: &Viewer) -> Self {
        Self {
            author: viewer.actor(),
            value: serde_json::json!({ "source": "manual" }),
        }
    }
}

fn validate(title: &str, body: &str, change_note: Option<&str>) -> Result<()> {
    let invalid = |m: &str| Err(KnowledgeError::Invalid(m.to_owned()));
    let t = title.trim();
    if t.is_empty() || t.chars().count() > 120 {
        return invalid("A title has 1–120 characters.");
    }
    if t.contains("[[") || t.contains("]]") || t.contains('|') {
        return invalid("A title can't contain [[, ]] or |.");
    }
    if body.chars().count() > 20_000 {
        return invalid("A note's body has at most 20,000 characters.");
    }
    if change_note.is_some_and(|c| c.chars().count() > 500) {
        return invalid("A change note has at most 500 characters.");
    }
    Ok(())
}

/// Checks a computable definition's syntax on save, so a note can't hold a formula the
/// compiler can't read. (Whether its entities and columns exist is checked per project, where
/// they are bound.)
fn validate_definition(kind: NoteKind, definition: Option<&serde_json::Value>) -> Result<()> {
    let Some(d) = definition else { return Ok(()) };
    let invalid = |m: String| Err(KnowledgeError::Invalid(m));
    if !d.is_object() {
        return invalid("A definition is an object.".into());
    }
    let text = |key: &str| {
        d.get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    match kind {
        NoteKind::Metric => {
            if let Some(f) = text("formula")
                && let Err(e) = lattice_semantic::formula(f)
            {
                return invalid(format!("Formula: {e}"));
            }
        }
        NoteKind::Rule => {
            if let Some(c) = text("condition")
                && let Err(e) = lattice_semantic::condition(c)
            {
                return invalid(format!("Condition: {e}"));
            }
            if let Some(m) = d.get("fiscal_year_start_month")
                && !m.as_u64().is_some_and(|m| (1..=12).contains(&m))
            {
                return invalid("The fiscal year starts in month 1–12.".into());
            }
        }
        NoteKind::Relationship => {
            for key in ["from", "to"] {
                match text(key) {
                    Some(p) => {
                        if let Err(e) = lattice_semantic::model::parse_path(p) {
                            return invalid(format!("Relationship {key}: {e}"));
                        }
                    }
                    None => {
                        return invalid(format!("A relationship needs \"{key}\": entity.column."));
                    }
                }
            }
        }
        NoteKind::Dimension => {
            if let Some(p) = text("path")
                && let Err(e) = lattice_semantic::model::parse_path(p)
            {
                return invalid(format!("Dimension: {e}"));
            }
        }
        _ => {}
    }
    validate_cases(kind, d)
}

/// Most example cases a definition carries, and rows per entity in one case.
pub const MAX_CASES: usize = 20;
pub const MAX_CASE_ROWS: usize = 50;

/// Example cases (docs/adr/0006): a few rows per entity and the answer the definition must give
/// on them. `{"name", "rows": {"order": [{"amount": 100, "status": "settled"}]}, "expect": 100}`;
/// a rule's case also names the entity to count (`"count": "order"`) and expects how many of
/// its rows the rule keeps. `"today"` pins relative dates.
fn validate_cases(kind: NoteKind, d: &serde_json::Value) -> Result<()> {
    let Some(cases) = d.get("cases") else {
        return Ok(());
    };
    let invalid = |m: String| Err(KnowledgeError::Invalid(m));
    let Some(cases) = cases.as_array() else {
        return invalid("\"cases\" is a list.".into());
    };
    if !matches!(kind, NoteKind::Metric | NoteKind::Rule) {
        return invalid("Only metrics and rules have example cases.".into());
    }
    if cases.len() > MAX_CASES {
        return invalid(format!("At most {MAX_CASES} example cases."));
    }
    for (i, case) in cases.iter().enumerate() {
        let n = i + 1;
        let name = case.get("name").and_then(|v| v.as_str()).map(str::trim);
        if !name.is_some_and(|s| (1..=120).contains(&s.chars().count())) {
            return invalid(format!(
                "Example case {n} needs a name (up to 120 characters)."
            ));
        }
        let Some(rows) = case.get("rows").and_then(|v| v.as_object()) else {
            return invalid(format!(
                "Example case {n} needs rows: {{\"entity\": [{{…}}]}}."
            ));
        };
        for (entity, list) in rows {
            let Some(list) = list.as_array() else {
                return invalid(format!("Example case {n}: rows of {entity} are a list."));
            };
            if list.len() > MAX_CASE_ROWS {
                return invalid(format!(
                    "Example case {n}: at most {MAX_CASE_ROWS} rows of {entity}."
                ));
            }
            for row in list {
                let scalars = row.as_object().is_some_and(|o| {
                    !o.is_empty() && o.values().all(|v| !v.is_array() && !v.is_object())
                });
                if !scalars {
                    return invalid(format!(
                        "Example case {n}: each row of {entity} maps columns to plain values."
                    ));
                }
            }
        }
        match case.get("expect") {
            Some(v) if v.is_number() || v.is_null() => {}
            _ => {
                return invalid(format!(
                    "Example case {n} needs \"expect\": a number or null."
                ));
            }
        }
        if kind == NoteKind::Rule
            && !case
                .get("count")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty())
        {
            return invalid(format!(
                "Example case {n}: a rule's case says which entity's rows to count (\"count\")."
            ));
        }
        if let Some(today) = case.get("today")
            && !today
                .as_str()
                .is_some_and(|t| chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d").is_ok())
        {
            return invalid(format!(
                "Example case {n}: \"today\" is a date, YYYY-MM-DD."
            ));
        }
    }
    Ok(())
}

/// A computable note as the semantic model needs it: its approved revision, or (if it has never
/// been approved) its open one, marked unapproved so results using it carry a badge.
#[derive(Debug, Clone)]
pub struct ModelNote {
    pub note_id: Uuid,
    pub kind: NoteKind,
    pub title: String,
    pub version: i32,
    pub approved: bool,
    pub definition: Option<serde_json::Value>,
}

/// The computable knowledge one project sees: shared notes plus its own, deprecated ones left out.
pub async fn model_notes(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: Uuid,
) -> Result<Vec<ModelNote>> {
    let rows = sqlx::query!(
        r#"SELECT n.id, n.kind, r.title, r.version, r.definition, r.status = 'approved' AS "approved!"
           FROM knowledge_notes n
           JOIN LATERAL (
               SELECT title, version, definition, status FROM knowledge_revisions r
               WHERE r.note_id = n.id AND r.status IN ('approved', 'draft', 'pending')
               ORDER BY (r.status = 'approved') DESC, r.version DESC LIMIT 1
           ) r ON true
           WHERE n.workspace_id = $1 AND (n.project_id IS NULL OR n.project_id = $2)
             AND n.deprecated_at IS NULL
             AND n.kind IN ('entity', 'metric', 'dimension', 'rule', 'relationship')
           ORDER BY n.created_at"#,
        workspace_id,
        project_id
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(ModelNote {
                note_id: r.id,
                kind: NoteKind::parse(&r.kind)?,
                title: r.title,
                version: r.version,
                approved: r.approved,
                definition: r.definition,
            })
        })
        .collect()
}

// ---- reads ----

/// One visible note with its head revision: the live one when there is one (what everything
/// uses), otherwise the open or most recent one.
#[derive(Debug, Clone)]
struct Row {
    id: Uuid,
    kind: NoteKind,
    project_id: Option<Uuid>,
    owner_id: Option<Uuid>,
    owner_team: Option<String>,
    deprecated: bool,
    head_id: Uuid,
    title: String,
    title_key: String,
    has_live: bool,
    open: Option<RevisionStatus>,
}

impl Row {
    fn state(&self) -> NoteState {
        state_of(self.has_live, self.open, self.deprecated)
    }

    fn reference(&self) -> NoteRef {
        NoteRef {
            id: self.id,
            kind: self.kind,
            title: self.title.clone(),
            state: self.state(),
        }
    }
}

async fn visible_rows(pool: &PgPool, viewer: &Viewer) -> Result<Vec<Row>> {
    let projects: Vec<Uuid> = viewer.projects.keys().copied().collect();
    let rows = sqlx::query!(
        r#"SELECT n.id, n.kind, n.project_id, n.owner_id, n.owner_team,
                  n.deprecated_at IS NOT NULL AS "deprecated!",
                  h.id AS head_id, h.title, h.title_key,
                  EXISTS (SELECT 1 FROM knowledge_revisions r
                          WHERE r.note_id = n.id AND r.status = 'approved') AS "has_live!",
                  (SELECT r.status FROM knowledge_revisions r
                   WHERE r.note_id = n.id AND r.status IN ('draft', 'pending')) AS open
           FROM knowledge_notes n
           JOIN LATERAL (
               SELECT r.id, r.title, r.title_key FROM knowledge_revisions r
               WHERE r.note_id = n.id
               ORDER BY (r.status = 'approved') DESC, (r.status IN ('draft', 'pending')) DESC,
                        r.version DESC
               LIMIT 1
           ) h ON true
           WHERE n.workspace_id = $1 AND (n.project_id IS NULL OR n.project_id = ANY($2))
           ORDER BY n.created_at"#,
        viewer.workspace_id,
        &projects,
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(Row {
                id: r.id,
                kind: NoteKind::parse(&r.kind)?,
                project_id: r.project_id,
                owner_id: r.owner_id,
                owner_team: r.owner_team,
                deprecated: r.deprecated,
                head_id: r.head_id,
                title: r.title,
                title_key: r.title_key,
                has_live: r.has_live,
                open: r.open.as_deref().map(RevisionStatus::parse).transpose()?,
            })
        })
        .collect()
}

/// Links resolve by title key; from a project's note, its own project's notes win over shared.
struct Resolver<'a> {
    by_key: HashMap<&'a str, Vec<&'a Row>>,
}

impl<'a> Resolver<'a> {
    fn new(rows: &'a [Row]) -> Self {
        let mut by_key: HashMap<&str, Vec<&Row>> = HashMap::new();
        for row in rows {
            by_key.entry(row.title_key.as_str()).or_default().push(row);
        }
        Self { by_key }
    }

    fn resolve(&self, key: &str, from_project: Option<Uuid>) -> Option<&'a Row> {
        let candidates = self.by_key.get(key)?;
        candidates
            .iter()
            .find(|r| r.project_id.is_some() && r.project_id == from_project)
            .or_else(|| candidates.iter().find(|r| r.project_id.is_none()))
            .copied()
    }
}

async fn links_of(
    pool: &PgPool,
    revisions: &[Uuid],
) -> Result<HashMap<Uuid, Vec<(String, String)>>> {
    let rows = sqlx::query!(
        "SELECT revision_id, target_key, target_title FROM knowledge_links
         WHERE revision_id = ANY($1)",
        revisions
    )
    .fetch_all(pool)
    .await?;
    let mut out: HashMap<Uuid, Vec<(String, String)>> = HashMap::new();
    for r in rows {
        out.entry(r.revision_id)
            .or_default()
            .push((r.target_key, r.target_title));
    }
    Ok(out)
}

/// The viewer's graph: shared notes plus those of their projects, or with `project`, the
/// shared notes plus that project's.
pub async fn graph(
    pool: &PgPool,
    viewer: &Viewer,
    project: Option<Uuid>,
) -> Result<KnowledgeGraph> {
    let mut rows = visible_rows(pool, viewer).await?;
    if let Some(p) = project {
        rows.retain(|r| r.project_id.is_none() || r.project_id == Some(p));
    }
    let heads: Vec<Uuid> = rows.iter().map(|r| r.head_id).collect();
    let links = links_of(pool, &heads).await?;
    let resolver = Resolver::new(&rows);
    let mut edges = Vec::new();
    let mut unresolved = Vec::new();
    let mut seen = HashSet::new();
    for row in &rows {
        for (key, title) in links.get(&row.head_id).into_iter().flatten() {
            match resolver.resolve(key, row.project_id) {
                Some(target) if target.id != row.id => {
                    if seen.insert((row.id, target.id)) {
                        edges.push(GraphEdge {
                            from: row.id,
                            to: target.id,
                        });
                    }
                }
                Some(_) => {}
                None => unresolved.push(UnresolvedLink {
                    from: row.id,
                    title: title.clone(),
                }),
            }
        }
    }
    let nodes = rows
        .iter()
        .map(|r| GraphNode {
            id: r.id,
            kind: r.kind,
            title: r.title.clone(),
            state: r.state(),
            project_id: r.project_id,
            owner_team: r.owner_team.clone(),
        })
        .collect();
    Ok(KnowledgeGraph {
        nodes,
        edges,
        unresolved,
    })
}

async fn revisions(pool: &PgPool, note_id: Uuid) -> Result<Vec<Revision>> {
    let rows = sqlx::query!(
        r#"SELECT r.id, r.version, r.title, r.body, r.definition, r.status, r.author,
                  r.provenance, r.change_note, r.reviewed_at, r.review_reason,
                  r.created_at, r.updated_at,
                  a.display_name AS "author_name?", v.display_name AS "reviewer_name?"
           FROM knowledge_revisions r
           LEFT JOIN users a ON r.author = 'user:' || a.id::text
           LEFT JOIN users v ON v.id = r.reviewed_by
           WHERE r.note_id = $1 ORDER BY r.version DESC"#,
        note_id
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(Revision {
                id: r.id,
                version: r.version,
                title: r.title,
                body: r.body,
                definition: r.definition,
                status: RevisionStatus::parse(&r.status)?,
                author_name: r.author_name.or_else(|| match r.author.as_str() {
                    "ai" => Some("Lattice agent".into()),
                    "system" => Some("Lattice".into()),
                    _ => None,
                }),
                author: r.author,
                provenance: r.provenance,
                change_note: r.change_note,
                reviewed_by_name: r.reviewer_name,
                reviewed_at: r.reviewed_at,
                review_reason: r.review_reason,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
        })
        .collect()
}

pub async fn detail(pool: &PgPool, viewer: &Viewer, note_id: Uuid) -> Result<NoteDetail> {
    let rows = visible_rows(pool, viewer).await?;
    let row = rows
        .iter()
        .find(|r| r.id == note_id)
        .ok_or(KnowledgeError::NotFound)?;
    let heads: Vec<Uuid> = rows.iter().map(|r| r.head_id).collect();
    let links = links_of(pool, &heads).await?;
    let resolver = Resolver::new(&rows);

    let own_links = links
        .get(&row.head_id)
        .into_iter()
        .flatten()
        .filter_map(|(key, title)| match resolver.resolve(key, row.project_id) {
            Some(t) if t.id == row.id => None,
            target => Some(NoteLink {
                title: title.clone(),
                note: target.map(Row::reference),
            }),
        })
        .collect();
    let backlinks = rows
        .iter()
        .filter(|other| other.id != row.id)
        .filter(|other| {
            links
                .get(&other.head_id)
                .into_iter()
                .flatten()
                .any(|(key, _)| {
                    resolver
                        .resolve(key, other.project_id)
                        .is_some_and(|t| t.id == row.id)
                })
        })
        .map(Row::reference)
        .collect();

    let history = revisions(pool, note_id).await?;
    let live = history
        .iter()
        .find(|r| r.status == RevisionStatus::Approved)
        .cloned();
    let open = history
        .iter()
        .find(|r| matches!(r.status, RevisionStatus::Draft | RevisionStatus::Pending))
        .cloned();
    let owner = sqlx::query_scalar!("SELECT display_name FROM users WHERE id = $1", row.owner_id)
        .fetch_optional(pool)
        .await?;
    Ok(NoteDetail {
        id: row.id,
        kind: row.kind,
        project_id: row.project_id,
        state: row.state(),
        owner: NoteOwner {
            user_id: row.owner_id,
            name: owner,
            team: row.owner_team.clone(),
        },
        live,
        open,
        history,
        links: own_links,
        backlinks,
        can: Permissions {
            edit: viewer.can_draft(row.project_id) && !row.deprecated,
            review: viewer.can_review(row.project_id, row.owner_id),
        },
    })
}

/// Pending changes the viewer may review, then their own drafts.
pub async fn queue(pool: &PgPool, viewer: &Viewer) -> Result<Vec<QueueItem>> {
    let rows = visible_rows(pool, viewer).await?;
    let mut items = Vec::new();
    for row in rows.iter().filter(|r| r.open.is_some()) {
        let can_review = viewer.can_review(row.project_id, row.owner_id);
        let history = revisions(pool, row.id).await?;
        let Some(open) = history
            .iter()
            .find(|r| matches!(r.status, RevisionStatus::Draft | RevisionStatus::Pending))
        else {
            continue;
        };
        let mine = open.author == viewer.actor();
        let relevant = match open.status {
            RevisionStatus::Pending => can_review || mine,
            _ => mine,
        };
        if relevant {
            items.push(QueueItem {
                note: row.reference(),
                revision: open.clone(),
                live: history
                    .iter()
                    .find(|r| r.status == RevisionStatus::Approved)
                    .cloned(),
                can_review,
            });
        }
    }
    items.sort_by_key(|i| {
        (
            i.revision.status != RevisionStatus::Pending,
            std::cmp::Reverse(i.revision.updated_at),
        )
    });
    Ok(items)
}

// ---- writes ----

struct NoteAccess {
    kind: NoteKind,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
    owner_id: Option<Uuid>,
    deprecated: bool,
}

async fn access(tx: &mut sqlx::PgConnection, viewer: &Viewer, note_id: Uuid) -> Result<NoteAccess> {
    let note = sqlx::query!(
        r#"SELECT kind, workspace_id, project_id, owner_id, deprecated_at IS NOT NULL AS "deprecated!"
           FROM knowledge_notes WHERE id = $1 FOR UPDATE"#,
        note_id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(KnowledgeError::NotFound)?;
    if note.workspace_id != viewer.workspace_id || !viewer.sees(note.project_id) {
        return Err(KnowledgeError::NotFound);
    }
    Ok(NoteAccess {
        kind: NoteKind::parse(&note.kind)?,
        workspace_id: note.workspace_id,
        project_id: note.project_id,
        owner_id: note.owner_id,
        deprecated: note.deprecated,
    })
}

/// Refuses a title another visible note in the same scope already has.
async fn ensure_unique_title(
    tx: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
    key: &str,
    except: Option<Uuid>,
) -> Result<()> {
    let taken = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM knowledge_notes n JOIN knowledge_revisions r ON r.note_id = n.id
               WHERE n.workspace_id = $1 AND n.project_id IS NOT DISTINCT FROM $2
                 AND r.title_key = $3 AND r.status IN ('draft', 'pending', 'approved')
                 AND n.deprecated_at IS NULL
                 AND ($4::uuid IS NULL OR n.id <> $4)
           ) AS "taken!""#,
        workspace_id,
        project_id,
        key,
        except,
    )
    .fetch_one(&mut *tx)
    .await?;
    if taken {
        return Err(KnowledgeError::Conflict(
            "Another note already has that title.".into(),
        ));
    }
    Ok(())
}

async fn write_links(tx: &mut sqlx::PgConnection, revision_id: Uuid, body: &str) -> Result<()> {
    sqlx::query!(
        "DELETE FROM knowledge_links WHERE revision_id = $1",
        revision_id
    )
    .execute(&mut *tx)
    .await?;
    let (keys, titles): (Vec<String>, Vec<String>) = links::extract(body).into_iter().unzip();
    sqlx::query!(
        "INSERT INTO knowledge_links (revision_id, target_key, target_title)
         SELECT $1, * FROM UNNEST($2::text[], $3::text[])",
        revision_id,
        &keys,
        &titles
    )
    .execute(&mut *tx)
    .await?;
    Ok(())
}

async fn record(
    tx: &mut sqlx::PgConnection,
    viewer: &Viewer,
    action: &str,
    note_id: Uuid,
    detail: serde_json::Value,
) -> Result<()> {
    audit::record(
        &mut *tx,
        AuditEvent {
            workspace_id: Some(viewer.workspace_id),
            actor: Actor::User(viewer.user_id),
            action,
            target: Some(format!("note:{note_id}")),
            detail,
        },
    )
    .await?;
    Ok(())
}

/// Creates a note as a draft (or straight into review) owned by its author.
pub async fn create(
    pool: &PgPool,
    viewer: &Viewer,
    new: &NewNote,
    provenance: Provenance,
) -> Result<Uuid> {
    validate(&new.title, &new.body, None)?;
    validate_definition(new.kind, new.definition.as_ref())?;
    if let Some(team) = &new.owner_team
        && team.chars().count() > 60
    {
        return Err(KnowledgeError::Invalid(
            "A team name has at most 60 characters.".into(),
        ));
    }
    if !viewer.can_draft(new.project_id) {
        return Err(KnowledgeError::Forbidden);
    }
    let title = new.title.trim();
    let key = title_key(title);
    let mut tx = pool.begin().await?;
    ensure_unique_title(&mut tx, viewer.workspace_id, new.project_id, &key, None).await?;
    let note_id = sqlx::query_scalar!(
        "INSERT INTO knowledge_notes (workspace_id, project_id, kind, owner_id, owner_team)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
        viewer.workspace_id,
        new.project_id,
        new.kind.as_str(),
        viewer.user_id,
        new.owner_team
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty()),
    )
    .fetch_one(&mut *tx)
    .await?;
    let status = if new.submit == Some(true) {
        RevisionStatus::Pending
    } else {
        RevisionStatus::Draft
    };
    let revision_id = sqlx::query_scalar!(
        "INSERT INTO knowledge_revisions
             (note_id, version, title, title_key, body, definition, status, author, provenance)
         VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
        note_id,
        title,
        key,
        new.body,
        new.definition,
        status.as_str(),
        provenance.author,
        provenance.value,
    )
    .fetch_one(&mut *tx)
    .await?;
    write_links(&mut tx, revision_id, &new.body).await?;
    record(
        &mut tx,
        viewer,
        "note.create",
        note_id,
        serde_json::json!({ "kind": new.kind, "title": title, "status": status }),
    )
    .await?;
    tx.commit().await?;
    Ok(note_id)
}

/// Saves the open change, starting one from the live version if there is none. Editing a
/// change that is waiting for review takes it back to draft: it must be submitted again.
pub async fn save_draft(
    pool: &PgPool,
    viewer: &Viewer,
    note_id: Uuid,
    edit: &NoteEdit,
) -> Result<()> {
    validate(&edit.title, &edit.body, edit.change_note.as_deref())?;
    let mut tx = pool.begin().await?;
    let note = access(&mut tx, viewer, note_id).await?;
    if !viewer.can_draft(note.project_id) || note.deprecated {
        return Err(KnowledgeError::Forbidden);
    }
    validate_definition(note.kind, edit.definition.as_ref())?;
    let title = edit.title.trim();
    let key = title_key(title);
    ensure_unique_title(
        &mut tx,
        note.workspace_id,
        note.project_id,
        &key,
        Some(note_id),
    )
    .await?;
    let open = sqlx::query_scalar!(
        "SELECT id FROM knowledge_revisions WHERE note_id = $1 AND status IN ('draft', 'pending')",
        note_id
    )
    .fetch_optional(&mut *tx)
    .await?;
    let revision_id = match open {
        Some(id) => {
            sqlx::query!(
                "UPDATE knowledge_revisions
                 SET title = $2, title_key = $3, body = $4, definition = $5, change_note = $6,
                     status = 'draft', author = $7, updated_at = now()
                 WHERE id = $1",
                id,
                title,
                key,
                edit.body,
                edit.definition,
                edit.change_note,
                viewer.actor(),
            )
            .execute(&mut *tx)
            .await?;
            id
        }
        None => {
            sqlx::query_scalar!(
                "INSERT INTO knowledge_revisions
                     (note_id, version, title, title_key, body, definition, status, author,
                      provenance, change_note)
                 SELECT $1, COALESCE(max(version), 0) + 1, $2, $3, $4, $5, 'draft', $6,
                        '{\"source\": \"manual\"}'::jsonb, $7
                 FROM knowledge_revisions WHERE note_id = $1
                 RETURNING id",
                note_id,
                title,
                key,
                edit.body,
                edit.definition,
                viewer.actor(),
                edit.change_note,
            )
            .fetch_one(&mut *tx)
            .await?
        }
    };
    write_links(&mut tx, revision_id, &edit.body).await?;
    tx.commit().await?;
    Ok(())
}

/// Moves the open draft into review.
pub async fn submit(pool: &PgPool, viewer: &Viewer, note_id: Uuid) -> Result<()> {
    let mut tx = pool.begin().await?;
    let note = access(&mut tx, viewer, note_id).await?;
    if !viewer.can_draft(note.project_id) {
        return Err(KnowledgeError::Forbidden);
    }
    let done = sqlx::query!(
        "UPDATE knowledge_revisions SET status = 'pending', updated_at = now()
         WHERE note_id = $1 AND status = 'draft'",
        note_id
    )
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 0 {
        return Err(KnowledgeError::Conflict(
            "There's no draft to submit.".into(),
        ));
    }
    record(
        &mut tx,
        viewer,
        "note.submit",
        note_id,
        serde_json::json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Approves the change in review: it becomes the live version (the previous one is kept as
/// superseded).
pub async fn approve(pool: &PgPool, viewer: &Viewer, note_id: Uuid) -> Result<()> {
    approve_version(pool, viewer, note_id, None).await
}

/// Approves the pending revision, provided it is still `version` (what the reviewer checked).
pub async fn approve_version(
    pool: &PgPool,
    viewer: &Viewer,
    note_id: Uuid,
    version: Option<i32>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let note = access(&mut tx, viewer, note_id).await?;
    if !viewer.can_review(note.project_id, note.owner_id) {
        return Err(KnowledgeError::Forbidden);
    }
    let Some(pending) = sqlx::query!(
        "SELECT id, version FROM knowledge_revisions WHERE note_id = $1 AND status = 'pending'",
        note_id
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Err(KnowledgeError::Conflict(
            "There's nothing waiting for review.".into(),
        ));
    };
    if version.is_some_and(|v| v != pending.version) {
        return Err(KnowledgeError::Conflict(
            "The proposal changed while you were reviewing it.".into(),
        ));
    }
    sqlx::query!(
        "UPDATE knowledge_revisions SET status = 'superseded'
         WHERE note_id = $1 AND status = 'approved'",
        note_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE knowledge_revisions
         SET status = 'approved', reviewed_by = $2, reviewed_at = now(), updated_at = now()
         WHERE id = $1",
        pending.id,
        viewer.user_id
    )
    .execute(&mut *tx)
    .await?;
    record(
        &mut tx,
        viewer,
        "note.approve",
        note_id,
        serde_json::json!({ "version": pending.version }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Turns down the change in review, with a reason that is kept (and shown to the AI later, so it
/// doesn't propose the same thing again).
pub async fn reject(pool: &PgPool, viewer: &Viewer, note_id: Uuid, reason: &str) -> Result<()> {
    let reason = reason.trim();
    if reason.is_empty() || reason.chars().count() > 1000 {
        return Err(KnowledgeError::Invalid(
            "Say why (up to 1,000 characters): it's kept with the proposal.".into(),
        ));
    }
    let mut tx = pool.begin().await?;
    let note = access(&mut tx, viewer, note_id).await?;
    if !viewer.can_review(note.project_id, note.owner_id) {
        return Err(KnowledgeError::Forbidden);
    }
    let done = sqlx::query!(
        "UPDATE knowledge_revisions
         SET status = 'rejected', reviewed_by = $2, reviewed_at = now(), review_reason = $3,
             updated_at = now()
         WHERE note_id = $1 AND status = 'pending'",
        note_id,
        viewer.user_id,
        reason
    )
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 0 {
        return Err(KnowledgeError::Conflict(
            "There's nothing waiting for review.".into(),
        ));
    }
    record(
        &mut tx,
        viewer,
        "note.reject",
        note_id,
        serde_json::json!({ "reason": reason }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Throws away the open draft; a note that never had anything else goes with it.
pub async fn discard(pool: &PgPool, viewer: &Viewer, note_id: Uuid) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let note = access(&mut tx, viewer, note_id).await?;
    let author = sqlx::query_scalar!(
        "SELECT author FROM knowledge_revisions WHERE note_id = $1 AND status = 'draft'",
        note_id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| KnowledgeError::Conflict("There's no draft to discard.".into()))?;
    if author != viewer.actor() && !viewer.can_review(note.project_id, note.owner_id) {
        return Err(KnowledgeError::Forbidden);
    }
    sqlx::query!(
        "DELETE FROM knowledge_revisions WHERE note_id = $1 AND status = 'draft'",
        note_id
    )
    .execute(&mut *tx)
    .await?;
    let remaining = sqlx::query_scalar!(
        r#"SELECT count(*) AS "n!" FROM knowledge_revisions WHERE note_id = $1"#,
        note_id
    )
    .fetch_one(&mut *tx)
    .await?;
    let deleted = remaining == 0;
    if deleted {
        sqlx::query!("DELETE FROM knowledge_notes WHERE id = $1", note_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(deleted)
}

/// Starts a draft from an older version (undoing later changes once approved).
pub async fn revert(pool: &PgPool, viewer: &Viewer, note_id: Uuid, version: i32) -> Result<()> {
    let old = sqlx::query!(
        "SELECT title, body, definition FROM knowledge_revisions WHERE note_id = $1 AND version = $2",
        note_id,
        version
    )
    .fetch_optional(pool)
    .await?
    .ok_or(KnowledgeError::NotFound)?;
    save_draft(
        pool,
        viewer,
        note_id,
        &NoteEdit {
            title: old.title,
            body: old.body,
            definition: old.definition,
            change_note: Some(format!("Back to version {version}")),
        },
    )
    .await
}

/// Retires a note (or brings it back): it stays in history, out of answers.
pub async fn set_deprecated(
    pool: &PgPool,
    viewer: &Viewer,
    note_id: Uuid,
    deprecated: bool,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let note = access(&mut tx, viewer, note_id).await?;
    if !viewer.can_review(note.project_id, note.owner_id) {
        return Err(KnowledgeError::Forbidden);
    }
    sqlx::query!(
        "UPDATE knowledge_notes SET deprecated_at = CASE WHEN $2 THEN now() END WHERE id = $1",
        note_id,
        deprecated
    )
    .execute(&mut *tx)
    .await?;
    record(
        &mut tx,
        viewer,
        if deprecated {
            "note.deprecate"
        } else {
            "note.restore"
        },
        note_id,
        serde_json::json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Hands a note to another owner (a workspace member) and/or team.
pub async fn set_owner(
    pool: &PgPool,
    viewer: &Viewer,
    note_id: Uuid,
    owner_id: Option<Uuid>,
    team: Option<&str>,
) -> Result<()> {
    if team.is_some_and(|t| t.chars().count() > 60) {
        return Err(KnowledgeError::Invalid(
            "A team name has at most 60 characters.".into(),
        ));
    }
    let mut tx = pool.begin().await?;
    let note = access(&mut tx, viewer, note_id).await?;
    if !viewer.can_review(note.project_id, note.owner_id) {
        return Err(KnowledgeError::Forbidden);
    }
    if let Some(owner) = owner_id {
        let member = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM workspace_members
                              WHERE workspace_id = $1 AND user_id = $2) AS "member!""#,
            note.workspace_id,
            owner
        )
        .fetch_one(&mut *tx)
        .await?;
        if !member {
            return Err(KnowledgeError::Invalid(
                "The owner must be in this workspace.".into(),
            ));
        }
    }
    sqlx::query!(
        "UPDATE knowledge_notes SET owner_id = $2, owner_team = $3 WHERE id = $1",
        note_id,
        owner_id,
        team.map(str::trim).filter(|t| !t.is_empty())
    )
    .execute(&mut *tx)
    .await?;
    record(
        &mut tx,
        viewer,
        "note.owner",
        note_id,
        serde_json::json!({ "owner_id": owner_id, "team": team }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states() {
        use RevisionStatus::*;
        assert_eq!(state_of(false, Some(Draft), false), NoteState::Draft);
        assert_eq!(state_of(false, Some(Pending), false), NoteState::Pending);
        assert_eq!(state_of(true, None, false), NoteState::Approved);
        assert_eq!(state_of(true, Some(Draft), false), NoteState::ChangeDraft);
        assert_eq!(
            state_of(true, Some(Pending), false),
            NoteState::ChangePending
        );
        assert_eq!(state_of(false, None, false), NoteState::Rejected);
        assert_eq!(state_of(true, None, true), NoteState::Deprecated);
    }

    #[test]
    fn rights() {
        let (me, other, project) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(9));
        let viewer = |ws: WorkspaceRole, role: Option<ProjectRole>| Viewer {
            user_id: me,
            workspace_id: Uuid::nil(),
            workspace_role: ws,
            projects: role
                .map(|r| HashMap::from([(project, r)]))
                .unwrap_or_default(),
        };
        let member = viewer(WorkspaceRole::Member, Some(ProjectRole::Viewer));
        assert!(member.can_draft(None), "anyone drafts shared notes");
        assert!(!member.can_draft(Some(project)), "project viewers don't");
        assert!(!member.can_review(None, Some(other)));
        assert!(
            member.can_review(None, Some(me)),
            "owners review their notes"
        );
        assert!(viewer(WorkspaceRole::Admin, None).can_review(None, Some(other)));
        assert!(!viewer(WorkspaceRole::Admin, None).can_review(Some(project), Some(other)));
        assert!(
            viewer(WorkspaceRole::Member, Some(ProjectRole::Admin))
                .can_review(Some(project), Some(other))
        );
        assert!(!member.sees(Some(Uuid::from_u128(5))));
    }

    #[test]
    fn titles() {
        assert!(validate("Net revenue", "", None).is_ok());
        assert!(validate("  ", "", None).is_err());
        assert!(validate("a [[b]]", "", None).is_err());
        assert!(validate("a|b", "", None).is_err());
    }
}
