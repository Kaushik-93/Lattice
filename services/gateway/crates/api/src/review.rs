//! What a reviewer needs before approving a definition (docs/adr/0006): its example cases, run
//! against their own rows in a scratch database, and what approving it would change.

use std::collections::{HashMap, HashSet, VecDeque};

use chrono::{NaiveDate, Utc};
use lattice_connectors::ident as quote;
use lattice_engine::scratch::{self, FixtureTable};
use lattice_knowledge::{self as knowledge, ModelNote, NoteDetail, NoteKind, Revision, Viewer};
use lattice_planner::ParamValue;
use lattice_semantic::{
    self as semantic, BoundTable, Column, Meta, MetricDef, Model, RawDefinition, SemanticQuery,
    Standing, ident,
};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::dto::{CaseResult, ImpactResponse, ImpactVisual};
use crate::error::ApiError;
use crate::state::AppState;

/// Runs `revision`'s example cases. Everything else the definition uses comes from the graph as
/// it stands; the definition under review is computed as if approved.
pub async fn run_cases(
    state: &AppState,
    workspace_id: Uuid,
    note: &NoteDetail,
    revision: &Revision,
) -> Result<Vec<CaseResult>, ApiError> {
    let cases = revision
        .definition
        .as_ref()
        .and_then(|d| d.get("cases"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if cases.is_empty() || !matches!(note.kind, NoteKind::Metric | NoteKind::Rule) {
        return Ok(Vec::new());
    }
    let notes = knowledge::model_notes(
        &state.pool,
        workspace_id,
        note.project_id.unwrap_or(Uuid::nil()),
    )
    .await?;
    let mut results = Vec::with_capacity(cases.len());
    for case in &cases {
        let expected = case.get("expect").and_then(Value::as_f64);
        let name = case
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Example")
            .to_owned();
        results.push(match evaluate(&notes, note, revision, case).await {
            Ok(actual) => CaseResult {
                name,
                passed: close(expected, actual),
                expected,
                actual,
                error: None,
            },
            Err(error) => CaseResult {
                name,
                passed: false,
                expected,
                actual: None,
                error: Some(error),
            },
        });
    }
    Ok(results)
}

fn close(expected: Option<f64>, actual: Option<f64>) -> bool {
    match (expected, actual) {
        (None, None) => true,
        (Some(e), Some(a)) => (e - a).abs() <= 1e-9 * e.abs().max(1.0),
        _ => false,
    }
}

async fn evaluate(
    notes: &[ModelNote],
    note: &NoteDetail,
    revision: &Revision,
    case: &Value,
) -> Result<Option<f64>, String> {
    let mut raws: Vec<RawDefinition<'_>> = notes
        .iter()
        .filter(|n| n.note_id != note.id)
        .map(|n| RawDefinition {
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
        })
        .collect();
    raws.push(RawDefinition {
        meta: Meta {
            note_id: note.id,
            title: revision.title.clone(),
            version: revision.version,
            standing: Standing::Approved,
        },
        kind: note.kind.as_str(),
        definition: revision.definition.as_ref(),
    });
    let entities: HashMap<Uuid, String> = raws
        .iter()
        .filter(|r| r.kind == "entity")
        .map(|r| (r.meta.note_id, ident(&r.meta.title)))
        .collect();

    let empty = Map::new();
    let rows = case
        .get("rows")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let mut fixtures = HashMap::new();
    for (entity, list) in rows {
        let name = ident(entity);
        if !entities.values().any(|e| *e == name) {
            return Err(format!("the rows name \"{entity}\", which isn't an entity"));
        }
        let rows: Vec<&Map<String, Value>> = list
            .as_array()
            .map(|l| l.iter().filter_map(Value::as_object).collect())
            .unwrap_or_default();
        fixtures.insert(name.clone(), fixture(&name, &rows));
    }

    let bound = |id: Uuid| {
        let table = fixtures.get(entities.get(&id)?)?;
        Some(BoundTable {
            table_id: Uuid::nil(),
            reference: quote(&table.name),
            columns: table
                .columns
                .iter()
                .map(|(name, data_type)| Column {
                    name: name.clone(),
                    data_type: data_type.clone(),
                })
                .collect(),
            data_version: 0,
        })
    };
    let mut model: Model = Model::build(raws, bound);

    let metric = if note.kind == NoteKind::Rule {
        // A rule's case counts the rows it keeps.
        let count = case
            .get("count")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let id = Uuid::new_v4();
        model.metrics.push(MetricDef {
            meta: Meta {
                note_id: id,
                title: "Rows kept".into(),
                version: 0,
                standing: Standing::Approved,
            },
            formula: Some(semantic::formula(&format!(
                "COUNT({}) WHERE [[{}]]",
                ident(count),
                revision.title
            ))),
        });
        id
    } else {
        note.id
    };
    let today = case
        .get("today")
        .and_then(Value::as_str)
        .and_then(|t| NaiveDate::parse_from_str(t, "%Y-%m-%d").ok())
        .unwrap_or_else(|| Utc::now().date_naive());
    let query = SemanticQuery {
        metrics: vec![metric],
        dimensions: Vec::new(),
        filters: Vec::new(),
        context_filters: Vec::new(),
        sort: Vec::new(),
        limit: None,
        today,
        top: None,
    };
    let compiled = semantic::compile(&model, &query).map_err(|e| e.0)?;
    scratch::scalar(
        fixtures.into_values().collect(),
        compiled.query.sql,
        compiled.query.params,
    )
    .await
    .map_err(|e| e.to_string())
}

/// A table of example rows, its column types read off the values: whole numbers, decimals,
/// true/false, `YYYY-MM-DD` dates, otherwise text.
fn fixture(entity: &str, rows: &[&Map<String, Value>]) -> FixtureTable {
    let mut names: Vec<String> = Vec::new();
    for row in rows {
        for key in row.keys() {
            if !names.iter().any(|n| n.eq_ignore_ascii_case(key)) {
                names.push(key.clone());
            }
        }
    }
    let date = |v: &Value| {
        v.as_str()
            .is_some_and(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok())
    };
    let columns: Vec<(String, String)> = names
        .iter()
        .map(|name| {
            let values: Vec<&Value> = rows
                .iter()
                .filter_map(|r| r.get(name))
                .filter(|v| !v.is_null())
                .collect();
            let ty = if values.is_empty() {
                "VARCHAR"
            } else if values.iter().all(|v| v.is_boolean()) {
                "BOOLEAN"
            } else if values.iter().all(|v| v.is_i64()) {
                "BIGINT"
            } else if values.iter().all(|v| v.is_number()) {
                "DOUBLE"
            } else if values.iter().all(|v| date(v)) {
                "DATE"
            } else {
                "VARCHAR"
            };
            (name.to_lowercase(), ty.to_owned())
        })
        .collect();
    let rows = rows
        .iter()
        .map(|row| {
            names
                .iter()
                .zip(&columns)
                .map(|(name, (_, ty))| param(row.get(name)?, ty))
                .collect()
        })
        .collect();
    FixtureTable {
        name: format!("fx_{entity}"),
        columns,
        rows,
    }
}

fn param(v: &Value, ty: &str) -> Option<ParamValue> {
    Some(match (v, ty) {
        (Value::Null, _) => return None,
        (Value::Bool(b), "BOOLEAN") => ParamValue::Bool(*b),
        (Value::Number(n), "BIGINT") => ParamValue::BigInt(n.as_i64()?),
        (Value::Number(n), "DOUBLE") => ParamValue::Double(n.as_f64()?),
        (Value::String(s), _) => ParamValue::Text(s.clone()),
        (other, _) => ParamValue::Text(other.to_string()),
    })
}

/// What approving a change to `note` touches: definitions that use it (directly or through
/// others) and the visuals charting any of them.
pub async fn impact(
    state: &AppState,
    viewer: &Viewer,
    note: &NoteDetail,
) -> Result<ImpactResponse, ApiError> {
    let live = sqlx::query!(
        r#"SELECT n.id, r.title_key, r.definition FROM knowledge_notes n
           JOIN knowledge_revisions r ON r.note_id = n.id AND r.status = 'approved'
           WHERE n.workspace_id = $1 AND n.deprecated_at IS NULL
             AND n.kind IN ('metric', 'rule', 'dimension', 'relationship')"#,
        viewer.workspace_id
    )
    .fetch_all(&state.pool)
    .await?;
    // Who refers to whom, by `[[title]]` in a formula or condition.
    let keys: HashMap<String, Uuid> = live.iter().map(|r| (r.title_key.clone(), r.id)).collect();
    let mut used_by: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for r in &live {
        let text = ["formula", "condition"]
            .iter()
            .filter_map(|k| r.definition.as_ref()?.get(k)?.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        for (key, _) in knowledge::links::extract(&text) {
            if let Some(target) = keys.get(&key) {
                used_by.entry(*target).or_default().push(r.id);
            }
        }
    }
    let mut affected: HashSet<Uuid> = HashSet::from([note.id]);
    let mut queue = VecDeque::from([note.id]);
    while let Some(id) = queue.pop_front() {
        for user in used_by.get(&id).into_iter().flatten() {
            if affected.insert(*user) {
                queue.push_back(*user);
            }
        }
    }

    let graph = knowledge::graph(&state.pool, viewer, None).await?;
    let dependents = graph
        .nodes
        .into_iter()
        .filter(|n| n.id != note.id && affected.contains(&n.id))
        .map(|n| knowledge::NoteRef {
            id: n.id,
            kind: n.kind,
            title: n.title,
            state: n.state,
        })
        .collect();

    let ids: Vec<String> = affected.iter().map(Uuid::to_string).collect();
    let rows = sqlx::query!(
        r#"SELECT v.id, v.spec->>'title' AS title, s.id AS sheet_id, s.name AS sheet_name,
                  p.id AS project_id, p.name AS project_name
           FROM visuals v
           JOIN sheets s ON s.id = v.sheet_id
           JOIN projects p ON p.id = s.project_id
           WHERE p.workspace_id = $1
             AND EXISTS (SELECT 1 FROM jsonb_array_elements(v.spec->'measures') m
                         WHERE m->>'metric' = ANY($2))
           ORDER BY p.name, s.position, v.position"#,
        viewer.workspace_id,
        &ids
    )
    .fetch_all(&state.pool)
    .await?;
    let (visible, hidden): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .partition(|r| viewer.projects.contains_key(&r.project_id));
    Ok(ImpactResponse {
        dependents,
        visuals: visible
            .into_iter()
            .map(|r| ImpactVisual {
                visual_id: r.id,
                title: r.title.unwrap_or_default(),
                sheet_id: r.sheet_id,
                sheet_name: r.sheet_name,
                project_id: r.project_id,
                project_name: r.project_name,
            })
            .collect(),
        hidden_visuals: u32::try_from(hidden.len()).unwrap_or(u32::MAX),
    })
}
