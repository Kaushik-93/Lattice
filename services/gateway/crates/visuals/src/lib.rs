//! Visual specs and their compilation to table queries.
//!
//! A [`VisualSpec`] is what a sheet stores for each chart: its table, the fields on each axis,
//! measures, visual-level filters and display options. The gateway compiles a spec plus the
//! sheet's interactive state — filter values, cross-filter selections, the drill path — into one
//! or more [`TableQuery`]s, which the planner turns into SQL. Compiling here rather than in the
//! browser is what lets a shared link run a sheet's own visuals without being able to run
//! anything else (docs/adr/0005).

mod compile;

pub use compile::{QueryPart, VisualError, VisualQuery, compile};

use lattice_planner::{Dimension, Filter, Measure, Scalar, SortDir};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum VisualKind {
    Kpi,
    Line,
    Area,
    /// Horizontal bars.
    Bar,
    /// Vertical bars.
    Column,
    Scatter,
    Pie,
    Donut,
    Table,
    Pivot,
    Heatmap,
    Histogram,
    Slicer,
}

/// One visual on a sheet. Which fields a kind uses:
///
/// | kind | `x` | `series` | `measures` | `columns` |
/// |---|---|---|---|---|
/// | kpi | | | first | |
/// | line, area, bar, column | axis | split into series | 1+ | |
/// | pie, donut | slices | | first | |
/// | scatter | x column | colour | | `[y]` |
/// | table | optional group | | optional | shown columns (raw rows) |
/// | pivot | rows | columns | 1+ | |
/// | heatmap | x | y | first | |
/// | histogram | binned column | | count by default | |
/// | slicer | values to pick | | count by default | |
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VisualSpec {
    pub kind: VisualKind,
    pub title: String,
    pub table_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub x: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub series: Option<Dimension>,
    #[serde(default)]
    pub measures: Vec<Measure>,
    #[serde(default)]
    pub columns: Vec<String>,
    /// Levels below `x` to drill into. Empty with a bucketed date `x` means the finer time
    /// grains (year → quarter → month → day).
    #[serde(default)]
    pub drill: Vec<Dimension>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sort: Option<VisualSort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub options: VisualOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VisualSort {
    pub by: SortBy,
    #[serde(default)]
    pub dir: SortDir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SortBy {
    /// The axis field.
    X,
    /// The first measure.
    Value,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VisualOptions {
    /// Stack series (bar, column, area) instead of grouping them.
    #[serde(default)]
    pub stacked: bool,
    /// KPI: compare with an earlier period of the sheet's date range on this column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compare: Option<String>,
    /// Which earlier period (docs/adr/0006 decision 5).
    #[serde(default, skip_serializing_if = "CompareTo::is_previous_period")]
    #[ts(as = "Option<CompareTo>", optional)]
    pub compare_to: CompareTo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub format: Option<ValueFormat>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CompareTo {
    /// The period just before, of the same length (for a to-date range, the same stretch of the
    /// previous period).
    #[default]
    PreviousPeriod,
    /// The same dates a year earlier.
    PreviousYear,
}

impl CompareTo {
    #[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if passes a reference
    fn is_previous_period(&self) -> bool {
        *self == CompareTo::PreviousPeriod
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ValueFormat {
    Number,
    Currency,
    Percent,
}

/// The interactive state of a sheet, shared by all its visuals.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SheetContext {
    /// Sheet filter values. Each applies to every visual whose table has the column.
    #[serde(default)]
    pub filters: Vec<Filter>,
    /// Cross-filter selections made on visuals.
    #[serde(default)]
    pub selections: Vec<Selection>,
}

/// A value picked on one visual (a bar, a slice, a slicer item) that filters the others.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Selection {
    pub visual_id: Uuid,
    pub table_id: Uuid,
    #[serde(default)]
    pub scope: SelectionScope,
    pub filter: Filter,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SelectionScope {
    /// Filters visuals on the same table (cross-filtering). Relationships between tables come
    /// with the business-logic layer (phase 3).
    #[default]
    Table,
    /// Filters every visual whose table has the column (slicers).
    Sheet,
}

/// A visual to run in a sheet batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VisualRequest {
    pub id: Uuid,
    pub spec: VisualSpec,
    /// Values picked at each drill level so far.
    #[serde(default)]
    pub drill: Vec<Scalar>,
    /// Raw-row tables: the first row of the page to fetch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub offset: Option<u32>,
}
