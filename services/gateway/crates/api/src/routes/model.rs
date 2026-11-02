//! A project's semantic model (docs/adr/0006): which tables its entities are, and whether each
//! metric, dimension and rule can be computed here.

use axum::Json;
use axum::extract::{Path, State};
use lattice_catalog as catalog;
use lattice_knowledge::{self as knowledge, NoteKind};
use lattice_security::audit::{self, Actor, AuditEvent};
use lattice_security::{Principal, ProjectRole};
use lattice_semantic::{self as semantic, Meta};
use uuid::Uuid;

use crate::auth::Authed;
use crate::dto::{
    BindEntityRequest, ModelDefinition, ModelDimension, ModelEntity, ModelMetric,
    ModelRelationship, ModelRule, ModelTable, ProjectModelResponse, PromoteRequest,
    PromoteResponse,
};
use crate::error::ApiError;
use crate::routes::projects::require_role;
use crate::semantic::{LoadedModel, load};
use crate::state::AppState;

pub async fn get(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ProjectModelResponse>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Viewer).await?;
    let loaded = load(&state, me.workspace_id, project_id).await?;
    Ok(Json(describe(&loaded)))
}

/// Says which table an entity is in this project.
pub async fn bind(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, note_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<BindEntityRequest>,
) -> Result<Json<ProjectModelResponse>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    require_entity(&state, &me, project_id, note_id).await?;
    if catalog::resolve_table(&state.pool, project_id, req.table_id)
        .await?
        .is_none()
    {
        return Err(ApiError::BadRequest(
            "That table isn't in this project.".into(),
        ));
    }
    let mut tx = state.pool.begin().await?;
    sqlx::query!(
        "INSERT INTO entity_bindings (project_id, note_id, table_id, bound_by)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (project_id, note_id)
         DO UPDATE SET table_id = EXCLUDED.table_id, bound_by = EXCLUDED.bound_by, bound_at = now()",
        project_id,
        note_id,
        req.table_id,
        me.user_id,
    )
    .execute(&mut *tx)
    .await?;
    record(
        &mut tx,
        &me,
        project_id,
        "entity.bind",
        note_id,
        Some(req.table_id),
    )
    .await?;
    tx.commit().await?;
    get(State(state), Authed(me), Path(project_id)).await
}

