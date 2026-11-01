//! The project's semantic model (docs/adr/0006): the computable knowledge notes it sees, with
//! entities bound to its tables.

use std::collections::HashMap;
use std::time::Instant;

use chrono::NaiveDate;
use lattice_connectors::SourceKind;
use lattice_knowledge::{self as knowledge, ModelNote};
use lattice_planner::TableQuery;
use lattice_semantic::ast::Path;
use lattice_semantic::model::parse_path;
use lattice_semantic::{
    BoundTable, Column, Meta, Model, RawDefinition, SemDimension, SemFilter, SemanticQuery,
    Standing,
};
use lattice_visuals::{SortBy, VisualQuery};
use uuid::Uuid;

use crate::dto::UnapprovedLogic;
use crate::error::ApiError;
use crate::query::{Prepared, QueryFailure};
use crate::sources::table_reference;
use crate::state::AppState;

pub struct LoadedModel {
    pub model: Model,
    /// Bound table id → its name, for display.
    pub table_names: HashMap<Uuid, String>,
    /// Note id → its definition as written.
    pub definitions: HashMap<Uuid, serde_json::Value>,
}

impl LoadedModel {
    /// A text field of a note's definition as written, e.g. a metric's `formula`.
    pub fn text(&self, note_id: Uuid, key: &str) -> Option<String> {
        self.definitions
            .get(&note_id)?
            .get(key)?
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }
}

/// Whether a query computes governed metrics (and so compiles through the model).
pub fn governed(query: &TableQuery) -> bool {
    query.measures.iter().any(|m| m.metric.is_some())
}

