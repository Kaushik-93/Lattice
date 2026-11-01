//! The project's semantic model (docs/adr/0006): the computable knowledge notes it sees, with
//! entities bound to its tables.

use std::collections::HashMap;

use lattice_catalog as catalog;
use lattice_knowledge::{self as knowledge, ModelNote};
use lattice_semantic::{BoundTable, Column, Meta, Model, RawDefinition, Standing};
use uuid::Uuid;

use crate::error::ApiError;
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

pub async fn load(
    state: &AppState,
    workspace_id: Uuid,
    project_id: Uuid,
) -> Result<LoadedModel, ApiError> {
    let notes = knowledge::model_notes(&state.pool, workspace_id, project_id).await?;
    let bindings = sqlx::query!(
        "SELECT note_id, table_id FROM entity_bindings WHERE project_id = $1",
        project_id
    )
    .fetch_all(&state.pool)
    .await?;
    let mut tables = HashMap::new();
    let mut table_names = HashMap::new();
    for b in bindings {
        let Some(t) = catalog::resolve_table(&state.pool, project_id, b.table_id).await? else {
            continue;
        };
        table_names.insert(b.table_id, t.table_name.clone());
        tables.insert(
            b.note_id,
            BoundTable {
                table_id: b.table_id,
                reference: table_reference(t.kind, t.source_id, &t.schema_name, &t.table_name),
                columns: t
                    .columns
                    .into_iter()
                    .map(|(name, data_type)| Column { name, data_type })
                    .collect(),
                data_version: t.data_version,
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
