//! Knowledge suggested from a project's data profiles (docs/adr/0006): worked out from the
//! catalog whenever asked for, less what people have already accepted or dismissed.

use std::collections::{HashMap, HashSet};

use lattice_knowledge::capture::{
    self, Known, ProfiledColumn, ProfiledTable, Suggestion, SuggestionKind,
};
use lattice_knowledge::{self as knowledge, NewNote, NoteKind, Provenance, Viewer, title_key};
use lattice_security::audit::{self, Actor, AuditEvent};
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::AppState;

/// The suggestions still open for a project.
pub async fn open(
    state: &AppState,
    viewer: &Viewer,
    project_id: Uuid,
) -> Result<Vec<Suggestion>, ApiError> {
    let rows = sqlx::query!(
        r#"SELECT t.id, t.table_name, t.row_count, s.name AS source_name,
                  c.name AS "column_name?", c.data_type AS "data_type?", c.profile AS "profile?"
           FROM source_tables t
           JOIN sources s ON s.id = t.source_id
           LEFT JOIN source_columns c ON c.table_id = t.id
           WHERE s.project_id = $1
           ORDER BY s.created_at, t.table_name, t.id, c.ordinal"#,
        project_id
    )
    .fetch_all(&state.pool)
    .await?;
    let mut tables: Vec<ProfiledTable> = Vec::new();
    for r in rows {
        if tables.last().is_none_or(|t| t.table_id != r.id) {
            tables.push(ProfiledTable {
                table_id: r.id,
                label: format!("{} · {}", r.source_name, r.table_name),
                name: r.table_name.clone(),
                row_count: r.row_count,
                columns: Vec::new(),
            });
        }
        if let (Some(name), Some(data_type), Some(table)) =
            (r.column_name, r.data_type, tables.last_mut())
        {
            let profile = r.profile.unwrap_or_default();
            table.columns.push(ProfiledColumn {
                name,
                data_type,
                distinct: profile.get("approx_distinct").and_then(|v| v.as_i64()),
                null_fraction: profile.get("null_fraction").and_then(|v| v.as_f64()),
                samples: profile
                    .get("samples")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| s.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
    }

    let known = known(state, viewer, project_id).await?;
    let decided: HashSet<String> = sqlx::query_scalar!(
        "SELECT key FROM knowledge_suggestion_decisions WHERE project_id = $1",
        project_id
    )
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .collect();
    Ok(capture::suggest(&tables, &known)
        .into_iter()
        .filter(|s| !decided.contains(&s.key))
        .collect())
}

/// What the project's knowledge already covers.
async fn known(state: &AppState, viewer: &Viewer, project_id: Uuid) -> Result<Known, ApiError> {
    let notes = knowledge::model_notes(&state.pool, viewer.workspace_id, project_id).await?;
    let bound: HashMap<Uuid, Uuid> = sqlx::query!(
        "SELECT note_id, table_id FROM entity_bindings WHERE project_id = $1",
        project_id
    )
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|b| (b.note_id, b.table_id))
    .collect();
    let text = |d: &Option<serde_json::Value>, key: &str| {
        d.as_ref()
            .and_then(|d| d.get(key))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_lowercase())
    };
    let mut known = Known::default();
    for n in &notes {
        match n.kind {
            NoteKind::Entity => {
                known
                    .entities
                    .push((n.note_id, n.title.clone(), bound.get(&n.note_id).copied()))
            }
            NoteKind::Relationship => {
                if let (Some(from), Some(to)) =
                    (text(&n.definition, "from"), text(&n.definition, "to"))
                {
                    known.relationships.insert((from, to));
                }
            }
            NoteKind::Dimension => {
                if let Some(path) = text(&n.definition, "path") {
                    known.dimensions.insert(path);
                }
            }
            _ => {}
        }
    }
    known.titles = knowledge::graph(&state.pool, viewer, Some(project_id))
        .await?
        .nodes
        .iter()
        .map(|n| title_key(&n.title))
        .collect();
    Ok(known)
}

/// Accepts a suggestion: a draft note (an entity also connected to its table), or a binding of
/// an existing entity. Returns the note.
pub async fn accept(
    state: &AppState,
    viewer: &Viewer,
    project_id: Uuid,
    key: &str,
) -> Result<Uuid, ApiError> {
    let suggestion = open(state, viewer, project_id)
        .await?
        .into_iter()
        .find(|s| s.key == key)
        .ok_or(ApiError::NotFound)?;
    let note_id = match (suggestion.kind, suggestion.note_id) {
        (SuggestionKind::Binding, Some(note_id)) => note_id,
        (SuggestionKind::Binding, None) => return Err(ApiError::NotFound),
        (kind, _) => {
            let new = NewNote {
                kind: match kind {
                    SuggestionKind::Entity => NoteKind::Entity,
                    SuggestionKind::Relationship => NoteKind::Relationship,
                    _ => NoteKind::Dimension,
                },
                title: suggestion.title.clone(),
                body: suggestion.body.clone(),
                definition: suggestion.definition.clone(),
                project_id: None,
                owner_team: None,
                submit: Some(false),
            };
            let provenance = Provenance {
                author: viewer.actor(),
                value: serde_json::json!({
                    "source": "profile",
                    "table": suggestion.table,
                    "evidence": suggestion.evidence,
                }),
            };
            knowledge::create(&state.pool, viewer, &new, provenance).await?
        }
    };
    let mut tx = state.pool.begin().await?;
    if matches!(
        suggestion.kind,
        SuggestionKind::Entity | SuggestionKind::Binding
    ) {
        sqlx::query!(
            "INSERT INTO entity_bindings (project_id, note_id, table_id, bound_by)
             VALUES ($1, $2, $3, $4) ON CONFLICT (project_id, note_id) DO NOTHING",
            project_id,
            note_id,
            suggestion.table_id,
            viewer.user_id,
        )
        .execute(&mut *tx)
        .await?;
    }
    decide(&mut tx, viewer, project_id, key, "accepted", Some(note_id)).await?;
    tx.commit().await?;
    Ok(note_id)
}

pub async fn dismiss(
    state: &AppState,
    viewer: &Viewer,
    project_id: Uuid,
    key: &str,
) -> Result<(), ApiError> {
    if !open(state, viewer, project_id)
        .await?
        .iter()
        .any(|s| s.key == key)
    {
        return Err(ApiError::NotFound);
    }
    let mut tx = state.pool.begin().await?;
    decide(&mut tx, viewer, project_id, key, "dismissed", None).await?;
    tx.commit().await?;
    Ok(())
}

async fn decide(
    tx: &mut sqlx::PgConnection,
    viewer: &Viewer,
    project_id: Uuid,
    key: &str,
    decision: &str,
    note_id: Option<Uuid>,
) -> Result<(), ApiError> {
    sqlx::query!(
        "INSERT INTO knowledge_suggestion_decisions (project_id, key, decision, note_id, decided_by)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (project_id, key) DO UPDATE
         SET decision = EXCLUDED.decision, note_id = EXCLUDED.note_id,
             decided_by = EXCLUDED.decided_by, decided_at = now()",
        project_id,
        key,
        decision,
        note_id,
        viewer.user_id,
    )
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut *tx,
        AuditEvent {
            workspace_id: Some(viewer.workspace_id),
            actor: Actor::User(viewer.user_id),
            action: if decision == "accepted" {
                "knowledge.suggestion.accept"
            } else {
                "knowledge.suggestion.dismiss"
            },
            target: Some(format!("project:{project_id}")),
            detail: serde_json::json!({ "key": key, "note_id": note_id }),
        },
    )
    .await?;
    Ok(())
}