pub async fn unbind(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, note_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ProjectModelResponse>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    let mut tx = state.pool.begin().await?;
    let removed = sqlx::query!(
        "DELETE FROM entity_bindings WHERE project_id = $1 AND note_id = $2",
        project_id,
        note_id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if removed == 0 {
        return Err(ApiError::NotFound);
    }
    record(&mut tx, &me, project_id, "entity.unbind", note_id, None).await?;
    tx.commit().await?;
    get(State(state), Authed(me), Path(project_id)).await
}

/// The note must be an entity this project sees (a shared one, or its own).
async fn require_entity(
    state: &AppState,
    me: &Principal,
    project_id: Uuid,
    note_id: Uuid,
) -> Result<(), ApiError> {
    let notes = knowledge::model_notes(&state.pool, me.workspace_id, project_id).await?;
    if notes
        .iter()
        .any(|n| n.note_id == note_id && n.kind == NoteKind::Entity)
    {
        Ok(())
    } else {
        Err(ApiError::NotFound)
    }
}

async fn record(
    tx: &mut sqlx::PgConnection,
    me: &Principal,
    project_id: Uuid,
    action: &str,
    note_id: Uuid,
    table_id: Option<Uuid>,
) -> Result<(), ApiError> {
    audit::record(
        tx,
        AuditEvent {
            workspace_id: Some(me.workspace_id),
            actor: Actor::User(me.user_id),
            action,
            target: Some(format!("project:{project_id}")),
            detail: serde_json::json!({ "note_id": note_id, "table_id": table_id }),
        },
    )
    .await?;
    Ok(())
}

fn definition(meta: &Meta, problem: Option<String>) -> ModelDefinition {
    ModelDefinition {
        note_id: meta.note_id,
        title: meta.title.clone(),
        version: meta.version,
        standing: meta.standing,
        problem,
    }
}

fn describe(loaded: &LoadedModel) -> ProjectModelResponse {
    let model = &loaded.model;
    let problem = |r: Result<(), semantic::SemanticError>| r.err().map(|e| e.0);
    ProjectModelResponse {
        fiscal_year_start_month: model.fiscal_year_start_month,
        entities: model
            .entities
            .iter()
            .map(|e| ModelEntity {
                definition: definition(
                    &e.meta,
                    e.table
                        .is_none()
                        .then(|| "Not connected to a table in this project yet.".to_owned()),
                ),
                ident: e.ident.clone(),
                table: e.table.as_ref().map(|t| ModelTable {
                    table_id: t.table_id,
                    name: loaded
                        .table_names
                        .get(&t.table_id)
                        .cloned()
                        .unwrap_or_default(),
                    columns: t.columns.iter().map(|c| c.name.clone()).collect(),
                }),
            })
            .collect(),
        metrics: model
            .metrics
            .iter()
            .map(|m| {
                let (check, problem) = match semantic::check_metric(model, m.meta.note_id) {
                    Ok(c) => (Some(c), None),
                    Err(e) => (None, Some(e.0)),
                };
                ModelMetric {
                    definition: definition(&m.meta, problem),
                    formula: loaded.text(m.meta.note_id, "formula"),
                    grain: check.as_ref().map(|c| c.grain.clone()),
                    uses: check
                        .map(|c| {
                            c.uses
                                .into_iter()
                                .filter(|id| *id != m.meta.note_id)
                                .collect()
                        })
                        .unwrap_or_default(),
                }
            })
            .collect(),
        dimensions: model
            .dimensions
            .iter()
            .map(|d| ModelDimension {
                definition: definition(&d.meta, problem(semantic::check_dimension(model, d))),
                path: loaded.text(d.meta.note_id, "path"),
            })
            .collect(),
        rules: model
            .rules
            .iter()
            .map(|r| ModelRule {
                definition: definition(
                    &r.meta,
                    if r.condition.is_some() {
                        problem(semantic::check_rule(model, r))
                    } else {
                        None
                    },
                ),
                condition: loaded.text(r.meta.note_id, "condition"),
                always: r.always,
            })
            .collect(),
        relationships: model
            .relationships
            .iter()
            .map(|r| ModelRelationship {
                definition: definition(&r.meta, problem(semantic::check_relationship(model, r))),
                from: loaded.text(r.meta.note_id, "from"),
                to: loaded.text(r.meta.note_id, "to"),
            })
            .collect(),
    }
}

/// The expression-language formula for a raw aggregation over `entity`.
pub fn formula_for(entity: &str, m: &lattice_planner::Measure) -> Result<String, ApiError> {
    use lattice_planner::Aggregation as A;
    let column = m
        .column
        .as_deref()
        .map(|c| format!("{entity}.{}", c.to_lowercase()));
    Ok(match (m.agg, column) {
        (A::Count, None) => format!("COUNT({entity})"),
        (A::Count, Some(c)) => format!("COUNT({c})"),
        (A::Sum, Some(c)) => format!("SUM({c})"),
        (A::Avg, Some(c)) => format!("AVG({c})"),
        (A::Min, Some(c)) => format!("MIN({c})"),
        (A::Max, Some(c)) => format!("MAX({c})"),
        (A::CountDistinct | A::ApproxCountDistinct, Some(c)) => format!("COUNT DISTINCT({c})"),
        (_, None) => {
            return Err(ApiError::BadRequest(
                "That aggregation needs a column.".into(),
            ));
        }
    })
}

/// Promotes a raw aggregation to a draft metric over the table's entity. A table without one
/// gets it first, from its profile suggestion.
pub async fn promote(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(project_id): Path<Uuid>,
    Json(req): Json<PromoteRequest>,
) -> Result<Json<PromoteResponse>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    if req.measure.metric.is_some() {
        return Err(ApiError::BadRequest("That's already a metric.".into()));
    }
    let viewer = knowledge_viewer(&state, &me).await?;
    let bound = |loaded: &LoadedModel| {
        loaded
            .model
            .entities
            .iter()
            .find(|e| e.table.as_ref().is_some_and(|t| t.table_id == req.table_id))
            .map(|e| (e.ident.clone(), e.meta.title.clone()))
    };
    let mut entity = bound(&load(&state, me.workspace_id, project_id).await?);
    if entity.is_none() {
        let suggestion = crate::capture::open(&state, &viewer, project_id)
            .await?
            .into_iter()
            .find(|s| {
                s.table_id == req.table_id
                    && matches!(
                        s.kind,
                        lattice_knowledge::capture::SuggestionKind::Entity
                            | lattice_knowledge::capture::SuggestionKind::Binding
                    )
            })
            .ok_or_else(|| {
                ApiError::BadRequest(
                    "Connect this table to an entity first, on the Business logic page.".into(),
                )
            })?;
        crate::capture::accept(&state, &viewer, project_id, &suggestion.key).await?;
        entity = bound(&load(&state, me.workspace_id, project_id).await?);
    }
    let (ident, title) = entity.ok_or(ApiError::NotFound)?;
    let formula = formula_for(&ident, &req.measure)?;
    let what = match req.measure.column.as_deref() {
        Some(c) => format!("`{c}`"),
        None => "rows".to_owned(),
    };
    let new = lattice_knowledge::NewNote {
        kind: NoteKind::Metric,
        title: req.title.trim().to_owned(),
        body: format!("Aggregates {what} of each [[{title}]]. Promoted from a sheet."),
        definition: Some(serde_json::json!({ "formula": formula })),
        project_id: None,
        owner_team: None,
        submit: Some(false),
    };
    let provenance = lattice_knowledge::Provenance {
        author: viewer.actor(),
        value: serde_json::json!({ "source": "sheet", "table_id": req.table_id }),
    };
    let note_id = knowledge::create(&state.pool, &viewer, &new, provenance).await?;
    Ok(Json(PromoteResponse { note_id, formula }))
}

async fn knowledge_viewer(
    state: &AppState,
    me: &Principal,
) -> Result<lattice_knowledge::Viewer, ApiError> {
    Ok(lattice_knowledge::Viewer {
        user_id: me.user_id,
        workspace_id: me.workspace_id,
        workspace_role: me.workspace_role,
        projects: lattice_project::roles_for_user(&state.pool, me.workspace_id, me.user_id).await?,
    })
}
