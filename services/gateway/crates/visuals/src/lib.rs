//! Visual specs and their compilation to table queries.
//!
//! A [`VisualSpec`] is what a sheet stores for each chart: its table, the fields on each axis,
//! measures, visual-level filters and display options. The gateway compiles a spec plus the
//! sheet's interactive state — filter values, cross-filter selections, the drill path — into one
//! or more [`TableQuery`]s, which the planner turns into SQL. Compiling here rather than in the
//! browser is what lets a shared link run a sheet's own visuals without being able to run
//! anything else (docs/adr/0005).

mod compile;

pub use compile::{QueryPart, Shift, VisualError, VisualQuery, compile};

use lattice_planner::{Dimension, Filter, Measure, Scalar, SortDir};
use std::collections::BTreeMap;

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
    /// How the visual looks (ADR 0007).
    #[serde(default, skip_serializing_if = "VisualStyle::is_default")]
    #[ts(as = "Option<VisualStyle>", optional)]
    pub style: VisualStyle,
}

/// Display-only formatting, applied by the browser (ADR 0007): never part of a query, so it
/// changes neither results nor their cache keys.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VisualStyle {
    /// Colour per series, by the series' name as the legend shows it: `#rrggbb`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[ts(as = "Option<BTreeMap<String, String>>", optional)]
    pub colors: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub data_labels: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub legend: Option<LegendPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub x_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub y_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub y_min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub y_max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub y_log: Option<bool>,
    /// Shown unless set to false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub gridlines: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub decimals: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub suffix: Option<String>,
}

impl VisualStyle {
    fn is_default(&self) -> bool {
        *self == VisualStyle::default()
    }

    /// Whether a saved style is one the browser can render.
    pub fn check(&self) -> Result<(), String> {
        let short =
            |s: &Option<String>, max: usize| s.as_ref().is_none_or(|s| s.chars().count() <= max);
        if self.colors.len() > 24 {
            return Err("At most 24 series colours.".into());
        }
        for (name, color) in &self.colors {
            let hex = color.len() == 7
                && color.starts_with('#')
                && color[1..].chars().all(|c| c.is_ascii_hexdigit());
            if name.chars().count() > 120 || !hex {
                return Err(format!("{color:?} isn't a colour like #4338ca."));
            }
        }
        if !short(&self.x_title, 80) || !short(&self.y_title, 80) {
            return Err("Axis titles have at most 80 characters.".into());
        }
        if !short(&self.prefix, 8) || !short(&self.suffix, 8) {
            return Err("A prefix or suffix has at most 8 characters.".into());
        }
        if self.decimals.is_some_and(|d| d > 6) {
            return Err("At most 6 decimals.".into());
        }
        let finite = |v: Option<f64>| v.is_none_or(f64::is_finite);
        if !finite(self.y_min) || !finite(self.y_max) {
            return Err("Axis bounds are numbers.".into());
        }
        if let (Some(lo), Some(hi)) = (self.y_min, self.y_max)
            && lo >= hi
        {
            return Err("The axis minimum is below its maximum.".into());
        }
        if self.y_log == Some(true) && self.y_min.is_some_and(|v| v <= 0.0) {
            return Err("A log scale starts above zero.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LegendPosition {
    Top,
    Bottom,
    Right,
    Hidden,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_are_checked() {
        let ok = VisualStyle {
            colors: BTreeMap::from([("EMEA".into(), "#4338ca".into())]),
            y_min: Some(0.0),
            y_max: Some(100.0),
            decimals: Some(2),
            prefix: Some("€".into()),
            ..VisualStyle::default()
        };
        assert_eq!(ok.check(), Ok(()));
        let bad = |s: VisualStyle| assert!(s.check().is_err(), "{s:?}");
        bad(VisualStyle {
            colors: BTreeMap::from([("EMEA".into(), "red".into())]),
            ..ok.clone()
        });
        bad(VisualStyle {
            y_min: Some(5.0),
            y_max: Some(5.0),
            ..ok.clone()
        });
        bad(VisualStyle {
            y_log: Some(true),
            ..ok.clone()
        });
        bad(VisualStyle {
            suffix: Some("way too long".into()),
            ..ok.clone()
        });
        bad(VisualStyle {
            decimals: Some(9),
            ..ok
        });
        // A default style isn't stored at all.
        let json = serde_json::to_value(VisualOptions::default()).unwrap_or_default();
        assert!(json.get("style").is_none());
    }
}
