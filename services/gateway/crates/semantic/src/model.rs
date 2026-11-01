//! The semantic model one project sees: its shared and own knowledge notes, with entities bound
//! to that project's tables. Built by the gateway from the knowledge graph and the catalog.

use serde::Serialize;
use ts_rs::TS;
use uuid::Uuid;

use crate::ast::{Cond, Expr, Path};
use crate::parse::{self, ParseError};

/// The identifier a note's title gives an entity in formulas: "Product category" →
/// `product_category`.
pub fn ident(title: &str) -> String {
    let mut out = String::new();
    for c in title.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_owned()
}

#[derive(Debug, Clone)]
pub struct Column {
    pub name: String,
    pub data_type: String,
}

/// The table an entity is bound to in this project.
#[derive(Debug, Clone)]
pub struct BoundTable {
    pub table_id: Uuid,
    /// Fully qualified, quoted DuckDB reference.
    pub reference: String,
    pub columns: Vec<Column>,
    pub data_version: i64,
}

/// Where a definition stands, for badges and cache keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Standing {
    Approved,
    /// A draft or pending definition (no approved version yet, or a change being reviewed and
    /// asked for explicitly).
    Unapproved,
}

#[derive(Debug, Clone)]
pub struct Meta {
    pub note_id: Uuid,
    pub title: String,
    pub version: i32,
    pub standing: Standing,
}

#[derive(Debug, Clone)]
pub struct EntityDef {
    pub meta: Meta,
    pub ident: String,
    pub table: Option<BoundTable>,
}

/// Many `from` rows per `to` row (e.g. orders → customer).
#[derive(Debug, Clone)]
pub struct RelationshipDef {
    pub meta: Meta,
    pub from: Result<Path, String>,
    pub to: Result<Path, String>,
}

#[derive(Debug, Clone)]
pub struct MetricDef {
    pub meta: Meta,
    pub formula: Option<Result<Expr, ParseError>>,
}

#[derive(Debug, Clone)]
pub struct RuleDef {
    pub meta: Meta,
    pub condition: Option<Result<Cond, ParseError>>,
    /// Applies to every query reaching its entity ("exclude internal accounts").
    pub always: bool,
}

#[derive(Debug, Clone)]
pub struct DimensionDef {
    pub meta: Meta,
    pub path: Option<Result<Path, String>>,
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub entities: Vec<EntityDef>,
    pub relationships: Vec<RelationshipDef>,
    pub metrics: Vec<MetricDef>,
    pub rules: Vec<RuleDef>,
    pub dimensions: Vec<DimensionDef>,
    /// 1 = January. From the fiscal calendar rule; January when there is none.
    pub fiscal_year_start_month: u32,
}

/// A path written as text in a definition, e.g. `"order.customer_id"`.
pub fn parse_path(s: &str) -> Result<Path, String> {
    let mut parts = s.trim().split('.');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(e), Some(c), None) if !e.trim().is_empty() && !c.trim().is_empty() => Ok(Path {
            entity: e.trim().to_lowercase(),
            column: c.trim().to_lowercase(),
        }),
        _ => Err(format!("{s:?} isn't entity.column")),
    }
}

/// A computable note as stored: its kind, title and definition JSON.
pub struct RawDefinition<'a> {
    pub meta: Meta,
    pub kind: &'a str,
    pub definition: Option<&'a serde_json::Value>,
}

impl Model {
    /// Builds the model from notes; `bind` gives the table an entity note is bound to.
    pub fn build<'a>(
        notes: impl IntoIterator<Item = RawDefinition<'a>>,
        mut bind: impl FnMut(Uuid) -> Option<BoundTable>,
    ) -> Model {
        let mut model = Model {
            fiscal_year_start_month: 1,
            ..Model::default()
        };
        let text = |d: Option<&serde_json::Value>, key: &str| {
            d.and_then(|d| d.get(key))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        for note in notes {
            let d = note.definition;
            match note.kind {
                "entity" => model.entities.push(EntityDef {
                    ident: ident(&note.meta.title),
                    table: bind(note.meta.note_id),
                    meta: note.meta,
                }),
                "relationship" => model.relationships.push(RelationshipDef {
                    from: text(d, "from")
                        .map_or_else(|| Err("missing \"from\"".into()), |s| parse_path(&s)),
                    to: text(d, "to")
                        .map_or_else(|| Err("missing \"to\"".into()), |s| parse_path(&s)),
                    meta: note.meta,
                }),
                "metric" => model.metrics.push(MetricDef {
                    formula: text(d, "formula").map(|f| parse::formula(&f)),
                    meta: note.meta,
                }),
                "rule" => {
                    if let Some(month) = d
                        .and_then(|d| d.get("fiscal_year_start_month"))
                        .and_then(serde_json::Value::as_u64)
                        .filter(|m| (1..=12).contains(m))
                        && note.meta.standing == Standing::Approved
                    {
                        model.fiscal_year_start_month = u32::try_from(month).unwrap_or(1);
                    }
                    model.rules.push(RuleDef {
                        condition: text(d, "condition").map(|c| parse::condition(&c)),
                        always: d
                            .and_then(|d| d.get("always"))
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false),
                        meta: note.meta,
                    });
                }
                "dimension" => model.dimensions.push(DimensionDef {
                    path: text(d, "path").map(|p| parse_path(&p)),
                    meta: note.meta,
                }),
                _ => {}
            }
        }
        model
    }

    pub fn entity(&self, ident: &str) -> Option<&EntityDef> {
        self.entities.iter().find(|e| e.ident == ident)
    }

    pub fn metric(&self, title: &str) -> Option<&MetricDef> {
        let key = key(title);
        self.metrics
            .iter()
            .find(|m| key == crate::model::key(&m.meta.title))
    }

    pub fn rule(&self, title: &str) -> Option<&RuleDef> {
        let key = key(title);
        self.rules
            .iter()
            .find(|r| key == crate::model::key(&r.meta.title))
    }

    pub fn metric_by_id(&self, id: Uuid) -> Option<&MetricDef> {
        self.metrics.iter().find(|m| m.meta.note_id == id)
    }

    pub fn dimension_by_id(&self, id: Uuid) -> Option<&DimensionDef> {
        self.dimensions.iter().find(|d| d.meta.note_id == id)
    }
}

/// Titles compare ignoring case and spacing, as `[[links]]` do.
pub fn key(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_from_titles() {
        assert_eq!(ident("Order"), "order");
        assert_eq!(ident("Product category"), "product_category");
        assert_eq!(ident("  Line-item (v2) "), "line_item_v2");
        assert_eq!(
            parse_path(" Order.Customer_ID ").unwrap(),
            Path {
                entity: "order".into(),
                column: "customer_id".into()
            }
        );
        assert!(parse_path("order").is_err());
        assert!(parse_path("a.b.c").is_err());
    }
}
