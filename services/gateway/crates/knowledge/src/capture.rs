//! Knowledge picked up from data profiles (docs/adr/0006, "How knowledge gets in"): entities for
//! tables, relationships where an `…_id` column matches another table's key, and dimensions for
//! low-cardinality and date columns. Deterministic rules, no model: each suggestion says why.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use ts_rs::TS;
use uuid::Uuid;

use crate::links::title_key;

/// A catalog table with what profiling learned about its columns.
#[derive(Debug, Clone)]
pub struct ProfiledTable {
    pub table_id: Uuid,
    /// "Source · table", for people.
    pub label: String,
    pub name: String,
    pub row_count: Option<i64>,
    pub columns: Vec<ProfiledColumn>,
}

#[derive(Debug, Clone)]
pub struct ProfiledColumn {
    pub name: String,
    pub data_type: String,
    pub distinct: Option<i64>,
    pub null_fraction: Option<f64>,
    pub samples: Vec<String>,
}

/// What the graph already knows, so nothing is suggested twice.
#[derive(Debug, Clone, Default)]
pub struct Known {
    /// Entity notes: (note id, title, the table it's bound to in this project).
    pub entities: Vec<(Uuid, String, Option<Uuid>)>,
    /// Relationship ends, `entity.column`.
    pub relationships: HashSet<(String, String)>,
    /// Dimension paths, `entity.column`.
    pub dimensions: HashSet<String>,
    /// Every note's title key, whatever its kind.
    pub titles: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SuggestionKind {
    Entity,
    /// An entity the graph has, connected to this table.
    Binding,
    Relationship,
    Dimension,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Suggestion {
    /// Stable within a project: decisions are remembered by it.
    pub key: String,
    pub kind: SuggestionKind,
    pub title: String,
    pub body: String,
    #[ts(type = "Record<string, unknown> | null")]
    pub definition: Option<serde_json::Value>,
    pub table_id: Uuid,
    pub table: String,
    /// Why: what the profile showed.
    pub evidence: Vec<String>,
    /// For a binding, the entity it connects.
    pub note_id: Option<Uuid>,
}

const MAX_DIMENSION_VALUES: i64 = 50;

fn text(t: &str) -> bool {
    let t = t.to_ascii_uppercase();
    t.starts_with("VARCHAR")
        || t.starts_with("TEXT")
        || t.starts_with("STRING")
        || t.starts_with("CHAR")
}

fn temporal(t: &str) -> bool {
    let t = t.to_ascii_uppercase();
    t.starts_with("DATE") || t.starts_with("TIMESTAMP")
}

/// "order_items" → "order item", "categories" → "category": the last word made singular.
fn singular(name: &str) -> String {
    let words: Vec<&str> = name
        .split(['_', ' ', '-'])
        .filter(|w| !w.is_empty())
        .collect();
    let Some((last, rest)) = words.split_last() else {
        return name.to_lowercase();
    };
    let l = last.to_lowercase();
    let one = if let Some(stem) = l.strip_suffix("ies") {
        format!("{stem}y")
    } else if ["sses", "xes", "ches", "shes"]
        .iter()
        .any(|s| l.ends_with(s))
    {
        l[..l.len() - 2].to_owned()
    } else if l.ends_with('s') && !l.ends_with("ss") && l.len() > 3 {
        l[..l.len() - 1].to_owned()
    } else {
        l
    };
    let mut all: Vec<String> = rest.iter().map(|w| w.to_lowercase()).collect();
    all.push(one);
    all.join(" ")
}

/// "home_region" → "Home region".
fn humanize(name: &str) -> String {
    let words = name.replace(['_', '-'], " ");
    let words = words
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut c = words.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn ident(title: &str) -> String {
    lattice_semantic::ident(title)
}

/// The column that identifies a table's rows: `id`, or `<entity>_id`, unique and never empty
/// where the profile can tell.
fn key_column<'a>(t: &'a ProfiledTable, entity: &str) -> Option<&'a ProfiledColumn> {
    let names = [
        "id".to_owned(),
        format!("{entity}_id"),
        format!("{}_id", t.name.to_lowercase()),
    ];
    t.columns.iter().find(|c| {
        names.iter().any(|n| c.name.eq_ignore_ascii_case(n))
            && match (c.distinct, t.row_count) {
                (Some(d), Some(rows)) if rows > 0 => d as f64 >= 0.9 * rows as f64,
                _ => true,
            }
            && c.null_fraction.is_none_or(|f| f == 0.0)
    })
}

/// Suggestions for a project's tables, entities first, then relationships, then dimensions.
pub fn suggest(tables: &[ProfiledTable], known: &Known) -> Vec<Suggestion> {
    let mut out = Vec::new();
    // Table → (entity identifier, title), for the tables that have or get an entity.
    let mut entity_of: HashMap<Uuid, (String, String)> = HashMap::new();
    let mut titled: HashSet<String> = HashSet::new();
    let known_entity = |title: &str| {
        let k = title_key(title);
        known.entities.iter().find(|(_, t, _)| title_key(t) == k)
    };

    for t in tables {
        let title = humanize(&singular(&t.name));
        let key = title_key(&title);
        if title.is_empty() || !titled.insert(key.clone()) {
            continue;
        }
        let rows = t.row_count.map(|r| format!("{r} rows")).unwrap_or_default();
        if let Some((note_id, existing, bound)) = known_entity(&title) {
            match bound {
                Some(table) if *table == t.table_id => {
                    entity_of.insert(t.table_id, (ident(existing), existing.clone()));
                }
                Some(_) => {}
                None => {
                    entity_of.insert(t.table_id, (ident(existing), existing.clone()));
                    out.push(Suggestion {
                        key: format!("binding:{note_id}:{}", t.table_id),
                        kind: SuggestionKind::Binding,
                        title: format!("{existing} is {}", t.name),
                        body: format!("Connect [[{existing}]] to `{}` in this project.", t.label),
                        definition: None,
                        table_id: t.table_id,
                        table: t.label.clone(),
                        evidence: vec![format!("\"{}\" names {}", t.name, existing.to_lowercase())]
                            .into_iter()
                            .chain((!rows.is_empty()).then_some(rows))
                            .collect(),
                        note_id: Some(*note_id),
                    });
                }
            }
            continue;
        }
        if known.titles.contains(&key) {
            continue; // a note of another kind has this name
        }
        let id = ident(&title);
        let mut evidence: Vec<String> = (!rows.is_empty()).then_some(rows).into_iter().collect();
        let key_col = key_column(t, &id);
        if let Some(k) = key_col {
            evidence.push(format!("each row has its own {}", k.name));
        }
        entity_of.insert(t.table_id, (id, title.clone()));
        out.push(Suggestion {
            key: format!("entity:{}", t.table_id),
            kind: SuggestionKind::Entity,
            body: format!(
                "One row of `{}`{}.",
                t.label,
                key_col
                    .map(|k| format!(", identified by `{}`", k.name))
                    .unwrap_or_default()
            ),
            title,
            definition: None,
            table_id: t.table_id,
            table: t.label.clone(),
            evidence,
            note_id: None,
        });
    }

    // Relationships: an `x_id` column pointing at the table whose entity is x.
    for a in tables {
        let Some((ia, ta)) = entity_of.get(&a.table_id) else {
            continue;
        };
        let own_key = key_column(a, ia).map(|c| c.name.clone());
        for c in &a.columns {
            let Some(x) = c.name.to_lowercase().strip_suffix("_id").map(str::to_owned) else {
                continue;
            };
            if own_key
                .as_deref()
                .is_some_and(|k| k.eq_ignore_ascii_case(&c.name))
            {
                continue;
            }
            let target = tables.iter().find(|b| {
                b.table_id != a.table_id
                    && entity_of
                        .get(&b.table_id)
                        .is_some_and(|(ib, _)| *ib == ident(&singular(&x)))
            });
            let Some(b) = target else { continue };
            let Some((ib, tb)) = entity_of.get(&b.table_id) else {
                continue;
            };
            let Some(bkey) = key_column(b, ib) else {
                continue;
            };
            let (from, to) = (
                format!("{ia}.{}", c.name.to_lowercase()),
                format!("{ib}.{}", bkey.name.to_lowercase()),
            );
            if known.relationships.contains(&(from.clone(), to.clone())) {
                continue;
            }
            let title = format!("{ta} to {tb}");
            if known.titles.contains(&title_key(&title)) {
                continue;
            }
            out.push(Suggestion {
                key: format!("relationship:{}:{}", a.table_id, c.name.to_lowercase()),
                kind: SuggestionKind::Relationship,
                body: format!(
                    "Each [[{ta}]] belongs to one [[{tb}]], through `{}`.",
                    c.name
                ),
                title,
                definition: Some(serde_json::json!({ "from": from, "to": to })),
                table_id: a.table_id,
                table: a.label.clone(),
                evidence: vec![
                    format!("{}.{} matches {}.{}", a.name, c.name, b.name, bkey.name),
                    format!("{}.{} is unique", b.name, bkey.name),
                ],
                note_id: None,
            });
        }
    }

    // Dimensions: a handful of text values, or dates.
    for t in tables {
        let Some((ie, te)) = entity_of.get(&t.table_id) else {
            continue;
        };
        let key = key_column(t, ie).map(|c| c.name.to_lowercase());
        for c in &t.columns {
            let name = c.name.to_lowercase();
            if key.as_deref() == Some(name.as_str())
                || name.ends_with("_id")
                || name == "name"
                || name.ends_with("_name")
            {
                continue;
            }
            let categorical = text(&c.data_type)
                && c.distinct
                    .is_some_and(|d| (2..=MAX_DIMENSION_VALUES).contains(&d))
                && c.null_fraction.is_none_or(|f| f < 0.5);
            if !categorical && !temporal(&c.data_type) {
                continue;
            }
            let path = format!("{ie}.{name}");
            if known.dimensions.contains(&path) {
                continue;
            }
            let title = humanize(&c.name);
            if known.titles.contains(&title_key(&title)) || title_key(&title) == title_key(te) {
                continue;
            }
            let evidence = if categorical {
                let mut e = format!("{} values", c.distinct.unwrap_or_default());
                if !c.samples.is_empty() {
                    e.push_str(&format!(
                        ", e.g. {}",
                        c.samples
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                vec![e]
            } else {
                vec!["a date column".to_owned()]
            };
            out.push(Suggestion {
                key: format!("dimension:{}:{name}", t.table_id),
                kind: SuggestionKind::Dimension,
                body: format!("The {} of a [[{te}]].", title.to_lowercase()),
                title,
                definition: Some(serde_json::json!({ "path": path })),
                table_id: t.table_id,
                table: t.label.clone(),
                evidence,
                note_id: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn col(name: &str, ty: &str, distinct: Option<i64>) -> ProfiledColumn {
        ProfiledColumn {
            name: name.into(),
            data_type: ty.into(),
            distinct,
            null_fraction: Some(0.0),
            samples: vec!["EMEA".into(), "APAC".into()],
        }
    }

    fn table(id: u128, name: &str, rows: i64, columns: Vec<ProfiledColumn>) -> ProfiledTable {
        ProfiledTable {
            table_id: Uuid::from_u128(id),
            label: format!("Shop · {name}"),
            name: name.into(),
            row_count: Some(rows),
            columns,
        }
    }

    fn shop() -> Vec<ProfiledTable> {
        vec![
            table(
                1,
                "orders",
                1000,
                vec![
                    col("order_id", "BIGINT", Some(990)),
                    col("customer_id", "BIGINT", Some(300)),
                    col("region", "VARCHAR", Some(3)),
                    col("note", "VARCHAR", Some(900)),
                    col("order_date", "DATE", Some(400)),
                ],
            ),
            table(
                2,
                "customers",
                300,
                vec![
                    col("customer_id", "BIGINT", Some(300)),
                    col("customer_name", "VARCHAR", Some(300)),
                    col("segment", "VARCHAR", Some(2)),
                ],
            ),
        ]
    }

    #[test]
    fn names_and_plurals() {
        assert_eq!(singular("orders"), "order");
        assert_eq!(singular("order_items"), "order item");
        assert_eq!(singular("categories"), "category");
        assert_eq!(singular("addresses"), "address");
        assert_eq!(singular("bus"), "bus");
        assert_eq!(humanize("home_region"), "Home region");
    }

    #[test]
    fn suggests_entities_relationships_and_dimensions_from_profiles() {
        let s = suggest(&shop(), &Known::default());
        let titles: Vec<(SuggestionKind, &str)> =
            s.iter().map(|x| (x.kind, x.title.as_str())).collect();
        assert_eq!(
            titles,
            [
                (SuggestionKind::Entity, "Order"),
                (SuggestionKind::Entity, "Customer"),
                (SuggestionKind::Relationship, "Order to Customer"),
                (SuggestionKind::Dimension, "Region"),
                (SuggestionKind::Dimension, "Order date"),
                (SuggestionKind::Dimension, "Segment"),
            ]
        );
        let rel = &s[2];
        assert_eq!(
            rel.definition,
            Some(serde_json::json!({ "from": "order.customer_id", "to": "customer.customer_id" }))
        );
        assert!(s[0].evidence.iter().any(|e| e.contains("order_id")));
        // High-cardinality text and names aren't dimensions.
        assert!(
            !titles
                .iter()
                .any(|(_, t)| *t == "Note" || *t == "Customer name")
        );
    }

    #[test]
    fn nothing_the_graph_already_knows() {
        let order = Uuid::from_u128(10);
        let known = Known {
            entities: vec![
                (order, "Order".into(), Some(Uuid::from_u128(1))),
                (Uuid::from_u128(11), "Customer".into(), None),
            ],
            relationships: HashSet::from([(
                "order.customer_id".into(),
                "customer.customer_id".into(),
            )]),
            dimensions: HashSet::from(["order.region".into()]),
            titles: HashSet::from(["order".into(), "customer".into(), "segment".into()]),
        };
        let s = suggest(&shop(), &known);
        let titles: Vec<(SuggestionKind, &str)> =
            s.iter().map(|x| (x.kind, x.title.as_str())).collect();
        // Customer exists but isn't connected here: connect it. Segment is taken by another note.
        assert_eq!(
            titles,
            [
                (SuggestionKind::Binding, "Customer is customers"),
                (SuggestionKind::Dimension, "Order date"),
            ]
        );
        assert_eq!(s[0].note_id, Some(Uuid::from_u128(11)));
    }
}
