//! Compiles structured queries into DuckDB SQL.
//!
//! Clients never send SQL. They send a [`TableQuery`] — dimensions, measures, filters, sort —
//! which is validated against the catalog's columns and compiled here with quoted identifiers
//! and bound parameters. Phase 2 compiles VisualSpecs through the same path; phase 3 adds the
//! semantic layer in front of it and row-level-security predicates inside it.

mod compile;
pub mod sql;

pub use compile::{
    ColumnRole, CompiledQuery, DimParts, OutputColumn, ParamValue, bucket_name, compile, dimension_parts,
};

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Hard cap on rows returned to a browser (spec §3: cap points per series).
pub const MAX_ROWS: u32 = 100_000;
/// Cap for raw point queries that project at most [`MAX_POINT_COLUMNS`] columns (a scatter
/// plot's x, y, colour, size). The browser draws these with WebGL, not as table rows.
pub const MAX_POINTS: u32 = 1_000_000;
pub const MAX_POINT_COLUMNS: usize = 4;
/// Rows returned when a query doesn't ask for a limit.
pub const DEFAULT_ROWS: u32 = 10_000;

/// A query over one catalog table. With neither dimensions nor measures it returns raw rows
/// (paged with `limit`/`offset`); otherwise it aggregates measures grouped by dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TableQuery {
    pub table_id: Uuid,
    /// Raw-row queries only: the columns to return, in order (every column when empty).
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub columns: Vec<String>,
    #[serde(default)]
    pub dimensions: Vec<Dimension>,
    #[serde(default)]
    pub measures: Vec<Measure>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub sort: Vec<Sort>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Dimension {
    pub column: String,
    /// Truncate a date/timestamp column to this grain.
    #[serde(default)]
    pub bucket: Option<TimeBucket>,
    /// Group a numeric column into bins of this width (histograms). Each row is labelled with
    /// its bin's lower bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub bin: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TimeBucket {
    Year,
    Quarter,
    Month,
    Week,
    Day,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Measure {
    /// Omitted only for `count` (counts rows).
    #[serde(default)]
    pub column: Option<String>,
    pub agg: Aggregation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Aggregation {
    Count,
    Sum,
    Avg,
    Min,
    Max,
    CountDistinct,
    /// HyperLogLog estimate: ~7× faster than an exact distinct count on millions of values.
    ApproxCountDistinct,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Filter {
    pub column: String,
    pub op: FilterOp,
    #[serde(default)]
    pub value: Option<FilterValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FilterOp {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    In,
    NotIn,
    Between,
    IsNull,
    IsNotNull,
    /// Case-insensitive substring match.
    Contains,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub enum FilterValue {
    One(Scalar),
    Many(Vec<Scalar>),
}

/// JSON scalars. Dates and timestamps travel as ISO-8601 strings and are cast to the column type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub enum Scalar {
    Bool(bool),
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Sort {
    /// An output column name (see `CompiledQuery::columns`).
    pub by: String,
    #[serde(default)]
    pub dir: SortDir,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

/// What the planner needs to know about the table being queried.
#[derive(Debug, Clone)]
pub struct TableSchema {
    /// Fully qualified, already-quoted DuckDB reference, e.g. `"project"."s_1"."orders"`.
    pub reference: String,
    pub columns: Vec<ColumnInfo>,
}

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    /// DuckDB type name as reported by introspection, e.g. `DECIMAL(12,2)`.
    pub data_type: String,
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum PlanError {
    #[error("unknown column {0:?}")]
    UnknownColumn(String),
    #[error("{agg} needs a numeric column, but {column:?} is {data_type}")]
    NotNumeric {
        agg: &'static str,
        column: String,
        data_type: String,
    },
    #[error("time buckets need a date or timestamp column, but {column:?} is {data_type}")]
    NotTemporal { column: String, data_type: String },
    #[error("invalid bins on {column:?}: {reason}")]
    InvalidBin {
        column: String,
        reason: &'static str,
    },
    #[error("{0} needs a column")]
    MissingColumn(&'static str),
    #[error("invalid filter on {column:?}: {reason}")]
    InvalidFilter { column: String, reason: String },
    #[error("cannot sort by {0:?}: not an output column")]
    UnknownSort(String),
}