/// The model of a project, whichever workspace it's in (share links have no workspace member).
pub async fn load_for_project(state: &AppState, project_id: Uuid) -> Result<LoadedModel, ApiError> {
    let workspace_id = sqlx::query_scalar!(
        "SELECT workspace_id FROM projects WHERE id = $1",
        project_id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    load(state, workspace_id, project_id).await
}

/// Compiles a visual's query over metrics. Its table is the grain: the entity bound to it in
/// this project. Bare columns (dimensions, filters) are that entity's; `entity.column` reaches
/// a related one.
pub fn prepare(
    loaded: &LoadedModel,
    visual: &VisualQuery,
    today: NaiveDate,
) -> Result<Prepared, QueryFailure> {
    let started = Instant::now();
    let invalid = |m: String| QueryFailure::new("invalid_query", m);
    // A comparison measures the earlier period as it was then.
    let today = visual.shift.and_then(|s| s.back(today)).unwrap_or(today);
    let model = &loaded.model;
    let q = &visual.query;
    let grain = model
        .entities
        .iter()
        .find(|e| e.table.as_ref().is_some_and(|t| t.table_id == q.table_id))
        .ok_or_else(|| {
            invalid(
                "This visual's table isn't connected to an entity, so it can't use metrics.".into(),
            )
        })?;
    let path = |column: &str| -> Result<Path, QueryFailure> {
        if column.contains('.') {
            parse_path(column).map_err(invalid)
        } else {
            Ok(Path {
                entity: grain.ident.clone(),
                column: column.to_lowercase(),
            })
        }
    };
    let mut query = SemanticQuery {
        metrics: q
            .measures
            .iter()
            .map(|m| {
                m.metric.ok_or_else(|| {
                    invalid("A visual uses metrics or raw columns, not both. Define the raw one as a metric.".into())
                })
            })
            .collect::<Result<_, _>>()?,
        dimensions: q
            .dimensions
            .iter()
            .map(|d| {
                Ok(SemDimension {
                    path: path(&d.column)?,
                    bucket: d.bucket,
                    bin: d.bin,
                })
            })
            .collect::<Result<_, QueryFailure>>()?,
        filters: q
            .filters
            .iter()
            .map(|f| SemFilter {
                column: f.column.clone(),
                op: f.op,
                value: f.value.clone(),
            })
            .collect(),
        context_filters: Vec::new(),
        sort: Vec::new(),
        limit: q.limit,
        today,
    };
    let fail = |e: lattice_semantic::SemanticError| invalid(e.0);
    let mut compiled = lattice_semantic::compile(model, &query).map_err(fail)?;
    // Output names exist only once compiled: sort by position, then compile again.
    if let Some((by, dir)) = visual.sort {
        let position = match by {
            SortBy::X => 0,
            SortBy::Value => query.dimensions.len(),
        };
        if let Some(c) = compiled.query.columns.get(position) {
            query.sort = vec![(c.name.clone(), dir)];
            compiled = lattice_semantic::compile(model, &query).map_err(fail)?;
        }
    }
    let unapproved = compiled
        .unapproved
        .iter()
        .map(|id| UnapprovedLogic {
            note_id: *id,
            title: title_of(model, *id).unwrap_or_default(),
        })
        .collect();
    Ok(Prepared {
        compiled: compiled.query,
        data_versions: compiled.data_versions,
        logic: compiled.logic,
        unapproved,
        compile_ms: (started.elapsed().as_secs_f64() * 1e6).round() / 1e3,
    })
}

fn title_of(model: &Model, id: Uuid) -> Option<String> {
    let metas = model
        .entities
        .iter()
        .map(|e| &e.meta)
        .chain(model.metrics.iter().map(|m| &m.meta))
        .chain(model.rules.iter().map(|r| &r.meta))
        .chain(model.dimensions.iter().map(|d| &d.meta))
        .chain(model.relationships.iter().map(|r| &r.meta));
    metas
        .into_iter()
        .find(|m| m.note_id == id)
        .map(|m| m.title.clone())
}

pub async fn load(
    state: &AppState,
    workspace_id: Uuid,
    project_id: Uuid,
) -> Result<LoadedModel, ApiError> {
    let notes = knowledge::model_notes(&state.pool, workspace_id, project_id).await?;
    // Every binding with its table and columns, in one round trip (this runs per sheet batch).
    let bindings = sqlx::query!(
        r#"SELECT b.note_id, b.table_id, s.id AS source_id, s.kind, t.schema_name, t.table_name,
                  s.data_version,
                  COALESCE(array_agg(c.name ORDER BY c.ordinal) FILTER (WHERE c.name IS NOT NULL),
                           '{}') AS "names!",
                  COALESCE(array_agg(c.data_type ORDER BY c.ordinal) FILTER (WHERE c.name IS NOT NULL),
                           '{}') AS "types!"
           FROM entity_bindings b
           JOIN source_tables t ON t.id = b.table_id
           JOIN sources s ON s.id = t.source_id AND s.project_id = b.project_id
           LEFT JOIN source_columns c ON c.table_id = t.id
           WHERE b.project_id = $1
           GROUP BY b.note_id, b.table_id, s.id, t.schema_name, t.table_name"#,
        project_id
    )
    .fetch_all(&state.pool)
    .await?;
    let mut tables = HashMap::new();
    let mut table_names = HashMap::new();
    for b in bindings {
        let Some(kind) = SourceKind::parse(&b.kind) else {
            continue;
        };
        table_names.insert(b.table_id, b.table_name.clone());
        tables.insert(
            b.note_id,
            BoundTable {
                table_id: b.table_id,
                reference: table_reference(kind, b.source_id, &b.schema_name, &b.table_name),
                columns: b
                    .names
                    .into_iter()
                    .zip(b.types)
                    .map(|(name, data_type)| Column { name, data_type })
                    .collect(),
                data_version: b.data_version,
            },
        );
    }
    let model = Model::build(notes.iter().map(raw), |id| tables.remove(&id));
    let definitions = notes
        .into_iter()
        .filter_map(|n| Some((n.note_id, n.definition?)))
        .collect();
    Ok(LoadedModel {
        model,
        table_names,
        definitions,
    })
}

fn raw(n: &ModelNote) -> RawDefinition<'_> {
    RawDefinition {
        meta: Meta {
            note_id: n.note_id,
            title: n.title.clone(),
            version: n.version,
            standing: if n.approved {
                Standing::Approved
            } else {
                Standing::Unapproved
            },
        },
        kind: n.kind.as_str(),
        definition: n.definition.as_ref(),
    }
}
