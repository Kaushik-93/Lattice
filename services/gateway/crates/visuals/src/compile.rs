use chrono::{Datelike, Days, Months, NaiveDate};
use lattice_planner::{
    Aggregation, Dimension, Filter, FilterOp, FilterValue, MAX_POINTS, Measure, PlanError, Scalar,
    Sort, SortDir, TableQuery, TableSchema, TimeBucket,
};
use uuid::Uuid;

use crate::{CompareTo, SelectionScope, SheetContext, SortBy, VisualKind, VisualSpec};

/// Which of a visual's queries a result answers. Results are framed with the visual id plus
/// [`QueryPart::suffix`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryPart {
    Main,
    /// The same KPI over the previous period.
    Compare,
}

impl QueryPart {
    pub const fn suffix(self) -> &'static str {
        match self {
            QueryPart::Main => "",
            QueryPart::Compare => ":compare",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisualQuery {
    pub part: QueryPart,
    pub query: TableQuery,
    /// The visual's ordering. For raw measures it is already in `query.sort`; a query over
    /// metrics names its output columns only once the semantic model compiles it, so it sorts
    /// from this.
    pub sort: Option<(SortBy, SortDir)>,
    /// For a comparison, how far back it looks.
    pub shift: Option<Shift>,
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum VisualError {
    #[error("this {kind} needs {what}")]
    Missing {
        kind: &'static str,
        what: &'static str,
    },
    #[error("cannot drill further")]
    DrillTooDeep,
    #[error("invalid drill value: {0}")]
    InvalidDrill(String),
    #[error(transparent)]
    Plan(#[from] PlanError),
}

/// Default rows for a scatter plot; a spec may ask for up to [`MAX_POINTS`].
const SCATTER_POINTS: u32 = 200_000;
/// Rows per page of a raw-row table.
const TABLE_PAGE: u32 = 500;

/// Compiles one visual of a sheet into the table queries that feed it.
///
/// Context filters and selections whose column the visual's table lacks are skipped, so a sheet
/// can mix visuals over different tables. A visual is never filtered by its own selection (it
/// highlights it instead).
pub fn compile(
    visual_id: Uuid,
    spec: &VisualSpec,
    drill: &[Scalar],
    offset: Option<u32>,
    ctx: &SheetContext,
    table: &TableSchema,
) -> Result<Vec<VisualQuery>, VisualError> {
    let has = |column: &str| table.columns.iter().any(|c| c.name == column);
    let mut filters = spec.filters.clone();
    filters.extend(ctx.filters.iter().filter(|f| has(&f.column)).cloned());
    filters.extend(
        ctx.selections
            .iter()
            .filter(|s| s.visual_id != visual_id && has(&s.filter.column))
            .filter(|s| s.scope == SelectionScope::Sheet || s.table_id == spec.table_id)
            .map(|s| s.filter.clone()),
    );

    let x = match &spec.x {
        Some(x) => {
            let levels = drill_levels(x, &spec.drill);
            if drill.len() >= levels.len() {
                return Err(VisualError::DrillTooDeep);
            }
            for (level, value) in levels.iter().zip(drill) {
                filters.extend(drill_filters(level, value)?);
            }
            Some(levels[drill.len()].clone())
        }
        None if !drill.is_empty() => return Err(VisualError::DrillTooDeep),
        None => None,
    };

    let kind = kind_name(spec.kind);
    let missing = |what| VisualError::Missing { kind, what };
    let need_x = || x.clone().ok_or(missing("an x axis"));
    let first_measure = || spec.measures.first().cloned().ok_or(missing("a measure"));
    let count_if_none = || {
        if spec.measures.is_empty() {
            vec![Measure {
                column: None,
                agg: Aggregation::Count,
                metric: None,
            }]
        } else {
            spec.measures.clone()
        }
    };
    let categorical = |d: &Dimension| d.bucket.is_none() && d.bin.is_none();

    let mut query = TableQuery {
        table_id: spec.table_id,
        columns: Vec::new(),
        dimensions: Vec::new(),
        measures: Vec::new(),
        filters,
        sort: Vec::new(),
        limit: spec.limit,
        offset: None,
    };
    // (default limit, default sort) per kind.
    let (limit, sort) = match spec.kind {
        VisualKind::Kpi => {
            query.measures = vec![first_measure()?];
            (1, None)
        }
        VisualKind::Line | VisualKind::Area | VisualKind::Bar | VisualKind::Column => {
            let x = need_x()?;
            if spec.measures.is_empty() {
                return Err(missing("a measure"));
            }
            let ranked = categorical(&x)
                && spec.series.is_none()
                && matches!(spec.kind, VisualKind::Bar | VisualKind::Column);
            query.dimensions = std::iter::once(x).chain(spec.series.clone()).collect();
            query.measures = spec.measures.clone();
            if ranked {
                (50, Some((SortBy::Value, SortDir::Desc)))
            } else {
                (10_000, None)
            }
        }
        VisualKind::Pie | VisualKind::Donut => {
            query.dimensions = vec![need_x()?];
            query.measures = vec![first_measure()?];
            (12, Some((SortBy::Value, SortDir::Desc)))
        }
        VisualKind::Scatter => {
            let x = need_x()?;
            let y = spec.columns.first().ok_or(missing("a y column"))?;
            query.columns = std::iter::once(x.column)
                .chain(std::iter::once(y.clone()))
                .chain(spec.series.as_ref().map(|s| s.column.clone()))
                .collect();
            (SCATTER_POINTS, None)
        }
        VisualKind::Table => {
            if x.is_none() && spec.measures.is_empty() {
                query.columns = spec.columns.clone();
                query.offset = offset;
                (TABLE_PAGE, None)
            } else {
                query.dimensions = x.into_iter().chain(spec.series.clone()).collect();
                query.measures = spec.measures.clone();
                (10_000, None)
            }
        }
        VisualKind::Pivot | VisualKind::Heatmap => {
            let rows = need_x()?;
            let cols = spec.series.clone().ok_or(missing("a second dimension"))?;
            query.dimensions = vec![rows, cols];
            query.measures = if spec.kind == VisualKind::Heatmap {
                vec![first_measure()?]
            } else if spec.measures.is_empty() {
                return Err(missing("a measure"));
            } else {
                spec.measures.clone()
            };
            (10_000, None)
        }
        VisualKind::Histogram => {
            let x = need_x()?;
            if x.bin.is_none() {
                return Err(missing("a bin width"));
            }
            query.dimensions = vec![x];
            query.measures = count_if_none();
            (1_000, None)
        }
        VisualKind::Slicer => {
            query.dimensions = vec![need_x()?];
            query.measures = count_if_none();
            (500, None)
        }
    };
    query.limit = Some(match spec.kind {
        VisualKind::Scatter => spec.limit.unwrap_or(limit).min(MAX_POINTS),
        _ => spec.limit.unwrap_or(limit),
    });

    let sort = spec
        .sort
        .map(|s| (s.by, s.dir))
        .or(sort)
        .filter(|_| !query.dimensions.is_empty());
    let governed = query.measures.iter().any(|m| m.metric.is_some());
    if let Some((by, dir)) = sort
        && !governed
    {
        // Output column names come from the planner (e.g. `sum_amount`), so ask it.
        let columns = lattice_planner::compile(&query, table)?.columns;
        let name = match by {
            SortBy::X => columns.first(),
            SortBy::Value => columns.get(query.dimensions.len()),
        };
        if let Some(c) = name {
            query.sort = vec![Sort {
                by: c.name.clone(),
                dir,
            }];
        }
    }

    let mut out = Vec::with_capacity(2);
    if spec.kind == VisualKind::Kpi
        && let Some(column) = &spec.options.compare
        && let Some((previous, shift)) = earlier(&query.filters, column, spec.options.compare_to)
    {
        let mut compare = query.clone();
        compare.filters = previous;
        out.push(VisualQuery {
            part: QueryPart::Main,
            query,
            sort,
            shift: None,
        });
        out.push(VisualQuery {
            part: QueryPart::Compare,
            query: compare,
            sort,
            shift: Some(shift),
        });
    } else {
        out.push(VisualQuery {
            part: QueryPart::Main,
            query,
            sort,
            shift: None,
        });
    }
    Ok(out)
}

fn kind_name(kind: VisualKind) -> &'static str {
    match kind {
        VisualKind::Kpi => "KPI",
        VisualKind::Line => "line chart",
        VisualKind::Area => "area chart",
        VisualKind::Bar => "bar chart",
        VisualKind::Column => "column chart",
        VisualKind::Scatter => "scatter plot",
        VisualKind::Pie => "pie chart",
        VisualKind::Donut => "donut chart",
        VisualKind::Table => "table",
        VisualKind::Pivot => "pivot table",
        VisualKind::Heatmap => "heatmap",
        VisualKind::Histogram => "histogram",
        VisualKind::Slicer => "slicer",
    }
}

/// `x` followed by the levels a click drills into.
fn drill_levels(x: &Dimension, drill: &[Dimension]) -> Vec<Dimension> {
    let mut levels = vec![x.clone()];
    if !drill.is_empty() {
        levels.extend(drill.iter().cloned());
        return levels;
    }
    let finer: &[TimeBucket] = match x.bucket {
        Some(TimeBucket::Year) => &[TimeBucket::Quarter, TimeBucket::Month, TimeBucket::Day],
        Some(TimeBucket::FiscalYear) => &[
            TimeBucket::FiscalQuarter,
            TimeBucket::Month,
            TimeBucket::Day,
        ],
        Some(TimeBucket::Quarter) | Some(TimeBucket::FiscalQuarter) => {
            &[TimeBucket::Month, TimeBucket::Day]
        }
        Some(TimeBucket::Month) | Some(TimeBucket::Week) => &[TimeBucket::Day],
        Some(TimeBucket::Day) | None => &[],
    };
    levels.extend(finer.iter().map(|b| Dimension {
        column: x.column.clone(),
        bucket: Some(*b),
        bin: None,
    }));
    levels
}

/// Filters that keep only the rows under `value` at `level`.
fn drill_filters(level: &Dimension, value: &Scalar) -> Result<Vec<Filter>, VisualError> {
    let filter = |op, value| Filter {
        column: level.column.clone(),
        op,
        value: Some(FilterValue::One(value)),
    };
    if let Some(bucket) = level.bucket {
        let Scalar::Text(text) = value else {
            return Err(VisualError::InvalidDrill("expected a date".into()));
        };
        let start = parse_date(text)?;
        let end = match bucket {
            TimeBucket::Year | TimeBucket::FiscalYear => start.checked_add_months(Months::new(12)),
            TimeBucket::Quarter | TimeBucket::FiscalQuarter => {
                start.checked_add_months(Months::new(3))
            }
            TimeBucket::Month => start.checked_add_months(Months::new(1)),
            TimeBucket::Week => start.checked_add_days(Days::new(7)),
            TimeBucket::Day => start.checked_add_days(Days::new(1)),
        }
        .ok_or_else(|| VisualError::InvalidDrill(text.clone()))?;
        return Ok(vec![
            filter(FilterOp::Gte, Scalar::Text(start.to_string())),
            filter(FilterOp::Lt, Scalar::Text(end.to_string())),
        ]);
    }
    if let Some(width) = level.bin {
        let Scalar::Number(lo) = value else {
            return Err(VisualError::InvalidDrill("expected a number".into()));
        };
        return Ok(vec![
            filter(FilterOp::Gte, Scalar::Number(*lo)),
            filter(FilterOp::Lt, Scalar::Number(lo + width)),
        ]);
    }
    Ok(vec![filter(FilterOp::Eq, value.clone())])
}

/// Accepts `YYYY-MM-DD`, optionally followed by a time (bucket starts arrive as dates or
/// midnight timestamps).
fn parse_date(text: &str) -> Result<NaiveDate, VisualError> {
    text.get(..10)
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .ok_or_else(|| VisualError::InvalidDrill(text.to_owned()))
}

/// The filters with the `between` range on `column` moved to the period it's compared with, or
/// `None` when there is no such range.
///
/// The previous period: a range starting on the 1st shifts by the months it touches, so whole
/// months compare with the months before (Aug–Oct → May–Jul) and a to-date range with the same
/// stretch of the previous period (1 Aug–15 Oct → 1 May–15 Jul); other ranges shift by their
/// length in days. The same period last year shifts both ends by a year.
fn earlier(filters: &[Filter], column: &str, to: CompareTo) -> Option<(Vec<Filter>, Shift)> {
    let i = filters
        .iter()
        .position(|f| f.column == column && f.op == FilterOp::Between)?;
    let Some(FilterValue::Many(bounds)) = &filters[i].value else {
        return None;
    };
    let [Scalar::Text(a), Scalar::Text(b)] = bounds.as_slice() else {
        return None;
    };
    let (start, end) = (parse_date(a).ok()?, parse_date(b).ok()?);
    if end < start {
        return None;
    }
    let (prev_start, prev_end, shift) = match to {
        CompareTo::PreviousYear => (
            start.checked_sub_months(Months::new(12))?,
            end.checked_sub_months(Months::new(12))?,
            Shift::Months(12),
        ),
        CompareTo::PreviousPeriod if start.day() == 1 => {
            let months =
                (end.year() - start.year()) * 12 + end.month() as i32 - start.month() as i32 + 1;
            let months = u32::try_from(months).ok()?;
            let whole = end.checked_add_days(Days::new(1))?.day() == 1;
            let prev_end = if whole {
                start.pred_opt()?
            } else {
                end.checked_sub_months(Months::new(months))?
            };
            (
                start.checked_sub_months(Months::new(months))?,
                prev_end,
                Shift::Months(months),
            )
        }
        CompareTo::PreviousPeriod => {
            let days = u64::try_from((end - start).num_days() + 1).ok()?;
            (
                start.checked_sub_days(Days::new(days))?,
                start.pred_opt()?,
                Shift::Days(days),
            )
        }
    };
    let mut shifted = filters.to_vec();
    shifted[i].value = Some(FilterValue::Many(vec![
        Scalar::Text(prev_start.to_string()),
        Scalar::Text(prev_end.to_string()),
    ]));
    Some((shifted, shift))
}

/// How far back a comparison looks. Relative dates in definitions ("the last 90 days") move
/// back with it, so the earlier period is measured as it was then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Months(u32),
    Days(u64),
}

impl Shift {
    pub fn back(self, date: NaiveDate) -> Option<NaiveDate> {
        match self {
            Shift::Months(m) => date.checked_sub_months(Months::new(m)),
            Shift::Days(d) => date.checked_sub_days(Days::new(d)),
        }
    }
}
